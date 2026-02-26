//! Typed error taxonomy for the Vox pipeline.
//!
//! Every pipeline failure is categorized into one of eight `VoxError` variants,
//! each mapping to exactly one `RecoveryAction` via `recovery_action_for()`.
//! This exhaustive mapping ensures the recovery dispatcher always knows what
//! to do — no unhandled error categories.

use std::fmt;
use std::path::PathBuf;

use crate::injector::InjectionResult;

/// Top-level error enum covering every categorized failure in the pipeline.
///
/// Each variant carries enough context for the recovery dispatcher to execute
/// the correct action without additional lookups.
#[derive(Debug)]
pub enum VoxError {
    /// Audio subsystem failure (device disconnect, missing device, permissions, stream error).
    Audio(AudioError),

    /// A required model file is not present on disk.
    ModelMissing {
        /// Which model is missing (e.g., "Whisper Large V3 Turbo Q5_0").
        model_name: String,
        /// Where the file was expected.
        expected_path: PathBuf,
    },

    /// A model file exists but is corrupted (SHA-256 mismatch or parse failure).
    ModelCorrupt {
        /// Which model is corrupt.
        model_name: String,
        /// Path to the corrupt file.
        path: PathBuf,
        /// What went wrong (checksum mismatch, truncated, invalid format).
        reason: String,
    },

    /// GPU ran out of memory while loading or running a model.
    ModelOom {
        /// Which model triggered the OOM.
        model_name: String,
        /// How much VRAM the model needs (bytes).
        vram_required: u64,
        /// How much VRAM was available (if detectable).
        vram_available: Option<u64>,
    },

    /// ASR (Whisper) transcription failed for a specific audio segment.
    AsrFailure {
        /// The underlying error from whisper-rs or the ASR subsystem.
        source: anyhow::Error,
        /// Which segment failed (monotonic counter from the orchestrator).
        segment_id: u64,
    },

    /// LLM (Qwen) post-processing failed for a specific segment.
    LlmFailure {
        /// The underlying error from llama-cpp-2 or the LLM subsystem.
        source: anyhow::Error,
        /// Which segment failed.
        segment_id: u64,
    },

    /// Text injection into the focused application failed.
    InjectionFailure {
        /// The injection result containing the failure reason.
        result: InjectionResult,
        /// The text that was not injected.
        text: String,
    },

    /// The GPU driver crashed or became unresponsive.
    GpuCrash {
        /// The underlying error from the GPU subsystem.
        source: anyhow::Error,
        /// Which platform this occurred on (for guidance messages).
        platform: String,
    },
}

/// Audio-specific failure sub-enum.
///
/// Extends the existing `error_flag` mechanism in `AudioCapture` with
/// categorized variants that map to distinct recovery actions.
#[derive(Debug)]
pub enum AudioError {
    /// The audio input device was physically disconnected or became unavailable.
    DeviceDisconnected {
        /// Name of the device that was lost.
        device_name: String,
    },

    /// No audio input device exists on the system.
    DeviceMissing,

    /// The operating system denied microphone access.
    PermissionDenied {
        /// Platform-specific guidance (e.g., macOS System Settings path).
        platform_message: String,
    },

    /// A cpal stream creation or runtime error.
    StreamError {
        /// The underlying cpal/OS error.
        source: anyhow::Error,
    },
}

/// What the recovery dispatcher should do for a given error category.
///
/// Each variant is a discrete recovery strategy. The dispatcher in `recovery.rs`
/// matches on these to execute the correct handler.
#[derive(Debug, Clone)]
pub enum RecoveryAction {
    /// Retry the same audio segment through the failed component once.
    RetrySegment,

    /// Drop the segment entirely and continue listening for the next one.
    DiscardSegment,

    /// Switch to the system's default audio input device.
    SwitchAudioDevice,

    /// Enter a 2-second polling loop waiting for any audio device to appear.
    AudioRetryLoop,

    /// Stop the pipeline, delete the corrupt file if present, and re-download the model.
    RedownloadModel,

    /// Show an actionable message in the overlay (no automatic recovery possible).
    DisplayGuidance {
        /// The message to display (includes user-facing instructions).
        message: String,
    },

    /// Show the buffered text in overlay with a Copy button, and poll for focus.
    BufferAndRetryFocus {
        /// The text that failed to inject.
        text: String,
    },
}

/// Map a `VoxError` to the appropriate `RecoveryAction`.
///
/// This is an exhaustive match — every error category has exactly one recovery
/// strategy. The orchestrator calls this to determine what to do after a failure.
pub fn recovery_action_for(error: &VoxError) -> RecoveryAction {
    match error {
        VoxError::Audio(AudioError::DeviceDisconnected { .. }) => RecoveryAction::SwitchAudioDevice,

        VoxError::Audio(AudioError::DeviceMissing) => RecoveryAction::AudioRetryLoop,

        VoxError::Audio(AudioError::PermissionDenied { platform_message }) => {
            RecoveryAction::DisplayGuidance {
                message: platform_message.clone(),
            }
        }

        VoxError::Audio(AudioError::StreamError { source }) => RecoveryAction::DisplayGuidance {
            message: format!("Audio stream error: {source}. Try reconnecting your microphone."),
        },

        VoxError::ModelMissing { model_name, .. } => {
            tracing::warn!("Model missing: {model_name} — triggering re-download");
            RecoveryAction::RedownloadModel
        }

        VoxError::ModelCorrupt {
            model_name, reason, ..
        } => {
            tracing::warn!("Model corrupt: {model_name} — {reason}");
            RecoveryAction::RedownloadModel
        }

        VoxError::ModelOom {
            model_name,
            vram_required,
            vram_available,
        } => {
            let available_str = match vram_available {
                Some(bytes) => format!("{} MB available", bytes / (1024 * 1024)),
                None => "unknown available VRAM".to_string(),
            };
            RecoveryAction::DisplayGuidance {
                message: format!(
                    "Not enough GPU memory to load {model_name}. \
                     Requires {} MB, {available_str}. \
                     Close other GPU applications and try again.",
                    vram_required / (1024 * 1024),
                ),
            }
        }

        VoxError::AsrFailure { .. } => RecoveryAction::RetrySegment,

        VoxError::LlmFailure { .. } => RecoveryAction::RetrySegment,

        VoxError::InjectionFailure { text, .. } => RecoveryAction::BufferAndRetryFocus {
            text: text.clone(),
        },

        VoxError::GpuCrash {
            source, platform, ..
        } => RecoveryAction::DisplayGuidance {
            message: format!(
                "GPU error on {platform}: {source}. \
                 Please restart Vox. If the problem persists, update your GPU drivers."
            ),
        },
    }
}

impl fmt::Display for VoxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VoxError::Audio(audio_err) => write!(f, "Audio error: {audio_err}"),
            VoxError::ModelMissing {
                model_name,
                expected_path,
            } => {
                write!(
                    f,
                    "Model '{model_name}' not found at {}",
                    expected_path.display()
                )
            }
            VoxError::ModelCorrupt {
                model_name, reason, ..
            } => write!(f, "Model '{model_name}' is corrupt: {reason}"),
            VoxError::ModelOom {
                model_name,
                vram_required,
                ..
            } => write!(
                f,
                "Out of GPU memory loading '{model_name}' (needs {} MB)",
                vram_required / (1024 * 1024)
            ),
            VoxError::AsrFailure { source, segment_id } => {
                write!(f, "ASR failed on segment {segment_id}: {source}")
            }
            VoxError::LlmFailure { source, segment_id } => {
                write!(f, "LLM failed on segment {segment_id}: {source}")
            }
            VoxError::InjectionFailure { text, result } => {
                write!(
                    f,
                    "Injection failed for {} chars: {result:?}",
                    text.len()
                )
            }
            VoxError::GpuCrash {
                source, platform, ..
            } => write!(f, "GPU crash on {platform}: {source}"),
        }
    }
}

impl std::error::Error for VoxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VoxError::AsrFailure { source, .. } => Some(source.as_ref()),
            VoxError::LlmFailure { source, .. } => Some(source.as_ref()),
            VoxError::GpuCrash { source, .. } => Some(source.as_ref()),
            VoxError::Audio(AudioError::StreamError { source }) => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::DeviceDisconnected { device_name } => {
                write!(f, "Audio device '{device_name}' disconnected")
            }
            AudioError::DeviceMissing => write!(f, "No audio input device found"),
            AudioError::PermissionDenied { platform_message } => {
                write!(f, "Microphone permission denied: {platform_message}")
            }
            AudioError::StreamError { source } => write!(f, "Audio stream error: {source}"),
        }
    }
}

impl std::error::Error for AudioError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AudioError::StreamError { source } => Some(source.as_ref()),
            _ => None,
        }
    }
}

impl From<AudioError> for VoxError {
    fn from(err: AudioError) -> Self {
        VoxError::Audio(err)
    }
}

impl fmt::Display for RecoveryAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecoveryAction::RetrySegment => write!(f, "RetrySegment"),
            RecoveryAction::DiscardSegment => write!(f, "DiscardSegment"),
            RecoveryAction::SwitchAudioDevice => write!(f, "SwitchAudioDevice"),
            RecoveryAction::AudioRetryLoop => write!(f, "AudioRetryLoop"),
            RecoveryAction::RedownloadModel => write!(f, "RedownloadModel"),
            RecoveryAction::DisplayGuidance { message } => {
                write!(f, "DisplayGuidance({message})")
            }
            RecoveryAction::BufferAndRetryFocus { .. } => write!(f, "BufferAndRetryFocus"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::injector::InjectionError;

    #[test]
    fn test_asr_failure_maps_to_retry() {
        let error = VoxError::AsrFailure {
            source: anyhow::anyhow!("whisper decode failed"),
            segment_id: 42,
        };
        assert!(matches!(recovery_action_for(&error), RecoveryAction::RetrySegment));
    }

    #[test]
    fn test_llm_failure_maps_to_retry() {
        let error = VoxError::LlmFailure {
            source: anyhow::anyhow!("context creation failed"),
            segment_id: 7,
        };
        assert!(matches!(recovery_action_for(&error), RecoveryAction::RetrySegment));
    }

    #[test]
    fn test_device_disconnected_maps_to_switch() {
        let error = VoxError::Audio(AudioError::DeviceDisconnected {
            device_name: "Blue Yeti".to_string(),
        });
        assert!(matches!(
            recovery_action_for(&error),
            RecoveryAction::SwitchAudioDevice
        ));
    }

    #[test]
    fn test_device_missing_maps_to_retry_loop() {
        let error = VoxError::Audio(AudioError::DeviceMissing);
        assert!(matches!(
            recovery_action_for(&error),
            RecoveryAction::AudioRetryLoop
        ));
    }

    #[test]
    fn test_model_missing_maps_to_redownload() {
        let error = VoxError::ModelMissing {
            model_name: "Whisper".to_string(),
            expected_path: PathBuf::from("/models/whisper.bin"),
        };
        assert!(matches!(
            recovery_action_for(&error),
            RecoveryAction::RedownloadModel
        ));
    }

    #[test]
    fn test_model_corrupt_maps_to_redownload() {
        let error = VoxError::ModelCorrupt {
            model_name: "Whisper".to_string(),
            path: PathBuf::from("/models/whisper.bin"),
            reason: "SHA-256 mismatch".to_string(),
        };
        assert!(matches!(
            recovery_action_for(&error),
            RecoveryAction::RedownloadModel
        ));
    }

    #[test]
    fn test_model_oom_maps_to_guidance() {
        let error = VoxError::ModelOom {
            model_name: "Qwen".to_string(),
            vram_required: 2_000_000_000,
            vram_available: Some(1_000_000_000),
        };
        match recovery_action_for(&error) {
            RecoveryAction::DisplayGuidance { message } => {
                assert!(message.contains("Qwen"));
                assert!(message.contains("MB"));
            }
            other => panic!("expected DisplayGuidance, got {other:?}"),
        }
    }

    #[test]
    fn test_injection_failure_maps_to_buffer() {
        let error = VoxError::InjectionFailure {
            result: InjectionResult::Blocked {
                reason: InjectionError::NoFocusedWindow,
                text: "hello world".to_string(),
            },
            text: "hello world".to_string(),
        };
        match recovery_action_for(&error) {
            RecoveryAction::BufferAndRetryFocus { text } => {
                assert_eq!(text, "hello world");
            }
            other => panic!("expected BufferAndRetryFocus, got {other:?}"),
        }
    }

    #[test]
    fn test_gpu_crash_maps_to_guidance() {
        let error = VoxError::GpuCrash {
            source: anyhow::anyhow!("device lost"),
            platform: "CUDA".to_string(),
        };
        match recovery_action_for(&error) {
            RecoveryAction::DisplayGuidance { message } => {
                assert!(message.contains("CUDA"));
                assert!(message.contains("restart"));
            }
            other => panic!("expected DisplayGuidance, got {other:?}"),
        }
    }

    #[test]
    fn test_permission_denied_maps_to_guidance() {
        let error = VoxError::Audio(AudioError::PermissionDenied {
            platform_message: "System Settings > Privacy > Microphone".to_string(),
        });
        match recovery_action_for(&error) {
            RecoveryAction::DisplayGuidance { message } => {
                assert!(message.contains("Privacy"));
            }
            other => panic!("expected DisplayGuidance, got {other:?}"),
        }
    }

    #[test]
    fn test_display_formatting() {
        let error = VoxError::AsrFailure {
            source: anyhow::anyhow!("decode failed"),
            segment_id: 1,
        };
        let display = format!("{error}");
        assert!(display.contains("ASR failed"));
        assert!(display.contains("segment 1"));
    }

    #[test]
    fn test_audio_error_from_conversion() {
        let audio_err = AudioError::DeviceMissing;
        let vox_err: VoxError = audio_err.into();
        assert!(matches!(vox_err, VoxError::Audio(AudioError::DeviceMissing)));
    }
}
