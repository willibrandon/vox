# Vox — Design Document

**Project:** Vox — Local-First Intelligent Voice Dictation Engine
**Version:** 2.0.0
**Date:** February 19, 2026
**Status:** Draft

---

## 1. Executive Summary

Vox is a privacy-first, locally-executed voice dictation application that transforms natural speech into polished, context-aware text in any application. It combines real-time speech recognition, intelligent post-processing via a local LLM, and universal text injection to deliver a Wispr Flow-class experience with zero cloud dependency.

The UI is built with GPUI — Zed's GPU-accelerated Rust UI framework. No web tech, no JavaScript, no bundled browser engine. Pure Rust from audio capture to pixel output.

### 1.1 Core Value Proposition

- All processing happens on-device. Audio never leaves the machine.
- Sub-500ms end-to-end latency from speech to rendered text.
- Intelligent post-processing: filler word removal, punctuation, tone adaptation, course correction.
- Works in every text field on the OS — editors, browsers, terminals, chat apps, IDEs.
- Cross-platform: Windows (RTX 4090 / CUDA) and macOS (M4 Pro / Metal).

### 1.2 Target Hardware

| Machine | OS | GPU/Accelerator | VRAM/Unified | Role |
|---|---|---|---|---|
| Desktop | Windows 11 | NVIDIA RTX 4090 | 24 GB GDDR6X | Primary dev, max performance |
| Laptop | macOS 26 Tahoe | Apple M4 Pro (16-core GPU) | 24 GB unified | Mobile dev, daily driver |

### 1.3 Non-Goals

- Mobile (iOS/Android) — desktop only.
- Cloud/hybrid mode — strictly local.
- Speaker diarization — single-user dictation only.
- Real-time translation — English only.

---

## 2. Architecture Overview

```
┌────────────────────────────────────────────────────────────────────────┐
│                     Vox Application (Pure Rust)                        │
│                                                                        │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────────┐   │
│  │  System Tray  │  │  Global      │  │     Global AppState        │   │
│  │  (tray-icon)  │  │  Hotkeys     │  │  (Entity via cx.set_global)│   │
│  └──────┬───────┘  └──────┬───────┘  └──────────────┬─────────────┘   │
│         │                 │                          │                  │
│  ┌──────▼─────────────────▼──────────────────────────▼──────────────┐  │
│  │                   Audio Pipeline (Rust)                           │  │
│  │  ┌──────────┐   ┌───────────┐   ┌────────────┐   ┌──────────┐  │  │
│  │  │  cpal    │──▶│ Ring Buf  │──▶│ Silero VAD │──▶│ Chunker  │  │  │
│  │  │ (capture)│   │ (16kHz)   │   │  (ONNX RT) │   │          │  │  │
│  │  └──────────┘   └───────────┘   └────────────┘   └────┬─────┘  │  │
│  └─────────────────────────────────────────────────────────┼────────┘  │
│                                                            │           │
│  ┌─────────────────────────────────────────────────────────▼────────┐  │
│  │                   ASR Engine (C FFI)                              │  │
│  │  ┌───────────────────────────────────────────────────────────┐   │  │
│  │  │  whisper.cpp  (CUDA on Windows / Metal on macOS)          │   │  │
│  │  │  Model: Whisper Large V3 Turbo (ggml-large-v3-turbo-q5)  │   │  │
│  │  └───────────────────────────────┬───────────────────────────┘   │  │
│  └──────────────────────────────────┼───────────────────────────────┘  │
│                                     │ raw transcript                   │
│  ┌──────────────────────────────────▼───────────────────────────────┐  │
│  │                   LLM Post-Processor (C FFI)                      │  │
│  │  ┌───────────────────────────────────────────────────────────┐   │  │
│  │  │  llama.cpp  (CUDA on Windows / Metal on macOS)            │   │  │
│  │  │  Model: Qwen 2.5 3B Instruct (Q4_K_M)                    │   │  │
│  │  └───────────────────────────────┬───────────────────────────┘   │  │
│  └──────────────────────────────────┼───────────────────────────────┘  │
│                                     │ polished text                    │
│  ┌──────────────────────────────────▼───────────────────────────────┐  │
│  │                   Text Injector                                    │  │
│  │  Windows: SendInput (Win32 API via windows 0.62)                  │  │
│  │  macOS:   CGEvent (objc2 0.6 + objc2-core-graphics 0.3)          │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                                                        │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                  GPUI Frontend (GPU-accelerated Rust)              │  │
│  │  Overlay HUD · Settings Panel · Transcript History · Dictionary   │  │
│  │  Waveform Visualizer · Model Manager · Log Viewer                 │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

### 2.1 Why GPUI

| Property | GPUI | Tauri |
|---|---|---|
| Build time (incremental) | ~3–10s | ~60–300s+ (Tauri + WebView + Vite + Node) |
| Language | Pure Rust | Rust + TypeScript + HTML/CSS |
| Rendering | GPU-accelerated (Metal/Vulkan/DX12) | OS WebView (Chromium/WebKit) |
| IPC overhead | Zero — direct Rust function calls | JSON serialization across process boundary |
| Dependencies | Cargo only | Cargo + Node + pnpm + Vite + WebView2 runtime |
| Binary size | Single static binary | Binary + bundled web assets |
| Battle-tested | Powers Zed editor (millions of users) | Broad ecosystem but plugin compatibility issues |

---

## 3. Technology Stack

### 3.1 Core Runtime

| Layer | Technology | Version | Rationale |
|---|---|---|---|
| UI Framework | GPUI | git (zed-industries/zed) | GPU-accelerated, pure Rust, hybrid immediate/retained mode, powers Zed |
| Language | Rust | 1.85+ (2024 edition) | Zero-cost abstractions, FFI to C/C++, fearless concurrency |
| Async Runtime | Tokio | 1.49 | Background ML inference, model downloads, file I/O |
| Build | Cargo | — | Single toolchain, no Node/pnpm/Vite |

### 3.2 Audio & ML

| Component | Technology | Version | Rationale |
|---|---|---|---|
| Audio Capture | cpal | 0.17 | Cross-platform audio I/O, WASAPI/CoreAudio. SampleRate is u32, auto RT priority |
| Ring Buffer | ringbuf | 0.4 | Lock-free SPSC, zero allocation after init |
| Resampler | rubato | 1.0 | High-quality async resampling, AudioAdapter trait API |
| VAD | Silero VAD v5 | ONNX | 1.1 MB, sub-ms per frame, best open-source VAD, runs on CPU |
| ONNX Runtime | ort | 2.0.0-rc.11 | Official ONNX Runtime Rust bindings, hardware-agnostic inference |
| ASR | whisper.cpp | via whisper-rs | C/C++, CUDA + Metal, ggml quantized models, battle-tested |
| ASR Bindings | whisper-rs | 0.15.1 | Safe Rust bindings. Codeberg. Flash attn disabled by default |
| LLM | llama.cpp | via llama-cpp-2 | C/C++, CUDA + Metal, ggml quantized models |
| LLM Bindings | llama-cpp-2 | 0.1 (utilityai) | Safe Rust bindings. NOT llama-cpp-rs 0.4 — different crate entirely |
| ASR Model | Whisper Large V3 Turbo | ggml Q5_0 | 809M params, 6x faster than V3, ~3 GB VRAM |
| LLM Model | Qwen 2.5 3B Instruct | gguf Q4_K_M | 3B params, ~2.2 GB VRAM, excellent instruction following |

### 3.3 Platform Integration

| Component | Windows | macOS |
|---|---|---|
| Text Injection | `windows` 0.62 (SendInput) | `objc2` 0.6 + `objc2-core-graphics` 0.3 (CGEvent) |
| Global Hotkeys | `global-hotkey` crate | `global-hotkey` crate |
| System Tray | `tray-icon` crate | `tray-icon` crate |
| GPU Acceleration | CUDA 12.8+ | Metal (automatic via ggml) |
| Audio Backend | WASAPI (via cpal) | CoreAudio (via cpal) |

### 3.4 Storage & Utilities

| Component | Technology | Version | Rationale |
|---|---|---|---|
| Database | rusqlite | 0.38 (bundled) | Dictionary, transcript history, settings, workspace state |
| HTTP | reqwest | 0.13 | Model downloads. rustls default, query/form features opt-in |
| Serialization | serde + serde_json | 1.x | Settings, state persistence |
| Logging | tracing + tracing-subscriber | 0.1 / 0.3 | Structured logging with env-filter |
| Error handling | anyhow | 1.x | Application-level errors |

---

## 4. Detailed Component Design

### 4.1 Audio Capture Pipeline

The audio pipeline is the heartbeat of the system. It must be lock-free on the hot path and deliver 16 kHz mono f32 PCM to the VAD and ASR engine with minimal latency.

#### 4.1.1 Capture Configuration

```rust
pub struct AudioConfig {
    pub sample_rate: u32,        // 16_000 Hz (whisper.cpp native)
    pub channels: u16,           // 1 (mono)
    pub sample_format: SampleFormat, // F32
    pub device: Option<String>,  // None = system default input
}
```

**cpal 0.17 notes:**
- `SampleRate` is a bare `u32`, not a struct.
- `BufferSize::Default` defers to host defaults — do not set manually.
- `device.description()` returns `DeviceDescription` struct — use `.name()` for display.
- cpal handles real-time thread priority automatically. Do not set it manually.

#### 4.1.2 Ring Buffer Design

Single-producer single-consumer (SPSC) lock-free ring buffer between the audio callback thread and the processing thread.

```
Audio Callback Thread          Processing Thread
        │                              │
        ▼                              ▼
  ┌───────────┐    SPSC Ring     ┌───────────┐
  │ cpal      │───(lock-free)───▶│ VAD +     │
  │ callback  │   64KB buffer    │ Chunker   │
  └───────────┘                  └───────────┘
```

**Buffer sizing:** 64 KB ring = ~2 seconds of 16 kHz mono f32 audio. Provides headroom for processing jitter without dropping samples.

**ringbuf 0.4 note:** `occupied_len()` lives on the `Observer` trait (parent of `Consumer`). Must `use ringbuf::traits::Observer`.

#### 4.1.3 Resampling

If the system default input device does not natively support 16 kHz, we resample using rubato 1.0. Resampling happens on the **processing thread**, not the audio callback (to avoid blocking the real-time audio thread).

**rubato 1.0 note:** Use the `AudioAdapter` trait with `SequentialSliceOfVecs` adapter. The old vector-of-vectors API from 0.16 is gone.

### 4.2 Voice Activity Detection (VAD)

Silero VAD v5 is the gatekeeper. It determines when the user is speaking and segments the audio stream into utterances.

#### 4.2.1 VAD Parameters

```rust
pub struct VadConfig {
    pub threshold: f32,           // 0.5 — speech probability threshold
    pub min_speech_ms: u32,       // 250 — minimum speech duration
    pub min_silence_ms: u32,      // 500 — silence to end an utterance
    pub max_speech_ms: u32,       // 30_000 — force-segment long speech
    pub speech_pad_ms: u32,       // 100 — padding around detected speech
    pub window_size_samples: u32, // 512 — Silero expects 512 at 16kHz
}
```

#### 4.2.2 Streaming State Machine

```
                    speech_prob >= threshold
    ┌─────────┐  ──────────────────────────▶  ┌───────────┐
    │  SILENT  │                               │ SPEAKING  │
    └─────────┘  ◀──────────────────────────  └───────────┘
                  silence_duration >= min_silence_ms
                         │
                         ▼
                 ┌───────────────┐
                 │ EMIT SEGMENT  │──▶ ASR Engine
                 └───────────────┘
```

The VAD runs on every 512-sample window (32ms). When a transition from SPEAKING → SILENT occurs and `min_silence_ms` is exceeded, the accumulated speech buffer is dispatched to the ASR engine. If `max_speech_ms` is reached while still speaking, we force-segment to prevent unbounded memory growth.

#### 4.2.3 ONNX Runtime Integration

```rust
use ort::{Session, SessionBuilder, Value};

pub struct SileroVad {
    session: Session,
    state: Vec<f32>,  // hidden state, carried across calls
    sample_rate: i64,
}

impl SileroVad {
    pub fn new(model_path: &Path) -> Result<Self> {
        let session = SessionBuilder::new()?
            .with_intra_threads(1)?
            .with_model_from_file(model_path)?;
        Ok(Self {
            session,
            state: vec![0.0; 2 * 1 * 128], // 2 layers, 1 batch, 128 hidden
            sample_rate: 16000,
        })
    }

    pub fn process(&mut self, audio: &[f32]) -> Result<f32> {
        // Returns speech probability [0.0, 1.0]
        let input = Value::from_array(([1, audio.len()], audio))?;
        let sr = Value::from_array(([], &[self.sample_rate]))?;
        let h = Value::from_array(([2, 1, 128], &self.state))?;
        let outputs = self.session.run(ort::inputs![input, sr, h]?)?;
        let prob = outputs[0].extract_tensor::<f32>()?[[0, 0]];
        // Update hidden state from output
        let new_h = outputs[1].extract_tensor::<f32>()?;
        self.state.copy_from_slice(new_h.as_slice().unwrap());
        Ok(prob)
    }
}
```

### 4.3 ASR Engine (whisper.cpp)

#### 4.3.1 Model Selection Rationale

| Model | Params | VRAM | RTFx (4090) | RTFx (M4 Pro) | WER | Languages |
|---|---|---|---|---|---|---|
| Whisper Large V3 | 1.55B | ~10 GB | ~50x | ~15x | 7.4% | 99+ |
| **Whisper Large V3 Turbo** | **809M** | **~3 GB** | **~300x** | **~80x** | **~8%** | **99+** |
| Whisper Medium | 769M | ~5 GB | ~100x | ~30x | 9.2% | 99+ |

**Choice: Whisper Large V3 Turbo (Q5_0 quantized).** 6x speed improvement over V3 with only ~1% WER degradation. On the RTX 4090, a 10-second utterance completes in ~33ms. On M4 Pro with Metal, ~125ms.

#### 4.3.2 whisper-rs Integration

```rust
use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};
use std::sync::{Arc, Mutex};

pub struct AsrEngine {
    // WhisperContext is NOT thread-safe — must wrap in Arc<Mutex<>>
    ctx: Arc<Mutex<WhisperContext>>,
}

impl AsrEngine {
    pub fn new(model_path: &Path, use_gpu: bool) -> Result<Self> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        // Flash attention disabled by default in 0.15.1
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap(),
            params,
        )?;
        Ok(Self { ctx: Arc::new(Mutex::new(ctx)) })
    }

    pub fn transcribe(&self, audio_pcm: &[f32]) -> Result<String> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_no_speech_thold(0.6);
        params.set_suppress_non_speech_tokens(true);
        params.set_single_segment(true);
        params.set_no_context(true);
        params.set_n_threads(4);

        let ctx = self.ctx.lock().unwrap();
        // Create new WhisperState per transcription — do not reuse
        let mut state = ctx.create_state()?;
        state.full(params, audio_pcm)?;

        // full_n_segments() returns c_int, NOT Result
        let n_segments = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n_segments {
            // Segment text via state.get_segment(i).to_str_lossy()
            let segment_text = state.full_get_segment_text(i)?;
            text.push_str(&segment_text);
        }
        Ok(text.trim().to_string())
    }
}
```

#### 4.3.3 Streaming Strategy

We do **not** use Whisper in true streaming mode (which degrades accuracy). Instead, we use a "chunked-batch" approach:

1. VAD detects speech segments (typically 1–10 seconds).
2. Each segment is transcribed as a complete batch.
3. Partial results shown from VAD state (waveform indicator).
4. Final text appears when the segment completes.

For longer continuous speech, force-segment at 10 seconds and stitch results with 1-second overlap for context continuity.

### 4.4 LLM Post-Processor

The raw transcript from Whisper is good but not polished. The LLM handles:

1. **Filler word removal** — "um", "uh", "like", "you know"
2. **Punctuation and capitalization** — Whisper's punctuation is decent but inconsistent
3. **Course correction** — "let's meet Tuesday, wait no, Wednesday" → "let's meet Wednesday"
4. **Formatting** — numbers, dates, email addresses, code identifiers
5. **Tone adaptation** — adjust formality based on active application
6. **Command detection** — "delete that", "new line", "select all" → OS actions

#### 4.4.1 Model Selection

| Model | Params | VRAM (Q4_K_M) | Tok/s (4090) | Tok/s (M4 Pro) | Quality |
|---|---|---|---|---|---|
| Phi-3.5 Mini | 3.8B | ~2.5 GB | ~120 | ~45 | Good |
| **Qwen 2.5 3B Instruct** | **3B** | **~2.2 GB** | **~150** | **~55** | **Excellent** |
| Llama 3.2 3B | 3B | ~2.2 GB | ~140 | ~50 | Good |
| Gemma 2 2B | 2.6B | ~1.8 GB | ~170 | ~65 | Decent |

**Choice: Qwen 2.5 3B Instruct (Q4_K_M).** Best instruction-following at the 3B tier. Combined VRAM: ~3 GB (Whisper) + ~2.2 GB (Qwen) = **~5.2 GB**. Leaves 18+ GB free on both machines.

#### 4.4.2 System Prompt

```
You are a dictation post-processor. Your ONLY job is to clean up speech-to-text output.

Rules:
1. Remove filler words (um, uh, like, you know, basically, literally, so, I mean).
2. Fix punctuation and capitalization.
3. Apply course correction: if the speaker corrects themselves, keep only the correction.
   Example input: "send it to john at gmail dot com wait no john at outlook dot com"
   Output: "Send it to john@outlook.com"
4. Format numbers, dates, emails, and URLs naturally.
   "twenty five dollars" → "$25"
   "january third twenty twenty six" → "January 3, 2026"
   "h t t p s colon slash slash github dot com" → "https://github.com"
5. Detect and execute voice commands. Return them as JSON commands, not text:
   "delete that" → {"cmd": "delete_last"}
   "new line" → {"cmd": "newline"}
   "new paragraph" → {"cmd": "paragraph"}
   "select all" → {"cmd": "select_all"}
   "undo that" → {"cmd": "undo"}
6. Preserve the speaker's voice and intent. Do NOT rephrase or summarize.
7. Output ONLY the cleaned text or a JSON command. No explanations.
```

#### 4.4.3 llama-cpp-2 Integration

```rust
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::LlamaBackend;
use llama_cpp_2::token::AddBos;
use std::sync::Arc;

pub struct PostProcessor {
    // LlamaModel is Send+Sync — can be shared via Arc
    model: Arc<LlamaModel>,
    backend: LlamaBackend,
}

impl PostProcessor {
    pub fn new(model_path: &Path, use_gpu: bool) -> Result<Self> {
        let backend = LlamaBackend::init()?;
        let mut model_params = LlamaModelParams::default();
        if use_gpu {
            model_params.set_n_gpu_layers(-1); // all layers on GPU
        }
        // load_from_file needs &LlamaBackend as first arg
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)?;
        Ok(Self {
            model: Arc::new(model),
            backend,
        })
    }

    pub fn process(&self, raw_text: &str, dictionary_hints: &str) -> Result<ProcessorOutput> {
        // LlamaContext is NOT Send/Sync — create one per call
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZero::new(2048));
        let mut ctx = self.model.new_context(&self.backend, ctx_params)?;

        let prompt = format!(
            "{system_prompt}\n{dictionary_hints}\nRaw transcript: \"{raw_text}\"\nCleaned output:",
        );

        // str_to_token takes AddBos enum
        let tokens = self.model.str_to_token(&prompt, AddBos::Always)?;
        // ... run inference, collect output tokens ...

        let output = String::new(); // collected from token generation
        if output.trim_start().starts_with('{') {
            Ok(ProcessorOutput::Command(serde_json::from_str(&output)?))
        } else {
            Ok(ProcessorOutput::Text(output.trim().to_string()))
        }
    }
}

pub enum ProcessorOutput {
    Text(String),
    Command(VoiceCommand),
}

#[derive(serde::Deserialize)]
pub struct VoiceCommand {
    pub cmd: String,
    pub args: Option<serde_json::Value>,
}
```

#### 4.4.4 Inference Strategy

To keep latency down:

- Small context window (2048 tokens max).
- Persistent KV cache session across calls to keep system prompt cached.
- Stream tokens to the text injector as they're generated.
- Temperature 0.1 for near-deterministic output.

### 4.5 Text Injection

The text injector types polished text into whatever application has focus, simulating keyboard input at the OS level.

#### 4.5.1 Windows Implementation

```rust
#[cfg(target_os = "windows")]
mod injector {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    pub fn inject_text(text: &str) -> Result<()> {
        let chars: Vec<u16> = text.encode_utf16().collect();
        let mut inputs: Vec<INPUT> = Vec::with_capacity(chars.len() * 2);

        for ch in &chars {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: *ch,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: *ch,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
        Ok(())
    }
}
```

**Windows note:** SendInput (via `windows` 0.62) cannot inject into elevated processes due to UIPI.

#### 4.5.2 macOS Implementation

Uses `objc2` 0.6 + `objc2-core-graphics` 0.3 (NOT the Servo `core-graphics` crate which is heading toward deprecation).

```rust
#[cfg(target_os = "macos")]
mod injector {
    // objc2-core-graphics 0.3
    use objc2_core_graphics::*;

    pub fn inject_text(text: &str) -> Result<()> {
        // CGEvent has an undocumented 20-character limit per call.
        // Must chunk text into 20-char segments.
        for chunk in text.as_bytes().chunks(20) {
            let chunk_str = std::str::from_utf8(chunk)?;
            inject_chunk(chunk_str)?;
        }
        Ok(())
    }

    fn inject_chunk(text: &str) -> Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)?;
        let event = CGEvent::new_keyboard_event(source.clone(), 0, true)?;
        let buf: Vec<u16> = text.encode_utf16().collect();
        event.set_string_from_utf16_unchecked(&buf);
        event.post(CGEventTapLocation::HID);

        let event_up = CGEvent::new_keyboard_event(source, 0, false)?;
        event_up.post(CGEventTapLocation::HID);
        Ok(())
    }
}
```

#### 4.5.3 Command Execution

Voice commands map to keyboard shortcuts injected at the OS level:

| Command | Windows | macOS |
|---|---|---|
| `delete_last` | Ctrl+Backspace | Option+Delete |
| `undo` | Ctrl+Z | Cmd+Z |
| `select_all` | Ctrl+A | Cmd+A |
| `newline` | Enter | Enter |
| `paragraph` | Enter, Enter | Enter, Enter |
| `copy` | Ctrl+C | Cmd+C |
| `paste` | Ctrl+V | Cmd+V |
| `tab` | Tab | Tab |

### 4.6 GPUI Frontend

The frontend is a GPU-accelerated native Rust UI built with GPUI. It consists of two windows: a compact overlay HUD for recording state, and a full settings/management window accessible from the system tray.

#### 4.6.1 Application Startup State Machine

The app must be functional from the moment it launches, regardless of whether models are downloaded, GPU is available, or any component fails to initialize. **No launch state is an error state — every state has a working UI.**

```
┌─────────────┐     models missing?     ┌──────────────────┐
│   LAUNCH    │────────────────────────▶│  DOWNLOADING     │
│  (instant)  │                         │  (auto, no prompt)│
└──────┬──────┘                         └────────┬─────────┘
       │ models present                          │ all complete
       ▼                                         ▼
┌──────────────┐                       ┌──────────────────┐
│ LOADING      │◀──────────────────────│  ALL MODELS      │
│ (load models)│                       │  DOWNLOADED       │
└──────┬───────┘                       └──────────────────┘
       │ all loaded
       ▼
┌──────────────┐
│    READY     │
│ (full pipeline: VAD + ASR + LLM)
└──────────────┘
```

**Critical rules:**
1. The overlay HUD opens **immediately** on launch — before models load, before GPU init, before anything.
2. If models are missing, download starts **automatically** with progress in the overlay. No confirmation dialog. No "click to download" button. It just starts.
3. If download fails (no internet), the overlay shows the model directory path and URLs so the user can manually place files. The app stays open and polls for model files every 5 seconds.
4. **All three models (VAD, Whisper, LLM) must be present and loaded before the pipeline activates.**
5. **The hotkey works in every state.** If models are still downloading and the user presses the hotkey, show "Models downloading... 43%" in the overlay instead of silently doing nothing.

```rust
/// The app is always in exactly one of these states.
/// Every state has a corresponding UI. None of them are "broken."
#[derive(Clone, Debug)]
pub enum AppReadiness {
    /// Models not found, downloading automatically
    Downloading {
        vad_progress: DownloadProgress,
        whisper_progress: DownloadProgress,
        llm_progress: DownloadProgress,
    },
    /// All models downloaded, loading into GPU memory
    Loading { stage: &'static str },
    /// Full pipeline operational (VAD + GPU ASR + GPU LLM)
    Ready,
}

#[derive(Clone, Debug)]
pub enum DownloadProgress {
    Pending,
    InProgress { bytes_downloaded: u64, bytes_total: u64 },
    Complete,
    Failed { error: String, manual_url: String },
}
```

#### 4.6.1a Application Entry Point

```rust
use gpui::*;

fn main() {
    // Initialize logging
    let _guard = init_logging();

    Application::new().run(|cx: &mut App| {
        // 1. Create state (SQLite, settings, dictionary — lightweight, no ML)
        let state = VoxState::new().expect("Failed to create app data directory");
        cx.set_global(state);
        cx.set_global(VoxTheme::dark());

        // 2. Register actions & key bindings
        register_actions(cx);
        register_key_bindings(cx);

        // 3. Open overlay HUD IMMEDIATELY (before model loading)
        open_overlay_window(cx);

        // 4. Set up system tray
        setup_system_tray(cx);

        // 5. Set up global hotkey
        setup_global_hotkey(cx);

        // 6. Kick off async model check + download + pipeline init
        //    This runs in the background. The UI is already visible.
        cx.spawn(|cx| async move {
            initialize_pipeline(cx).await;
        }).detach();

        cx.activate(true);
    });
}

async fn initialize_pipeline(cx: AsyncApp) {
    // Check which models exist on disk
    let missing = check_models(&cx);

    if !missing.is_empty() {
        // Update UI: AppReadiness::Downloading
        // Start downloads automatically — no user action needed
        download_missing_models(missing, &cx).await;
        // All three models must complete before proceeding
    }

    // Update UI: AppReadiness::Loading
    // Load all models into GPU memory: VAD, Whisper, LLM
    load_pipeline(&cx).await.expect("Pipeline initialization failed");
    // Update UI: AppReadiness::Ready
    // Pipeline is live, hotkey now triggers dictation
}
```

#### 4.6.2 Overlay HUD Window

```rust
fn open_overlay_window(cx: &mut App) {
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(
            Bounds::centered(None, size(px(360.0), px(80.0)), cx),
        )),
        window_decorations: Some(WindowDecorations::Client), // no title bar
        window_min_size: Some(Size { width: px(200.0), height: px(60.0) }),
        focus: false,            // don't steal focus from user's active app
        show: true,
        is_movable: true,
        // always on top handled per-platform
        ..Default::default()
    };

    cx.open_window(window_options, |window, cx| {
        cx.new(|cx| OverlayHud::new(window, cx))
    }).expect("Failed to open overlay window");
}
```

#### 4.6.3 UI States

Every possible app state has a visible, informative overlay. No state is invisible or silent.

```
STARTUP STATES (first launch or models missing):

┌─────────────────────────────────────────┐
│  ↓ DOWNLOADING  Vox  ▾  [≡]            │
│  Whisper model: 43% (780 MB / 1.8 GB)  │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│  ↓ DOWNLOADING  Vox  ▾  [≡]            │
│  LLM model: 12% (192 MB / 1.6 GB)     │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│  ⟳ LOADING      Vox  ▾  [≡]            │
│  Loading Whisper model onto GPU...      │
└─────────────────────────────────────────┘

DOWNLOAD FAILURE (no internet, bad URL, etc.):

┌─────────────────────────────────────────┐
│  ⚠ NEEDS MODELS  Vox  ▾  [≡]           │
│  Place models in: %LOCALAPPDATA%/...    │
│  [Open Folder]  [Retry Download]        │
└─────────────────────────────────────────┘

NORMAL OPERATION:

┌─────────────────────────────────────────┐
│  IDLE           Vox  ▾  [≡]            │
│  Press [Fn] to start dictating          │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│  ● LISTENING    Vox  ▾  [≡]            │
│  ████████░░░░░░░░  (waveform animation) │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│  ⟳ PROCESSING   Vox  ▾  [≡]            │
│  "let's meet wednesday at three pm"     │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│  ✓ INJECTED     Vox  ▾  [≡]            │
│  Let's meet Wednesday at 3 PM.          │
└─────────────────────────────────────────┘

HOTKEY PRESSED WHILE NOT READY:

┌─────────────────────────────────────────┐
│  ↓ NOT READY    Vox  ▾  [≡]            │
│  Models downloading... 43%              │
└─────────────────────────────────────────┘
```

#### 4.6.4 Component Architecture

Following the Tusk/Zed pattern — workspace crate for UI, core crate for backend:

```rust
// Overlay HUD — compact floating pill
pub struct OverlayHud {
    pipeline_state: PipelineState,
    waveform_data: Vec<f32>,
    raw_transcript: Option<String>,
    polished_transcript: Option<String>,
    focus_handle: FocusHandle,
}

impl Render for OverlayHud {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        div()
            .flex()
            .flex_col()
            .w_full()
            .h_full()
            .bg(theme.colors.overlay_bg)
            .rounded(radius::LG)
            .child(self.render_status_bar(cx))
            .child(self.render_content(cx))
    }
}

// Settings window — full management UI
pub struct SettingsWindow {
    workspace: Entity<VoxWorkspace>,
}

pub struct VoxWorkspace {
    active_panel: Panel,
    settings_panel: Entity<SettingsPanel>,
    history_panel: Entity<HistoryPanel>,
    dictionary_panel: Entity<DictionaryPanel>,
    model_panel: Entity<ModelPanel>,
    log_panel: Entity<LogPanel>,
}
```

#### 4.6.5 Settings Window Panels

| Panel | Purpose |
|---|---|
| `SettingsPanel` | Audio device, VAD tuning, hotkey config, appearance |
| `HistoryPanel` | Past transcriptions with search |
| `DictionaryPanel` | Custom words/phrases CRUD |
| `ModelPanel` | Model download, swap, benchmark |
| `LogPanel` | Live tracing output for debugging |

#### 4.6.6 Waveform Visualizer

Custom GPUI element using the low-level `Element` trait for efficient real-time rendering of audio levels.

```rust
pub struct WaveformVisualizer {
    samples: Vec<f32>,  // recent RMS values
    width: Pixels,
    height: Pixels,
}

impl IntoElement for WaveformVisualizer {
    // Custom paint using GPUI's Path API for smooth waveform curves
}
```

---

## 5. Data Flow

### 5.0 First Launch (No Models On Disk)

```
Time       Event
─────────  ──────────────────────────────────────────────────
   0ms     App launches. Overlay HUD appears immediately.
   5ms     Check model directory → empty. Overlay: "Downloading..."
  10ms     Start downloading all three models concurrently:
           - Silero VAD v5 (1.1 MB)
           - Whisper Large V3 Turbo Q5_0 (1.8 GB)
           - Qwen 2.5 3B Instruct Q4_K_M (1.6 GB)
   1s      VAD downloaded.
   1s+     Overlay: "Downloading models: 12% (0.4 / 3.4 GB)"
   ...     Progress updates every 500ms in overlay
 ~5 min    All three models downloaded.
           Overlay: "Loading models onto GPU..."
 ~10s      All models loaded. Pipeline fully active.
           Overlay: "IDLE — Press [Fn] to start dictating"
           User never had to restart, click anything, or configure anything.
```

If user presses hotkey at any point during download:
- Overlay shows: "Models downloading... 43%"
- No silent failure. No crash. No hanging.

### 5.1 Happy Path (End-to-End)

```
Time(ms)  Event
────────  ──────────────────────────────────────────────────
   0      User presses hotkey (global-hotkey crate fires)
   1      Pipeline toggle → audio capture begins via cpal
   5      First 512-sample window → Silero VAD
  32      VAD: speech_prob = 0.02 (silence, waiting...)
 200      User starts speaking
 232      VAD: speech_prob = 0.91 → state = SPEAKING
 232      Overlay: state = Listening, waveform animating
 232+     Audio accumulating in speech buffer
2500      User pauses (natural utterance boundary)
3000      VAD: 500ms silence → state = SILENT → emit segment
3000      Overlay: state = Processing
3001      Speech buffer (2.3s of audio) → whisper.cpp
3035      Whisper returns: "um let's meet tuesday wait no wednesday at three pm"
3035      Raw transcript → LLM post-processor
3036      Overlay: shows raw transcript
3250      LLM returns: "Let's meet Wednesday at 3 PM."
3250      Overlay: shows polished transcript
3251      Text injector → types into active application
3280      Overlay: state = Listening (still recording)
3280      User continues speaking...
```

**Total latency breakdown (RTX 4090):**
- VAD decision: ~0ms (CPU, negligible)
- Whisper transcription (2.3s audio): ~35ms
- LLM post-processing (~15 tokens): ~100ms
- Text injection: ~30ms
- **Total: ~165ms** from end-of-utterance to text appearing

**Total latency breakdown (M4 Pro):**
- VAD decision: ~0ms
- Whisper transcription (2.3s audio): ~125ms
- LLM post-processing (~15 tokens): ~275ms
- Text injection: ~30ms
- **Total: ~430ms** from end-of-utterance to text appearing

Both well under the 500ms target.

### 5.2 Hands-Free Mode

Double-press hotkey activates hands-free mode. Recording continues indefinitely, VAD auto-segments, each segment flows through the pipeline automatically. Single press to exit.

### 5.3 Command Mode

When the user says "hey vox" (wake word) followed by a command, the pipeline routes to command execution instead of text injection. The LLM detects the command intent and returns a JSON command object.

Example: "hey vox, delete the last sentence" → `{"cmd": "delete_last_sentence"}`

Wake word detection is a simple keyword spotter on the raw transcript, not a separate model.

---

## 6. Project Structure

```
vox/
├── Cargo.toml                    # Workspace root
├── Cargo.lock
├── LICENSE                        # MIT
├── README.md
├── CLAUDE.md
│
├── crates/
│   ├── vox/                      # Main binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs           # Application::new().run(), window setup
│   │       ├── app.rs            # VoxApp root component
│   │       └── tray.rs           # System tray setup (tray-icon)
│   │
│   ├── vox_core/                 # Backend: pipeline, ML, state
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── state.rs          # VoxState (global, Arc<RwLock<...>>)
│   │       ├── pipeline/
│   │       │   ├── mod.rs
│   │       │   └── orchestrator.rs
│   │       ├── audio/
│   │       │   ├── mod.rs
│   │       │   ├── capture.rs    # cpal 0.17 audio capture
│   │       │   ├── ring_buffer.rs # ringbuf 0.4 SPSC wrapper
│   │       │   └── resampler.rs  # rubato 1.0 resampler
│   │       ├── vad/
│   │       │   ├── mod.rs
│   │       │   ├── silero.rs     # Silero VAD via ort 2.0
│   │       │   └── chunker.rs    # Speech segment accumulator
│   │       ├── asr/
│   │       │   ├── mod.rs
│   │       │   └── whisper.rs    # whisper-rs 0.15.1 wrapper
│   │       ├── llm/
│   │       │   ├── mod.rs
│   │       │   ├── processor.rs  # llama-cpp-2 0.1 post-processor
│   │       │   └── prompts.rs    # System prompts and templates
│   │       ├── injector/
│   │       │   ├── mod.rs
│   │       │   ├── windows.rs    # Win32 SendInput (windows 0.62)
│   │       │   ├── macos.rs      # CGEvent (objc2 0.6)
│   │       │   └── commands.rs   # Voice command → keystrokes
│   │       ├── dictionary/
│   │       │   ├── mod.rs
│   │       │   └── store.rs      # SQLite dictionary (rusqlite 0.38)
│   │       ├── config/
│   │       │   ├── mod.rs
│   │       │   └── settings.rs   # User settings (serde + JSON file)
│   │       ├── hotkey.rs         # global-hotkey crate wrapper
│   │       └── models.rs         # Model download manager (reqwest 0.13)
│   │
│   └── vox_ui/                   # GPUI UI components
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── theme.rs          # VoxTheme + color palette
│           ├── layout.rs         # Spacing, sizing, radius constants
│           ├── overlay_hud.rs    # Floating pill overlay
│           ├── waveform.rs       # Real-time audio waveform element
│           ├── workspace.rs      # Settings window layout
│           ├── settings_panel.rs # Audio, VAD, hotkey, appearance settings
│           ├── history_panel.rs  # Past transcriptions
│           ├── dictionary_panel.rs # Custom words/phrases
│           ├── model_panel.rs    # Model download & management
│           ├── log_panel.rs      # Live log viewer
│           ├── text_input.rs     # Custom text input component
│           ├── button.rs         # Styled button component
│           ├── key_bindings.rs   # Actions and keyboard shortcuts
│           └── icons.rs          # SVG icon system
│
├── assets/                       # Icons, SVGs, fonts
│   └── icons/
│
├── models/                       # Git-ignored, downloaded at first run
│   ├── ggml-large-v3-turbo-q5_0.bin
│   ├── qwen2.5-3b-instruct-q4_k_m.gguf
│   └── silero_vad_v5.onnx
│
├── scripts/
│   ├── download-models.sh
│   └── download-models.ps1
│
└── tests/
    ├── audio_fixtures/           # Test WAV files
    ├── test_vad.rs
    ├── test_asr.rs
    ├── test_llm.rs
    ├── test_injector.rs
    └── test_pipeline_e2e.rs
```

---

## 7. Build System

### 7.1 Prerequisites

**Both platforms:**
- Rust 1.85+ (2024 edition) via rustup
- CMake 4.0+

**Windows additional:**
- Visual Studio 2022 Build Tools (MSVC)
- CUDA Toolkit 12.8+
- cuDNN 9.x
- `CMAKE_GENERATOR=Visual Studio 17 2022` (CUDA doesn't support VS 18 Insiders)
- `CUDA_PATH` set as persistent user env var

**macOS additional:**
- Xcode 26.x + Command Line Tools
- No additional GPU setup needed (Metal is automatic)

**No Node.js, pnpm, Vite, or any web toolchain required.**

### 7.2 Cargo.toml (Workspace Root)

```toml
[workspace]
members = ["crates/vox", "crates/vox_core", "crates/vox_ui"]
resolver = "2"

[workspace.package]
version = "1.0.0"
edition = "2024"
license = "MIT"

[workspace.dependencies]
gpui = { git = "https://github.com/zed-industries/zed", rev = "TBD" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1.49", features = ["rt-multi-thread", "sync", "time", "macros"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
parking_lot = "0.12"
uuid = { version = "1", features = ["v4", "serde"] }

[profile.release]
opt-level = "s"
lto = true
strip = "symbols"
codegen-units = 1
```

### 7.3 vox_core/Cargo.toml

```toml
[package]
name = "vox_core"
version.workspace = true
edition.workspace = true

[features]
default = []
cuda = ["whisper-rs/cuda", "llama-cpp-2/cuda"]
metal = ["whisper-rs/metal", "llama-cpp-2/metal"]

[dependencies]
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
anyhow.workspace = true
tracing.workspace = true
parking_lot.workspace = true
uuid.workspace = true

# Audio
cpal = "0.17"
ringbuf = "0.4"
rubato = "1.0"

# ML
ort = { version = "2.0.0-rc.11", features = ["load-dynamic"] }
whisper-rs = "0.15.1"
llama-cpp-2 = "0.1"

# Platform
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.62", features = [
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
] }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.6"
objc2-core-graphics = "0.3"

# Storage & networking
rusqlite = { version = "0.38", features = ["bundled"] }
reqwest = { version = "0.13", features = ["stream"] }

# Hotkey & tray
global-hotkey = "0.6"
tray-icon = "0.19"
```

### 7.4 vox_ui/Cargo.toml

```toml
[package]
name = "vox_ui"
version.workspace = true
edition.workspace = true

[dependencies]
gpui.workspace = true
vox_core = { path = "../vox_core" }
serde.workspace = true
parking_lot.workspace = true
smallvec = { version = "1.11", features = ["union"] }
```

### 7.5 Build Commands

```bash
# Development (Windows, CUDA)
cargo run -p vox --features vox_core/cuda

# Development (macOS, Metal)
cargo run -p vox --features vox_core/metal

# Release build
cargo build --release -p vox --features vox_core/cuda

# Run tests
cargo test -p vox_core --features cuda

# Run a single test
cargo test -p vox_core test_name --features cuda -- --nocapture

# Latency benchmarks
cargo test -p vox_core --release --features cuda benchmark_ -- --nocapture
```

Incremental builds after initial compilation: **~3–10 seconds** (vs 60–300s+ with Tauri).

---

## 8. Pipeline Orchestration

The orchestrator is the central coordinator that wires all components together and manages the async pipeline.

```rust
use tokio::sync::{mpsc, broadcast};
use std::sync::Arc;

pub struct Pipeline {
    audio_capture: AudioCapture,
    vad: SileroVad,
    asr: AsrEngine,
    llm: PostProcessor,
    injector: TextInjector,
    dictionary: DictionaryCache,
    state_tx: broadcast::Sender<PipelineState>,
}

#[derive(Clone, Debug)]
pub enum PipelineState {
    Idle,
    Listening,
    Processing { raw_text: Option<String> },
    Injecting { polished_text: String },
    Error { message: String },
}

impl Pipeline {
    /// All components are required. Pipeline does not start without all of them.
    pub fn new(
        audio: AudioCapture,
        vad: SileroVad,
        asr: AsrEngine,
        llm: PostProcessor,
        injector: TextInjector,
        dictionary: DictionaryCache,
        state_tx: broadcast::Sender<PipelineState>,
    ) -> Self {
        Self { audio_capture: audio, vad, asr, llm, injector, dictionary, state_tx }
    }

    pub async fn run(&mut self) -> Result<()> {
        let (segment_tx, mut segment_rx) = mpsc::channel::<Vec<f32>>(8);

        let audio_handle = self.spawn_audio_thread(segment_tx);

        while let Some(audio_segment) = segment_rx.recv().await {
            self.state_tx.send(PipelineState::Processing { raw_text: None })?;

            // ASR (GPU-bound)
            let raw_text = tokio::task::spawn_blocking({
                let asr = self.asr.clone();
                move || asr.transcribe(&audio_segment)
            }).await??;

            if raw_text.is_empty() {
                self.state_tx.send(PipelineState::Listening)?;
                continue;
            }

            self.state_tx.send(PipelineState::Processing {
                raw_text: Some(raw_text.clone()),
            })?;

            // Dictionary substitution (fast, O(1) lookups)
            let substituted = self.dictionary.apply_substitutions(&raw_text);

            // LLM post-processing (GPU-bound)
            let hints = self.dictionary.top_hints(50);
            let result = tokio::task::spawn_blocking({
                let llm = self.llm.clone();
                let text = substituted.clone();
                move || llm.process(&text, &hints)
            }).await??;

            match result {
                ProcessorOutput::Text(polished) => {
                    self.state_tx.send(PipelineState::Injecting {
                        polished_text: polished.clone(),
                    })?;
                    self.injector.inject_text(&polished)?;
                }
                ProcessorOutput::Command(cmd) => {
                    self.injector.execute_command(&cmd)?;
                }
            }

            self.state_tx.send(PipelineState::Listening)?;
        }

        Ok(())
    }
}
```

---

## 9. Custom Dictionary

### 9.1 Storage (SQLite)

```sql
CREATE TABLE dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    spoken TEXT NOT NULL UNIQUE,
    written TEXT NOT NULL,
    category TEXT DEFAULT 'general',
    is_command_phrase INTEGER DEFAULT 0,
    use_count INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now'))
);
```

**rusqlite 0.38 note:** No `FromSql` for `chrono::DateTime<Utc>`. Use `String` (ISO 8601) for timestamps.

### 9.2 Dictionary Cache

```rust
pub struct DictionaryCache {
    cache: RwLock<HashMap<String, String>>,  // spoken → written, O(1) lookup
}
```

Loaded into memory at startup. Updated on dictionary changes. Command phrase exclusion (FR-007) — entries marked `is_command_phrase = 1` are excluded from substitution.

### 9.3 LLM Integration

Top 50 dictionary entries injected into the system prompt as hints:

```
Custom dictionary (apply these substitutions):
- "vox" → "Vox"
- "my email" → "engineer@example.com"
```

---

## 10. Settings & Configuration

### 10.1 User Settings Schema

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    // Audio
    pub input_device: Option<String>,
    pub noise_gate: f32,              // 0.0–1.0, default 0.0

    // VAD
    pub vad_threshold: f32,           // 0.0–1.0, default 0.5
    pub min_silence_ms: u32,          // default 500
    pub min_speech_ms: u32,           // default 250

    // ASR
    pub language: String,             // "en", "auto", or BCP-47
    pub whisper_model: String,        // model filename

    // LLM
    pub llm_model: String,            // model filename
    pub temperature: f32,             // 0.0–1.0, default 0.1
    pub remove_fillers: bool,         // default true
    pub course_correction: bool,      // default true
    pub punctuation: bool,            // default true

    // Hotkey
    pub activation_hotkey: String,    // default "CapsLock"
    pub hold_to_talk: bool,           // true = push-to-talk, false = toggle
    pub hands_free_double_press: bool,// default true

    // Appearance
    pub overlay_position: OverlayPosition,
    pub overlay_opacity: f32,         // 0.0–1.0, default 0.85
    pub show_raw_transcript: bool,    // default false
    pub theme: ThemeMode,             // System, Light, Dark

    // Advanced
    pub max_segment_ms: u32,          // default 10000
    pub overlap_ms: u32,              // default 1000
    pub command_prefix: String,       // default "hey vox"
}
```

### 10.2 Persistence

Settings stored as JSON file. Dictionary and transcript history in SQLite. All in platform app data directory.

```
# Windows
%APPDATA%/com.vox.app/settings.json
%APPDATA%/com.vox.app/vox.db

# macOS
~/Library/Application Support/com.vox.app/settings.json
~/Library/Application Support/com.vox.app/vox.db
```

---

## 11. Model Management

### 11.1 First-Run: Fully Automatic

There is no "welcome screen," no "click to download" prompt, no setup wizard. The app launches, the overlay appears, and if models are missing it starts downloading them immediately. The user sees progress in the overlay HUD.

**All three models download concurrently:**
- **Silero VAD v5** (~1.1 MB)
- **Whisper Large V3 Turbo Q5_0** (~1.8 GB)
- **Qwen 2.5 3B Instruct Q4_K_M** (~1.6 GB)

Total: ~3.4 GB. All three must complete before the pipeline activates.

**If download fails:**
- Overlay shows: model directory path + direct download URLs
- "Open Folder" button opens the model directory in file explorer
- "Retry Download" button retries
- App keeps running and polls for model files every 5 seconds
- The moment models appear on disk (manual download, USB transfer, whatever), they're detected and loaded

**If models already exist** (user copied them, reinstall, etc.): download is skipped, pipeline loads immediately.

### 11.2 Model Sources

| Model | Source | URL |
|---|---|---|
| Whisper Large V3 Turbo Q5_0 | Hugging Face | `huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin` |
| Qwen 2.5 3B Instruct Q4_K_M | Hugging Face | `huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF/resolve/main/Qwen2.5-3B-Instruct-Q4_K_M.gguf` |
| Silero VAD v5 | GitHub | `github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx` |

### 11.3 Model Storage

```
# Windows
%LOCALAPPDATA%/com.vox.app/models/

# macOS
~/Library/Application Support/com.vox.app/models/
```

### 11.4 Model Swapping

Users can swap models via the Model panel. Vox validates GGUF/GGML/ONNX format and runs a quick benchmark on load to verify GPU acceleration is working.

---

## 12. Error Handling Strategy

### 12.1 Core Principle: Never Stop Working

Every error has an automatic recovery path. The user should never see a dead app. If something breaks, the pipeline restarts itself. If a component crashes, it is restarted and retried — not skipped.

### 12.2 Error Categories

| Category | Examples | Recovery |
|---|---|---|
| Audio | Device disconnected, permission denied | Switch to default device. If no device: pause pipeline, show message, retry every 2s |
| Model missing | File not found on disk | Auto-download. If no internet: show manual instructions, poll for files every 5s |
| Model corrupt | Bad GGML/GGUF/ONNX file | Delete and re-download automatically |
| Model OOM | GPU out of memory | Show error with specific guidance (close other GPU apps, or use a smaller quantization) |
| ASR failure | Whisper crash on a segment | Log error, retry the segment once. If retry fails, discard segment and continue listening. |
| LLM failure | Timeout, garbled output on a segment | Retry the segment once. If retry fails, discard segment and continue listening. |
| Injection | Focus lost, permission denied | Buffer text, show in overlay with "Copy" button. Retry injection on next focus event. |
| GPU crash | CUDA error, driver crash | Show error with instructions to restart the app. |

### 12.3 Pipeline Recovery

```
Pipeline Running (VAD + GPU ASR + GPU LLM)
        │ component crashes on a segment
        ▼
Retry Segment (restart component, reprocess same audio)
        │ retry succeeds → back to running
        │ retry fails → discard segment, log error
        ▼
Continue Listening (pipeline stays active for next segment)
```

If a model file becomes corrupted or deleted while the app is running, the pipeline stops and re-enters the downloading state. Once the model is back on disk, it reloads and resumes.

### 12.3 Logging

`tracing` crate with structured logging. Logs written to:

```
%LOCALAPPDATA%/com.vox.app/logs/  (Windows)
~/Library/Logs/com.vox.app/       (macOS)
```

Log levels: `ERROR` (always), `WARN` (default), `INFO` (verbose), `DEBUG` (development), `TRACE` (pipeline timing). Rotated daily, 7-day retention.

---

## 13. Performance Targets & Benchmarks

### 13.1 Latency Budget

| Phase | Target (4090) | Target (M4 Pro) |
|---|---|---|
| Audio callback → VAD | < 5 ms | < 5 ms |
| VAD decision | < 1 ms | < 1 ms |
| Whisper transcription (5s audio) | < 50 ms | < 150 ms |
| LLM post-processing (30 tokens) | < 200 ms | < 550 ms |
| Text injection | < 30 ms | < 30 ms |
| **End-to-end (utterance end → text)** | **< 300 ms** | **< 750 ms** |

### 13.2 Resource Budget

| Resource | 4090 Desktop | M4 Pro MacBook |
|---|---|---|
| VRAM / Unified Memory | < 6 GB (of 24 GB) | < 6 GB (of 24 GB) |
| System RAM | < 500 MB | < 500 MB |
| CPU (idle) | < 2% | < 2% |
| CPU (active dictation) | < 15% | < 20% |
| Disk (models) | ~3.5 GB | ~3.5 GB |
| Disk (app + data) | < 50 MB | < 50 MB |
| Binary size | < 15 MB | < 15 MB |

### 13.3 Build Time Budget

| Build Type | Target |
|---|---|
| Clean build (first time, includes whisper.cpp/llama.cpp C++ compilation) | < 5 min |
| Incremental build (Rust-only changes) | < 10 sec |

---

## 14. Security & Privacy

### 14.1 Threat Model

| Threat | Mitigation |
|---|---|
| Audio exfiltration | All processing local. No network calls after model download. |
| Model tampering | SHA-256 checksum verification on download. Models are read-only after download. |
| Keystroke injection abuse | Injection only active when user explicitly activates recording. Tray icon shows state. |
| Transcript leakage | Transcript history stored locally in SQLite. |
| Malicious model | Only download from pinned Hugging Face URLs. Verify file hashes. |

### 14.2 Permissions

**Windows:**
- Microphone access (prompted by OS)
- No admin/elevated privileges required

**macOS:**
- Microphone access (runtime prompt)
- Accessibility permission (for CGEvent text injection)
- Input Monitoring permission (for global hotkeys)

### 14.3 Audio Data Policy

- Audio is processed in memory and immediately discarded after transcription.
- No audio is written to disk at any point.
- Transcript history can be disabled entirely in settings.
- "Clear history" performs a secure delete (overwrite + VACUUM on SQLite).

---

## 15. Testing Strategy

### 15.1 Unit Tests

| Component | Test Coverage |
|---|---|
| Ring buffer | Capacity, overflow, underflow, concurrent read/write |
| Resampler | 44.1kHz→16kHz, 48kHz→16kHz, quality verification |
| VAD | Known speech/silence fixtures, state machine transitions |
| ASR | Known audio → expected transcript (LibriSpeech samples) |
| LLM prompts | Filler removal, course correction, command detection |
| Dictionary | CRUD, substitution logic, conflict resolution |
| Text injector | Character encoding (ASCII, Unicode, emoji), command mapping |
| Settings | Serialization round-trip, migration from older schema |

### 15.2 Integration Tests

```rust
#[tokio::test]
#[ignore] // requires ML models
async fn test_full_pipeline_hello_world() {
    let pipeline = TestPipeline::new().await;
    let audio = load_wav("fixtures/hello_world.wav");
    let result = pipeline.process_segment(&audio).await.unwrap();
    assert_eq!(result, "Hello, world.");
}

#[tokio::test]
#[ignore]
async fn test_course_correction() {
    let pipeline = TestPipeline::new().await;
    let audio = load_wav("fixtures/correction_tuesday_wednesday.wav");
    let result = pipeline.process_segment(&audio).await.unwrap();
    assert!(result.contains("Wednesday"));
    assert!(!result.contains("Tuesday"));
}
```

### 15.3 Performance Tests

- **Latency regression:** Assert e2e < 500ms (4090) / < 1000ms (M4 Pro) on standard 5s clip.
- **Memory leak:** Run 1000 segments, assert RSS within 2x baseline.
- **VRAM leak:** Monitor after 1000 segments.

### 15.4 Manual Test Matrix

| Scenario | Windows | macOS |
|---|---|---|
| Basic dictation into Notepad / TextEdit | ☐ | ☐ |
| Dictation into VS Code | ☐ | ☐ |
| Dictation into Chrome (Gmail compose) | ☐ | ☐ |
| Dictation into Slack desktop | ☐ | ☐ |
| Dictation into Terminal | ☐ | ☐ |
| Hold-to-talk mode | ☐ | ☐ |
| Toggle mode | ☐ | ☐ |
| Hands-free continuous mode | ☐ | ☐ |
| Voice command: "delete that" | ☐ | ☐ |
| Voice command: "new line" | ☐ | ☐ |
| Course correction in utterance | ☐ | ☐ |
| Custom dictionary term | ☐ | ☐ |
| Switch audio input device mid-session | ☐ | ☐ |
| Unplug audio device during recording | ☐ | ☐ |
| System sleep/wake during idle | ☐ | ☐ |
| System sleep/wake during recording | ☐ | ☐ |

---

## 16. Packaging & Distribution

### 16.1 Build Artifacts

| Platform | Format | Size Target |
|---|---|---|
| Windows | Single `.exe` (portable) + `.msi` installer | < 15 MB |
| macOS | `.app` bundle in `.dmg` | < 15 MB |

No web assets to bundle. Single static Rust binary.

### 16.2 First-Run Experience

1. User installs Vox (< 15 MB).
2. First launch: overlay HUD appears **instantly**.
3. All three models auto-download concurrently (progress shown in overlay, ~3.4 GB total).
4. All models downloaded. GPU detected. Models loaded onto GPU.
5. Pipeline activates. Overlay shows "IDLE — Press [Fn] to start dictating."
6. macOS: Accessibility permission prompt fires on first dictation attempt (OS-triggered, unavoidable).

**Zero-click setup.** Install → launch → wait for download → dictate. Nothing to configure unless they want to.

---

## 17. Roadmap

### v1.0 — Foundation (This Document)

- [ ] Core pipeline: Audio → VAD → ASR → LLM → Inject
- [ ] Windows (CUDA) + macOS (Metal)
- [ ] Hold-to-talk + toggle + hands-free modes
- [ ] Custom dictionary
- [ ] Voice commands (basic set)
- [ ] System tray + overlay HUD
- [ ] Settings panel (GPUI native)
- [ ] Transcript history

### v1.1 — Polish

- [ ] Whisper mode (quiet/whispered speech)
- [ ] Snippet shortcuts ("my address" → full address block)
- [ ] Audio device hot-swap
- [ ] Improved Unicode handling (CJK, RTL)
- [ ] Latency overlay (debug mode)

### v1.2 — Multilingual

- [ ] Auto language detection
- [ ] Per-language dictionaries
- [ ] Mixed-language dictation (code-switching)

### v2.0 — Intelligence

- [ ] Context-aware tone (formal in email, casual in Slack)
- [ ] Screen context via screenshot
- [ ] Shared team dictionaries
- [ ] Custom fine-tuned ASR models

---

## 18. Appendix

### A. Key Crate Versions (Pinned)

```toml
# Verified compatible set as of 2026-02-19
gpui = { git = "https://github.com/zed-industries/zed" }
cpal = "0.17"
ringbuf = "0.4"
rubato = "1.0"
ort = "2.0.0-rc.11"
whisper-rs = "0.15.1"
llama-cpp-2 = "0.1"          # utilityai crate, NOT llama-cpp-rs
serde = "1.0"
serde_json = "1.0"
tokio = "1.49"
reqwest = "0.13"
anyhow = "1.0"
tracing = "0.1"
rusqlite = "0.38"
windows = "0.62"
objc2 = "0.6"
objc2-core-graphics = "0.3"  # NOT Servo core-graphics
global-hotkey = "0.6"
tray-icon = "0.19"
parking_lot = "0.12"
```

### B. Known API Gotchas

| Crate | Gotcha |
|---|---|
| ringbuf 0.4 | `occupied_len()` lives on `Observer` trait. Must `use ringbuf::traits::Observer` |
| whisper-rs 0.15 | `full_n_segments()` returns `c_int` (NOT Result). Segment text via `state.get_segment(i).to_str_lossy()` |
| llama-cpp-2 0.1 | Types nested: `model::LlamaModel`, `model::params::LlamaModelParams`. `load_from_file` needs `&LlamaBackend` first arg. `str_to_token` takes `AddBos` enum |
| cpal 0.17 | `device.description()` returns `DeviceDescription` struct. Sample rate methods return `u32` directly |
| rusqlite 0.38 | No `FromSql` for `chrono::DateTime<Utc>`. Use `String` (ISO 8601) instead |
| macOS CGEvent | Undocumented 20-char limit per call. Must chunk text |
| Windows SendInput | Can't inject into elevated processes (UIPI) |

### C. GGML Quantization Reference

| Quantization | Bits/Weight | Size (3B model) | Quality Loss | Speed Gain |
|---|---|---|---|---|
| F16 | 16 | ~6 GB | None | Baseline |
| Q8_0 | 8 | ~3 GB | Negligible | ~1.5x |
| Q5_K_M | 5.5 | ~2.2 GB | Minimal | ~2x |
| **Q4_K_M** | **4.5** | **~1.8 GB** | **Small** | **~2.5x** |
| Q3_K_M | 3.5 | ~1.4 GB | Noticeable | ~3x |
| Q2_K | 2.5 | ~1.1 GB | Significant | ~3.5x |

### D. Whisper Model Size Reference

| Model | Params | Disk (Q5_0) | VRAM (Q5_0) | English WER |
|---|---|---|---|---|
| Tiny | 39M | ~40 MB | ~100 MB | ~15% |
| Base | 74M | ~80 MB | ~200 MB | ~11% |
| Small | 244M | ~250 MB | ~500 MB | ~9.5% |
| Medium | 769M | ~800 MB | ~1.5 GB | ~8.5% |
| Large V3 | 1.55B | ~1.6 GB | ~3 GB | ~7.4% |
| **Large V3 Turbo** | **809M** | **~900 MB** | **~1.8 GB** | **~8%** |

### E. Alternative ASR Engines (Evaluated)

| Engine | Verdict | Notes |
|---|---|---|
| **whisper.cpp** | **Selected** | Best ecosystem, CUDA+Metal, active maintenance, Rust bindings |
| Moonshine | v2 candidate | Best for edge/mobile. Overkill on 4090/M4 Pro. |
| faster-whisper | Python only | CTranslate2 backend, no simple C FFI |
| Canary Qwen 2.5B | Accuracy king | NeMo dependency is heavy, no simple C FFI |

### F. Alternative LLM Models (Evaluated)

| Model | Verdict | Notes |
|---|---|---|
| **Qwen 2.5 3B Instruct** | **Selected** | Best instruction following at 3B, multilingual, fast |
| Phi-3.5 Mini 3.8B | Close second | Slightly larger, good at structured output |
| Llama 3.2 3B | Good | Strong general capability, weaker on formatting |
| Gemma 2 2B | Smallest | Fastest inference, quality drops on edge cases |

### G. GPU Detection Logic

**Windows:** Query `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader` to detect CUDA GPUs. GPU is required on Windows — if not detected, show error with driver installation instructions.
**macOS:** Metal is always available on Apple Silicon. Query unified memory via `sysctl hw.memsize`.

### H. macOS Accessibility Permissions

Vox requires two macOS permissions granted manually:

1. **Accessibility** (System Settings → Privacy & Security → Accessibility) — for CGEvent text injection
2. **Input Monitoring** (System Settings → Privacy & Security → Input Monitoring) — for global hotkeys

*End of Design Document*
