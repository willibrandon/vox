# Feature 007: Pipeline Orchestration

**Status:** Not Started
**Dependencies:** 002-audio-capture, 003-voice-activity-detection, 004-speech-recognition, 005-llm-post-processing, 006-text-injection
**Design Reference:** Sections 5 (Data Flow), 8 (Pipeline Orchestration)
**Estimated Scope:** Async pipeline wiring, state machine, tokio channels, spawn_blocking

---

## Overview

Wire all pipeline components together into a single coordinated async system. The pipeline orchestrator manages the flow: Audio → VAD → ASR → LLM → Inject, handling async state transitions, GPU-bound work via `spawn_blocking`, and broadcasting state changes to the UI. This is the integration point where individual components become a working dictation engine.

---

## Requirements

### FR-001: Pipeline State

```rust
// crates/vox_core/src/pipeline/mod.rs

#[derive(Clone, Debug)]
pub enum PipelineState {
    /// Pipeline is idle, waiting for hotkey activation
    Idle,
    /// Actively listening to microphone, VAD processing
    Listening,
    /// Processing a speech segment (ASR + LLM)
    Processing { raw_text: Option<String> },
    /// Injecting polished text into target application
    Injecting { polished_text: String },
    /// Error state (recoverable — will retry or return to Idle)
    Error { message: String },
}
```

State transitions:

```
Idle ──(hotkey)──▶ Listening ──(speech segment)──▶ Processing
  ▲                    ▲                               │
  │                    │                               ▼
  │                    └────(segment done)────── Injecting
  │                                                    │
  └──────────────(hotkey off)──────────────────────────┘
```

### FR-002: Pipeline Struct

```rust
// crates/vox_core/src/pipeline/orchestrator.rs

use tokio::sync::{mpsc, broadcast};

pub struct Pipeline {
    audio_capture: AudioCapture,
    vad: SileroVad,
    asr: AsrEngine,
    llm: PostProcessor,
    injector: TextInjector,
    dictionary: DictionaryCache,
    state_tx: broadcast::Sender<PipelineState>,
}
```

**All components are required.** The pipeline does not start until:
- AudioCapture is ready (device detected and configured)
- SileroVad model is loaded (ONNX)
- AsrEngine model is loaded (Whisper on GPU)
- PostProcessor model is loaded (Qwen on GPU)
- TextInjector is initialized (OS permissions checked)
- DictionaryCache is loaded (SQLite → memory)

No degraded modes. No fallbacks. No optional components. This is Constitution Principle III.

### FR-003: Pipeline Construction

```rust
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
    ) -> Self;
}
```

### FR-004: Pipeline Run Loop

```rust
impl Pipeline {
    pub async fn run(&mut self) -> Result<()> {
        let (segment_tx, mut segment_rx) = mpsc::channel::<Vec<f32>>(8);

        // Spawn audio thread with VAD processing
        let audio_handle = self.spawn_audio_thread(segment_tx);

        while let Some(audio_segment) = segment_rx.recv().await {
            self.state_tx.send(PipelineState::Processing { raw_text: None })?;

            // ASR (GPU-bound) — run in spawn_blocking
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

            // Get active application name for tone adaptation
            let active_app = self.injector.get_focused_app_name()
                .unwrap_or_else(|_| "Unknown".to_string());

            // LLM post-processing (GPU-bound) — run in spawn_blocking
            // Streams tokens to injector as they are generated
            let hints = self.dictionary.top_hints(50);
            let result = tokio::task::spawn_blocking({
                let llm = self.llm.clone();
                let text = substituted.clone();
                let app = active_app.clone();
                move || llm.process(&text, &hints, &app)
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

### FR-005: Activation Modes

Three recording activation modes (configured in settings):

1. **Hold-to-talk (default):** User holds hotkey → recording active. Release → recording stops, pipeline processes remaining audio.

2. **Toggle mode:** Press hotkey once → recording starts. Press again → recording stops.

3. **Hands-free mode:** Double-press hotkey → continuous recording. VAD auto-segments. Each segment flows through the pipeline automatically. Single press to exit.

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum ActivationMode {
    HoldToTalk,
    Toggle,
    HandsFree,
}

pub struct PipelineController {
    mode: ActivationMode,
    is_active: bool,
    pipeline: Pipeline,
}

impl PipelineController {
    pub fn on_hotkey_press(&mut self);
    pub fn on_hotkey_release(&mut self);
    pub fn on_double_press(&mut self);
    pub fn stop(&mut self);
}
```

### FR-006: State Broadcasting

Pipeline state changes are broadcast to all UI subscribers via `tokio::sync::broadcast`:

```rust
let (state_tx, _) = broadcast::channel::<PipelineState>(16);

// UI subscribes:
let mut state_rx = state_tx.subscribe();
tokio::spawn(async move {
    while let Ok(state) = state_rx.recv().await {
        // Update overlay HUD
    }
});
```

### FR-007: Transcript History

After successful injection, save the transcript to history:

```rust
pub struct TranscriptEntry {
    pub id: Uuid,
    pub raw_text: String,
    pub polished_text: String,
    pub target_app: String,  // Name of the focused application
    pub duration_ms: u32,    // Audio segment duration
    pub latency_ms: u32,     // End-to-end processing time
    pub created_at: String,  // ISO 8601
}
```

---

## Data Flow: End-to-End Timing

### Happy Path (RTX 4090)

```
Time(ms)  Event
────────  ──────────────────────────────────────────────
   0      User presses hotkey → audio capture begins
   5      First 512-sample window → Silero VAD
  32      VAD: speech_prob = 0.02 (silence, waiting)
 200      User starts speaking
 232      VAD: speech_prob = 0.91 → SPEAKING
2500      User pauses (natural utterance boundary)
3000      VAD: 500ms silence → EMIT SEGMENT
3001      Segment (2.3s audio) → whisper.cpp
3035      Whisper returns raw transcript (~35ms)
3036      Raw → LLM post-processor
3250      LLM returns polished text (~215ms)
3251      Text injector → types into active application
3280      Done. Pipeline → Listening
```

**Total: ~165ms** from end-of-utterance to text appearing (4090)
**Total: ~430ms** on M4 Pro

---

## Acceptance Criteria

- [ ] Full pipeline: audio → VAD → ASR → LLM → inject works end-to-end
- [ ] Hold-to-talk mode works correctly
- [ ] Toggle mode works correctly
- [ ] Hands-free mode auto-segments and processes continuously
- [ ] Pipeline state changes broadcast to subscribers
- [ ] Empty/silent segments are correctly skipped
- [ ] Voice commands route to command execution (not text injection)
- [ ] Dictionary substitutions are applied before LLM processing
- [ ] Transcript history is saved after successful injection
- [ ] Pipeline starts only when ALL components are loaded
- [ ] Zero compiler warnings

---

## Testing Requirements

### Integration Tests (require all models, `#[ignore]`)

| Test | Description |
|---|---|
| `test_full_pipeline_hello_world` | "hello world" WAV → "Hello, world." injected |
| `test_pipeline_filler_removal` | "um let's um meet" → "Let's meet." |
| `test_pipeline_course_correction` | "tuesday wait no wednesday" → "Wednesday" |
| `test_pipeline_command` | "delete that" → command executed, not text injected |
| `test_pipeline_empty_audio` | Silent audio → no injection |
| `test_pipeline_multiple_segments` | WAV with 3 utterances → 3 separate injections |

---

## Performance Targets

| Metric | RTX 4090 | M4 Pro |
|---|---|---|
| End-to-end (utterance → text) | < 300 ms | < 750 ms |
| VRAM / unified (Whisper + Qwen + VAD) | < 6 GB | < 6 GB |
| System RAM | < 500 MB | < 500 MB |
| CPU (idle) | < 2% | < 2% |
| CPU (active dictation) | < 15% | < 20% |
