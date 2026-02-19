# Feature 004: Speech Recognition (ASR)

**Status:** Not Started
**Dependencies:** 003-voice-activity-detection
**Design Reference:** Section 4.3 (ASR Engine)
**Estimated Scope:** whisper.cpp via whisper-rs 0.15.1, model loading, transcription, chunked-batch strategy

---

## Overview

Implement the Automatic Speech Recognition engine using whisper.cpp (via whisper-rs 0.15.1 Rust bindings). The ASR takes complete speech segments from the VAD and transcribes them into raw text. It runs on GPU (CUDA on Windows, Metal on macOS) and must transcribe a 5-second utterance in under 50ms on RTX 4090 / 150ms on M4 Pro.

---

## Requirements

### FR-001: ASR Engine

```rust
// crates/vox_core/src/asr/whisper.rs

use whisper_rs::{WhisperContext, WhisperContextParameters, FullParams, SamplingStrategy};
use std::sync::{Arc, Mutex};

pub struct AsrEngine {
    /// WhisperContext is NOT thread-safe — must wrap in Arc<Mutex<>>
    ctx: Arc<Mutex<WhisperContext>>,
}
```

**Thread safety:** `WhisperContext` is **not** `Send` or `Sync`. It must be wrapped in `Arc<Mutex<>>`. A new `WhisperState` must be created for each transcription call — do not reuse state across calls.

### FR-002: Model Loading

```rust
impl AsrEngine {
    /// Load the Whisper model from disk.
    /// Model: Whisper Large V3 Turbo, ggml Q5_0 quantization
    /// File: ggml-large-v3-turbo-q5_0.bin (~900 MB on disk, ~1.8 GB VRAM)
    pub fn new(model_path: &Path, use_gpu: bool) -> Result<Self> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);
        // Flash attention is disabled by default in whisper-rs 0.15.1
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap(),
            params,
        )?;
        Ok(Self { ctx: Arc::new(Mutex::new(ctx)) })
    }
}
```

### FR-003: Transcription

```rust
impl AsrEngine {
    /// Transcribe a complete speech segment (f32 PCM, 16 kHz mono).
    /// Returns the raw transcript text.
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
            let segment_text = state.full_get_segment_text(i)?;
            text.push_str(&segment_text);
        }
        Ok(text.trim().to_string())
    }
}
```

**whisper-rs 0.15.1 API notes:**
- `full_n_segments()` returns `c_int` (a plain integer), NOT a `Result`. Do not try to unwrap it.
- whisper-rs 0.15.1 is from Codeberg, not crates.io.
- Flash attention is disabled by default. Do not enable it.
- Create a new `WhisperState` per transcription. State holds internal buffers that are not safe to reuse.

### FR-004: Transcription Parameters

| Parameter | Value | Rationale |
|---|---|---|
| `SamplingStrategy` | `Greedy { best_of: 1 }` | Fastest, good enough for clean audio |
| `language` | `"en"` | English only for v1.0 |
| `no_speech_thold` | `0.6` | Threshold for no-speech detection |
| `suppress_non_speech_tokens` | `true` | Suppress [music], [laughter] etc. |
| `single_segment` | `true` | Treat input as one segment (VAD already segmented) |
| `no_context` | `true` | No cross-utterance context (each segment is independent) |
| `n_threads` | `4` | CPU threads for non-GPU work |

### FR-005: Chunked-Batch Strategy

We do NOT use Whisper in true streaming mode (which degrades accuracy). Instead, we use a chunked-batch approach:

1. VAD detects speech segments (typically 1–10 seconds)
2. Each complete segment is transcribed as a batch
3. For long continuous speech (force-segmented at 10 seconds), stitch results with 1-second overlap for context continuity

Stitching algorithm for force-segmented speech:
- Previous segment ends with the last 1 second of audio
- Next segment starts from the force-segment point minus 1 second
- Compare the overlap region text and deduplicate

### FR-006: Model Selection

| Model | Params | VRAM (Q5_0) | Speed (4090) | Speed (M4 Pro) | WER |
|---|---|---|---|---|---|
| Whisper Large V3 Turbo | 809M | ~1.8 GB | ~300x real-time | ~80x real-time | ~8% |

**Choice rationale:** 6x faster than Large V3 with only ~1% WER degradation. On RTX 4090, a 10-second utterance completes in ~33ms. On M4 Pro with Metal, ~125ms. Well within latency budget.

### FR-007: Clone for Async

The `AsrEngine` must be cheaply cloneable (it wraps an `Arc<Mutex<>>`) so it can be moved into `tokio::task::spawn_blocking` for GPU-bound transcription:

```rust
impl Clone for AsrEngine {
    fn clone(&self) -> Self {
        Self { ctx: Arc::clone(&self.ctx) }
    }
}
```

---

## Acceptance Criteria

- [ ] Whisper model loads from disk with GPU acceleration
- [ ] Known speech WAV transcribes to expected text
- [ ] Empty/silent audio returns empty string (not an error)
- [ ] Multiple transcriptions work sequentially (new state per call)
- [ ] Model can be shared across threads via Arc<Mutex<>>
- [ ] Transcription runs in spawn_blocking without deadlocks
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests (require model file, `#[ignore]`)

| Test | Description |
|---|---|
| `test_asr_hello_world` | Transcribe "hello world" WAV fixture |
| `test_asr_empty_audio` | Empty/silent audio returns empty string |
| `test_asr_short_segment` | Very short speech (< 1 second) transcribes |
| `test_asr_long_segment` | 10-second segment transcribes correctly |
| `test_asr_sequential` | Multiple sequential transcriptions produce correct results |

### Performance Tests (require model file)

| Test | Description | Target |
|---|---|---|
| `bench_asr_5s_audio` | Transcribe 5s of speech | < 50 ms (4090), < 150 ms (M4 Pro) |
| `bench_asr_10s_audio` | Transcribe 10s of speech | < 100 ms (4090), < 300 ms (M4 Pro) |

---

## Performance Targets

| Metric | RTX 4090 | M4 Pro |
|---|---|---|
| Transcription (5s audio) | < 50 ms | < 150 ms |
| Transcription (10s audio) | < 100 ms | < 300 ms |
| VRAM usage | ~1.8 GB | ~1.8 GB (unified) |
| Model load time | < 5 s | < 5 s |
