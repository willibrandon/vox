# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Vox — local-first intelligent voice dictation engine. Pure Rust, GPUI frontend, GPU-accelerated ML inference. Transforms speech into polished text injected into any application.

Pipeline: Audio Capture (cpal) → Ring Buffer → Silero VAD (ONNX) → Whisper ASR (whisper.cpp) → Qwen LLM post-processing (llama.cpp) → Text Injection (OS-level keystroke simulation).

Design document: `docs/design.md`. Constitution: `.specify/memory/constitution.md`.

## Constitution (All Principles Are Non-Negotiable)

Every change must comply with these 6 principles. Violations are rejected.

1. **Local-Only Processing** — All audio/ML processing on-device. No network calls except model download. No telemetry. SHA-256 checksum verification on downloaded models.
2. **Real-Time Latency Budget** — End-to-end < 300ms (RTX 4090), < 750ms (M4 Pro). No blocking on audio callback thread. ML inference on processing/GPU threads only.
3. **Full Pipeline — No Fallbacks** — VAD + ASR + LLM + Text Injection all required. No degraded modes, no optional components, no CPU fallbacks. Pipeline does not start until all components are loaded.
4. **Pure Rust / GPUI — No Web Tech** — No JavaScript, TypeScript, HTML, CSS, WebView, Node.js. Single static binary. UI calls Rust functions directly, no IPC serialization.
5. **Zero-Click First Launch** — Models auto-download concurrently on first launch. No setup wizards, no confirmation dialogs. Hotkey responds in every app state.
6. **Scope Only Increases** — No feature may be removed, deferred, made optional, deprioritized, or marked as a future version goal. Only scope increases are permitted. If it's in the design doc, it gets implemented.

## Performance Budgets (Binding)

| Resource | RTX 4090 | M4 Pro |
|---|---|---|
| End-to-end latency | < 300 ms | < 750 ms |
| VRAM / Unified Memory | < 6 GB | < 6 GB |
| System RAM | < 500 MB | < 500 MB |
| CPU (idle / active) | < 2% / < 15% | < 2% / < 20% |
| Binary size (excl. models) | < 15 MB | < 15 MB |
| Incremental build | < 10 s | < 10 s |

## Build Commands

```bash
# Development
cargo run -p vox --features vox_core/cuda     # Windows (CUDA)
cargo run -p vox --features vox_core/metal     # macOS (Metal)

# Tests
cargo test -p vox_core --features cuda         # Windows
cargo test -p vox_core --features metal        # macOS

# Single test
cargo test -p vox_core test_name --features cuda -- --nocapture

# Release build
cargo build --release -p vox --features vox_core/cuda
```

Zero warnings required. `#[allow(...)]` only with justifying comment.

## Build Prerequisites

- Rust 1.85+ (2024 edition), CMake 4.0+
- **Windows**: Visual Studio 2022 Build Tools, CUDA 12.8+, cuDNN 9.x
- **Windows CUDA gotcha**: CUDA doesn't support VS 18 Insiders. `CMAKE_GENERATOR` must be set to `Visual Studio 17 2022`. Both `CMAKE_GENERATOR` and `CUDA_PATH` are persistent user env vars.
- **macOS**: Xcode 26.x + Command Line Tools. Metal is automatic.
- No Node.js, pnpm, or any web toolchain.

## Architecture

Three-crate workspace:

- **`crates/vox/`** — Binary entry point. GPUI Application, window setup, system tray (`tray-icon`), global hotkeys (`global-hotkey`).
- **`crates/vox_core/`** — Backend. Audio pipeline, VAD, ASR, LLM, text injection, dictionary, settings, model download. Feature-gated: `cuda` and `metal`.
- **`crates/vox_ui/`** — GPUI UI components. Overlay HUD, settings panel, history, dictionary editor, model manager, log viewer.

GPUI patterns (from Zed): `Entity<T>` for state, `Render` trait for views, `cx.set_global()` for app-wide state, `div()` builder API, `Action` trait for keybindings.

## Pinned Dependency Versions

These are verified compatible. Using wrong versions will cause compile failures or runtime bugs.

| Crate | Version | Critical Notes |
|---|---|---|
| gpui | git (zed-industries/zed) | Pin to specific rev |
| cpal | 0.17 | `SampleRate` is `u32`. `device.description()` returns `DeviceDescription` struct — use `.name()`. Auto RT priority. |
| ringbuf | 0.4 | `occupied_len()` on `Observer` trait — must `use ringbuf::traits::Observer` |
| rubato | 1.0 | Major API redesign from 0.16. Use `AudioAdapter` trait + `SequentialSliceOfVecs` |
| ort | 2.0.0-rc.11 | RC but production-ready |
| whisper-rs | 0.15.1 | Codeberg (not crates.io). Flash attn disabled. `full_n_segments()` returns `c_int` not Result |
| llama-cpp-2 | 0.1 (utilityai) | **NOT `llama-cpp-rs` 0.4** — completely different crate. Types nested: `model::LlamaModel`. `load_from_file` needs `&LlamaBackend` first arg |
| windows | 0.62 | Win32 SendInput. Can't inject into elevated processes (UIPI) |
| objc2 | 0.6 | **NOT Servo `core-graphics`** (heading toward deprecation). Use `objc2-core-graphics` 0.3 |
| rusqlite | 0.38 | No `FromSql` for `chrono::DateTime<Utc>` — use `String` (ISO 8601) for timestamps |
| tokio | 1.49 | — |
| reqwest | 0.13 | rustls default. `query`/`form` features are opt-in |

## Thread Safety

- `WhisperContext` is **NOT** thread-safe → wrap in `Arc<Mutex<>>`. Create new `WhisperState` per transcription.
- `LlamaModel` is `Send+Sync` → `Arc`. `LlamaContext` is **NOT** → one per inference call.
- cpal audio callback is real-time — no allocations, no locks, no ML. Resampling on processing thread.
- macOS `CGEvent` has undocumented 20-char limit per call — must chunk text.

## Commit Style

Use `/vox.commit` command. Conventional commits (`type(scope): message`), imperative mood, no emojis, no AI attribution, no words like "comprehensive/robust/enhance/streamline/leverage".

## Spec-Kit Workflow

Feature specs live in `specs/NNN-feature-name/`. Commands: `/speckit.specify` → `/speckit.plan` → `/speckit.tasks` → `/speckit.implement`. Every plan must pass a Constitution Check against all 6 principles before implementation begins.
