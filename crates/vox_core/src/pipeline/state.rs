//! Pipeline state types for state broadcasting and command handling.
//!
//! Defines the operational states of the pipeline (broadcast to UI subscribers)
//! and the command vocabulary for controlling the pipeline's async run loop
//! from external hotkey handlers.

/// The operational state of the pipeline, broadcast to all UI subscribers
/// on every transition.
///
/// Each segment processing cycle produces a sequence of state transitions
/// (Listening → Processing → Injecting → Listening). Subscribers receive
/// every transition via `tokio::sync::broadcast`.
#[derive(Clone, Debug, PartialEq)]
pub enum PipelineState {
    /// Pipeline is loaded and waiting for hotkey activation.
    Idle,

    /// Microphone is active, VAD is processing audio windows.
    Listening,

    /// A speech segment is being processed through ASR and/or LLM.
    /// `raw_text` is None until ASR completes, then Some(transcript).
    Processing {
        /// Raw ASR output, present only after transcription completes.
        raw_text: Option<String>,
    },

    /// Polished text is being injected into the focused application.
    Injecting {
        /// The final text after LLM post-processing.
        polished_text: String,
    },

    /// A recoverable error occurred. Pipeline returns to Listening or Idle
    /// depending on whether the controller is still active.
    Error {
        /// Human-readable error description.
        message: String,
    },

    /// Text injection failed. The polished text is preserved for clipboard recovery.
    ///
    /// This state persists until the user clicks Copy (→ Idle) or presses the
    /// hotkey to start a new dictation (→ Listening). Uncopied text is lost
    /// on hotkey press.
    InjectionFailed {
        /// The polished text that failed to inject (available for Copy).
        polished_text: String,
        /// Human-readable error description.
        error: String,
    },
}

/// Commands sent from PipelineController to Pipeline via mpsc channel.
///
/// Decouples hotkey handling from the async run loop, avoiding `&mut` aliasing
/// between `run()` and hotkey handlers. The Pipeline's `run()` method uses
/// `tokio::select!` to listen for both segments and commands concurrently.
#[derive(Debug)]
pub enum PipelineCommand {
    /// Stop the pipeline after the current segment completes (FR-018).
    Stop,
    /// Cancel an active injection focus-retry task without stopping the pipeline.
    ///
    /// Sent when the user clicks Copy on buffered text or the overlay
    /// transitions to idle. Without this, the retry task would keep polling
    /// and could inject stale text into a later-focused window.
    CancelInjectionRetry,
}
