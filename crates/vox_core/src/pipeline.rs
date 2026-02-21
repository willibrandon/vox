//! Pipeline orchestration for the Vox dictation engine.
//!
//! Coordinates the full audio-to-text flow: Audio Capture → VAD → ASR →
//! Dictionary substitution → LLM post-processing → Text Injection. The pipeline
//! uses a three-tier threading model: cpal OS audio thread → dedicated VAD
//! processing thread (std::thread) → tokio async orchestrator with spawn_blocking
//! for GPU-bound ASR/LLM work.

/// Pipeline state enum and command types for state broadcasting.
pub mod state;
/// Pipeline orchestrator that coordinates the audio-to-text flow.
pub mod orchestrator;
/// Hotkey-to-command translation for activation modes.
pub mod controller;
/// Transcript persistence with SQLite-backed storage.
pub mod transcript;

pub use controller::{ActivationMode, PipelineController};
pub use orchestrator::Pipeline;
pub use state::{PipelineCommand, PipelineState};
pub use transcript::{TranscriptEntry, TranscriptStore};
