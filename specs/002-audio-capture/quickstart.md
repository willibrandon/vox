# Quickstart: Audio Capture Pipeline

**Branch**: `002-audio-capture` | **Date**: 2026-02-19

## Prerequisites

Same as workspace scaffolding ([001 quickstart](../001-workspace-scaffolding/quickstart.md)):
- Rust 1.85+, CMake 4.0+
- Windows: VS 2022 Build Tools, CUDA 12.8+, cuDNN 9.x
- macOS: Xcode 26.x, Metal Toolchain

Additionally:
- A working audio input device (microphone, line-in, or virtual device)

## New Dependencies

This feature adds two new crates to `crates/vox_core/Cargo.toml`:

```toml
audioadapter = "2.0"
audioadapter-buffers = { version = "2.0", features = ["std"] }
```

These are required by rubato 1.0's redesigned adapter API.

## Build Commands

```bash
# Windows — build with CUDA
cargo build -p vox_core --features cuda

# macOS — build with Metal
cargo build -p vox_core --features metal

# Run all audio module tests (Windows)
cargo test -p vox_core --features cuda -- audio

# Run all audio module tests (macOS)
cargo test -p vox_core --features metal -- audio

# Run a specific test
cargo test -p vox_core --features cuda test_ring_buffer_basic -- --nocapture
```

## Verification

After building, verify:

1. **Zero warnings**: Build output shows no compiler warnings in audio modules.
2. **Tests pass**: All `audio::` tests pass (ring buffer, resampler, device enumeration).
3. **Device available**: `test_device_enumeration` returns at least one device.

## Test Scenarios

### Ring Buffer Tests (no hardware required)

| Test | What It Verifies |
|------|-----------------|
| `test_ring_buffer_basic` | Write N samples, read N, verify content matches |
| `test_ring_buffer_overflow` | Write more than capacity, no panic, data integrity |
| `test_ring_buffer_concurrent` | Producer/consumer on separate threads, no corruption |

### Resampler Tests (no hardware required)

| Test | What It Verifies |
|------|-----------------|
| `test_resampler_48000_to_16000` | Known sine wave resampled, frequency preserved |
| `test_resampler_44100_to_16000` | Same for 44.1 kHz source |
| `test_resampler_16000_bypass` | Returns None — no resampler needed |

### Device Tests (requires microphone)

| Test | What It Verifies |
|------|-----------------|
| `test_device_enumeration` | At least one input device listed |
| `test_capture_to_buffer` | Samples appear in ring buffer within 100 ms |
| `test_capture_stop_clean` | Start/stop with no hanging threads or leaks |

## Module Layout

```
crates/vox_core/src/
├── vox_core.rs          # pub mod audio;
├── audio.rs             # Module root: pub mod capture; pub mod ring_buffer; pub mod resampler;
└── audio/
    ├── capture.rs       # AudioCapture, AudioConfig, AudioDeviceInfo, list_input_devices()
    ├── ring_buffer.rs   # AudioRingBuffer wrapper around ringbuf::HeapRb
    └── resampler.rs     # AudioResampler wrapper around rubato::Fft<f32>
```

## Troubleshooting

| Problem | Cause | Fix |
|---------|-------|-----|
| `audioadapter` not found | Missing dependency | Add `audioadapter = "2.0"` to vox_core Cargo.toml |
| No audio devices found | No microphone connected | Connect a microphone or enable virtual audio device |
| Ring buffer tests hang | Deadlock in test | Ensure producer/consumer are on separate threads, not blocking |
| Resampler chunk size error | Input buffer too small | Ensure input length >= resampler chunk_size (1024 frames) |
| cpal `DeviceNotAvailable` | Microphone disconnected | Reconnect device; capture should detect via error_flag |
