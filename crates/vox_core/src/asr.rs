//! Automatic Speech Recognition (ASR) engine using Whisper.
//!
//! Provides [`AsrEngine`] for transcribing 16 kHz mono PCM audio into text
//! using the Whisper Large V3 Turbo model via whisper.cpp FFI bindings.
//! Each transcription creates a fresh internal state to prevent cross-utterance
//! contamination. The engine is cheaply cloneable for use across threads.
//!
//! For force-segmented long speech, the [`stitcher`] submodule provides
//! word-level overlap deduplication between consecutive segments.

pub mod stitcher;

pub use stitcher::stitch_segments;

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperError,
};

/// Speech recognition engine wrapping a loaded Whisper model.
///
/// Holds a thread-safe handle to the Whisper model context. Each call to
/// [`transcribe`](AsrEngine::transcribe) creates a fresh internal state,
/// so consecutive transcriptions never contaminate each other.
///
/// Cheaply cloneable via [`Arc`] — cloning shares the underlying model
/// memory and is suitable for moving into background tasks.
pub struct AsrEngine {
    ctx: Arc<Mutex<WhisperContext>>,
}

impl Clone for AsrEngine {
    fn clone(&self) -> Self {
        Self {
            ctx: Arc::clone(&self.ctx),
        }
    }
}

impl AsrEngine {
    /// Load a Whisper model from disk and create a new ASR engine.
    ///
    /// `model_path` must point to a valid ggml-format Whisper model file.
    /// When `use_gpu` is true, GPU acceleration (CUDA on Windows, Metal on
    /// macOS) is enabled for inference.
    pub fn new(model_path: &Path, use_gpu: bool) -> Result<Self> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(use_gpu);

        let path_str = model_path
            .to_str()
            .context("model path contains invalid UTF-8")?;

        let ctx = WhisperContext::new_with_params(path_str, params)
            .map_err(|err| anyhow::anyhow!("failed to load Whisper model from {path_str}: {err}"))?;

        Ok(Self {
            ctx: Arc::new(Mutex::new(ctx)),
        })
    }

    /// Transcribe a complete speech segment into text.
    ///
    /// Accepts 16 kHz mono PCM float samples and returns the transcribed text.
    /// Returns an empty string for empty or silent audio (not an error).
    /// Each call creates a fresh [`WhisperState`](whisper_rs::WhisperState)
    /// internally, so no state leaks between consecutive transcriptions.
    pub fn transcribe(&self, audio_pcm: &[f32]) -> Result<String> {
        if audio_pcm.is_empty() {
            return Ok(String::new());
        }

        // Skip transcription if audio energy is near zero — Whisper hallucinates
        // phantom text (e.g. "Thank you.") on synthetic silence because the
        // no_speech_probability check doesn't catch all-zero input.
        let energy = audio_pcm.iter().map(|s| s * s).sum::<f32>() / audio_pcm.len() as f32;
        if energy < 1e-6 {
            return Ok(String::new());
        }

        let ctx = self.ctx.lock().map_err(|err| anyhow::anyhow!("whisper context mutex poisoned: {err}"))?;
        let mut state = ctx
            .create_state()
            .map_err(|err| anyhow::anyhow!("failed to create whisper state: {err}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_no_speech_thold(0.6);
        params.set_suppress_nst(true);
        params.set_single_segment(true);
        params.set_no_context(true);
        params.set_n_threads(4);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);

        match state.full(params, audio_pcm) {
            Ok(_) => {}
            Err(WhisperError::NoSamples) => return Ok(String::new()),
            Err(err) => return Err(anyhow::anyhow!("whisper transcription failed: {err}")),
        }

        let no_speech_threshold = 0.6;
        let n_segments = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n_segments {
            if let Some(segment) = state.get_segment(i) {
                // Skip segments where the model thinks there's no speech — whisper
                // hallucinates phantom text (e.g. "Thank you.") on silent audio.
                if segment.no_speech_probability() > no_speech_threshold {
                    continue;
                }
                match segment.to_str() {
                    Ok(segment_text) => text.push_str(segment_text),
                    Err(err) => return Err(anyhow::anyhow!("failed to read segment {i} text: {err}")),
                }
            }
        }

        Ok(text.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn model_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("ggml-large-v3-turbo-q5_0.bin")
    }

    fn load_speech_samples() -> Vec<f32> {
        let wav_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("speech_sample.wav");
        let reader = hound::WavReader::open(wav_path).expect("failed to open speech_sample.wav");
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16000, "expected 16 kHz WAV");
        assert_eq!(spec.channels, 1, "expected mono WAV");
        reader
            .into_samples::<i16>()
            .map(|s| s.expect("failed to read WAV sample") as f32 / 32768.0)
            .collect()
    }

    #[test]
    fn test_asr_model_loads() {
        let engine = AsrEngine::new(&model_path(), true);
        assert!(engine.is_ok(), "model should load: {:?}", engine.err());
    }

    #[test]
    fn test_asr_transcribe_speech() {
        let engine = AsrEngine::new(&model_path(), true).expect("model should load");
        let samples = load_speech_samples();
        let text = engine.transcribe(&samples).expect("transcription should succeed");
        assert!(!text.is_empty(), "transcribed text should not be empty");
    }

    #[test]
    fn test_asr_empty_audio() {
        let engine = AsrEngine::new(&model_path(), true).expect("model should load");
        let text = engine.transcribe(&[]).expect("empty audio should not error");
        assert!(text.is_empty(), "empty audio should produce empty string, got: {text:?}");
    }

    #[test]
    fn test_asr_silent_audio() {
        let engine = AsrEngine::new(&model_path(), true).expect("model should load");
        let silence = vec![0.0f32; 16000];
        let text = engine.transcribe(&silence).expect("silent audio should not error");
        assert!(text.is_empty(), "silent audio should produce empty string, got: {text:?}");
    }

    #[test]
    fn test_asr_short_segment() {
        let engine = AsrEngine::new(&model_path(), true).expect("model should load");
        let samples = load_speech_samples();
        let short = &samples[..samples.len().min(8000)];
        let result = engine.transcribe(short);
        assert!(result.is_ok(), "short segment should not panic: {:?}", result.err());
    }

    #[test]
    fn test_asr_model_load_error() {
        let bad_path = Path::new("/nonexistent/path/model.bin");
        let result = AsrEngine::new(bad_path, true);
        assert!(result.is_err(), "nonexistent path should return error");
        let err_msg = result.err().expect("already asserted is_err").to_string();
        assert!(
            err_msg.contains("failed to load Whisper model"),
            "error should be descriptive, got: {err_msg}"
        );
    }

    #[test]
    fn test_asr_sequential() {
        let engine = AsrEngine::new(&model_path(), true).expect("model should load");
        let samples = load_speech_samples();

        let mut results = Vec::new();
        for i in 0..5 {
            let text = engine
                .transcribe(&samples)
                .unwrap_or_else(|err| panic!("transcription {i} failed: {err}"));
            assert!(!text.is_empty(), "transcription {i} should not be empty");
            results.push(text);
        }

        for (i, text) in results.iter().enumerate().skip(1) {
            assert_eq!(
                &results[0], text,
                "transcription {i} differs from transcription 0"
            );
        }

        let cloned_engine = engine.clone();
        let clone_text = cloned_engine
            .transcribe(&samples)
            .expect("cloned engine transcription should succeed");
        assert_eq!(
            results[0], clone_text,
            "cloned engine should produce identical result"
        );
    }
}
