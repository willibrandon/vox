# Implementation Plan: Voice Activity Detection

**Branch**: `003-voice-activity-detection` | **Date**: 2026-02-19 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/003-voice-activity-detection/spec.md`

## Summary

Implement the Voice Activity Detection (VAD) subsystem using Silero VAD v5 via ONNX Runtime (`ort` 2.0.0-rc.11). The VAD is the pipeline gatekeeper — it processes 512-sample audio windows (32ms at 16 kHz) on CPU, tracks speech/silence transitions via a streaming state machine, accumulates speech into padded segments via a chunker, and dispatches complete utterances to the ASR engine over a channel. All processing runs on the processing thread, never the audio callback.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: `ort` 2.0.0-rc.11 (ONNX Runtime, `load-dynamic` feature), `ringbuf` 0.4 (consumer types), `tokio` 1.49 (async channel + processing loop)
**Storage**: N/A (in-memory buffers only; model file on disk)
**Testing**: `cargo test -p vox_core --features cuda` (Windows) / `--features metal` (macOS)
**Target Platform**: Windows (CUDA) + macOS (Metal) — VAD itself is CPU-only
**Project Type**: Three-crate Rust workspace (`vox`, `vox_core`, `vox_ui`)
**Performance Goals**: < 1ms per 512-sample window inference, < 5ms audio-to-decision latency
**Constraints**: < 5 MB memory (model + state), CPU-only inference, no blocking on audio callback thread
**Scale/Scope**: Single-user local dictation, one VAD instance per session

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evidence |
|-----------|--------|----------|
| **I. Local-Only Processing** | PASS | Silero VAD runs entirely on-device via ONNX Runtime. No network calls. Model file loaded from local disk. |
| **II. Real-Time Latency Budget** | PASS | VAD inference < 1ms per window (CPU). State machine < 0.01ms. Total < 5ms. Well within the 300ms/750ms end-to-end budget. No blocking on audio callback thread — all VAD work on processing thread. |
| **III. Full Pipeline — No Fallbacks** | PASS | VAD is a required pipeline component. No degraded modes. Pipeline does not start until model is loaded. No CPU fallback needed (VAD already runs on CPU by design). |
| **IV. Pure Rust / GPUI — No Web Tech** | PASS | Pure Rust implementation. `ort` crate provides Rust bindings to ONNX Runtime (C FFI). No web dependencies. |
| **V. Zero-Click First Launch** | PASS | VAD model (~1.1 MB) will be auto-downloaded by the model management feature. No setup wizards. VAD loads model from disk path provided by the pipeline orchestrator. |
| **VI. Scope Only Increases** | PASS | All components from the design doc Section 4.2 are implemented: SileroVad, VadStateMachine, SpeechChunker, VadConfig, processing loop. Nothing deferred or removed. |
| **VII. Public API Documentation** | PASS | All `pub` items will have `///` doc comments. Module-level `//!` docs on the vad module. |

**Pre-design gate: PASSED. No violations.**

## Project Structure

### Documentation (this feature)

```text
specs/003-voice-activity-detection/
├── plan.md              # This file
├── research.md          # Phase 0: ort API research, Silero model I/O
├── data-model.md        # Phase 1: Entity definitions and relationships
├── quickstart.md        # Phase 1: Build/test instructions
├── contracts/
│   └── vad-api.md       # Phase 1: Public module interface contract
├── checklists/
│   └── requirements.md  # Spec quality checklist
└── tasks.md             # Phase 2 output (/speckit.tasks)
```

### Source Code (repository root)

```text
crates/vox_core/src/
├── vox_core.rs          # Library root (already has `pub mod vad;`)
├── audio.rs             # Feature 002 module root (re-exports)
├── audio/               # Feature 002 (consumed by VAD)
│   ├── capture.rs       # AudioCapture, HeapCons<f32>
│   ├── ring_buffer.rs   # AudioRingBuffer factory
│   └── resampler.rs     # AudioResampler
├── vad.rs               # VAD module root: VadConfig, VadState, VadEvent, VadStateMachine, re-exports
└── vad/                 # NEW — this feature
    ├── silero.rs        # SileroVad (ONNX model wrapper)
    └── chunker.rs       # SpeechChunker (segment accumulator + padding)
```

**Structure Decision**: The VAD module follows the same sub-module pattern as the `audio` module — `vad.rs` is the module root (not `vad/mod.rs`, per coding guidelines) with `vad/` directory for submodules. Three files keep concerns separated (model inference, state logic, audio accumulation) while `vad.rs` holds shared types (`VadConfig`, `VadState`, `VadEvent`, `VadStateMachine`) and re-exports.

## Complexity Tracking

> No constitution violations. Table intentionally empty.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (none)    | —          | —                                   |
