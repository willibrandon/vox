//! Recovery dispatcher and retry primitives.
//!
//! Provides `retry_once()` — a generic async wrapper that retries a fallible
//! operation exactly once — and `execute_recovery()` which dispatches on
//! `RecoveryAction` variants to invoke the appropriate handler.

use std::future::Future;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::error::{AudioError, RecoveryAction, VoxError};
use crate::models::{self, ModelInfo};

/// Retry an async operation once on failure.
///
/// Calls `operation` with the provided `input`. If it returns `Err`, logs the
/// first failure at warn level and calls the operation a second time with the
/// same input. Returns the result of whichever attempt succeeds first, or
/// the second attempt's error.
///
/// The `label` parameter is used in log messages to identify which component
/// failed (e.g., "ASR transcribe", "LLM process").
pub async fn retry_once<F, Fut, I, O>(label: &str, input: I, operation: F) -> Result<O, VoxError>
where
    F: Fn(I) -> Fut,
    Fut: Future<Output = Result<O, VoxError>>,
    I: Clone,
{
    let retry_start = Instant::now();
    let first_input = input.clone();
    match operation(first_input).await {
        Ok(output) => {
            tracing::debug!(
                component = label,
                attempt = 1,
                duration_ms = retry_start.elapsed().as_millis() as u64,
                success = true,
                "retry_once: succeeded on first attempt"
            );
            Ok(output)
        }
        Err(first_error) => {
            tracing::warn!(
                component = label,
                attempt = 1,
                error = %first_error,
                "retry_once: first attempt failed, retrying"
            );
            let result = operation(input).await;
            let success = result.is_ok();
            tracing::info!(
                component = label,
                attempt = 2,
                duration_ms = retry_start.elapsed().as_millis() as u64,
                success,
                "retry_once: second attempt complete"
            );
            result
        }
    }
}

/// Execute the recovery action for a given error.
///
/// This is the central dispatcher that routes each `RecoveryAction` to the
/// appropriate handler. Some actions (like RetrySegment) are handled inline
/// by the orchestrator before calling this; this function handles the
/// remaining recovery strategies that need standalone execution.
///
/// Returns a human-readable description of what was done, for logging.
pub async fn execute_recovery(action: &RecoveryAction) -> RecoveryOutcome {
    let recovery_start = Instant::now();
    tracing::info!(
        action = %action,
        "recovery_attempt: start"
    );

    let outcome = match action {
        RecoveryAction::RetrySegment => {
            tracing::info!("RetrySegment: orchestrator handles inline via retry_once()");
            RecoveryOutcome::HandledInline
        }

        RecoveryAction::DiscardSegment => {
            tracing::info!("DiscardSegment: dropping segment, returning to Listening");
            RecoveryOutcome::SegmentDiscarded
        }

        RecoveryAction::SwitchAudioDevice => {
            tracing::info!("SwitchAudioDevice: attempting to switch to default audio device");
            RecoveryOutcome::AudioSwitchRequested
        }

        RecoveryAction::AudioRetryLoop => {
            tracing::info!("AudioRetryLoop: entering 2-second device polling loop");
            RecoveryOutcome::AudioRetryLoopRequested
        }

        RecoveryAction::RedownloadModel => {
            tracing::info!("RedownloadModel: stopping pipeline, will re-download model");
            RecoveryOutcome::ModelRedownloadRequested
        }

        RecoveryAction::DisplayGuidance { message } => {
            tracing::info!(guidance = %message, "DisplayGuidance: showing user message in overlay");
            RecoveryOutcome::GuidanceDisplayed {
                message: message.clone(),
            }
        }

        RecoveryAction::BufferAndRetryFocus { text } => {
            tracing::info!(
                text_len = text.len(),
                "BufferAndRetryFocus: buffering text, spawning focus retry"
            );
            RecoveryOutcome::InjectionBuffered {
                text: text.clone(),
            }
        }
    };

    tracing::info!(
        action = %action,
        outcome = ?outcome,
        duration_ms = recovery_start.elapsed().as_millis() as u64,
        "recovery_attempt: complete"
    );

    outcome
}

/// Outcome of a recovery action execution.
///
/// Each variant tells the caller (typically the orchestrator) what happened
/// and what state transitions are needed.
#[derive(Debug, Clone)]
pub enum RecoveryOutcome {
    /// The action is handled inline (retry_once) — no further dispatch needed.
    HandledInline,

    /// The segment was dropped. Caller should broadcast Listening.
    SegmentDiscarded,

    /// The audio device switch was requested. Caller should invoke
    /// `AudioCapture::switch_to_default()`.
    AudioSwitchRequested,

    /// A device polling loop was requested. Caller should start the loop.
    AudioRetryLoopRequested,

    /// A model re-download was requested. Caller should stop the pipeline
    /// and trigger the download flow.
    ModelRedownloadRequested,

    /// A guidance message was displayed. Caller should update the overlay.
    GuidanceDisplayed {
        /// The message that was logged/displayed.
        message: String,
    },

    /// Injection text was buffered. Caller should show it in the overlay
    /// and spawn the focus retry task.
    InjectionBuffered {
        /// The buffered text.
        text: String,
    },
}

/// Check a model file's integrity after an inference error.
///
/// When ASR or LLM inference fails, this function checks if the underlying
/// model file is missing or corrupted (wrong size). Returns the appropriate
/// `VoxError` if the model needs re-downloading, or `None` if the file looks
/// intact (meaning the error was a runtime issue, not file corruption).
pub fn check_model_integrity(model_info: &ModelInfo) -> Option<VoxError> {
    let dir = match models::model_dir() {
        Ok(dir) => dir,
        Err(_) => return None,
    };

    let file_path = dir.join(model_info.filename);

    if !file_path.exists() {
        return Some(VoxError::ModelMissing {
            model_name: model_info.name.to_string(),
            expected_path: file_path,
        });
    }

    match std::fs::metadata(&file_path) {
        Ok(metadata) => {
            let actual_size = metadata.len();
            // Allow 10% tolerance for size check (models may have slightly different
            // sizes across builds). A complete corruption would show a much larger
            // deviation or zero size.
            let min_expected = model_info.size_bytes * 9 / 10;
            if actual_size < min_expected {
                Some(VoxError::ModelCorrupt {
                    model_name: model_info.name.to_string(),
                    path: file_path,
                    reason: format!(
                        "file size {} bytes is below expected minimum {} bytes (expected ~{} bytes)",
                        actual_size, min_expected, model_info.size_bytes
                    ),
                })
            } else {
                None
            }
        }
        Err(e) => Some(VoxError::ModelCorrupt {
            model_name: model_info.name.to_string(),
            path: file_path,
            reason: format!("failed to read file metadata: {e}"),
        }),
    }
}

/// Delete a corrupt model file and prepare for re-download.
///
/// Removes the file at the given path (after removing read-only permissions
/// if needed). Returns `Ok(())` if the file was deleted or didn't exist.
pub fn delete_corrupt_model(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    // Remove read-only permission before deletion (FR-027 sets read-only after download)
    if let Ok(metadata) = std::fs::metadata(path) {
        if metadata.permissions().readonly() {
            let mut perms = metadata.permissions();
            perms.set_readonly(false);
            if let Err(e) = std::fs::set_permissions(path, perms) {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to remove read-only permission before deletion"
                );
            }
        }
    }

    std::fs::remove_file(path)
        .map_err(|e| anyhow::anyhow!("failed to delete corrupt model at {}: {e}", path.display()))
}

/// Result of the audio device recovery loop.
///
/// Tells the caller whether a working device was found, the user needs to
/// grant a permission, or retries were exhausted.
#[derive(Debug, Clone)]
pub enum AudioRecoveryResult {
    /// A working audio device was found and connected.
    Recovered,

    /// Microphone permission was denied — caller should display guidance.
    PermissionDenied {
        /// Platform-specific guidance (e.g., macOS System Settings path).
        platform_message: String,
    },

    /// Recovery timed out after the retry window expired.
    TimedOut {
        /// The last error encountered before giving up.
        last_error: String,
    },
}

/// Attempt audio device recovery in a polling loop.
///
/// Calls `attempt_recovery` immediately, then every 2 seconds for up to 30
/// seconds total. Short-circuits immediately on `PermissionDenied` — no
/// amount of retrying will fix a missing OS permission.
///
/// The caller provides the recovery closure, typically wrapping
/// `AudioCapture::reconnect()` or `AudioCapture::switch_to_default()`.
/// Because `AudioCapture` is NOT Send, the closure bridges the call to
/// whichever thread owns the capture instance.
pub async fn audio_recovery_loop<F, Fut>(mut attempt_recovery: F) -> AudioRecoveryResult
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), AudioError>>,
{
    const MAX_DURATION: Duration = Duration::from_secs(30);
    const RETRY_INTERVAL: Duration = Duration::from_secs(2);

    let start = Instant::now();

    loop {
        match attempt_recovery().await {
            Ok(()) => {
                tracing::info!(
                    elapsed_ms = start.elapsed().as_millis() as u64,
                    "audio device recovered successfully"
                );
                return AudioRecoveryResult::Recovered;
            }
            Err(AudioError::PermissionDenied { platform_message }) => {
                tracing::warn!(
                    message = %platform_message,
                    "audio recovery aborted — microphone permission denied"
                );
                return AudioRecoveryResult::PermissionDenied { platform_message };
            }
            Err(err) => {
                if start.elapsed() >= MAX_DURATION {
                    tracing::error!(
                        error = %err,
                        elapsed_s = start.elapsed().as_secs(),
                        "audio recovery timed out after 30 seconds"
                    );
                    return AudioRecoveryResult::TimedOut {
                        last_error: err.to_string(),
                    };
                }
                tracing::info!(
                    error = %err,
                    elapsed_s = start.elapsed().as_secs(),
                    "audio recovery attempt failed, retrying in 2s"
                );
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_retry_once_succeeds_first_try() {
        let result = retry_once("test", 42_i32, |input| async move {
            Ok::<i32, VoxError>(input * 2)
        })
        .await;
        assert_eq!(result.unwrap(), 84);
    }

    #[tokio::test]
    async fn test_retry_once_succeeds_second_try() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let call_count = std::sync::Arc::new(AtomicU32::new(0));

        let counter = call_count.clone();
        let result = retry_once("test", "hello", move |input| {
            let counter = counter.clone();
            async move {
                let count = counter.fetch_add(1, Ordering::SeqCst);
                if count == 0 {
                    Err(VoxError::AsrFailure {
                        source: anyhow::anyhow!("simulated failure"),
                        segment_id: 1,
                    })
                } else {
                    Ok::<String, VoxError>(input.to_uppercase())
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), "HELLO");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_retry_once_fails_both_tries() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let call_count = std::sync::Arc::new(AtomicU32::new(0));

        let counter = call_count.clone();
        let result: Result<(), VoxError> = retry_once("test", (), move |_| {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err(VoxError::LlmFailure {
                    source: anyhow::anyhow!("always fails"),
                    segment_id: 5,
                })
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_execute_recovery_discard() {
        let outcome = execute_recovery(&RecoveryAction::DiscardSegment).await;
        assert!(matches!(outcome, RecoveryOutcome::SegmentDiscarded));
    }

    #[tokio::test]
    async fn test_execute_recovery_guidance() {
        let outcome = execute_recovery(&RecoveryAction::DisplayGuidance {
            message: "Update your GPU drivers".to_string(),
        })
        .await;
        match outcome {
            RecoveryOutcome::GuidanceDisplayed { message } => {
                assert!(message.contains("GPU drivers"));
            }
            other => panic!("expected GuidanceDisplayed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_execute_recovery_buffer_injection() {
        let outcome = execute_recovery(&RecoveryAction::BufferAndRetryFocus {
            text: "buffered text".to_string(),
        })
        .await;
        match outcome {
            RecoveryOutcome::InjectionBuffered { text } => {
                assert_eq!(text, "buffered text");
            }
            other => panic!("expected InjectionBuffered, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_execute_recovery_switch_audio() {
        let outcome = execute_recovery(&RecoveryAction::SwitchAudioDevice).await;
        assert!(matches!(outcome, RecoveryOutcome::AudioSwitchRequested));
    }

    #[tokio::test]
    async fn test_execute_recovery_redownload() {
        let outcome = execute_recovery(&RecoveryAction::RedownloadModel).await;
        assert!(matches!(outcome, RecoveryOutcome::ModelRedownloadRequested));
    }

    #[tokio::test]
    async fn test_execute_recovery_audio_retry_loop() {
        let outcome = execute_recovery(&RecoveryAction::AudioRetryLoop).await;
        assert!(matches!(outcome, RecoveryOutcome::AudioRetryLoopRequested));
    }

    #[tokio::test]
    async fn test_audio_recovery_loop_immediate_success() {
        let result = audio_recovery_loop(|| async { Ok(()) }).await;
        assert!(matches!(result, AudioRecoveryResult::Recovered));
    }

    #[tokio::test]
    async fn test_audio_recovery_loop_permission_denied() {
        let result = audio_recovery_loop(|| async {
            Err(AudioError::PermissionDenied {
                platform_message: "System Settings > Privacy > Microphone".to_string(),
            })
        })
        .await;
        match result {
            AudioRecoveryResult::PermissionDenied { platform_message } => {
                assert!(platform_message.contains("Privacy"));
            }
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_audio_recovery_loop_recovers_on_second_try() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let call_count = std::sync::Arc::new(AtomicU32::new(0));

        let counter = call_count.clone();
        let result = audio_recovery_loop(move || {
            let counter = counter.clone();
            async move {
                let count = counter.fetch_add(1, Ordering::SeqCst);
                if count == 0 {
                    Err(AudioError::DeviceMissing)
                } else {
                    Ok(())
                }
            }
        })
        .await;

        assert!(matches!(result, AudioRecoveryResult::Recovered));
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}
