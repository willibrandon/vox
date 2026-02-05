# VoxFlow — Design Document

**Project:** VoxFlow — Local-First Intelligent Voice Dictation Engine  
**Version:** 1.0.0  
**Date:** February 4, 2026  
**Author:** Engineering Team  
**Status:** Draft  

---

## 1. Executive Summary

VoxFlow is a privacy-first, locally-executed voice dictation application that transforms natural speech into polished, context-aware text in any application. It combines real-time speech recognition, intelligent post-processing via a local LLM, and universal text injection to deliver a Wispr Flow-class experience with zero cloud dependency.

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

### 1.3 Non-Goals (v1)

- Mobile (iOS/Android) — Tauri v2 supports it, but we defer to v2.
- Cloud/hybrid mode — strictly local.
- Speaker diarization — single-user dictation only.
- Real-time translation — English-first, multilingual transcription deferred.

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Tauri v2 Shell (Rust)                        │
│  ┌──────────────┐  ┌──────────────┐  ┌───────────────────────────┐ │
│  │  System Tray  │  │  Hotkey Mgr  │  │    Global State (Rust)    │ │
│  └──────┬───────┘  └──────┬───────┘  └─────────────┬─────────────┘ │
│         │                 │                         │               │
│  ┌──────▼─────────────────▼─────────────────────────▼─────────────┐ │
│  │                   Audio Pipeline (Rust)                         │ │
│  │  ┌──────────┐   ┌───────────┐   ┌────────────┐   ┌──────────┐ │ │
│  │  │  cpal    │──▶│ Ring Buf  │──▶│ Silero VAD │──▶│ Chunker  │ │ │
│  │  │ (capture)│   │ (16kHz)   │   │  (ONNX RT) │   │          │ │ │
│  │  └──────────┘   └───────────┘   └────────────┘   └────┬─────┘ │ │
│  └────────────────────────────────────────────────────────┼───────┘ │
│                                                           │         │
│  ┌────────────────────────────────────────────────────────▼───────┐ │
│  │                   ASR Engine (C FFI)                            │ │
│  │  ┌─────────────────────────────────────────────────────────┐   │ │
│  │  │  whisper.cpp  (CUDA on Windows / Metal on macOS)        │   │ │
│  │  │  Model: Whisper Large V3 Turbo (ggml-large-v3-turbo-q5) │   │ │
│  │  └───────────────────────────────┬─────────────────────────┘   │ │
│  └──────────────────────────────────┼─────────────────────────────┘ │
│                                     │ raw transcript                │
│  ┌──────────────────────────────────▼─────────────────────────────┐ │
│  │                   LLM Post-Processor (C FFI)                    │ │
│  │  ┌─────────────────────────────────────────────────────────┐   │ │
│  │  │  llama.cpp  (CUDA on Windows / Metal on macOS)          │   │ │
│  │  │  Model: Qwen 2.5 3B Instruct (Q4_K_M)                  │   │ │
│  │  └───────────────────────────────┬─────────────────────────┘   │ │
│  └──────────────────────────────────┼─────────────────────────────┘ │
│                                     │ polished text                 │
│  ┌──────────────────────────────────▼─────────────────────────────┐ │
│  │                   Text Injector                                 │ │
│  │  Windows: SendInput (Win32 API via windows-rs)                 │ │
│  │  macOS:   CGEventCreateKeyboardEvent (Core Graphics via objc2) │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │          Frontend (SolidJS + TypeScript + Tailwind CSS)         │ │
│  │  Overlay HUD · Settings Panel · Transcript History · Dictionary │ │
│  └────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 3. Technology Stack

### 3.1 Core Runtime

| Layer | Technology | Version | Rationale |
|---|---|---|---|
| App Framework | Tauri v2 | 2.10.x | Rust backend, web frontend, tiny binary, system tray, global hotkeys, security-audited |
| Language (backend) | Rust | 1.84 (2025 edition) | Zero-cost abstractions, FFI to C/C++, fearless concurrency, cross-platform |
| Language (frontend) | TypeScript | 5.7 | Type safety for UI layer |
| UI Framework | SolidJS | 1.9.x | Smallest bundle, finest-grained reactivity, no virtual DOM overhead |
| CSS | Tailwind CSS | 4.x | Utility-first, works inside Tauri webview |
| Build | Cargo + Vite | — | Tauri default toolchain |

### 3.2 Audio & ML

| Component | Technology | Version | Rationale |
|---|---|---|---|
| Audio Capture | cpal | 0.15.x | Cross-platform audio I/O in Rust, WASAPI/CoreAudio backends |
| VAD | Silero VAD v5 | ONNX | 1.1 MB, sub-ms per frame, best open-source VAD, runs on CPU |
| ONNX Runtime | ort (Rust crate) | 2.x | Official ONNX Runtime Rust bindings, hardware-agnostic inference |
| ASR | whisper.cpp | 1.8.x | C/C++, CUDA + Metal, ggml quantized models, battle-tested |
| LLM | llama.cpp | latest | C/C++, CUDA + Metal, ggml quantized models, same ecosystem |
| ASR Model | Whisper Large V3 Turbo | ggml Q5_0 | 809M params, 6x faster than V3, ~3 GB VRAM, 99+ languages |
| LLM Model | Qwen 2.5 3B Instruct | ggml Q4_K_M | 3B params, ~2.5 GB VRAM, excellent instruction following, fast |
| Rust FFI (whisper) | whisper-rs | 0.13.x | Safe Rust bindings over whisper.cpp C API |
| Rust FFI (llama) | llama-cpp-rs | 0.4.x | Safe Rust bindings over llama.cpp C API |

### 3.3 Platform Integration

| Component | Windows | macOS |
|---|---|---|
| Text Injection | `windows-rs` (SendInput) | `objc2` + Core Graphics (CGEvent) |
| Global Hotkeys | Tauri globalShortcut plugin | Tauri globalShortcut plugin |
| System Tray | Tauri tray plugin | Tauri tray plugin |
| Auto-start | Tauri autostart plugin | Tauri autostart plugin |
| GPU Acceleration | CUDA 12.6 | Metal 3 (automatic via ggml) |
| Audio Backend | WASAPI (via cpal) | CoreAudio (via cpal) |

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
    pub buffer_size: usize,      // 512 samples = 32ms at 16kHz
    pub device: Option<String>,  // None = system default input
}
```

#### 4.1.2 Ring Buffer Design

We use a single-producer single-consumer (SPSC) lock-free ring buffer between the audio callback thread and the processing thread. The `ringbuf` crate (v0.4) provides this with zero allocation after initialization.

```
Audio Callback Thread          Processing Thread
        │                              │
        ▼                              ▼
  ┌───────────┐    SPSC Ring     ┌───────────┐
  │ cpal      │───(lock-free)───▶│ VAD +     │
  │ callback  │   64KB buffer    │ Chunker   │
  └───────────┘                  └───────────┘
```

**Buffer sizing:** 64 KB ring = ~2 seconds of 16 kHz mono f32 audio. This provides headroom for processing jitter without dropping samples.

#### 4.1.3 Resampling

If the system default input device does not natively support 16 kHz, we resample using the `rubato` crate (0.16.x), which provides high-quality async resampling with SIMD acceleration. The resampling happens in the audio callback to keep the processing thread simple.

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

**Choice: Whisper Large V3 Turbo (Q5_0 quantized).** The 6x speed improvement over V3 with only ~1% WER degradation makes it the clear winner for real-time dictation. The Q5_0 quantization reduces disk and VRAM footprint further while maintaining accuracy.

On the RTX 4090, this model processes audio at roughly 300x real-time — a 10-second utterance completes in ~33ms. On the M4 Pro with Metal, expect ~80x real-time — the same utterance in ~125ms.

#### 4.3.2 whisper-rs Integration

```rust
use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};

pub struct AsrEngine {
    ctx: WhisperContext,
}

impl AsrEngine {
    pub fn new(model_path: &Path, use_gpu: bool) -> Result<Self> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        // Flash attention for faster decoding (supported in recent whisper.cpp)
        params.flash_attn(true);
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap(),
            params,
        )?;
        Ok(Self { ctx })
    }

    pub fn transcribe(&self, audio_pcm: &[f32]) -> Result<String> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en")); // or None for auto-detect
        params.set_no_speech_thold(0.6);
        params.set_suppress_non_speech_tokens(true);
        params.set_single_segment(true);    // low-latency: one segment
        params.set_no_context(true);         // no cross-segment context
        params.set_n_threads(4);             // CPU threads (GPU does heavy lifting)

        let mut state = self.ctx.create_state()?;
        state.full(params, audio_pcm)?;

        let n_segments = state.full_n_segments()?;
        let mut text = String::new();
        for i in 0..n_segments {
            text.push_str(state.full_get_segment_text(i)?.as_str());
        }
        Ok(text.trim().to_string())
    }
}
```

#### 4.3.3 Streaming Strategy

We do **not** use Whisper in a true streaming mode (which degrades accuracy significantly). Instead, we use a "chunked-batch" approach:

1. VAD detects speech segments (typically 1–10 seconds).
2. Each segment is transcribed as a complete batch.
3. Partial results are shown from the VAD state (waveform indicator).
4. Final text appears when the segment completes.

This keeps Whisper in its optimal operating mode (batch) while still feeling responsive because utterances are naturally short in dictation.

For longer continuous speech, we force-segment at 10 seconds and stitch results, using a 1-second overlap for context continuity.

### 4.4 LLM Post-Processor

The raw transcript from Whisper is good but not polished. The LLM handles:

1. **Filler word removal** — "um", "uh", "like", "you know"
2. **Punctuation and capitalization** — Whisper's punctuation is decent but inconsistent
3. **Course correction** — "let's meet Tuesday, wait no, Wednesday" → "let's meet Wednesday"
4. **Formatting** — numbers, dates, email addresses, code identifiers
5. **Tone adaptation** — (v2 goal) adjust formality based on active application
6. **Command detection** — "delete that", "new line", "select all" → OS actions

#### 4.4.1 Model Selection

| Model | Params | VRAM (Q4_K_M) | Tok/s (4090) | Tok/s (M4 Pro) | Quality |
|---|---|---|---|---|---|
| Phi-3.5 Mini | 3.8B | ~2.5 GB | ~120 | ~45 | Good |
| **Qwen 2.5 3B Instruct** | **3B** | **~2.2 GB** | **~150** | **~55** | **Excellent** |
| Llama 3.2 3B | 3B | ~2.2 GB | ~140 | ~50 | Good |
| Gemma 2 2B | 2.6B | ~1.8 GB | ~170 | ~65 | Decent |

**Choice: Qwen 2.5 3B Instruct (Q4_K_M).** Best instruction-following at the 3B tier, excellent at text editing/formatting tasks, multilingual-capable for future expansion. At Q4_K_M quantization, it fits comfortably alongside Whisper Turbo with room to spare on both the 4090 (24 GB) and M4 Pro (24 GB unified).

Combined VRAM budget: ~3 GB (Whisper Turbo) + ~2.2 GB (Qwen 2.5 3B) = **~5.2 GB**. Leaves 18+ GB free on both machines.

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

#### 4.4.3 Inference Strategy

The LLM processes each utterance (typically 5–50 tokens of raw transcript) and produces cleaned output. At 150 tok/s on the 4090, a 30-token utterance completes in ~200ms. On M4 Pro at ~55 tok/s, the same takes ~550ms.

To keep latency down, we:

- Use a small context window (2048 tokens max).
- Keep the system prompt in the KV cache across calls (persistent session).
- Stream tokens to the text injector as they're generated.
- Use speculative decoding where supported (llama.cpp draft model).

```rust
pub struct PostProcessor {
    model: LlamaModel,
    session: LlamaSession, // persistent KV cache
}

impl PostProcessor {
    pub fn process(&mut self, raw_text: &str) -> Result<ProcessorOutput> {
        let prompt = format!(
            "Raw transcript: \"{}\"\nCleaned output:",
            raw_text
        );
        let mut output = String::new();
        for token in self.session.generate(&prompt, GenerateParams {
            max_tokens: 256,
            temperature: 0.1, // near-deterministic for editing tasks
            stop: &["\n", "Raw transcript:"],
        })? {
            output.push_str(&token);
        }

        // Parse command or return text
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

#[derive(Deserialize)]
pub struct VoiceCommand {
    pub cmd: String,
    pub args: Option<serde_json::Value>,
}
```

### 4.5 Text Injection

The text injector types the polished text into whatever application has focus, character by character, simulating keyboard input at the OS level.

#### 4.5.1 Windows Implementation

```rust
#[cfg(target_os = "windows")]
mod injector {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    pub fn inject_text(text: &str) -> Result<()> {
        let chars: Vec<u16> = text.encode_utf16().collect();
        let mut inputs: Vec<INPUT> = Vec::with_capacity(chars.len() * 2);

        for ch in &chars {
            // Key down
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
            // Key up
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

#### 4.5.2 macOS Implementation

```rust
#[cfg(target_os = "macos")]
mod injector {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    pub fn inject_text(text: &str) -> Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)?;

        for ch in text.chars() {
            let event = CGEvent::new_keyboard_event(source.clone(), 0, true)?;
            let buf = [ch as u16];
            event.set_string_from_utf16_unchecked(&buf);
            event.post(CGEventTapLocation::HID);

            let event_up = CGEvent::new_keyboard_event(source.clone(), 0, false)?;
            event_up.post(CGEventTapLocation::HID);
        }
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

### 4.6 Frontend (Overlay HUD)

The frontend is a minimal, always-on-top overlay that shows recording state and transcript feedback. It is intentionally small — most interaction happens via the hotkey and the user's active application.

#### 4.6.1 UI States

```
┌─────────────────────────────────────────┐
│  IDLE           VoxFlow  ▾  [≡]         │
│  Press [Fn] to start dictating          │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│  ● LISTENING    VoxFlow  ▾  [≡]         │
│  ████████░░░░░░░░  (waveform animation) │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│  ⟳ PROCESSING   VoxFlow  ▾  [≡]         │
│  "let's meet wednesday at three pm"     │
└─────────────────────────────────────────┘

┌─────────────────────────────────────────┐
│  ✓ INJECTED     VoxFlow  ▾  [≡]         │
│  Let's meet Wednesday at 3 PM.          │
└─────────────────────────────────────────┘
```

#### 4.6.2 Component Architecture

```
src/
├── App.tsx                  # Root component, state machine
├── components/
│   ├── OverlayHud.tsx       # Floating pill overlay
│   ├── WaveformVisualizer.tsx  # Real-time audio waveform
│   ├── TranscriptDisplay.tsx   # Raw → polished text transition
│   ├── SettingsPanel.tsx       # Full settings UI
│   ├── DictionaryEditor.tsx    # Custom words/phrases
│   └── HistoryView.tsx         # Past transcriptions
├── hooks/
│   ├── useAudioState.ts     # Tauri event subscription for audio state
│   ├── useTranscript.ts     # Transcript streaming from backend
│   └── useSettings.ts       # Persistent settings via Tauri store
├── lib/
│   ├── tauri.ts             # Typed Tauri invoke/event wrappers
│   └── commands.ts          # Command definitions
└── styles/
    └── app.css              # Tailwind base + custom animations
```

#### 4.6.3 Tauri IPC Contract

Frontend ↔ Backend communication via Tauri commands and events:

**Commands (frontend → backend):**

```typescript
// Start/stop recording
invoke('toggle_recording'): Promise<void>

// Get current state
invoke('get_state'): Promise<AppState>

// Update settings
invoke('update_settings', { settings: Settings }): Promise<void>

// Get transcript history
invoke('get_history', { limit: number }): Promise<TranscriptEntry[]>

// Add word to custom dictionary
invoke('add_dictionary_word', { word: string, replacement?: string }): Promise<void>

// List available audio input devices
invoke('list_audio_devices'): Promise<AudioDevice[]>

// Select audio input device
invoke('set_audio_device', { deviceId: string }): Promise<void>
```

**Events (backend → frontend):**

```typescript
// Audio state changes
listen('audio-state', (e: { state: 'idle' | 'listening' | 'processing' | 'injecting' }) => void)

// Real-time audio level (for waveform)
listen('audio-level', (e: { rms: number, peak: number }) => void)

// Raw transcript available
listen('transcript-raw', (e: { text: string, segment_id: string }) => void)

// Polished transcript available
listen('transcript-polished', (e: { text: string, segment_id: string }) => void)

// Error
listen('error', (e: { message: string, code: string }) => void)
```

---

## 5. Data Flow

### 5.1 Happy Path (End-to-End)

```
Time(ms)  Event
────────  ──────────────────────────────────────────────────
   0      User presses hotkey (Fn / CapsLock / custom)
   1      Tauri global shortcut fires → toggle_recording()
   2      Audio capture begins via cpal
   5      First 512-sample window → Silero VAD
  32      VAD: speech_prob = 0.02 (silence, waiting...)
 200      User starts speaking
 232      VAD: speech_prob = 0.91 → state = SPEAKING
 232      UI event: audio-state = "listening"
 232+     Audio accumulating in speech buffer
2500      User pauses (natural utterance boundary)
3000      VAD: 500ms silence → state = SILENT → emit segment
3000      UI event: audio-state = "processing"
3001      Speech buffer (2.3s of audio) → whisper.cpp
3035      Whisper returns: "um let's meet tuesday wait no wednesday at three pm"
3035      Raw transcript → LLM post-processor
3036      UI event: transcript-raw (for display)
3250      LLM returns: "Let's meet Wednesday at 3 PM."
3250      UI event: transcript-polished
3251      Text injector → types into active application
3280      UI event: audio-state = "listening" (still recording)
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

For extended dictation sessions, the user activates hands-free mode (double-press hotkey). In this mode, recording continues indefinitely, the VAD auto-segments, and each segment flows through the pipeline automatically. The user can exit by pressing the hotkey once.

### 5.3 Command Mode

When the user says "hey vox" (wake word) followed by a command, the pipeline routes to command execution instead of text injection. The LLM detects the command intent and returns a JSON command object.

Example: "hey vox, delete the last sentence" → `{"cmd": "delete_last_sentence"}`

Wake word detection is handled by a simple keyword spotter on the raw transcript, not a separate model. This avoids always-on audio processing when not dictating.

---

## 6. Project Structure

```
voxflow/
├── Cargo.toml                    # Workspace root
├── Cargo.lock
├── LICENSE                        # MIT
├── README.md
│
├── src-tauri/                    # Tauri Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json            # Tauri config (window, permissions, etc.)
│   ├── capabilities/
│   │   └── default.json           # Tauri v2 capability permissions
│   ├── icons/                     # App icons
│   ├── build.rs                   # Build script (link whisper.cpp, llama.cpp)
│   └── src/
│       ├── main.rs                # Tauri entry point
│       ├── lib.rs                 # Module declarations
│       ├── state.rs               # Global AppState (Arc<Mutex<...>>)
│       ├── commands.rs            # Tauri command handlers
│       ├── audio/
│       │   ├── mod.rs
│       │   ├── capture.rs         # cpal audio capture
│       │   ├── ring_buffer.rs     # SPSC ring buffer wrapper
│       │   └── resampler.rs       # rubato resampler
│       ├── vad/
│       │   ├── mod.rs
│       │   ├── silero.rs          # Silero VAD ONNX wrapper
│       │   └── chunker.rs         # Speech segment accumulator
│       ├── asr/
│       │   ├── mod.rs
│       │   └── whisper.rs         # whisper-rs wrapper
│       ├── llm/
│       │   ├── mod.rs
│       │   ├── processor.rs       # LLM post-processor
│       │   └── prompts.rs         # System prompts and templates
│       ├── injector/
│       │   ├── mod.rs
│       │   ├── windows.rs         # Win32 SendInput
│       │   ├── macos.rs           # CGEvent text injection
│       │   └── commands.rs        # Voice command → keystrokes
│       ├── dictionary/
│       │   ├── mod.rs
│       │   └── store.rs           # Custom word dictionary (SQLite)
│       ├── config/
│       │   ├── mod.rs
│       │   └── settings.rs        # User settings (serde + Tauri store)
│       └── pipeline/
│           ├── mod.rs
│           └── orchestrator.rs    # End-to-end pipeline coordinator
│
├── src/                           # Frontend (SolidJS + TypeScript)
│   ├── index.html
│   ├── main.tsx
│   ├── App.tsx
│   ├── components/
│   ├── hooks/
│   ├── lib/
│   └── styles/
│
├── models/                        # Git-ignored, downloaded at first run
│   ├── ggml-large-v3-turbo-q5_0.bin
│   ├── qwen2.5-3b-instruct-q4_k_m.gguf
│   └── silero_vad_v5.onnx
│
├── scripts/
│   ├── download-models.sh         # Model download script
│   ├── download-models.ps1        # Windows variant
│   └── benchmark.sh               # Latency benchmark harness
│
└── tests/
    ├── audio_fixtures/            # Test WAV files
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
- Rust 1.84+ (rustup)
- Node.js 22 LTS + pnpm 9.x
- CMake 3.28+

**Windows additional:**
- Visual Studio 2022 Build Tools (MSVC)
- CUDA Toolkit 12.6
- cuDNN 9.x

**macOS additional:**
- Xcode 16.x + Command Line Tools
- No additional GPU setup needed (Metal is automatic)

### 7.2 build.rs

The `build.rs` calls `tauri_build::build()`. GPU backends are handled by `whisper-rs` and `llama-cpp-rs` via Cargo feature flags — no manual linking required. On Windows with CUDA, ensure `CUDA_PATH` is set.

### 7.3 Cargo.toml Feature Flags

```toml
[workspace]
members = ["src-tauri"]

# src-tauri/Cargo.toml
[package]
name = "voxflow"
version = "1.0.0"
edition = "2024"

[features]
default = []
cuda = ["whisper-rs/cuda", "llama-cpp-rs/cuda"]
metal = ["whisper-rs/metal", "llama-cpp-rs/metal"]

[dependencies]
tauri = { version = "2.10", features = ["tray-icon"] }
tauri-plugin-global-shortcut = "2"
tauri-plugin-autostart = "2"
tauri-plugin-store = "2"
tauri-plugin-dialog = "2"
tauri-plugin-notification = "2"
tauri-plugin-shell = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Audio
cpal = "0.15"
ringbuf = "0.4"
rubato = "0.16"

# ML
ort = { version = "2", features = ["load-dynamic"] }
whisper-rs = "0.13"
llama-cpp-rs = "0.4"

# Platform
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.58", features = [
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
] }

[target.'cfg(target_os = "macos")'.dependencies]
objc2 = "0.5"
core-graphics = "0.24"

# Storage
rusqlite = { version = "0.32", features = ["bundled"] }
```

### 7.4 Build Commands

```bash
# Windows (CUDA)
cargo tauri build --features cuda

# macOS (Metal)
cargo tauri build --features metal

# Development
cargo tauri dev --features cuda    # or metal
```

---

## 8. Pipeline Orchestration

The orchestrator is the central coordinator that wires all components together and manages the async pipeline.

```rust
// src-tauri/src/pipeline/orchestrator.rs

use tokio::sync::{mpsc, broadcast};
use std::sync::Arc;

pub struct Pipeline {
    audio_capture: AudioCapture,
    vad: SileroVad,
    asr: AsrEngine,
    llm: PostProcessor,
    injector: TextInjector,
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
    pub async fn run(&mut self) -> Result<()> {
        let (segment_tx, mut segment_rx) = mpsc::channel::<Vec<f32>>(8);

        // Spawn audio capture + VAD on a dedicated thread (real-time priority)
        let audio_handle = self.spawn_audio_thread(segment_tx);

        // Process segments as they arrive
        while let Some(audio_segment) = segment_rx.recv().await {
            self.state_tx.send(PipelineState::Processing { raw_text: None })?;

            // ASR (CPU-bound, run on blocking thread pool)
            let raw_text = tokio::task::spawn_blocking({
                let asr = self.asr.clone();
                move || asr.transcribe(&audio_segment)
            }).await??;

            if raw_text.is_empty() {
                continue;
            }

            self.state_tx.send(PipelineState::Processing {
                raw_text: Some(raw_text.clone()),
            })?;

            // LLM post-processing
            let result = tokio::task::spawn_blocking({
                let mut llm = self.llm.clone();
                move || llm.process(&raw_text)
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

    fn spawn_audio_thread(
        &self,
        segment_tx: mpsc::Sender<Vec<f32>>,
    ) -> std::thread::JoinHandle<()> {
        let mut vad = self.vad.clone();
        let audio_config = self.audio_capture.config().clone();

        std::thread::Builder::new()
            .name("voxflow-audio".into())
            .spawn(move || {
                // Set thread to real-time priority
                #[cfg(target_os = "windows")]
                unsafe {
                    windows::Win32::System::Threading::SetThreadPriority(
                        windows::Win32::System::Threading::GetCurrentThread(),
                        windows::Win32::System::Threading::THREAD_PRIORITY_TIME_CRITICAL,
                    );
                }

                let mut speech_buffer: Vec<f32> = Vec::with_capacity(16000 * 30);
                let mut is_speaking = false;
                let mut silence_samples = 0u32;

                // Audio callback fills ring buffer; this thread drains it
                loop {
                    let chunk = audio_config.read_chunk(512);
                    let prob = vad.process(&chunk).unwrap_or(0.0);

                    if prob >= 0.5 {
                        is_speaking = true;
                        silence_samples = 0;
                        speech_buffer.extend_from_slice(&chunk);
                    } else if is_speaking {
                        silence_samples += 512;
                        speech_buffer.extend_from_slice(&chunk);

                        if silence_samples >= 8000 { // 500ms at 16kHz
                            // End of utterance
                            let segment = std::mem::take(&mut speech_buffer);
                            let _ = segment_tx.blocking_send(segment);
                            is_speaking = false;
                            silence_samples = 0;
                        }
                    }

                    // Force-segment at 10 seconds
                    if speech_buffer.len() > 160_000 {
                        let segment = std::mem::take(&mut speech_buffer);
                        let _ = segment_tx.blocking_send(segment);
                    }
                }
            })
            .expect("Failed to spawn audio thread")
    }
}
```

---

## 9. Custom Dictionary

Users need to teach VoxFlow proper nouns, technical terms, and custom substitutions.

### 9.1 Storage (SQLite)

```sql
CREATE TABLE dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    spoken TEXT NOT NULL UNIQUE,      -- what the user says
    written TEXT NOT NULL,            -- what should be typed
    category TEXT DEFAULT 'general',  -- general, name, technical, abbreviation
    use_count INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Examples:
INSERT INTO dictionary (spoken, written, category) VALUES
    ('vox flow', 'VoxFlow', 'name'),
    ('tauri', 'Tauri', 'technical'),
    ('rust lang', 'Rust', 'technical'),
    ('my email', 'engineer@example.com', 'abbreviation'),
    ('sig block', 'Best regards,\nJohn Smith\nSenior Engineer', 'abbreviation');
```

### 9.2 Integration

The dictionary is injected into the LLM prompt as additional context:

```
Custom dictionary (apply these substitutions):
- "vox flow" → "VoxFlow"
- "tauri" → "Tauri"
- "my email" → "engineer@example.com"
```

For high-frequency terms, we also apply simple string replacement on the raw transcript before it reaches the LLM, to reduce inference load.

---

## 10. Settings & Configuration

### 10.1 User Settings Schema

```typescript
interface Settings {
    // Audio
    inputDevice: string | null;       // null = system default
    noiseGate: number;                // 0.0–1.0, default 0.0 (disabled)

    // VAD
    vadThreshold: number;             // 0.0–1.0, default 0.5
    minSilenceMs: number;             // default 500
    minSpeechMs: number;              // default 250

    // ASR
    language: string;                 // "en", "auto", or BCP-47 code
    whisperModel: string;             // model filename

    // LLM
    llmModel: string;                 // model filename
    temperature: number;              // 0.0–1.0, default 0.1
    removeFillersEnabled: boolean;    // default true
    courseCorrectionEnabled: boolean; // default true
    punctuationEnabled: boolean;      // default true

    // Hotkey
    activationHotkey: string;         // default "Fn" or "CapsLock"
    holdToTalk: boolean;              // true = push-to-talk, false = toggle
    handsFreeDoublePress: boolean;    // default true

    // Appearance
    overlayPosition: 'bottom-center' | 'bottom-left' | 'bottom-right' | 'top-center';
    overlayOpacity: number;           // 0.0–1.0, default 0.85
    showRawTranscript: boolean;       // default false (debug mode)
    theme: 'system' | 'light' | 'dark';

    // Advanced
    maxSegmentMs: number;             // default 10000
    overlapMs: number;                // default 1000
    commandPrefix: string;            // default "hey vox"
}
```

### 10.2 Persistence

Settings are stored via the Tauri Store plugin (JSON file in the app data directory). The dictionary uses SQLite (via `rusqlite` with bundled SQLite).

```
# Windows
%APPDATA%/com.voxflow.app/settings.json
%APPDATA%/com.voxflow.app/dictionary.db

# macOS
~/Library/Application Support/com.voxflow.app/settings.json
~/Library/Application Support/com.voxflow.app/dictionary.db
```

---

## 11. Model Management

### 11.1 First-Run Download

On first launch, VoxFlow checks for models and offers to download them:

```
┌─────────────────────────────────────────────────────────────┐
│                   Welcome to VoxFlow                        │
│                                                             │
│  VoxFlow needs to download AI models to work.               │
│  This is a one-time download (~3.5 GB total).               │
│                                                             │
│  ┌─────────────────────────────────────────────────────┐    │
│  │ ☑ Whisper Large V3 Turbo (Q5_0)     ~1.8 GB        │    │
│  │ ☑ Qwen 2.5 3B Instruct (Q4_K_M)    ~1.6 GB        │    │
│  │ ☑ Silero VAD v5                     ~1.1 MB        │    │
│  └─────────────────────────────────────────────────────┘    │
│                                                             │
│               [ Download & Continue ]                       │
│               [ I have models already → browse ]            │
└─────────────────────────────────────────────────────────────┘
```

### 11.2 Model Sources

| Model | Source | URL |
|---|---|---|
| Whisper Large V3 Turbo Q5_0 | Hugging Face (ggerganov) | `huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin` |
| Qwen 2.5 3B Instruct Q4_K_M | Hugging Face (bartowski) | `huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF/resolve/main/Qwen2.5-3B-Instruct-Q4_K_M.gguf` |
| Silero VAD v5 | GitHub (snakers4) | `github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx` |

### 11.3 Model Storage

```
# Windows
%LOCALAPPDATA%/com.voxflow.app/models/

# macOS
~/Library/Application Support/com.voxflow.app/models/
```

### 11.4 Model Swapping

Users can swap models via the settings panel. VoxFlow validates GGUF/GGML/ONNX format and runs a quick benchmark on load to verify GPU acceleration is working.

---

## 12. Error Handling Strategy

### 12.1 Error Categories

| Category | Examples | Recovery |
|---|---|---|
| Audio | Device disconnected, permission denied | Notify user, pause pipeline, retry on device change |
| Model | File missing, corrupt, OOM | Show download prompt, suggest smaller model |
| ASR | Whisper crash, empty result | Log, skip segment, continue |
| LLM | Timeout, OOM, garbled output | Fall back to raw transcript (inject without polishing) |
| Injection | Focus lost, permission denied | Buffer text, retry, show in overlay for manual copy |
| System | GPU driver crash, CUDA error | Restart pipeline, fall back to CPU |

### 12.2 Graceful Degradation Chain

```
Full Pipeline (GPU ASR + GPU LLM)
        │ LLM fails
        ▼
Reduced Pipeline (GPU ASR only, raw transcript injected)
        │ GPU fails
        ▼
CPU Pipeline (whisper.cpp CPU mode, no LLM)
        │ ASR fails entirely
        ▼
Error State (notify user, offer to restart)
```

### 12.3 Logging

We use the `tracing` crate with structured logging. Logs are written to:

```
# Rotated daily, 7-day retention
%LOCALAPPDATA%/com.voxflow.app/logs/  (Windows)
~/Library/Logs/com.voxflow.app/       (macOS)
```

Log levels: `ERROR` (always), `WARN` (default), `INFO` (verbose), `DEBUG` (development), `TRACE` (pipeline timing).

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

### 13.3 Benchmark Harness

```bash
# Run the latency benchmark
cargo test --release --features cuda benchmark_ -- --nocapture

# Benchmark output format:
# [BENCH] audio_capture_latency: p50=0.8ms p99=2.1ms
# [BENCH] vad_inference:         p50=0.3ms p99=0.9ms
# [BENCH] whisper_5s_audio:      p50=38ms  p99=52ms
# [BENCH] llm_30_tokens:         p50=180ms p99=220ms
# [BENCH] text_injection:        p50=15ms  p99=28ms
# [BENCH] e2e_pipeline:          p50=245ms p99=310ms
```

---

## 14. Security & Privacy

### 14.1 Threat Model

| Threat | Mitigation |
|---|---|
| Audio exfiltration | All processing local. No network calls after model download. Firewall the app. |
| Model tampering | SHA-256 checksum verification on download. Models are read-only after download. |
| Keystroke injection abuse | Injection only active when user explicitly activates recording. System tray shows state. |
| Transcript leakage | Transcript history is stored locally in SQLite with optional encryption (SQLCipher). |
| Malicious model | Only download from pinned Hugging Face URLs. Verify file hashes. |

### 14.2 Permissions

**Windows:**
- Microphone access (prompted by OS)
- No admin/elevated privileges required

**macOS:**
- Microphone access (Info.plist + runtime prompt)
- Accessibility permission (for CGEvent text injection — prompted by OS)
- Input Monitoring permission (for global hotkeys)

These are configured in `tauri.conf.json` and the macOS `Info.plist`:

```json
{
  "bundle": {
    "macOS": {
      "entitlements": "./Entitlements.plist",
      "infoPlist": {
        "NSMicrophoneUsageDescription": "VoxFlow needs microphone access for voice dictation.",
        "NSAppleEventsUsageDescription": "VoxFlow needs accessibility access to type text into other applications."
      }
    }
  }
}
```

### 14.3 Audio Data Policy

- Audio is processed in memory and immediately discarded after transcription.
- No audio is written to disk at any point.
- Transcript history can be disabled entirely in settings.
- The "clear history" button performs a secure delete (overwrite + VACUUM on SQLite).

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
async fn test_full_pipeline_hello_world() {
    let pipeline = TestPipeline::new().await;
    let audio = load_wav("fixtures/hello_world.wav");
    let result = pipeline.process_segment(&audio).await.unwrap();
    assert_eq!(result, "Hello, world.");
}

#[tokio::test]
async fn test_course_correction() {
    let pipeline = TestPipeline::new().await;
    let audio = load_wav("fixtures/correction_tuesday_wednesday.wav");
    let result = pipeline.process_segment(&audio).await.unwrap();
    assert!(result.contains("Wednesday"));
    assert!(!result.contains("Tuesday"));
}

#[tokio::test]
async fn test_filler_removal() {
    let pipeline = TestPipeline::new().await;
    let audio = load_wav("fixtures/um_uh_like.wav");
    let result = pipeline.process_segment(&audio).await.unwrap();
    assert!(!result.contains("um"));
    assert!(!result.contains("uh"));
    assert!(!result.to_lowercase().contains("like,"));
}

#[tokio::test]
async fn test_voice_command_delete() {
    let pipeline = TestPipeline::new().await;
    let audio = load_wav("fixtures/delete_that.wav");
    let result = pipeline.process_segment_raw(&audio).await.unwrap();
    assert!(matches!(result, ProcessorOutput::Command(_)));
}
```

### 15.3 Performance Tests

- **Latency regression test:** Assert e2e latency < 500ms (4090) / < 1000ms (M4 Pro) on a standard 5-second audio clip.
- **Memory leak test:** Run 1000 segments through the pipeline, assert RSS stays within 2x of baseline.
- **VRAM leak test:** Monitor `nvidia-smi` / Metal memory after 1000 segments.

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

| Platform | Format | Signing |
|---|---|---|
| Windows | `.msi` installer + portable `.exe` | Self-signed (dev), EV code sign (release) |
| macOS | `.dmg` with signed `.app` bundle | Apple Developer ID (notarized) |

Tauri v2 handles both via `cargo tauri build`.

### 16.2 Auto-Update

Tauri's built-in updater plugin checks for updates on launch (configurable). Updates are signed with an Ed25519 key pair.

```json
// tauri.conf.json
{
  "plugins": {
    "updater": {
      "active": true,
      "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ...",
      "endpoints": [
        "https://releases.voxflow.app/{{target}}/{{arch}}/{{current_version}}"
      ]
    }
  }
}
```

### 16.3 First-Run Experience

1. User installs VoxFlow (< 15 MB).
2. First launch: welcome screen + model download (~3.5 GB, progress bar).
3. GPU detection + quick benchmark (5 seconds).
4. Accessibility permission prompts (macOS).
5. Hotkey configuration.
6. Ready to dictate.

---

## 17. Roadmap

### v1.0 — Foundation (This Document)

- [x] Core pipeline: Audio → VAD → ASR → LLM → Inject
- [x] Windows (CUDA) + macOS (Metal)
- [x] Hold-to-talk + toggle + hands-free modes
- [x] Custom dictionary
- [x] Voice commands (basic set)
- [x] System tray + overlay HUD
- [x] Settings panel
- [x] Transcript history

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
- [ ] Moonshine integration for ultra-low-latency languages

### v2.0 — Intelligence

- [ ] Context-aware tone (formal in email, casual in Slack)
- [ ] Screen context via screenshot (à la Wispr Flow)
- [ ] iOS companion app (Tauri v2 mobile)
- [ ] Shared team dictionaries
- [ ] Custom fine-tuned ASR models

---

## 18. Appendix

### A. Key Crate Versions (Pinned)

```toml
# Cargo.toml — verified compatible set as of 2026-02-04
tauri = "2.10"
cpal = "0.15"
ringbuf = "0.4"
rubato = "0.16"
ort = "2.0"
whisper-rs = "0.13"
llama-cpp-rs = "0.4"
serde = "1.0"
serde_json = "1.0"
tokio = "1.43"
anyhow = "1.0"
tracing = "0.1"
rusqlite = "0.32"
windows = "0.58"
objc2 = "0.5"
core-graphics = "0.24"
```

### B. GGML Quantization Reference

| Quantization | Bits/Weight | Size (3B model) | Quality Loss | Speed Gain |
|---|---|---|---|---|
| F16 | 16 | ~6 GB | None | Baseline |
| Q8_0 | 8 | ~3 GB | Negligible | ~1.5x |
| Q5_K_M | 5.5 | ~2.2 GB | Minimal | ~2x |
| **Q4_K_M** | **4.5** | **~1.8 GB** | **Small** | **~2.5x** |
| Q3_K_M | 3.5 | ~1.4 GB | Noticeable | ~3x |
| Q2_K | 2.5 | ~1.1 GB | Significant | ~3.5x |

### C. Whisper Model Size Reference

| Model | Params | Disk (Q5_0) | VRAM (Q5_0) | English WER |
|---|---|---|---|---|
| Tiny | 39M | ~40 MB | ~100 MB | ~15% |
| Base | 74M | ~80 MB | ~200 MB | ~11% |
| Small | 244M | ~250 MB | ~500 MB | ~9.5% |
| Medium | 769M | ~800 MB | ~1.5 GB | ~8.5% |
| Large V3 | 1.55B | ~1.6 GB | ~3 GB | ~7.4% |
| **Large V3 Turbo** | **809M** | **~900 MB** | **~1.8 GB** | **~8%** |

### D. Alternative ASR Engines (Evaluated)

| Engine | Verdict | Notes |
|---|---|---|
| **whisper.cpp** | **✅ Selected** | Best ecosystem, CUDA+Metal, active maintenance, Rust bindings |
| Moonshine | ✅ v2 candidate | Best for edge/mobile. Overkill on 4090/M4 Pro, but ideal for future iOS. |
| faster-whisper | ⚠️ Python only | CTranslate2 backend, excellent perf, but Python runtime dependency. |
| Canary Qwen 2.5B | ⚠️ Accuracy king | #1 on Open ASR, but NeMo dependency is heavy, no simple C FFI. |
| Parakeet TDT | ⚠️ Speed king | 2000x RTFx, but NeMo-only and English-only. |
| Distil-Whisper | ⚠️ Good tradeoff | 6x faster than V3, but no ggml format, Python-native. |

### E. Alternative LLM Models (Evaluated)

| Model | Verdict | Notes |
|---|---|---|
| **Qwen 2.5 3B Instruct** | **✅ Selected** | Best instruction following at 3B, multilingual, fast |
| Phi-3.5 Mini 3.8B | ⚠️ Close second | Slightly larger, good at structured output |
| Llama 3.2 3B | ⚠️ Good | Strong general capability, weaker on formatting tasks |
| Gemma 2 2B | ⚠️ Smallest | Fastest inference, but quality drops on edge cases |
| SmolLM2 1.7B | ⚠️ Ultra-small | For extreme resource constraints only |

### F. Accessibility Permissions Setup (macOS)

VoxFlow requires two macOS permissions that cannot be programmatically requested — the user must grant them manually:

1. **Accessibility** (System Settings → Privacy & Security → Accessibility)
   - Required for: CGEvent text injection into other applications
   - VoxFlow will show a dialog with instructions if not granted

2. **Input Monitoring** (System Settings → Privacy & Security → Input Monitoring)
   - Required for: Global hotkey capture when VoxFlow is not focused
   - Tauri's global shortcut plugin triggers the OS prompt automatically

### G. GPU Detection Logic

On Windows, query `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader` to detect CUDA GPUs. On macOS, Metal is always available on Apple Silicon — query unified memory via `sysctl hw.memsize`. Fall back to CPU-only if no GPU is detected, and suggest the user install appropriate drivers.
*End of Design Document*
