# Vox

Local-first intelligent voice dictation engine. Pure Rust, GPUI frontend, GPU-accelerated ML inference. Transforms speech into polished text injected into any application.

**Pipeline:** Audio Capture → Ring Buffer → Silero VAD (ONNX) → Whisper ASR → Qwen LLM post-processing → Text Injection

All processing happens on-device. Audio never leaves the machine.

## Status

Early development. The three-crate workspace compiles on both platforms.

### Implemented

- **Audio capture** — cpal input with lock-free ring buffer, rubato resampler, real-time RMS amplitude via AtomicU32
- **Voice activity detection** — Silero VAD v5 via ONNX Runtime, split pre/post padding (300ms/100ms) for soft speech onset capture
- **Speech recognition** — Whisper Large V3 Turbo via whisper.cpp, 200ms silence pre-padding, energy-based hallucination guard
- **LLM post-processing** — Qwen 2.5 3B Instruct via llama.cpp (filler removal, punctuation, course correction, number/date/email formatting, voice command detection, tone adaptation, token streaming, command misclassification guard)
- **Text injection** — OS-level keystroke simulation with voice command mapping (Windows SendInput with UIPI elevation detection, macOS CGEvent with UTF-16 chunking and AX focus detection)
- **Pipeline orchestration** — Tokio select loop, state broadcasting, transcript persistence, activation modes, dictionary substitution, generation-gated session lifecycle
- **Model management** — Registry with platform-specific directories, concurrent downloading with SHA-256 verification, atomic file writes, GGUF/GGML/ONNX format detection, per-instance model directory for test isolation
- **Application state** — VoxState as GPUI Global, JSON settings with atomic write and corrupt-file recovery, SQLite transcript history with search/delete/secure-clear, AppReadiness state machine, privacy-enforced transcript writes
- **Custom dictionary** — SQLite-backed word mappings with in-memory cache, case-insensitive whole-word substitution, LLM hint integration, use count tracking, command phrase exclusion, JSON import/export
- **GPUI application shell** — System tray with PNG icons, global hotkey dispatch, structured logging with daily rotation, async pipeline initialization loading ASR and LLM onto GPU before marking Ready
- **Overlay HUD** — Always-on-top draggable pill window with state-dependent rendering (download progress, waveform visualizer, transcript preview, injected text fade, error display, quick settings), position persistence with display bounds clamping
- **Settings window** — Full management window with sidebar navigation, configurable audio/VAD/hotkey/LLM/appearance settings, transcript history browser, dictionary editor, model status, and live log viewer

## Prerequisites

### Both Platforms
- Rust 1.85+ (`rustup update`)
- CMake 4.0+

### Windows
- Visual Studio 2022 Build Tools (C++ workload)
- CUDA Toolkit 12.8+ with cuDNN 9.x
- Environment variables (persistent user-level):
  ```
  CMAKE_GENERATOR=Visual Studio 17 2022
  CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8
  ```

### macOS
- Xcode 26.x + Command Line Tools
- Metal Toolchain: `xcodebuild -downloadComponent MetalToolchain`

## Build

```bash
# Windows (CUDA)
cargo build -p vox --features vox_core/cuda

# macOS (Metal)
cargo build -p vox --features vox_core/metal

# Tests
cargo test -p vox_core --features cuda    # Windows
cargo test -p vox_core --features metal   # macOS

# Release
cargo build --release -p vox --features vox_core/cuda
```

## Project Structure

```
assets/icons/   Icon assets
crates/
  vox/          Binary entry point
  vox_core/     Backend — audio, VAD, ASR, LLM, text injection (13 modules)
  vox_ui/       GPUI UI components — overlay, panels, controls (14 modules)
tests/          Integration tests
scripts/        Model download scripts
specs/          Feature specifications
```

## Architecture

Three-crate Cargo workspace:

- **vox** — Binary. GPUI application shell, window setup, system tray, global hotkeys.
- **vox_core** — Library. Audio pipeline, VAD, ASR, LLM, text injection, dictionary, config, state, model management. Feature-gated for `cuda` and `metal`.
- **vox_ui** — Library. GPUI UI components. Overlay HUD, settings, history, dictionary editor, model manager, log viewer.

## Target Hardware

| Platform | GPU | Backend |
|---|---|---|
| Windows | NVIDIA RTX 4090 | CUDA |
| macOS | Apple M4 Pro | Metal |

## License

MIT
