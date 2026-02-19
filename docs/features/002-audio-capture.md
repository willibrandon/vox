# Feature 002: Audio Capture Pipeline

**Status:** Not Started
**Dependencies:** 001-workspace-scaffolding
**Design Reference:** Section 4.1 (Audio Capture Pipeline)
**Estimated Scope:** cpal integration, ring buffer, resampler, audio configuration

---

## Overview

Implement the audio capture subsystem that feeds the entire dictation pipeline. Audio is captured from the system's input device via cpal 0.17, written into a lock-free SPSC ring buffer (ringbuf 0.4), and optionally resampled to 16 kHz mono f32 PCM (rubato 1.0). This is the real-time hot path — zero allocations, zero locks, zero ML on the audio callback thread.

---

## Requirements

### FR-001: Audio Configuration

```rust
// crates/vox_core/src/audio/mod.rs

pub struct AudioConfig {
    pub sample_rate: u32,        // 16_000 Hz (whisper.cpp native)
    pub channels: u16,           // 1 (mono)
    pub sample_format: SampleFormat, // F32
    pub device: Option<String>,  // None = system default input
}
```

Default configuration targets 16 kHz mono f32, which is whisper.cpp's native format.

### FR-002: Audio Capture (cpal 0.17)

Implement `AudioCapture` in `crates/vox_core/src/audio/capture.rs`:

- Enumerate available input devices via `cpal::default_host()`
- Select device by name (from settings) or fall back to system default
- Configure the input stream for the device's native format
- Write samples into the ring buffer from the audio callback

**cpal 0.17 API notes:**
- `SampleRate` is a bare `u32`, not a wrapper struct
- `BufferSize::Default` — do not set buffer size manually, let the host decide
- `device.description()` returns a `DeviceDescription` struct — use `.name()` to get the display string
- cpal handles real-time thread priority automatically. Do not set it manually.
- The audio callback runs on a real-time OS thread. No allocations, no locks, no blocking calls, no ML inside the callback.

```rust
// crates/vox_core/src/audio/capture.rs

pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    producer: ringbuf::HeapProd<'static, f32>,
    device_name: String,
    native_sample_rate: u32,
}

impl AudioCapture {
    pub fn new(config: &AudioConfig) -> Result<Self>;
    pub fn start(&mut self) -> Result<()>;
    pub fn stop(&mut self);
    pub fn device_name(&self) -> &str;
    pub fn native_sample_rate(&self) -> u32;
}
```

The callback writes raw samples into the ring buffer producer. If the device's native sample rate differs from 16 kHz, raw samples go into the buffer and resampling happens on the processing thread (Feature 002, FR-004).

### FR-003: Ring Buffer (ringbuf 0.4)

Implement `AudioRingBuffer` in `crates/vox_core/src/audio/ring_buffer.rs`:

- SPSC (single-producer, single-consumer) lock-free ring buffer
- 64 KB capacity = ~2 seconds of 16 kHz mono f32 audio
- Producer end held by the audio callback thread
- Consumer end held by the processing thread (VAD + ASR)

**ringbuf 0.4 API notes:**
- `occupied_len()` lives on the `Observer` trait, NOT directly on the consumer
- Must `use ringbuf::traits::Observer` to access it
- Use `ringbuf::HeapRb` for heap-allocated ring buffer
- Split into `HeapProd` (producer) and `HeapCons` (consumer)

```rust
// crates/vox_core/src/audio/ring_buffer.rs

use ringbuf::{HeapRb, HeapProd, HeapCons};
use ringbuf::traits::Observer; // Required for occupied_len()

pub struct AudioRingBuffer {
    // Created once, then split into producer/consumer
}

impl AudioRingBuffer {
    /// Create a new ring buffer with the given capacity in samples.
    /// 64 KB = 16384 f32 samples = ~1.024 seconds at 16 kHz.
    /// Use 32768 samples (~2.048 seconds) for jitter headroom.
    pub fn new(capacity: usize) -> (HeapProd<'static, f32>, HeapCons<'static, f32>);
}
```

**Buffer sizing rationale:** 32768 f32 samples × 4 bytes = 128 KB. At 16 kHz, this holds ~2 seconds of audio. Provides headroom for processing thread jitter without dropping samples.

### FR-004: Resampler (rubato 1.0)

Implement `AudioResampler` in `crates/vox_core/src/audio/resampler.rs`:

If the system's default input device does not natively support 16 kHz, resample the audio on the **processing thread** (never on the audio callback thread).

**rubato 1.0 API notes:**
- Major API redesign from 0.16 — old vector-of-vectors API is gone
- Use the `AudioAdapter` trait with `SequentialSliceOfVecs` adapter
- `FftFixedIn` or `SincFixedIn` for the resampler implementation
- Process in chunks matching the resampler's chunk size

```rust
// crates/vox_core/src/audio/resampler.rs

pub struct AudioResampler {
    resampler: Box<dyn rubato::Resampler<f32>>,
    input_rate: u32,
    output_rate: u32,
}

impl AudioResampler {
    /// Create a resampler from the device's native rate to 16 kHz.
    /// Returns None if the native rate is already 16 kHz.
    pub fn new(input_rate: u32, output_rate: u32) -> Option<Self>;

    /// Resample a buffer of audio samples.
    /// Input: samples at input_rate. Output: samples at output_rate.
    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>>;
}
```

Common device sample rates to handle: 44100, 48000, 96000, 16000 Hz. At 16 kHz native, the resampler is bypassed entirely.

### FR-005: Device Enumeration

Provide a function to list available input devices for the settings UI:

```rust
pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>>;

pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
    pub supported_sample_rates: Vec<u32>,
}
```

### FR-006: Audio Device Hot-Swap

When the user changes the input device in settings (or the current device disconnects):

1. Stop the current audio stream
2. Drain the ring buffer
3. Start a new stream on the new device
4. Update the resampler if the native sample rate changed

Device disconnection must be detected and reported to the pipeline (not silently fail).

---

## Thread Safety Model

```
Audio Callback Thread          Processing Thread
        │                              │
        ▼                              ▼
  ┌───────────┐    SPSC Ring     ┌───────────┐
  │ cpal      │───(lock-free)───▶│ Resample  │
  │ callback  │   32768 samples  │ + VAD     │
  └───────────┘                  └───────────┘
```

- **Audio callback thread**: Writes raw samples. No allocations, no locks, no blocking.
- **Processing thread**: Reads samples, resamples if needed, feeds VAD. Can allocate, can block briefly.
- The ring buffer is the only communication channel between these threads.

---

## Acceptance Criteria

- [ ] Audio capture starts and stops cleanly
- [ ] Samples appear in ring buffer from default input device
- [ ] Ring buffer handles overflow gracefully (drops oldest samples)
- [ ] Resampler correctly converts 44.1/48 kHz to 16 kHz
- [ ] Device enumeration returns available input devices
- [ ] Device disconnection is detected and reported
- [ ] Zero allocations in the audio callback (verified via review)
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_ring_buffer_basic` | Write N samples, read N samples, verify content |
| `test_ring_buffer_overflow` | Write more than capacity, verify no panic, oldest dropped |
| `test_ring_buffer_concurrent` | Producer/consumer on separate threads, verify no data corruption |
| `test_resampler_44100_to_16000` | Resample known sine wave, verify frequency preserved |
| `test_resampler_48000_to_16000` | Same for 48 kHz |
| `test_resampler_16000_bypass` | 16 kHz input returns None (no resampler needed) |
| `test_device_enumeration` | List devices, verify at least one exists |

### Integration Tests

| Test | Description |
|---|---|
| `test_capture_to_buffer` | Start capture, verify samples appear in ring buffer within 100ms |
| `test_capture_stop_clean` | Start then stop capture, verify no hanging threads |

---

## Performance Targets

| Metric | Target |
|---|---|
| Audio callback latency | < 5 ms |
| Ring buffer read latency | < 1 ms |
| Resampling throughput | > 10x real-time (1 second of audio processed in < 100 ms) |
| Memory (ring buffer) | ~128 KB fixed |
| CPU (idle, stream open) | < 1% |
