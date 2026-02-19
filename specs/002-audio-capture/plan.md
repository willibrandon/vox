# Implementation Plan: Audio Capture Pipeline

**Branch**: `002-audio-capture` | **Date**: 2026-02-19 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/002-audio-capture/spec.md`

## Summary

Implement the audio capture subsystem using cpal 0.17 (cross-platform audio), ringbuf 0.4 (lock-free SPSC ring buffer), and rubato 1.0 (FFT resampler). Audio is captured from the OS input device on a real-time callback thread, written into a ring buffer, and consumed by the processing thread which resamples to 16 kHz mono f32 PCM for downstream VAD/ASR. The audio callback performs zero allocations and zero lock acquisitions.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+), CMake 4.0+
**Primary Dependencies**: cpal 0.17 (audio capture), ringbuf 0.4 (SPSC ring buffer), rubato 1.0 (FFT resampler), audioadapter 0.2 + audioadapter-buffers 2.0 (rubato adapter API)
**Storage**: N/A — in-memory ring buffer only
**Testing**: `cargo test -p vox_core --features cuda` (Windows), `cargo test -p vox_core --features metal` (macOS)
**Target Platform**: Windows (WASAPI via cpal) + macOS (CoreAudio via cpal)
**Project Type**: Library crate (`vox_core`) with `audio` module containing 3 submodules
**Performance Goals**: Audio callback < 5 ms, ring buffer read < 1 ms, resampling > 10x real-time, CPU < 1% idle
**Constraints**: Zero allocations in audio callback, zero lock acquisitions in audio callback, ring buffer ~512 KB at 48 kHz
**Scale/Scope**: Single audio stream, mono channel, common sample rates (16/44.1/48/96 kHz)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Local-Only Processing | PASS | Audio capture is entirely on-device. No network calls. |
| II. Real-Time Latency Budget | PASS | Audio callback is zero-alloc/zero-lock. Resampling on processing thread. Ring buffer provides ~2.7s headroom. |
| III. Full Pipeline — No Fallbacks | PASS | Audio capture is a required component — no bypass, no degraded mode. Resampler bypassed only when native rate already matches 16 kHz (not a fallback — it's the optimal path). |
| IV. Pure Rust / GPUI — No Web Tech | PASS | All dependencies are pure Rust crates. No web toolchain. |
| V. Zero-Click First Launch | PASS | Audio capture auto-selects default device. No user configuration required. |
| VI. Scope Only Increases | PASS | All spec requirements implemented: capture, ring buffer, resampling, device enumeration, device switching, disconnection detection. |

**Post-Design Re-Check**: All principles remain PASS. The addition of `audioadapter` and `audioadapter-buffers` dependencies are pure Rust crates required by rubato 1.0's new API — no web tech or network dependencies introduced.

## Project Structure

### Documentation (this feature)

```text
specs/002-audio-capture/
├── plan.md              # This file
├── research.md          # Phase 0 output — cpal/ringbuf/rubato API research
├── data-model.md        # Phase 1 output — entity definitions
├── quickstart.md        # Phase 1 output — build/test commands
└── tasks.md             # Phase 2 output (/speckit.tasks command)
```

### Source Code (repository root)

```text
crates/vox_core/
├── Cargo.toml           # Add audioadapter, audioadapter-buffers deps
└── src/
    ├── vox_core.rs      # Lib entry: pub mod audio; (already exists)
    ├── audio.rs         # Module root: pub mod capture; pub mod ring_buffer; pub mod resampler;
    └── audio/
        ├── capture.rs   # AudioCapture, AudioConfig, AudioDeviceInfo, list_input_devices()
        ├── ring_buffer.rs # AudioRingBuffer — SPSC ring buffer wrapper
        └── resampler.rs # AudioResampler — rubato Fft<f32> wrapper
```

**Structure Decision**: The existing `crates/vox_core/src/audio.rs` stub becomes the module root file. Submodules live in `crates/vox_core/src/audio/` directory — following the project's modern Rust convention (no `mod.rs` files). This matches the design doc's module decomposition while adhering to CLAUDE.md conventions.

## Complexity Tracking

> No constitution violations. Table intentionally empty.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| — | — | — |
