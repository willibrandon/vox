# API Contract: vox_core::vad

**Feature**: 003-voice-activity-detection
**Date**: 2026-02-19

## Module Public Interface

### vad.rs — Configuration, State Machine, Processing Loop

```rust
/// Configuration for the voice activity detection subsystem.
pub struct VadConfig {
    pub threshold: f32,
    pub min_speech_ms: u32,
    pub min_silence_ms: u32,
    pub max_speech_ms: u32,
    pub speech_pad_ms: u32,
    pub window_size_samples: u32,
}

/// Current state of the VAD streaming state machine.
pub enum VadState {
    Silent,
    Speaking {
        start_sample: usize,
        speech_duration_ms: u32,
    },
}

/// Events emitted by the VAD state machine on state transitions.
pub enum VadEvent {
    SpeechStart,
    SpeechEnd { duration_ms: u32 },
    ForceSegment { duration_ms: u32 },
}

/// Streaming state machine that converts speech probabilities into
/// discrete speech boundary events.
pub struct VadStateMachine { /* ... */ }

impl VadStateMachine {
    pub fn new(config: VadConfig) -> Self;
    pub fn update(&mut self, speech_prob: f32) -> Option<VadEvent>;
    pub fn state(&self) -> &VadState;
    pub fn reset(&mut self);
}
```

### vad/silero.rs — ONNX Model Wrapper

```rust
/// Silero VAD v5 inference engine using ONNX Runtime.
pub struct SileroVad { /* ... */ }

impl SileroVad {
    /// Load the Silero VAD v5 ONNX model from disk.
    pub fn new(model_path: &Path) -> Result<Self>;

    /// Process a single 512-sample window (32ms at 16 kHz).
    /// Returns speech probability in [0.0, 1.0].
    pub fn process(&mut self, audio: &[f32]) -> Result<f32>;

    /// Reset hidden state to zeros for a new dictation session.
    pub fn reset(&mut self);
}
```

### vad/chunker.rs — Speech Segment Accumulator

```rust
/// Accumulates audio samples during speech segments and emits
/// complete padded segments for ASR.
pub struct SpeechChunker { /* ... */ }

impl SpeechChunker {
    pub fn new(config: VadConfig) -> Self;

    /// Feed audio samples and an optional VAD event.
    /// Returns a complete speech segment when ready.
    pub fn feed(&mut self, samples: &[f32], event: Option<&VadEvent>) -> Option<Vec<f32>>;

    /// Flush any buffered audio as a final segment.
    pub fn flush(&mut self) -> Option<Vec<f32>>;
}
```

## Re-exports from vox_core::vad

```rust
// crates/vox_core/src/vad.rs
pub mod silero;
pub mod chunker;

pub use silero::SileroVad;
pub use chunker::SpeechChunker;
// VadConfig, VadState, VadEvent, VadStateMachine defined in mod.rs
```

## Consumer Interface

The pipeline orchestrator (Feature 004+) wires the VAD into the pipeline:

```rust
// Consuming code (in pipeline.rs or similar)
use vox_core::vad::{VadConfig, SileroVad, VadStateMachine, SpeechChunker};
use vox_core::audio::{AudioCapture, AudioResampler};
use tokio::sync::mpsc;

// Setup
let config = VadConfig::default();
let vad = SileroVad::new(&model_path)?;
let state_machine = VadStateMachine::new(config.clone());
let chunker = SpeechChunker::new(config);
let (segment_tx, segment_rx) = mpsc::channel::<Vec<f32>>(4);

// The processing loop runs on a dedicated thread
// segment_rx is consumed by the ASR engine
```

## Error Contracts

| Function | Error Condition | Behavior |
|----------|----------------|----------|
| `SileroVad::new()` | Model file not found or corrupt | Returns `Err` — caller must handle (pipeline won't start) |
| `SileroVad::process()` | ONNX inference failure (extremely rare) | Returns `Err` — processing loop logs and skips window |
| `SpeechChunker::feed()` | None (infallible) | Always succeeds — pure buffer logic |
| `SpeechChunker::flush()` | None (infallible) | Returns `None` if buffer empty |
| `VadStateMachine::update()` | None (infallible) | Always succeeds — pure state logic |
