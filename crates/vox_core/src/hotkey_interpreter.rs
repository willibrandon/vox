//! Hotkey event interpreter for three activation modes.
//!
//! Maps hotkey press/release events to recording actions based on the active
//! [`ActivationMode`]. This is a pure synchronous state machine with no OS
//! dependencies, no timers, and no async — safe to use in unit tests.
//!
//! The interpreter does NOT track whether recording is active. The caller
//! passes `is_recording: bool` on each event, keeping the authoritative
//! recording state in the pipeline layer where it belongs.

use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Recording activation mode chosen by the user.
///
/// Determines how hotkey press/release events translate into start/stop
/// recording actions. Persisted in `settings.json` as a kebab-case string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationMode {
    /// Hold the hotkey to record; release to stop. This is the default mode.
    HoldToTalk,
    /// Press once to start recording; press again to stop.
    Toggle,
    /// Double-press (within 300ms) to start continuous VAD-segmented
    /// recording; single press to stop.
    HandsFree,
}

impl Default for ActivationMode {
    /// Returns [`ActivationMode::HoldToTalk`] as the default (FR-006).
    fn default() -> Self {
        ActivationMode::HoldToTalk
    }
}

/// Action produced by the hotkey interpreter after processing a press or
/// release event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    /// No action. Waiting for potential double-press, or the event was not
    /// actionable in the current mode.
    None,
    /// Begin a new recording session.
    StartRecording,
    /// End the current recording session and process remaining audio.
    StopRecording,
    /// Begin continuous VAD-segmented recording (hands-free mode).
    /// Dispatches the same pipeline action as [`StartRecording`] — VAD
    /// auto-segmentation is the default pipeline behavior. The distinct
    /// variant exists for logging and potential future differentiation.
    StartHandsFree,
}

/// Duration window for detecting a double-press in hands-free mode.
const DOUBLE_PRESS_WINDOW: std::time::Duration = std::time::Duration::from_millis(300);

/// Stateful interpreter that maps hotkey press/release events to recording
/// actions based on the active activation mode.
///
/// Pure state machine: no OS dependencies, no timers, no async. Lives on the
/// GPUI foreground thread alongside the hotkey polling loop.
pub struct HotkeyInterpreter {
    /// Current activation mode (from settings).
    mode: ActivationMode,
    /// Timestamp of the most recent key press. Used for hands-free
    /// double-press detection. Initialized to a time far in the past so
    /// the first press is never mistaken for a double-press.
    last_press_time: Instant,
}

impl HotkeyInterpreter {
    /// Create a new interpreter with the given activation mode.
    pub fn new(mode: ActivationMode) -> Self {
        Self {
            mode,
            // Initialize to a point far enough in the past that the first
            // press can never be within the 300ms double-press window.
            last_press_time: Instant::now() - std::time::Duration::from_secs(10),
        }
    }

    /// Process a key press event.
    ///
    /// `is_recording` indicates whether a recording session is currently
    /// active. The interpreter does not track this internally — the caller
    /// knows the authoritative recording state from `RecordingSession`.
    pub fn on_press(&mut self, is_recording: bool) -> HotkeyAction {
        match self.mode {
            ActivationMode::HoldToTalk => {
                // Press always starts a new recording. If one is already
                // active, the caller starts a new session (old one continues
                // processing in the background).
                HotkeyAction::StartRecording
            }
            ActivationMode::Toggle => {
                if is_recording {
                    HotkeyAction::StopRecording
                } else {
                    HotkeyAction::StartRecording
                }
            }
            ActivationMode::HandsFree => {
                if is_recording {
                    // Any press while recording stops it. Reset the
                    // timestamp so the next press starts a fresh
                    // double-press detection cycle (prevents the stop
                    // press from counting as the first press of a new
                    // double-press sequence).
                    self.last_press_time =
                        Instant::now() - std::time::Duration::from_secs(10);
                    HotkeyAction::StopRecording
                } else {
                    let now = Instant::now();
                    let elapsed = now.duration_since(self.last_press_time);
                    self.last_press_time = now;

                    if elapsed < DOUBLE_PRESS_WINDOW {
                        // Second press within window → start hands-free.
                        HotkeyAction::StartHandsFree
                    } else {
                        // First press — record timestamp, wait for potential
                        // second press. No recording starts yet.
                        HotkeyAction::None
                    }
                }
            }
        }
    }

    /// Process a key release event.
    ///
    /// Only meaningful in hold-to-talk mode (release stops recording).
    /// In toggle and hands-free modes, release events are always ignored.
    pub fn on_release(&mut self, is_recording: bool) -> HotkeyAction {
        match self.mode {
            ActivationMode::HoldToTalk => {
                if is_recording {
                    HotkeyAction::StopRecording
                } else {
                    HotkeyAction::None
                }
            }
            ActivationMode::Toggle | ActivationMode::HandsFree => HotkeyAction::None,
        }
    }

    /// Change the activation mode.
    ///
    /// Takes effect on the next event. Does not affect any in-progress
    /// recording session — the current session continues under the old
    /// mode's rules until it completes.
    pub fn set_mode(&mut self, mode: ActivationMode) {
        self.mode = mode;
    }

    /// Get the current activation mode.
    pub fn mode(&self) -> ActivationMode {
        self.mode
    }
}
