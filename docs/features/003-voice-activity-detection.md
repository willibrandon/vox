# Feature 003: Voice Activity Detection

**Status:** Not Started
**Dependencies:** 002-audio-capture
**Design Reference:** Section 4.2 (Voice Activity Detection)
**Estimated Scope:** Silero VAD v5 via ONNX Runtime, streaming state machine, speech chunker

---

## Overview

Implement the Voice Activity Detection (VAD) subsystem using Silero VAD v5. The VAD is the gatekeeper of the entire pipeline — it determines when the user is speaking and segments the continuous audio stream into discrete utterances that get dispatched to the ASR engine. It runs on CPU (not GPU) and must process each 32ms window in sub-millisecond time.

---

## Requirements

### FR-001: Silero VAD Configuration

```rust
// crates/vox_core/src/vad/mod.rs

pub struct VadConfig {
    pub threshold: f32,           // 0.5 — speech probability threshold
    pub min_speech_ms: u32,       // 250 — minimum speech duration to accept
    pub min_silence_ms: u32,      // 500 — silence duration to end an utterance
    pub max_speech_ms: u32,       // 30_000 — force-segment long speech
    pub speech_pad_ms: u32,       // 100 — padding around detected speech
    pub window_size_samples: u32, // 512 — Silero expects exactly 512 at 16 kHz
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech_ms: 250,
            min_silence_ms: 500,
            max_speech_ms: 30_000,
            speech_pad_ms: 100,
            window_size_samples: 512,
        }
    }
}
```

### FR-002: ONNX Runtime Integration (ort 2.0.0-rc.11)

Implement `SileroVad` in `crates/vox_core/src/vad/silero.rs`:

```rust
use ort::{Session, SessionBuilder, Value};

pub struct SileroVad {
    session: Session,
    state: Vec<f32>,  // Hidden state carried across calls: 2 layers × 1 batch × 128 hidden
    sample_rate: i64, // Always 16000
}

impl SileroVad {
    /// Load the Silero VAD v5 ONNX model.
    /// Model file: silero_vad_v5.onnx (~1.1 MB)
    pub fn new(model_path: &Path) -> Result<Self>;

    /// Process a single 512-sample window (32ms at 16 kHz).
    /// Returns speech probability in [0.0, 1.0].
    /// Hidden state is updated internally between calls.
    pub fn process(&mut self, audio: &[f32]) -> Result<f32>;

    /// Reset hidden state (call when starting a new session).
    pub fn reset(&mut self);
}
```

**ONNX model inputs:**
- `input`: f32 tensor shape `[1, window_size]` — the audio samples
- `sr`: i64 tensor shape `[]` — sample rate (16000)
- `h`: f32 tensor shape `[2, 1, 128]` — hidden state from previous call

**ONNX model outputs:**
- `output`: f32 tensor shape `[1, 1]` — speech probability
- `hn`: f32 tensor shape `[2, 1, 128]` — updated hidden state

The hidden state must be preserved across calls and updated after each inference. Reset to zeros when starting a new session.

### FR-003: Streaming State Machine

Implement `VadStateMachine` in `crates/vox_core/src/vad/mod.rs`:

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

States:
- **Silent**: Speech probability below threshold. Waiting for speech.
- **Speaking**: Speech probability above threshold. Accumulating audio.
- **Emit**: Transition from Speaking → Silent with sufficient silence. Dispatch accumulated audio.

Transitions:
- Silent → Speaking: `speech_prob >= threshold` for at least one window
- Speaking → Silent: `speech_prob < threshold` for `min_silence_ms` consecutive milliseconds
- Speaking → Emit (force): `speech_duration >= max_speech_ms` (force-segment to prevent unbounded memory)

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum VadState {
    Silent,
    Speaking {
        start_sample: usize,
        speech_duration_ms: u32,
    },
}

pub struct VadStateMachine {
    config: VadConfig,
    state: VadState,
    silence_duration_ms: u32,
    total_samples_processed: usize,
}

impl VadStateMachine {
    pub fn new(config: VadConfig) -> Self;

    /// Feed a speech probability and get back an optional event.
    pub fn update(&mut self, speech_prob: f32) -> Option<VadEvent>;

    pub fn state(&self) -> &VadState;
    pub fn reset(&mut self);
}

pub enum VadEvent {
    /// Speech started. Begin accumulating audio.
    SpeechStart,
    /// Speech ended. The accumulated segment is ready for ASR.
    SpeechEnd {
        duration_ms: u32,
    },
    /// Force-segmented due to max_speech_ms. More speech may follow.
    ForceSegment {
        duration_ms: u32,
    },
}
```

### FR-004: Speech Chunker

Implement `SpeechChunker` in `crates/vox_core/src/vad/chunker.rs`:

The chunker accumulates raw audio samples during a speech segment and dispatches complete segments when the VAD signals speech end.

```rust
pub struct SpeechChunker {
    buffer: Vec<f32>,         // Accumulated speech samples
    config: VadConfig,
    is_accumulating: bool,
}

impl SpeechChunker {
    pub fn new(config: VadConfig) -> Self;

    /// Feed audio samples and VAD event. Returns completed segments.
    pub fn feed(&mut self, samples: &[f32], event: Option<VadEvent>) -> Option<Vec<f32>>;

    /// Flush any remaining audio (e.g., when stopping recording).
    pub fn flush(&mut self) -> Option<Vec<f32>>;
}
```

Speech padding: When `SpeechEnd` fires, include `speech_pad_ms` of audio before and after the detected speech boundaries for context.

For force-segmented long speech (> 10 seconds), include a 1-second overlap with the next segment for context continuity during ASR stitching.

### FR-005: VAD Processing Loop

The VAD processing loop runs on the processing thread (not the audio callback):

1. Read samples from ring buffer consumer
2. If resampling needed, resample to 16 kHz
3. Feed 512-sample windows to Silero VAD
4. Feed speech probability to state machine
5. Feed samples + events to chunker
6. When a complete segment is ready, send it to the ASR engine via channel

```rust
pub async fn vad_processing_loop(
    consumer: HeapCons<'static, f32>,
    resampler: Option<AudioResampler>,
    vad: SileroVad,
    state_machine: VadStateMachine,
    chunker: SpeechChunker,
    segment_tx: mpsc::Sender<Vec<f32>>,
) -> Result<()>;
```

---

## Acceptance Criteria

- [ ] Silero VAD model loads successfully from ONNX file
- [ ] VAD returns speech probabilities in [0.0, 1.0] for each 512-sample window
- [ ] State machine correctly transitions between Silent/Speaking states
- [ ] Speech segments are emitted when silence exceeds `min_silence_ms`
- [ ] Long speech (> 30s) is force-segmented
- [ ] Speech padding is applied before/after detected speech
- [ ] Hidden state is preserved across VAD calls within a session
- [ ] Hidden state resets correctly between sessions
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_vad_silent_audio` | Feed silence, verify speech_prob < 0.1 |
| `test_vad_speech_audio` | Feed known speech WAV, verify speech_prob > 0.7 |
| `test_vad_hidden_state_persistence` | Verify state carries across calls |
| `test_state_machine_silent_to_speaking` | Threshold crossing triggers SpeechStart |
| `test_state_machine_speaking_to_silent` | Silence duration triggers SpeechEnd |
| `test_state_machine_force_segment` | 30s+ speech triggers ForceSegment |
| `test_state_machine_min_speech` | Speech < 250ms is discarded |
| `test_chunker_accumulates` | Samples accumulate during speech |
| `test_chunker_emits_on_end` | Complete segment returned on SpeechEnd |
| `test_chunker_padding` | Output includes pad_ms before/after speech |
| `test_chunker_flush` | Remaining samples returned on flush |

### Integration Tests (require model file)

| Test | Description |
|---|---|
| `test_vad_end_to_end` | Feed WAV with speech + silence, verify correct segmentation |
| `test_vad_multiple_utterances` | Feed WAV with 3 utterances, verify 3 segments emitted |

---

## Performance Targets

| Metric | Target |
|---|---|
| VAD inference per 512-sample window | < 1 ms (CPU) |
| State machine update | < 0.01 ms |
| Memory (VAD model + state) | < 5 MB |
| Latency (audio → VAD decision) | < 5 ms |
