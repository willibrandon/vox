# Research: Audio Capture Pipeline

**Branch**: `002-audio-capture` | **Date**: 2026-02-19
**Input**: spec.md, design doc Section 4.1, crate documentation

## R-001: cpal 0.17 API

**Decision**: Use cpal 0.17 for cross-platform audio capture via WASAPI (Windows) and CoreAudio (macOS).

**Key API Details**:

- `SampleRate` is `type SampleRate = u32` — a plain u32, not a newtype struct. Breaking change from 0.16 where it was `SampleRate(pub u32)`.
- `device.description()` returns `Result<DeviceDescription, DeviceNameError>`. Use `.name()` on the result to get `&str`. The old `device.name()` method still works but is deprecated since 0.17.
- `build_input_stream<T>(&self, config: &StreamConfig, data_cb, error_cb, timeout)` — takes `&StreamConfig` not `&SupportedStreamConfig`. Convert with `.into()` or `.config()`.
- `stream.play()` MUST be called after building — streams do not auto-start on all platforms.
- Audio callback receives `&[T]` with **interleaved** samples. For stereo: `[L0, R0, L1, R1, ...]`. Extract first channel with `.step_by(channels)`.
- Device disconnection reported through error callback as `StreamError::DeviceNotAvailable`.
- New in 0.17: `StreamError::StreamInvalidated` and `StreamError::BufferUnderrun` variants.
- `default_input_config()` returns `Result<SupportedStreamConfig>` with the device's preferred format.

**Alternatives Considered**: None. cpal is the standard cross-platform audio library for Rust. Already in `Cargo.toml`.

## R-002: ringbuf 0.4 API

**Decision**: Use ringbuf 0.4 `HeapRb` for lock-free SPSC ring buffer between audio callback and processing thread.

**Key API Details**:

- `HeapRb::<f32>::new(capacity)` creates the buffer. Type alias: `HeapRb<T> = SharedRb<Heap<T>>`.
- `rb.split()` consumes the buffer, wraps in `Arc`, returns `(HeapProd<f32>, HeapCons<f32>)`.
- `HeapProd<T>` and `HeapCons<T>` have **NO lifetime parameter** — they are `CachingProd<Arc<HeapRb<T>>>`, inherently `'static` because `Arc` is owned. The design doc's `HeapProd<'static, f32>` notation is inaccurate — it's just `HeapProd<f32>`.
- Both are `Send + !Sync` — correct for SPSC where each handle moves to exactly one thread.
- `push_slice(&samples) -> usize`: writes as many as fit, returns count. Excess samples silently dropped (no panic, no blocking).
- `pop_slice(&mut buf) -> usize`: reads available samples into buffer.
- `occupied_len()` and `vacant_len()` from `Observer` trait — available on both producer and consumer.
- Import: `use ringbuf::traits::*` or selectively `use ringbuf::traits::{Observer, Producer, Consumer, Split}`.

**Overflow Semantics**: `push_slice` drops the **newest** samples when full (those that don't fit). The `push_overwrite` method (drops oldest) exists only on the unsplit `RingBuffer` trait and requires exclusive access to both ends — not usable with split SPSC. For our use case, dropping newest on overflow is acceptable: the buffer holds ~2.7 seconds at 48 kHz, so overflow means the processing thread fell catastrophically behind. The spec's FR-010 intent (no blocking, no crash) is satisfied.

**Alternatives Considered**: Manual atomic ring buffer. Rejected — ringbuf is battle-tested and zero-allocation after init.

## R-003: rubato 1.0 API

**Decision**: Use rubato 1.0 `Fft<f32>` (synchronous FFT resampler) for fixed-ratio downsampling from device native rate to 16 kHz.

**Key API Details — MAJOR API REDESIGN from 0.16**:

- Old types `FftFixedIn`, `SincFixedIn`, etc. are **gone**. Replaced by `Fft<T>` (sync) and `Async<T>` (async) with enum params.
- `Fft::<f32>::new(input_rate, output_rate, chunk_size, sub_chunks, channels, FixedSync::Input)` — constructor.
- The old `Vec<Vec<f32>>` process API is gone. Now uses `Adapter`/`AdapterMut` traits from external `audioadapter` crate.
- **Two new dependencies required**: `audioadapter = "0.2"` and `audioadapter-buffers = { version = "2.0", features = ["std"] }`.
- `process_into_buffer()` is real-time safe (no allocations). Use `Indexing` struct for offsets.
- `process_all_into_buffer()` handles the processing loop, partial chunks, and delay trimming automatically — best for batch processing from ring buffer.
- For mono: wrap input in `SequentialSliceOfVecs::new(&[samples_vec], 1, frame_count)`.

**Why Fft over Async**: For fixed known ratios (48kHz→16kHz, 44.1kHz→16kHz), `Fft<T>` is the fastest option AND highest quality. `Async` is for when the ratio needs to change at runtime (clock drift correction), which we don't need.

**Chunk size**: 1024 frames is a good starting point. `sub_chunks = 1` gives best quality.

**Alternatives Considered**: `Async<T>::new_sinc()` — higher quality sinc interpolation but slower and overkill for our fixed ratio. `Async<T>::new_poly()` — faster but no anti-aliasing filter.

## R-004: Module Structure (audio.rs → audio/ directory)

**Decision**: Use modern Rust module convention — `audio.rs` as module root alongside `audio/` directory with submodules.

**Rationale**: The project convention (CLAUDE.md) forbids `mod.rs` files. The design doc shows `audio/mod.rs` but Tusk (reference project) uses `mod.rs`. We follow our own convention:

```
crates/vox_core/src/
├── vox_core.rs      # lib entry point: pub mod audio;
├── audio.rs         # module root: pub mod capture; pub mod ring_buffer; pub mod resampler;
└── audio/
    ├── capture.rs
    ├── ring_buffer.rs
    └── resampler.rs
```

The existing empty `audio.rs` stub will be updated to declare submodules.

**Alternatives Considered**: Flat single file `audio.rs` with everything. Rejected — the spec has distinct entities (capture, ring buffer, resampler) that warrant separate files per the design doc.

## R-005: Multi-Channel Downmix Strategy

**Decision**: Extract first channel only from interleaved multi-channel input.

**Rationale**: The spec says "only the first channel is used (downmix to mono)." cpal delivers interleaved data, so first-channel extraction is: `data.iter().step_by(channels).copied()`. This is zero-allocation (can be done in-place or streamed into the ring buffer).

**Alternatives Considered**: Average all channels for true downmix. Rejected — adds computation in the audio callback, and first-channel extraction is the standard approach for speech (most mics are mono anyway; stereo mics have identical channels for voice).

## R-006: Ring Buffer Capacity Calculation

**Decision**: Calculate capacity as `next_power_of_two(native_rate * 2)` samples of f32.

**Rationale**: From spec clarification — buffer must hold at least 2 seconds at the device's native rate, rounded to next power of two for cheap index wrapping (bitwise AND instead of modulo). Examples:
- 48000 Hz: 96000 samples → 131072 (2^17) = ~2.73 seconds, 512 KB
- 44100 Hz: 88200 samples → 131072 (2^17) = ~2.97 seconds, 512 KB
- 16000 Hz: 32000 samples → 32768 (2^15) = ~2.05 seconds, 128 KB
- 96000 Hz: 192000 samples → 262144 (2^18) = ~2.73 seconds, 1 MB

Note: ringbuf's internal capacity does not require power-of-two (it uses modular arithmetic with any capacity), but power-of-two is a reasonable sizing heuristic that provides consistent ~2-3 second buffers.

## R-007: Additional Cargo Dependencies

**Decision**: Add `audioadapter` and `audioadapter-buffers` to `vox_core/Cargo.toml`.

```toml
audioadapter = "0.2"
audioadapter-buffers = { version = "2.0", features = ["std"] }
```

These are required by rubato 1.0's new `Adapter`/`AdapterMut` API and are maintained by the same author.

## R-008: Error Handling Strategy

**Decision**: Use `anyhow::Result` for public API, pattern-match on `StreamError` for device disconnection detection.

- `AudioCapture::new()` → `Result<Self>` (can fail on device not found, config not supported)
- `AudioCapture::start()` → `Result<()>` (can fail on stream build or play)
- Audio callback errors → sent through a channel or `Arc<AtomicBool>` flag to signal disconnection
- `AudioResampler::new()` → `Option<Self>` (None when native rate matches target, bypassing resampler)
- `AudioResampler::process()` → `Result<Vec<f32>>` (resampling can technically fail on invalid input)

For the real-time error path (device disconnection in callback): use an `Arc<AtomicBool>` flag that the error callback sets and the processing thread polls. No allocation, no lock.
