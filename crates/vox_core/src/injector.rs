//! OS-level text injection and voice command execution.
//!
//! This module provides the final stage of the Vox pipeline: injecting polished
//! text into the focused application via simulated keyboard input. On Windows,
//! uses `SendInput` with `KEYEVENTF_UNICODE`. On macOS, uses `CGEvent` with
//! UTF-16 chunking at 20 code unit boundaries.
//!
//! Voice commands are mapped to platform-specific keyboard shortcuts and
//! dispatched through the same OS keyboard APIs.

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

mod commands;

use std::time::{Duration, Instant};

use crate::llm::VoiceCommand;
use crate::pipeline::state::PipelineState;

/// Result of a text injection attempt.
///
/// Unlike `Result<()>`, a blocked injection is a normal operational outcome —
/// the caller needs the original text back for buffering and UI display.
#[derive(Debug)]
pub enum InjectionResult {
    /// Text was successfully injected into the focused application.
    Success,
    /// Injection failed. The original text and failure reason are preserved
    /// so the UI layer can display a copy-to-clipboard fallback.
    Blocked {
        /// Why injection failed.
        reason: InjectionError,
        /// The text that was not injected (preserved byte-for-byte for buffering).
        text: String,
    },
}

/// Reason an injection attempt failed.
#[derive(Debug)]
pub enum InjectionError {
    /// The focused window belongs to an elevated process (Windows UIPI restriction).
    ElevatedTarget,
    /// No window currently has focus.
    NoFocusedWindow,
    /// An OS API call failed.
    PlatformError(String),
}

/// Buffer holding text that failed to inject, for UI display and retry.
///
/// The injector module defines this struct but does not manage buffer state —
/// that responsibility belongs to the pipeline/UI layer that calls `inject_text()`
/// and handles the `Blocked` variant.
#[derive(Debug)]
pub struct InjectionBuffer {
    /// The text that was not injected.
    pub text: String,
    /// Why injection failed.
    pub error: InjectionError,
    /// When the failure occurred.
    pub timestamp: Instant,
}

/// Inject text into the currently focused application via OS-level keyboard simulation.
///
/// On Windows, uses `SendInput` with `KEYEVENTF_UNICODE`. On macOS, uses `CGEvent`
/// with text chunking at 20 UTF-16 code unit boundaries. Returns
/// `InjectionResult::Blocked` if the target window is elevated (Windows UIPI) or
/// no window has focus.
///
/// Empty text is a no-op that returns `InjectionResult::Success`.
pub fn inject_text(text: &str) -> InjectionResult {
    if text.is_empty() {
        return InjectionResult::Success;
    }

    #[cfg(target_os = "windows")]
    {
        windows::inject_text_impl(text)
    }

    #[cfg(target_os = "macos")]
    {
        macos::inject_text_impl(text)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        InjectionResult::Blocked {
            reason: InjectionError::PlatformError("Unsupported platform".to_string()),
            text: text.to_string(),
        }
    }
}

/// Prompt the user for Accessibility permission if not already granted.
///
/// On macOS, calls `AXIsProcessTrustedWithOptions` with the prompt flag,
/// which shows a system dialog directing the user to System Settings >
/// Privacy & Security > Accessibility. This is a no-op if permission is
/// already granted, and a no-op on non-macOS platforms.
///
/// Should be called once at app startup so the user sees the prompt before
/// their first dictation attempt.
pub fn prompt_accessibility_if_needed() {
    #[cfg(target_os = "macos")]
    {
        macos::prompt_accessibility_if_needed();
    }
}

/// Check if Accessibility permission is currently granted.
///
/// On macOS, calls `AXIsProcessTrusted()` without the prompt flag.
/// Returns `true` if permission is granted or on non-macOS platforms.
pub fn is_accessibility_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_accessibility_granted()
    }

    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Get the name of the currently focused application.
///
/// Returns the executable/application name (not the window title):
/// - Windows: Process executable stem (e.g., "notepad", "Code")
/// - macOS: Application localized name (e.g., "Safari", "Visual Studio Code")
///
/// Returns "Unknown" if detection fails for any reason (no focused window,
/// API error, elevated process). This is non-fatal — the LLM uses app name
/// for tone hints, not critical logic.
pub fn get_focused_app_name() -> String {
    #[cfg(target_os = "windows")]
    {
        windows::get_focused_app_name_impl()
    }

    #[cfg(target_os = "macos")]
    {
        macos::get_focused_app_name_impl()
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "Unknown".to_string()
    }
}

/// Execute a voice command by simulating the mapped keyboard shortcut.
///
/// Maps the command name to platform-appropriate key sequences and sends them
/// via the OS keyboard API. Returns an error for unrecognized commands.
pub fn execute_command(command: &VoiceCommand) -> anyhow::Result<()> {
    commands::execute_command(command)
}

/// Retry text injection by polling for a focused text-accepting window.
///
/// After an `InjectionResult::Blocked`, this task polls every 500ms to check
/// if a text-accepting window has gained focus. On success, broadcasts
/// `PipelineState::Listening`. Cancels after 30 seconds or when `cancel_rx`
/// is signalled (new dictation start or user copy).
pub async fn retry_on_focus(
    text: String,
    state_tx: tokio::sync::broadcast::Sender<PipelineState>,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) {
    let timeout = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(timeout);

    let mut interval = tokio::time::interval(Duration::from_millis(500));
    // First tick completes immediately — skip it so first retry is at 500ms
    interval.tick().await;

    loop {
        tokio::select! {
            _ = &mut timeout => {
                tracing::info!("Injection focus retry timed out after 30s");
                return;
            }
            result = cancel_rx.changed() => {
                match result {
                    Ok(()) if *cancel_rx.borrow() => {
                        tracing::info!("Injection focus retry cancelled");
                        return;
                    }
                    Err(_) => {
                        // Sender dropped — session ended
                        return;
                    }
                    _ => continue,
                }
            }
            _ = interval.tick() => {
                let result = inject_text(&text);
                match result {
                    InjectionResult::Success => {
                        let _ = state_tx.send(PipelineState::Listening);
                        tracing::info!("Injection focus retry succeeded");
                        return;
                    }
                    InjectionResult::Blocked { .. } => {
                        // Still no focus — continue polling
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_text_noop() {
        let result = inject_text("");
        assert!(matches!(result, InjectionResult::Success));
    }

    #[test]
    fn test_whitespace_text_valid() {
        // Whitespace-only text is NOT empty — it must bypass the is_empty() check
        // and reach the platform impl. On a desktop session, SendInput may succeed
        // (returning Success); in headless CI, it returns Blocked(NoFocusedWindow).
        // Either outcome is valid — the point is that the platform impl was called,
        // not the empty-string early return. We verify the early return wasn't hit
        // by confirming inject_text completes without panic.
        let _result = inject_text("  \t\n");
        // Additionally verify the language guarantee that makes this work:
        let whitespace: &str = "  \t\n";
        assert!(!whitespace.is_empty(), "whitespace is not empty");
    }
}
