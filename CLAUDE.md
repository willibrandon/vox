# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VoxFlow is a privacy-first, locally-executed voice dictation application built with Tauri v2. It transforms speech into polished text via an on-device pipeline: audio capture → VAD → ASR → LLM post-processing → OS-level text injection. Zero cloud dependency — audio never leaves the machine.

**Status:** Pre-implementation. The design document exists at `docs/design.md` but no source code has been written yet.

## Build Commands

```bash
# Development (hot-reload frontend + backend)
cargo tauri dev --features cuda       # Windows (NVIDIA GPU)
cargo tauri dev --features metal      # macOS (Apple Silicon)

# Production build
cargo tauri build --features cuda     # Windows
cargo tauri build --features metal    # macOS

# Run tests
cargo test --features cuda            # Windows
cargo test --features metal           # macOS

# Run a single test
cargo test test_name --features cuda -- --nocapture

# Latency benchmarks
cargo test --release --features cuda benchmark_ -- --nocapture

# Frontend only (Vite dev server)
pnpm install
pnpm dev
```

## Prerequisites

- Rust 1.84+ (2025 edition), Node.js 22 LTS, pnpm 9.x, CMake 3.28+
- **Windows:** MSVC Build Tools 2022, CUDA Toolkit 12.6, cuDNN 9.x
- **macOS:** Xcode 16.x + CLI tools (Metal is automatic)

## Architecture

The pipeline is the core data flow — understand this before touching anything:

```
Hotkey → cpal audio capture → SPSC ring buffer (lock-free)
  → Silero VAD (ONNX, CPU) → speech segment chunking
  → whisper.cpp (CUDA/Metal via whisper-rs) → raw transcript
  → llama.cpp (CUDA/Metal via llama-cpp-rs) → polished text
  → OS text injection (Win32 SendInput / macOS CGEvent)
```

**Key architectural decisions:**
- Audio callback thread runs at real-time priority, communicates via lock-free SPSC ring buffer (ringbuf crate) — never block the audio thread
- Whisper runs in "chunked-batch" mode (not streaming) — VAD segments utterances, each is transcribed as a complete batch for optimal accuracy
- ASR and LLM inference run on `tokio::task::spawn_blocking` — they are CPU/GPU-bound, not async
- The LLM post-processor maintains a persistent KV cache session across calls to keep the system prompt cached
- Force-segment at 10 seconds with 1-second overlap for context continuity

## Project Structure

```
src-tauri/src/           # Rust backend (Tauri v2)
  pipeline/orchestrator  # Central coordinator — wires all components
  audio/                 # cpal capture, ring buffer, rubato resampler
  vad/                   # Silero VAD (ONNX via ort crate), speech chunker
  asr/                   # whisper-rs wrapper
  llm/                   # llama-cpp-rs post-processor, prompt templates
  injector/              # Platform-specific text injection + voice commands
  dictionary/            # Custom word dictionary (SQLite via rusqlite)
  config/                # User settings (Tauri Store plugin)
  commands.rs            # Tauri IPC command handlers
  state.rs               # Global AppState (Arc<Mutex<...>>)

src/                     # Frontend (SolidJS + TypeScript + Tailwind CSS 4)
  components/            # OverlayHud, WaveformVisualizer, SettingsPanel, etc.
  hooks/                 # useAudioState, useTranscript, useSettings
  lib/                   # Typed Tauri invoke/event wrappers

models/                  # Git-ignored, downloaded at first run (~3.5 GB)
```

## Key Cargo Feature Flags

- `cuda` — enables GPU acceleration on Windows (whisper-rs/cuda + llama-cpp-rs/cuda)
- `metal` — enables GPU acceleration on macOS (whisper-rs/metal + llama-cpp-rs/metal)
- Neither flag → CPU-only fallback

## Frontend ↔ Backend IPC

Commands (frontend → backend): `toggle_recording`, `get_state`, `update_settings`, `get_history`, `add_dictionary_word`, `list_audio_devices`, `set_audio_device`

Events (backend → frontend): `audio-state`, `audio-level`, `transcript-raw`, `transcript-polished`, `error`

## Platform-Specific Notes

- **Text injection** uses completely different OS APIs per platform — `windows-rs` SendInput on Windows, `core-graphics` CGEvent on macOS. These are in separate platform-gated modules.
- **macOS** requires manual Accessibility and Input Monitoring permissions for text injection and global hotkeys.
- The audio thread sets `THREAD_PRIORITY_TIME_CRITICAL` on Windows for real-time audio processing.

## ML Models

| Model | Purpose | Format | Size |
|---|---|---|---|
| Whisper Large V3 Turbo | ASR | ggml Q5_0 | ~1.8 GB |
| Qwen 2.5 3B Instruct | Post-processing | gguf Q4_K_M | ~1.6 GB |
| Silero VAD v5 | Voice activity detection | ONNX | ~1.1 MB |

Combined VRAM: ~5.2 GB. Models stored in platform app data directory, not in repo.

## Error Handling Philosophy

Graceful degradation chain: Full GPU pipeline → raw transcript only (LLM fails) → CPU-only (GPU fails) → error state. The LLM failing should never block text output — fall back to injecting the raw Whisper transcript.

## Spec-Kit Integration

This repo uses spec-kit for feature planning. Artifacts live in `.specify/`. Use `/speckit.specify` to create feature specs, `/speckit.plan` for implementation planning, `/speckit.tasks` for task generation.
