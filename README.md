# VoxFlow

Privacy-first voice dictation that runs entirely on your machine. Speak naturally, get polished text in any application.

VoxFlow captures audio, detects speech, transcribes it with Whisper, cleans it up with a local LLM, and types the result into whatever app has focus. No cloud, no network calls, no data leaves your device.

**Status:** Pre-implementation — the [design document](docs/design.md) is complete but no source code exists yet.

## How It Works

```
Hotkey → Audio Capture → VAD → Whisper ASR → LLM Post-Processing → Text Injection
```

1. Press a hotkey to start dictating
2. Silero VAD detects when you're speaking and segments utterances
3. whisper.cpp transcribes each segment on the GPU
4. Qwen 2.5 3B cleans up filler words, fixes punctuation, handles corrections
5. The polished text is typed into the focused application via OS-level keyboard simulation

End-to-end latency: ~165ms on RTX 4090, ~430ms on M4 Pro.

## Target Hardware

| Machine | GPU | VRAM |
|---|---|---|
| Windows Desktop | NVIDIA RTX 4090 (CUDA) | 24 GB |
| macOS Laptop | Apple M4 Pro (Metal) | 24 GB unified |

## Prerequisites

- Rust 1.84+ (2025 edition)
- Node.js 22 LTS
- pnpm 9.x
- CMake 3.28+

**Windows:** MSVC Build Tools 2022, CUDA Toolkit 12.6, cuDNN 9.x

**macOS:** Xcode 16.x + Command Line Tools

## Getting Started

```bash
# Install frontend dependencies
pnpm install

# Development with hot-reload
cargo tauri dev --features cuda       # Windows
cargo tauri dev --features metal      # macOS

# Production build
cargo tauri build --features cuda     # Windows
cargo tauri build --features metal    # macOS
```

On first launch, VoxFlow downloads ~3.5 GB of ML models (one-time).

## ML Models

| Model | Purpose | Size |
|---|---|---|
| Whisper Large V3 Turbo (Q5_0) | Speech recognition | ~1.8 GB |
| Qwen 2.5 3B Instruct (Q4_K_M) | Text post-processing | ~1.6 GB |
| Silero VAD v5 (ONNX) | Voice activity detection | ~1.1 MB |

Combined VRAM usage: ~5.2 GB.

## Project Structure

```
src-tauri/src/           Rust backend (Tauri v2)
  pipeline/              Pipeline orchestrator
  audio/                 cpal capture, SPSC ring buffer, resampler
  vad/                   Silero VAD (ONNX), speech chunker
  asr/                   whisper-rs wrapper
  llm/                   llama-cpp-rs post-processor
  injector/              OS-level text injection (Win32 / macOS)
  dictionary/            Custom word dictionary (SQLite)
  config/                User settings

src/                     Frontend (SolidJS + TypeScript + Tailwind CSS 4)
  components/            Overlay HUD, settings, transcript history
  hooks/                 Tauri event subscriptions
  lib/                   Typed IPC wrappers
```

## Testing

```bash
# Run all tests
cargo test --features cuda            # Windows
cargo test --features metal           # macOS

# Single test
cargo test test_name --features cuda -- --nocapture

# Latency benchmarks
cargo test --release --features cuda benchmark_ -- --nocapture
```

## Features

- **Push-to-talk, toggle, and hands-free modes** — pick the workflow that suits you
- **Voice commands** — "delete that", "new line", "undo" are executed as keyboard shortcuts
- **Course correction** — say "Tuesday, wait no, Wednesday" and only "Wednesday" makes it through
- **Custom dictionary** — teach VoxFlow proper nouns, technical terms, and shortcuts
- **Graceful degradation** — if the LLM fails, raw transcript is injected; if the GPU fails, falls back to CPU

## License

MIT
