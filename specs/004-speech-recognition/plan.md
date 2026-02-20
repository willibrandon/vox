# Implementation Plan: Speech Recognition (ASR)

**Branch**: `004-speech-recognition` | **Date**: 2026-02-19 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/004-speech-recognition/spec.md`

## Summary

Implement the ASR engine using whisper.cpp via whisper-rs 0.15.1 Rust bindings. The engine loads the Whisper Large V3 Turbo model (Q5_0 quantization, ~900 MB) with GPU acceleration, accepts 16 kHz mono PCM segments from the VAD, and returns transcribed text. A stitching layer handles force-segmented long speech by deduplicating 1-second overlap regions.

## Technical Context

**Language/Version**: Rust 2024 (1.85+), CMake 4.0+
**Primary Dependencies**: whisper-rs 0.15.1 (whisper.cpp FFI bindings), ort 2.0.0-rc.11 (Silero VAD, already present)
**Storage**: N/A (no persistent state for ASR)
**Testing**: `cargo test -p vox_core --features cuda` (Windows), `cargo test -p vox_core --features metal` (macOS)
**Target Platform**: Windows (CUDA 12.8+, RTX 4090), macOS (Metal, M4 Pro)
**Project Type**: Existing three-crate Rust workspace (vox, vox_core, vox_ui)
**Performance Goals**: 5s audio < 50ms (4090) / < 150ms (M4 Pro); 10s audio < 100ms (4090) / < 300ms (M4 Pro); model load < 5s
**Constraints**: < 1.8 GB VRAM for model, < 6 GB total VRAM budget (shared with future LLM), GPU mandatory
**Scale/Scope**: Single-user local inference, one transcription at a time (serialized via mutex)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Local-Only Processing | PASS | All inference on-device. whisper.cpp runs locally. No network calls. Model file assumed pre-downloaded by model management system. |
| II. Real-Time Latency Budget | PASS | ASR budget: < 50ms/150ms for 5s audio. Well within 300ms/750ms end-to-end. No blocking on audio callback — transcription runs on processing thread via `spawn_blocking`. |
| III. Full Pipeline — No Fallbacks | PASS | ASR is a required pipeline component. No CPU fallback — GPU mandatory. Engine does not start without a loaded model. |
| IV. Pure Rust / GPUI — No Web Tech | PASS | whisper-rs is a Rust crate wrapping whisper.cpp (C++). No web dependencies. |
| V. Zero-Click First Launch | PASS | Model file managed by separate download system. ASR engine loads model from disk path. No user interaction required. |
| VI. Scope Only Increases | PASS | All spec requirements implemented: model loading, transcription, sequential calls, force-segment stitching, thread-safe cloning. |
| VII. Public API Documentation | PASS | All pub items will have `///` doc comments per constitution requirement. |

No violations. All gates pass.

## Project Structure

### Documentation (this feature)

```text
specs/004-speech-recognition/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # Phase 0: whisper-rs API research
├── data-model.md        # Phase 1: ASR data model
├── quickstart.md        # Phase 1: build/test quickstart
├── contracts/           # Phase 1: ASR API contracts
│   └── asr-engine.md    # AsrEngine public API
└── checklists/
    └── requirements.md  # Spec quality validation
```

### Source Code (repository root)

```text
crates/vox_core/
├── src/
│   ├── vox_core.rs          # Module declarations (asr already declared)
│   ├── asr.rs               # ASR module root: AsrEngine, transcribe, stitch
│   └── asr/
│       └── stitcher.rs      # Force-segment overlap stitching
├── Cargo.toml               # whisper-rs 0.15.1 already in [dependencies]
└── tests/
    └── fixtures/
        ├── speech_sample.wav           # Existing VAD fixture (reuse for ASR)
        └── ggml-large-v3-turbo-q5_0.bin  # Whisper model (gitignored, ~900 MB)
```

**Structure Decision**: ASR lives in `crates/vox_core/src/asr.rs` as the module root, matching the existing pattern (`vad.rs` is a module root with `vad/silero.rs` and `vad/chunker.rs` as submodules). The stitcher for force-segmented speech gets its own submodule since it has distinct logic. The main `asr.rs` contains `AsrEngine` with `new()` and `transcribe()`.
