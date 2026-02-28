//! User-configurable application settings for the Vox dictation engine.
//!
//! Settings are persisted to a JSON file (`settings.json`) in the platform data
//! directory. All fields have sensible defaults via the `Default` impl.
//! Forward-compatible (ignores unknown fields) and backward-compatible (uses
//! defaults for missing fields) via `#[serde(default)]`.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::hotkey_interpreter::ActivationMode;

/// Debug audio recording level.
///
/// Controls which audio tap points are active. At `Off` (default), no audio
/// files are written and runtime overhead is a single atomic load (~1 ns).
/// `Segments` records per-utterance WAV files only (VAD segment + ASR input).
/// `Full` adds continuous raw microphone and post-resampler streams.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DebugAudioLevel {
    /// No debug audio recording. Zero runtime overhead.
    #[default]
    Off,
    /// Record per-segment taps only (vad_segment + asr_input).
    /// Low data volume: ~1-5 small WAV files per minute.
    Segments,
    /// Record all taps including continuous raw capture and post-resample.
    /// High data volume: ~256 KB/s while recording.
    Full,
}

/// Overlay HUD placement on screen.
///
/// Determines where the dictation overlay appears. `Custom` allows the user
/// to drag the overlay to an arbitrary screen position.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum OverlayPosition {
    /// Centered at the top of the screen.
    TopCenter,
    /// Top-right corner of the screen.
    TopRight,
    /// Centered at the bottom of the screen.
    BottomCenter,
    /// Bottom-right corner of the screen.
    BottomRight,
    /// User-defined screen coordinates (normalized 0.0–1.0).
    Custom {
        /// Horizontal position (0.0 = left edge, 1.0 = right edge).
        x: f32,
        /// Vertical position (0.0 = top edge, 1.0 = bottom edge).
        y: f32,
    },
}

/// Application theme selection.
///
/// Controls the visual appearance of the settings panel and overlay.
/// `System` follows the OS dark/light mode preference.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ThemeMode {
    /// Follow the operating system's dark/light mode setting.
    System,
    /// Always use the light theme.
    Light,
    /// Always use the dark theme.
    Dark,
}

/// User-configurable application settings.
///
/// Persisted to JSON at `data_dir/settings.json`. All fields have sensible
/// defaults via `Default` impl. Forward-compatible (ignores unknown fields)
/// and backward-compatible (uses defaults for missing fields) via
/// `#[serde(default)]`.
///
/// 23 fields across 8 categories:
/// - Audio (2): input_device, noise_gate
/// - VAD (3): vad_threshold, min_silence_ms, min_speech_ms
/// - ASR (2): language, whisper_model
/// - LLM (5): llm_model, temperature, remove_fillers, course_correction, punctuation
/// - Hotkey (2): activation_hotkey, activation_mode
/// - Appearance (4): overlay_position, overlay_opacity, show_raw_transcript, theme
/// - Advanced (4): max_segment_ms, overlap_ms, command_prefix, save_history
/// - Debug (1): debug_audio
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    // --- Audio (2 fields) ---

    /// Selected audio input device name, or None for system default.
    pub input_device: Option<String>,
    /// Noise gate threshold (0.0–1.0). Audio below this level is ignored.
    pub noise_gate: f32,

    // --- VAD (3 fields) ---

    /// Voice Activity Detection confidence threshold (0.0–1.0).
    pub vad_threshold: f32,
    /// Minimum silence duration (ms) before ending a speech segment.
    pub min_silence_ms: u32,
    /// Minimum speech duration (ms) before starting a speech segment.
    pub min_speech_ms: u32,

    // --- ASR (2 fields) ---

    /// Language code for speech recognition (e.g., "en").
    pub language: String,
    /// Whisper model filename (relative to models directory).
    pub whisper_model: String,

    // --- LLM (5 fields) ---

    /// LLM model filename (relative to models directory).
    pub llm_model: String,
    /// LLM sampling temperature (0.0–1.0). Lower = more deterministic.
    pub temperature: f32,
    /// Whether to remove filler words (um, uh, like) from transcriptions.
    pub remove_fillers: bool,
    /// Whether to apply course correction (fixing mid-sentence restarts).
    pub course_correction: bool,
    /// Whether to add punctuation to raw transcriptions.
    pub punctuation: bool,

    // --- Hotkey (2 fields) ---

    /// Keyboard shortcut to activate/deactivate dictation.
    pub activation_hotkey: String,
    /// Recording trigger behavior: hold-to-talk, toggle, or hands-free.
    #[serde(default)]
    pub activation_mode: ActivationMode,

    // --- Appearance (4 fields) ---

    /// Where the overlay HUD appears on screen.
    pub overlay_position: OverlayPosition,
    /// Overlay background opacity (0.0–1.0).
    pub overlay_opacity: f32,
    /// Whether to show the raw ASR transcript alongside polished text.
    pub show_raw_transcript: bool,
    /// Application color theme.
    pub theme: ThemeMode,

    // --- Advanced (4 fields) ---

    /// Maximum audio segment duration (ms) before forced split.
    pub max_segment_ms: u32,
    /// Overlap between consecutive audio segments (ms).
    pub overlap_ms: u32,
    /// Prefix phrase that triggers voice command mode.
    pub command_prefix: String,
    /// Whether to persist transcript history to the database.
    pub save_history: bool,

    // --- Debug (1 field) ---

    /// Debug audio recording level (Off, Segments, or Full).
    #[serde(default)]
    pub debug_audio: DebugAudioLevel,

    // --- Window position (4 fields) ---

    /// Saved settings window X position (pixels from left).
    #[serde(default)]
    pub window_x: Option<f32>,
    /// Saved settings window Y position (pixels from top).
    #[serde(default)]
    pub window_y: Option<f32>,
    /// Saved settings window width in pixels.
    #[serde(default)]
    pub window_width: Option<f32>,
    /// Saved settings window height in pixels.
    #[serde(default)]
    pub window_height: Option<f32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_device: None,
            noise_gate: 0.0,
            vad_threshold: 0.5,
            min_silence_ms: 500,
            min_speech_ms: 250,
            language: "en".into(),
            whisper_model: "ggml-large-v3-turbo-q5_0.bin".into(),
            llm_model: "Qwen2.5-3B-Instruct-Q4_K_M.gguf".into(),
            temperature: 0.1,
            remove_fillers: true,
            course_correction: true,
            punctuation: true,
            activation_hotkey: "Ctrl+Shift+Space".into(),
            activation_mode: ActivationMode::default(),
            overlay_position: OverlayPosition::TopCenter,
            overlay_opacity: 0.85,
            show_raw_transcript: false,
            theme: ThemeMode::Dark,
            max_segment_ms: 10_000,
            overlap_ms: 1_000,
            command_prefix: "hey vox".into(),
            save_history: true,
            debug_audio: DebugAudioLevel::default(),
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
        }
    }
}

impl Settings {
    /// Load settings from the data directory.
    ///
    /// Returns defaults if file doesn't exist. Logs a warning and returns
    /// defaults if the file is corrupt (never crashes).
    pub fn load(data_dir: &Path) -> Result<Self> {
        let path = data_dir.join("settings.json");
        if !path.exists() {
            return Ok(Self::default());
        }

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read settings file at {}", path.display()))?;

        match serde_json::from_str::<Self>(&contents) {
            Ok(mut settings) => {
                settings.migrate();
                Ok(settings)
            }
            Err(error) => {
                tracing::warn!(
                    "corrupt settings file at {}, resetting to defaults: {error}",
                    path.display()
                );
                Ok(Self::default())
            }
        }
    }

    /// Apply one-time migrations for settings that have changed defaults.
    ///
    /// CapsLock was the original default but hijacks the system CapsLock key.
    /// F6 was a temporary intermediate default. Both are migrated to the
    /// WhisperFlow-style Ctrl+Shift+Space.
    fn migrate(&mut self) {
        let hotkey_lower = self.activation_hotkey.to_lowercase();
        if hotkey_lower == "capslock"
            || hotkey_lower == "caps_lock"
            || hotkey_lower == "capital"
            || hotkey_lower == "f6"
        {
            tracing::info!(
                old = %self.activation_hotkey,
                new = "Ctrl+Shift+Space",
                "migrating legacy hotkey to Ctrl+Shift+Space"
            );
            self.activation_hotkey = "Ctrl+Shift+Space".into();
        }
    }

    /// Save settings to the data directory using atomic write.
    ///
    /// Writes to a temporary file (`settings.json.tmp`), then renames to
    /// `settings.json`. This ensures the settings file is never left in a
    /// corrupt state if the process crashes or power is lost during write.
    pub fn save(&self, data_dir: &Path) -> Result<()> {
        let path = data_dir.join("settings.json");
        let tmp_path = data_dir.join("settings.json.tmp");

        let json = serde_json::to_string_pretty(self)
            .context("failed to serialize settings to JSON")?;

        std::fs::write(&tmp_path, json.as_bytes())
            .with_context(|| format!("failed to write temporary settings file at {}", tmp_path.display()))?;

        std::fs::rename(&tmp_path, &path)
            .with_context(|| format!("failed to rename settings file from {} to {}", tmp_path.display(), path.display()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert_eq!(settings.noise_gate, 0.0);
        assert_eq!(settings.vad_threshold, 0.5);
        assert_eq!(settings.min_silence_ms, 500);
        assert_eq!(settings.min_speech_ms, 250);
        assert_eq!(settings.language, "en");
        assert_eq!(settings.whisper_model, "ggml-large-v3-turbo-q5_0.bin");
        assert_eq!(settings.llm_model, "Qwen2.5-3B-Instruct-Q4_K_M.gguf");
        assert_eq!(settings.temperature, 0.1);
        assert!(settings.remove_fillers);
        assert!(settings.course_correction);
        assert!(settings.punctuation);
        assert_eq!(settings.activation_hotkey, "Ctrl+Shift+Space");
        assert_eq!(settings.activation_mode, ActivationMode::HoldToTalk);
        assert_eq!(settings.overlay_position, OverlayPosition::TopCenter);
        assert_eq!(settings.overlay_opacity, 0.85);
        assert!(!settings.show_raw_transcript);
        assert_eq!(settings.theme, ThemeMode::Dark);
        assert_eq!(settings.max_segment_ms, 10_000);
        assert_eq!(settings.overlap_ms, 1_000);
        assert_eq!(settings.command_prefix, "hey vox");
        assert!(settings.save_history);
        assert!(settings.input_device.is_none());
    }

    #[test]
    fn test_settings_roundtrip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut settings = Settings::default();
        settings.noise_gate = 0.42;
        settings.vad_threshold = 0.7;
        settings.min_silence_ms = 800;
        settings.min_speech_ms = 100;
        settings.language = "de".into();
        settings.whisper_model = "custom-model.bin".into();
        settings.llm_model = "custom-llm.gguf".into();
        settings.temperature = 0.5;
        settings.remove_fillers = false;
        settings.course_correction = false;
        settings.punctuation = false;
        settings.activation_hotkey = "F1".into();
        settings.activation_mode = ActivationMode::Toggle;
        settings.overlay_position = OverlayPosition::BottomRight;
        settings.overlay_opacity = 0.5;
        settings.show_raw_transcript = true;
        settings.theme = ThemeMode::Light;
        settings.max_segment_ms = 5_000;
        settings.overlap_ms = 500;
        settings.command_prefix = "ok vox".into();
        settings.save_history = false;

        settings.save(dir.path()).expect("save");
        let loaded = Settings::load(dir.path()).expect("load");

        assert_eq!(loaded.noise_gate, 0.42);
        assert_eq!(loaded.vad_threshold, 0.7);
        assert_eq!(loaded.min_silence_ms, 800);
        assert_eq!(loaded.min_speech_ms, 100);
        assert_eq!(loaded.language, "de");
        assert_eq!(loaded.whisper_model, "custom-model.bin");
        assert_eq!(loaded.llm_model, "custom-llm.gguf");
        assert_eq!(loaded.temperature, 0.5);
        assert!(!loaded.remove_fillers);
        assert!(!loaded.course_correction);
        assert!(!loaded.punctuation);
        assert_eq!(loaded.activation_hotkey, "F1");
        assert_eq!(loaded.activation_mode, ActivationMode::Toggle);
        assert_eq!(loaded.overlay_position, OverlayPosition::BottomRight);
        assert_eq!(loaded.overlay_opacity, 0.5);
        assert!(loaded.show_raw_transcript);
        assert_eq!(loaded.theme, ThemeMode::Light);
        assert_eq!(loaded.max_segment_ms, 5_000);
        assert_eq!(loaded.overlap_ms, 500);
        assert_eq!(loaded.command_prefix, "ok vox");
        assert!(!loaded.save_history);
    }

    #[test]
    fn test_settings_corrupt_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ this is not valid json }}}").expect("write corrupt");

        let settings = Settings::load(dir.path()).expect("load should not fail");
        // Should return defaults, not crash
        assert_eq!(settings.vad_threshold, 0.5);
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn test_settings_missing_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        // No settings.json exists
        let settings = Settings::load(dir.path()).expect("load should not fail");
        assert_eq!(settings.vad_threshold, 0.5);
        assert_eq!(settings.language, "en");
    }

    #[test]
    fn test_settings_forward_backward_compat() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("settings.json");

        // Forward compat: extra fields should be ignored
        std::fs::write(
            &path,
            r#"{"language": "fr", "future_field": "value", "another_new": 42}"#,
        )
        .expect("write");
        let settings = Settings::load(dir.path()).expect("load");
        assert_eq!(settings.language, "fr");
        // Other fields should be defaults
        assert_eq!(settings.vad_threshold, 0.5);

        // Backward compat: missing fields should use defaults
        std::fs::write(&path, r#"{"language": "ja"}"#).expect("write");
        let settings = Settings::load(dir.path()).expect("load");
        assert_eq!(settings.language, "ja");
        assert_eq!(settings.temperature, 0.1);
        assert!(settings.remove_fillers);
        assert!(settings.save_history);
    }
}
