# Quickstart: Pipeline Orchestration

**Feature**: 007-pipeline-orchestration
**Date**: 2026-02-20

## Prerequisites

- Rust 1.85+ (2024 edition), CMake 4.0+
- Windows: Visual Studio 2022 Build Tools, CUDA 12.8+, cuDNN 9.x
- macOS: Xcode 26.x + Command Line Tools, Metal Toolchain
- Model fixtures in `crates/vox_core/tests/fixtures/`:
  - `silero_vad_v5.onnx` (2.3 MB)
  - `speech_sample.wav` (5.2 MB)
  - `ggml-large-v3-turbo-q5_0.bin` (~547 MB, gitignored)
  - `qwen2.5-3b-instruct-q4_k_m.gguf` (~1.6 GB, gitignored)

## Build

```bash
# Windows (CUDA)
cargo build -p vox_core --features cuda

# macOS (Metal)
cargo build -p vox_core --features metal
```

## Test

```bash
# Windows — all tests including pipeline
cargo test -p vox_core --features cuda

# macOS
cargo test -p vox_core --features metal

# Single test with output
cargo test -p vox_core test_pipeline_hello_world --features cuda -- --nocapture
```

## Files Modified/Created by This Feature

### New files
- `crates/vox_core/src/pipeline.rs` → Module root with submodule declarations
- `crates/vox_core/src/pipeline/orchestrator.rs` → Pipeline struct and processing loop
- `crates/vox_core/src/pipeline/controller.rs` → PipelineController and ActivationMode
- `crates/vox_core/src/pipeline/state.rs` → PipelineState enum and broadcasting
- `crates/vox_core/src/pipeline/transcript.rs` → TranscriptEntry and TranscriptStore
- `crates/vox_core/src/dictionary.rs` → DictionaryCache (replaces empty stub)

### Modified files
- `crates/vox_core/src/vox_core.rs` → Update pipeline module declaration (file → directory)
- `crates/vox_core/src/injector.rs` → Add `get_focused_app_name()` public function
- `crates/vox_core/src/injector/windows.rs` → Add `get_focused_app_name_impl()`
- `crates/vox_core/src/injector/macos.rs` → Add `get_focused_app_name_impl()`
- `crates/vox_core/src/audio/capture.rs` → Add `take_consumer()` method
- `crates/vox_core/src/vad.rs` → Change `try_send` to `blocking_send` in `run_vad_loop()` (2 call sites) to prevent segment drops (FR-017)

### No changes needed
- `crates/vox_core/Cargo.toml` — All dependencies already declared (tokio, rusqlite, uuid, parking_lot)
- `Cargo.toml` (workspace) — No new workspace deps needed. Verified: `parking_lot = "0.12"` already declared in workspace Cargo.toml, and `parking_lot.workspace = true` already in vox_core's Cargo.toml.

## Architecture Overview

```
cpal audio callback ──► ring buffer ──► VAD thread ──► segment channel
                                                              │
                                                              ▼
                                              Pipeline async loop (tokio)
                                                │         │        │
                                         spawn_blocking  dict   spawn_blocking
                                           (ASR)       lookup    (LLM)
                                                                   │
                                                              inject/execute
                                                                   │
                                                              broadcast state
                                                                   │
                                                              save transcript
```

## Key Design Decisions

1. **VAD on std::thread, ASR/LLM via spawn_blocking** — SileroVad is NOT Send (ort::Session), so it runs on a dedicated thread. ASR and LLM are Clone+Send+Sync, safe for tokio's blocking pool.

2. **Pipeline doesn't own AudioCapture** — AudioCapture is NOT Send. Pipeline::start() takes an owned `HeapCons<f32>` consumer and native sample rate. AudioCapture stays on the caller's thread.

3. **Command channel for controller** — PipelineController communicates with Pipeline::run() via `mpsc::channel<PipelineCommand>`, not `&mut Pipeline`. Pipeline::run() uses `tokio::select!` on segments + commands. This avoids aliasing `&mut self` between run() and hotkey handlers.

4. **blocking_send for segment delivery** — run_vad_loop() uses `blocking_send` (not `try_send`) to guarantee no segment drops under backpressure (FR-017).

5. **Batch LLM delivery** — Full LLM output before inject/execute. Required for voice command classification (LLM must see complete output to decide text vs. command).

6. **SQLite for persistence** — Dictionary and transcripts stored in SQLite (rusqlite 0.38, already in deps). Single database file in app data directory.

7. **broadcast for state** — `tokio::sync::broadcast` with capacity 16. Latest-wins semantics for slow subscribers. No polling.
