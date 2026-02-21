# Implementation Plan: Pipeline Orchestration

**Branch**: `007-pipeline-orchestration` | **Date**: 2026-02-20 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/007-pipeline-orchestration/spec.md`

## Summary

Wire all existing pipeline components (Audio Capture, VAD, ASR, LLM, Text Injection) into a coordinated async system that transforms speech into polished text injected into any focused application. The pipeline uses a three-tier threading model: cpal OS audio thread → dedicated VAD processing thread (std::thread) → tokio async orchestrator with spawn_blocking for GPU-bound ASR/LLM work. The VAD thread delivers segments via `blocking_send` (not `try_send`) to guarantee no drops under backpressure. State changes are broadcast to UI subscribers via tokio::sync::broadcast. PipelineController communicates with the Pipeline's async run loop via an mpsc command channel (avoiding `&mut` aliasing between the long-running run loop and hotkey handlers). New subsystems include DictionaryCache (two-pass substitution: phrase replacement then word-level HashMap, both loaded from SQLite), TranscriptStore (SQLite persistence with 30-day auto-prune), PipelineController (hotkey-to-command translation for three activation modes), and focused app name detection (platform-specific, for LLM tone adaptation).

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+), CMake 4.0+
**Primary Dependencies**: tokio 1.49 (rt-multi-thread, sync, time, macros), rusqlite 0.38 (bundled), uuid 1 (v4, serde), cpal 0.17, ringbuf 0.4, ort 2.0.0-rc.11, whisper-rs 0.15.1, llama-cpp-2 0.1, windows 0.62, objc2 0.6
**Storage**: SQLite via rusqlite 0.38 (bundled) for dictionary and transcript persistence. Single database file in app data directory.
**Testing**: `cargo test -p vox_core --features cuda` (Windows) / `--features metal` (macOS). Inline `#[cfg(test)]` modules. 71 existing tests, all unconditional (Constitution Principle VIII).
**Target Platform**: Windows (CUDA/RTX 4090), macOS (Metal/M4 Pro)
**Project Type**: Three-crate Rust workspace — `vox` (binary), `vox_core` (backend), `vox_ui` (GPUI frontend)
**Performance Goals**: End-to-end latency < 300ms (RTX 4090), < 750ms (M4 Pro). Combined VRAM < 6 GB. System RAM < 500 MB. CPU < 2% idle, < 15%/20% active.
**Constraints**: All processing local-only (no network). All components required before pipeline starts. Zero compiler warnings. All tests run unconditionally.
**Scale/Scope**: Single-user desktop application. Pipeline processes one segment at a time in FIFO order. Transcript history pruned after 30 days.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Evidence |
|---|-----------|--------|----------|
| I | Local-Only Processing | PASS | All processing on-device. No network calls. Audio, ASR, LLM, injection all local. SQLite for persistence (local file). |
| II | Real-Time Latency Budget | PASS | ASR ~35ms + LLM ~215ms = ~250ms on RTX 4090 (within 300ms budget). VAD on dedicated thread, ASR/LLM on spawn_blocking — no blocking on audio callback. |
| III | Full Pipeline — No Fallbacks | PASS | Pipeline::new() requires all components. Pipeline::start() fails if any component unavailable. No degraded modes. FR-002 enforces this. |
| IV | Pure Rust / GPUI — No Web Tech | PASS | All new code is Rust. SQLite via rusqlite (Rust bindings, bundled C). No web dependencies introduced. |
| V | Zero-Click First Launch | PASS | No manual setup steps added. Dictionary starts empty (no config needed). Transcript database auto-created. |
| VI | Scope Only Increases | PASS | All 21 functional requirements from spec implemented. No features removed or deferred. DictionaryCache and get_focused_app_name() are new scope additions (required by FR-020, FR-013). |
| VII | Public API Documentation | PASS | All pub items will have /// doc comments. Module-level //! docs on all modules. |
| VIII | No Test Skipping | PASS | All tests run unconditionally. No #[ignore], no #[cfg(skip)], no conditional compilation. Integration tests use fixture models present in dev environment. |
| IX | Explicit Commit Only | PASS | No git operations in implementation. Commits only on user instruction. |
| X | No Deferral | PASS | All research questions resolved in research.md. All unknowns addressed. No items deferred to later phases. |

**Post-Phase 1 re-check**: All gates still pass. No Constitution violations introduced by the design.

## Project Structure

### Documentation (this feature)

```text
specs/007-pipeline-orchestration/
├── spec.md
├── plan.md                 # This file
├── research.md             # Phase 0: threading model, ownership, error recovery
├── data-model.md           # Phase 1: entity definitions, DB schema
├── quickstart.md           # Phase 1: build/test commands, file inventory
├── contracts/              # Phase 1: Rust API contracts
│   ├── pipeline.md         # Pipeline, PipelineState, PipelineController, ActivationMode
│   ├── dictionary.md       # DictionaryCache, DictionaryEntry
│   ├── transcript.md       # TranscriptStore, TranscriptEntry
│   └── focused_app.md      # get_focused_app_name()
├── checklists/
│   └── requirements.md     # Specification quality checklist (26/26 passing)
└── tasks.md                # Phase 2 output (generated by /speckit.tasks)
```

### Source Code (repository root)

```text
crates/vox_core/src/
├── vox_core.rs              # Library root (update: pipeline module → directory)
├── audio/
│   ├── audio.rs             # Module root (unchanged)
│   ├── capture.rs           # AudioCapture (MODIFIED: add take_consumer())
│   ├── ring_buffer.rs       # AudioRingBuffer (unchanged)
│   └── resampler.rs         # AudioResampler (unchanged)
├── vad/
│   ├── vad.rs               # run_vad_loop() (MODIFIED: try_send → blocking_send), VadStateMachine
│   ├── silero.rs            # SileroVad (unchanged)
│   └── chunker.rs           # SpeechChunker (unchanged)
├── asr/
│   ├── asr.rs               # AsrEngine (unchanged)
│   └── stitcher.rs          # stitch_segments() (unchanged)
├── llm/
│   ├── llm.rs               # Module root (unchanged)
│   ├── processor.rs         # PostProcessor (unchanged)
│   └── prompts.rs           # System prompt, builders (unchanged)
├── injector/
│   ├── injector.rs          # MODIFIED: add get_focused_app_name()
│   ├── windows.rs           # MODIFIED: add get_focused_app_name_impl()
│   ├── macos.rs             # MODIFIED: add get_focused_app_name_impl()
│   └── commands.rs          # Voice command dispatch (unchanged)
├── pipeline.rs              # Module root (replaces empty stub; declares submodules)
├── pipeline/                # NEW DIRECTORY for pipeline submodules
│   ├── orchestrator.rs      # Pipeline struct, start/stop/run
│   ├── controller.rs        # PipelineController, ActivationMode
│   ├── state.rs             # PipelineState enum
│   └── transcript.rs        # TranscriptEntry, TranscriptStore
├── dictionary.rs            # NEW CONTENT (replaces empty stub): DictionaryCache
├── config.rs                # Stub (unchanged — settings persistence is minimal)
├── models.rs                # Stub (unchanged — model download is a separate feature)
├── hotkey.rs                # Stub (unchanged — hotkey registration is a separate feature)
└── state.rs                 # Stub (unchanged — UI state is a separate feature)
```

**Structure Decision**: Existing three-crate workspace architecture is preserved. All new pipeline code lives in `crates/vox_core/src/pipeline/` (expanded from the empty `pipeline.rs` stub). The dictionary module fills the existing empty `dictionary.rs` stub. Injector module is extended (not restructured) with focused app name detection. No new crates, no new workspace members.

## Complexity Tracking

> No Constitution violations. Table intentionally empty.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (none) | — | — |
