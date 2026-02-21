# Implementation Plan: Model Management

**Branch**: `008-model-management` | **Date**: 2026-02-21 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/008-model-management/spec.md`

## Summary

Implement the model management subsystem in `vox_core::models` that handles automatic downloading, SHA-256 verification, storage, directory polling, and format validation of three ML models (Silero VAD, Whisper ASR, Qwen LLM). On first launch, all three models download concurrently to the platform-standard app data directory with throttled progress reporting via tokio broadcast channel. Downloads use atomic file writes (.tmp -> verify -> rename). A 5-second directory poll detects manually-placed files when downloads fail. Model format validation via magic byte inspection enables safe model swapping.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**:
- `reqwest 0.12` (existing) — streaming HTTP downloads with `stream` feature, rustls backend
- `tokio 1.49` (existing) — async runtime, broadcast channels, spawn, timers, filesystem ops
- `sha2 0.10` (NEW) — SHA-256 checksum verification during download
- `dirs 5` (NEW) — platform-standard data directory resolution
**Storage**: Filesystem — `%LOCALAPPDATA%/com.vox.app/models/` (Windows), `~/Library/Application Support/com.vox.app/models/` (macOS)
**Testing**: `cargo test -p vox_core --features cuda` (Windows) / `--features metal` (macOS). Unit tests use `tempfile` crate (existing dev-dep). Integration tests that download real models are marked with no special attributes (Constitution VIII forbids `#[ignore]`), but test the smallest model (VAD, ~1.1 MB) to keep test time reasonable.
**Target Platform**: Windows 10+ (CUDA), macOS 14+ (Metal)
**Project Type**: Three-crate workspace (vox, vox_core, vox_ui)
**Performance Goals**: SHA-256 verification < 5s for 1.8 GB file; model directory detection < 100ms; download speed limited only by network bandwidth
**Constraints**: Zero network calls after models present (FR-014); pipeline blocked until all 3 models verified (FR-018); no resume/partial downloads; single static binary
**Scale/Scope**: 3 models (~2.5 GB total); single-user desktop application

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | Evidence |
|-----------|--------|----------|
| I. Local-Only Processing | PASS | Network only for model download during first-run. SHA-256 verification on each download. FR-014 bans post-download network calls. No telemetry. |
| II. Real-Time Latency Budget | PASS | Model download/verification happens before pipeline starts. Not in dictation latency path. SHA-256 of 1.8 GB < 5s is well within budget. |
| III. Full Pipeline — No Fallbacks | PASS | FR-018: Pipeline MUST NOT start until all 3 models present and verified. No degraded modes. App shows download progress or failure UI until ready. |
| IV. Pure Rust / GPUI — No Web Tech | PASS | All Rust. reqwest for HTTP. dirs for paths. sha2 for hashing. No JS/HTML/CSS/WebView. |
| V. Zero-Click First Launch | PASS | Auto-download on first launch. No setup wizards, no confirmation dialogs. Progress reported via broadcast channel to overlay HUD. |
| VI. Scope Only Increases | PASS | All 18 functional requirements implemented. Model swapping (FR-016, FR-017) included. No features removed or deferred. |
| VII. Public API Documentation | PASS | All pub items will have `///` doc comments. Module-level `//!` docs on each file. |
| VIII. No Test Skipping | PASS | All tests run unconditionally. No `#[ignore]`, no conditional compilation guards. |
| IX. Explicit Commit Only | PASS | No auto-commits. |
| X. No Deferral | PASS | All requirements addressed in this plan. No items deferred to future phases. |

**Post-Design Re-check**: All gates still pass. No new dependencies introduce web tech or network calls beyond model download.

## Project Structure

### Documentation (this feature)

```text
specs/008-model-management/
├── spec.md              # Feature specification
├── plan.md              # This file
├── research.md          # Technical decisions and rationale
├── data-model.md        # Entity definitions and state transitions
├── quickstart.md        # Integration guide and test patterns
├── contracts/
│   └── models-api.md    # Public Rust API contract
├── checklists/
│   └── requirements.md  # Spec quality checklist (complete)
└── tasks.md             # (Created by /speckit.tasks)
```

### Source Code (repository root)

```text
crates/vox_core/
├── Cargo.toml                    # Add sha2, dirs dependencies
└── src/
    ├── vox_core.rs               # Module declarations (models already declared)
    ├── models.rs                 # Model registry, path resolution, ModelInfo, re-exports
    └── models/
        ├── downloader.rs         # ModelDownloader, concurrent downloads, SHA-256, progress, polling, retry
        └── format.rs             # Magic byte validation (GGUF, GGML, ONNX detection)
```

**Structure Decision**: Follows existing vox_core module pattern (`audio.rs` + `audio/capture.rs`, `vad.rs` + `vad/silero.rs`). The `models.rs` file serves as the module root with the static registry, path resolution functions, and re-exports. Submodules handle download logic (`downloader.rs`) and format validation (`format.rs`). No new crate needed — model management is a vox_core internal module alongside audio, vad, asr, llm, and pipeline.

## Complexity Tracking

No constitution violations. No complexity justifications needed.
