# Data Model: Audio Capture Pipeline

**Branch**: `002-audio-capture` | **Date**: 2026-02-19
**Input**: spec.md, plan.md, research.md

## Entities

### AudioConfig

Configuration for the audio capture pipeline. Immutable after creation.

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `sample_rate` | `u32` | Default: 16000. Must be > 0. | Target output sample rate (whisper.cpp native format) |
| `channels` | `u16` | Default: 1. Fixed at 1 (mono). | Output channel count |
| `device_name` | `Option<String>` | None = system default input | User-selected device name override |

**Validation**: `sample_rate` must be a standard audio rate (8000, 16000, 22050, 44100, 48000, 96000). `channels` is always 1 — multi-channel input is downmixed to mono.

**Relationships**: Used by `AudioCapture::new()` to configure the capture stream.

---

### AudioCapture

Active audio capture stream. Owns the OS audio stream and the ring buffer producer.

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `stream` | `Option<cpal::Stream>` | None when stopped | The OS audio stream handle |
| `producer` | `HeapProd<f32>` | Moved into audio callback closure | Ring buffer producer end |
| `device_name` | `String` | Non-empty after construction | Name of the active capture device |
| `native_sample_rate` | `u32` | > 0, set from device config | Device's native capture rate (may differ from target 16 kHz) |
| `error_flag` | `Arc<AtomicBool>` | Shared with error callback | Set to true on device disconnection |

**State Transitions**:
- `new()` → Created (stream not yet started)
- `start()` → Running (stream.play() called, audio flowing)
- `stop()` → Stopped (stream dropped, producer still valid but idle)
- Error callback sets `error_flag` → Disconnected (caller polls flag)

**Relationships**:
- Receives `AudioConfig` for initialization
- Produces to `AudioRingBuffer` via `HeapProd<f32>`
- Reports errors via `Arc<AtomicBool>` (real-time safe)

---

### AudioRingBuffer

Lock-free SPSC ring buffer. Created once, then split into producer/consumer halves.

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| (internal) | `HeapRb<f32>` | Consumed by `split()` | The underlying ring buffer |

**Public Interface**: `new(capacity: usize)` returns `(HeapProd<f32>, HeapCons<f32>)`.

**Capacity Calculation**: `next_power_of_two(native_sample_rate * 2)` samples.

| Native Rate | Raw (2s) | Power-of-Two | Duration | Memory |
|-------------|----------|--------------|----------|--------|
| 16000 Hz | 32000 | 32768 (2^15) | ~2.05 s | 128 KB |
| 44100 Hz | 88200 | 131072 (2^17) | ~2.97 s | 512 KB |
| 48000 Hz | 96000 | 131072 (2^17) | ~2.73 s | 512 KB |
| 96000 Hz | 192000 | 262144 (2^18) | ~2.73 s | 1 MB |

**Overflow Behavior**: `push_slice` drops newest samples that don't fit. No blocking, no panic, no allocation.

**Thread Safety**: Producer (`HeapProd<f32>`) is `Send + !Sync` — moves to audio callback thread. Consumer (`HeapCons<f32>`) is `Send + !Sync` — moves to processing thread. Both are `'static` (Arc-based ownership, no lifetime parameter).

**Relationships**:
- Producer owned by `AudioCapture` (audio callback thread)
- Consumer owned by processing thread (future pipeline feature)

---

### AudioResampler

Sample rate converter using rubato's synchronous FFT resampler.

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `resampler` | `Fft<f32>` | Created with fixed ratio | The rubato FFT resampler instance |
| `input_rate` | `u32` | Must differ from output_rate | Source sample rate (device native) |
| `output_rate` | `u32` | Default: 16000 | Target sample rate |
| `chunk_size` | `usize` | Default: 1024 frames | Processing chunk size |

**Construction**: `new(input_rate, output_rate)` returns `Option<Self>` — `None` when rates match (bypass path).

**Relationships**:
- Consumes samples from `AudioRingBuffer` consumer
- Outputs 16 kHz mono f32 PCM to downstream pipeline (VAD)
- Requires `audioadapter` crate for the `Adapter`/`AdapterMut` traits

---

### AudioDeviceInfo

Device metadata for the settings UI. Read-only.

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `name` | `String` | Non-empty | Human-readable device name |
| `is_default` | `bool` | — | Whether this is the system default input device |

**Relationships**: Returned by `list_input_devices()`. Used by settings UI to populate device picker. Selected device name passed to `AudioConfig`.

## Entity Relationship Diagram

```text
AudioConfig ──creates──▶ AudioCapture ──writes──▶ AudioRingBuffer
                              │                        │
                              │                   (split into)
                              │                   ┌────┴────┐
                              │              HeapProd    HeapCons
                              │            (callback)  (processing)
                              │                             │
                              │                             ▼
                         error_flag              AudioResampler
                      (Arc<AtomicBool>)           (if rate ≠ 16k)
                              │                             │
                              ▼                             ▼
                        Pipeline error              16 kHz f32 PCM
                        notification                (to VAD/ASR)

AudioDeviceInfo ◀──enumerates── list_input_devices()
```
