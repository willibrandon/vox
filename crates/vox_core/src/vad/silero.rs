//! Silero VAD v5 inference engine using ONNX Runtime.
//!
//! Wraps the Silero VAD v5 ONNX model for speech probability detection.
//! Each call to [`SileroVad::process`] takes a 512-sample audio window (32ms
//! at 16 kHz) and returns a speech probability in [0.0, 1.0]. Internal hidden
//! state and audio context are preserved across calls within a session and can
//! be reset between sessions via [`SileroVad::reset`].

use std::path::Path;

use anyhow::{bail, Result};
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::Tensor;

/// Initialize the ONNX Runtime dynamic library for the current platform.
///
/// Locates the platform-specific DLL/dylib/so in the `vendor/onnxruntime/`
/// directory (development) or next to the executable (production). Must be
/// called before creating any `ort::session::Session`. Safe to call multiple
/// times — the underlying `OnceLock` makes subsequent calls a no-op.
fn init_ort_runtime() -> Result<()> {
    #[cfg(target_os = "windows")]
    const ORT_LIB_NAME: &str = "onnxruntime.dll";
    #[cfg(target_os = "macos")]
    const ORT_LIB_NAME: &str = "libonnxruntime.dylib";
    #[cfg(target_os = "linux")]
    const ORT_LIB_NAME: &str = "libonnxruntime.so";

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();

    // Production: next to the executable (e.g. Contents/MacOS/ in a .app bundle).
    // Checked first so the signed bundled dylib is always preferred over the dev
    // vendor copy — this keeps library validation intact under hardened runtime
    // (both the dylib and the main executable share the same signing identity).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            candidates.push(exe_dir.join(ORT_LIB_NAME));
        }
    }

    // Development: vendor directory relative to workspace root (CARGO_MANIFEST_DIR
    // is baked at compile time, so this path only exists on the build machine).
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    candidates.push(
        manifest_dir
            .join("../../vendor/onnxruntime")
            .join(ORT_LIB_NAME),
    );

    // Try each candidate in order — a load failure (e.g. hardened runtime library
    // validation rejecting an unsigned dylib) must not abort; fall through to the
    // next candidate instead.
    for candidate in &candidates {
        if candidate.exists() {
            match ort::init_from(candidate) {
                Ok(_) => return Ok(()),
                Err(err) => {
                    tracing::warn!(
                        path = %candidate.display(),
                        %err,
                        "ONNX Runtime found but failed to load, trying next candidate"
                    );
                }
            }
        }
    }

    bail!(
        "ONNX Runtime library '{ORT_LIB_NAME}' not found or could not be loaded. Checked:\n{}",
        candidates
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Number of audio samples carried as context between consecutive inference calls.
/// The Silero VAD v5 model expects each input to be prefixed with the trailing
/// 64 samples from the previous call's input, providing raw audio overlap for
/// the model's internal convolution layers.
const CONTEXT_SAMPLES: usize = 64;

/// Silero VAD v5 speech detection engine backed by ONNX Runtime.
///
/// Loads the ~2.3 MB Silero VAD v5 ONNX model and runs single-threaded
/// CPU inference. The model processes 512-sample windows and returns a
/// speech probability score. Both LSTM hidden state and a 64-sample audio
/// context window are carried across consecutive calls for context-aware
/// detection.
pub struct SileroVad {
    session: Session,
    /// Hidden state tensor data: 2 layers × 1 batch × 128 hidden = 256 f32 elements.
    hidden_state: Vec<f32>,
    /// Audio context from the previous call's input (last 64 samples).
    /// Prepended to the next call's audio to give the model raw audio overlap.
    context: Vec<f32>,
    /// Sample rate passed to the model. Always 16000.
    sample_rate: i64,
}

impl SileroVad {
    /// Load the Silero VAD v5 ONNX model from disk.
    ///
    /// Creates a single-threaded ONNX Runtime session with Level3 optimization.
    /// The model file is approximately 1.1 MB. Logs input/output tensor names
    /// on load for debugging.
    pub fn new(model_path: &Path) -> Result<Self> {
        init_ort_runtime()?;

        let session = Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(1)?
            .commit_from_file(model_path)?;

        // Log model input/output names for verification
        for input in session.inputs() {
            tracing::debug!("VAD model input: {}", input.name());
        }
        for output in session.outputs() {
            tracing::debug!("VAD model output: {}", output.name());
        }

        Ok(Self {
            session,
            hidden_state: vec![0.0f32; 256], // shape [2, 1, 128]
            context: vec![0.0f32; CONTEXT_SAMPLES],
            sample_rate: 16000,
        })
    }

    /// Process a single 512-sample audio window and return a speech probability.
    ///
    /// The audio slice must contain exactly 512 f32 samples (32ms at 16 kHz).
    /// Returns a probability in [0.0, 1.0] where higher values indicate speech.
    /// The internal hidden state is updated after each call, enabling context-aware
    /// detection across consecutive windows.
    pub fn process(&mut self, audio: &[f32]) -> Result<f32> {
        if audio.len() != 512 {
            bail!(
                "SileroVad::process expects exactly 512 samples, got {}",
                audio.len()
            );
        }

        // Build the model input: [context (64) | audio (512)] = 576 samples.
        let input_len = CONTEXT_SAMPLES + 512;
        let mut input_data = Vec::with_capacity(input_len);
        input_data.extend_from_slice(&self.context);
        input_data.extend_from_slice(audio);

        let input_tensor = Tensor::from_array(([1usize, input_len], input_data.clone()))?;
        let sr_tensor = Tensor::<i64>::from_array(([1usize], vec![self.sample_rate]))?;
        let state_tensor =
            Tensor::from_array(([2usize, 1, 128], self.hidden_state.clone()))?;

        let outputs = self.session.run(ort::inputs! {
            "input" => input_tensor,
            "sr" => sr_tensor,
            "state" => state_tensor,
        })?;

        // Extract speech probability — try_extract_tensor returns (&Shape, &[f32])
        let (_shape, output_data) = outputs["output"].try_extract_tensor::<f32>()?;
        let speech_prob = output_data.first().copied().unwrap_or(0.0);

        // Update hidden state from model output
        let (_state_shape, state_data) =
            outputs["stateN"].try_extract_tensor::<f32>()?;
        if state_data.len() == self.hidden_state.len() {
            self.hidden_state.copy_from_slice(state_data);
        }

        // Carry the last 64 samples of the full input as context for next call
        self.context.copy_from_slice(&input_data[input_len - CONTEXT_SAMPLES..]);

        Ok(speech_prob.clamp(0.0, 1.0))
    }

    /// Reset the hidden state to zeros for a new dictation session.
    ///
    /// Call this between sessions to ensure the model starts with a clean
    /// slate and prior speech context does not influence the new session.
    pub fn reset(&mut self) {
        self.hidden_state.fill(0.0);
        self.context.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn model_path() -> PathBuf {
        fixture_dir().join("silero_vad_v5.onnx")
    }

    #[test]
    fn test_vad_model_loads() {
        let result = SileroVad::new(&model_path());
        assert!(result.is_ok(), "Failed to load VAD model: {:?}", result.err());
    }

    #[test]
    fn test_vad_silent_audio() {
        let mut vad = SileroVad::new(&model_path()).expect("Failed to load model");
        let silence = vec![0.0f32; 512];

        for i in 0..10 {
            let prob = vad.process(&silence).expect("process failed");
            assert!(
                prob < 0.1,
                "Window {i}: silence should have speech_prob < 0.1, got {prob}"
            );
        }
    }

    #[test]
    fn test_vad_speech_audio() {
        let mut vad = SileroVad::new(&model_path()).expect("Failed to load model");

        let wav_path = fixture_dir().join("speech_sample.wav");
        let mut reader =
            hound::WavReader::open(&wav_path).expect("Failed to open speech WAV");

        let samples: Vec<f32> = if reader.spec().sample_format == hound::SampleFormat::Float {
            reader.samples::<f32>().filter_map(|s| s.ok()).collect()
        } else {
            reader
                .samples::<i16>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / 32768.0)
                .collect()
        };

        let mut speech_count = 0;
        let mut total_windows = 0;

        for chunk in samples.chunks_exact(512) {
            let prob = vad.process(chunk).expect("process failed");
            total_windows += 1;
            if prob > 0.5 {
                speech_count += 1;
            }
        }

        assert!(
            total_windows > 0,
            "WAV file should contain at least one 512-sample window"
        );
        let speech_ratio = speech_count as f32 / total_windows as f32;
        assert!(
            speech_ratio > 0.5,
            "Expected speech in >50% of windows, got {speech_count}/{total_windows} ({:.0}%)",
            speech_ratio * 100.0
        );
    }

    #[test]
    fn test_vad_hidden_state_persistence() {
        let mut vad = SileroVad::new(&model_path()).expect("Failed to load model");
        let audio = vec![0.0f32; 512];

        // Process 3 windows
        for _ in 0..3 {
            vad.process(&audio).expect("process failed");
        }

        // Hidden state should no longer be all zeros
        let all_zero = vad.hidden_state.iter().all(|&v| v == 0.0);
        assert!(
            !all_zero,
            "Hidden state should have changed after processing windows"
        );
    }

    #[test]
    fn test_vad_reset() {
        let mut vad = SileroVad::new(&model_path()).expect("Failed to load model");
        let audio = vec![0.0f32; 512];

        // Process a window to modify hidden state
        vad.process(&audio).expect("process failed");

        // Reset
        vad.reset();

        // Hidden state should be all zeros
        assert!(
            vad.hidden_state.iter().all(|&v| v == 0.0),
            "Hidden state should be all zeros after reset"
        );
    }
}
