//! Synthetic audio injection for diagnostics testing.
//!
//! Loads audio from WAV files or base64-encoded PCM, runs it through
//! ASR and LLM post-processing, and returns the transcript without
//! injecting text into any application. This lets AI coding assistants
//! test the full speech-to-text pipeline programmatically.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};

use crate::audio::resampler::AudioResampler;
use crate::llm::ProcessorOutput;
use crate::state::VoxState;

/// Target sample rate for ASR input.
const ASR_SAMPLE_RATE: u32 = 16_000;

/// Result of a synthetic audio injection run.
pub struct InjectionResult {
    /// Raw transcript from ASR (before LLM post-processing).
    pub raw_transcript: String,
    /// Polished text from LLM post-processing, or the command JSON.
    pub polished_text: String,
    /// Whether the LLM detected a voice command (vs. dictated text).
    pub is_command: bool,
    /// End-to-end processing time in milliseconds.
    pub latency_ms: u64,
}

/// Load audio samples from a WAV file on disk.
///
/// Reads the WAV, converts to mono f32, and resamples to 16 kHz
/// if the source rate differs. Returns the samples and original
/// sample rate.
pub fn load_wav(path: &Path) -> Result<Vec<f32>> {
    let reader =
        hound::WavReader::open(path).with_context(|| format!("failed to open WAV: {}", path.display()))?;

    let spec = reader.spec();
    let source_rate = spec.sample_rate;

    let mono_samples = read_wav_to_mono_f32(reader, spec)?;

    if source_rate == ASR_SAMPLE_RATE {
        return Ok(mono_samples);
    }

    let mut resampler = AudioResampler::new(source_rate, ASR_SAMPLE_RATE)
        .context("failed to create resampler")?;
    resampler
        .process(&mono_samples)
        .with_context(|| format!("failed to resample from {source_rate} Hz to {ASR_SAMPLE_RATE} Hz"))
}

/// Decode base64-encoded little-endian f32 PCM samples.
///
/// If `sample_rate` differs from 16 kHz, resamples to match.
pub fn load_pcm_base64(base64_data: &str, sample_rate: u32) -> Result<Vec<f32>> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data)
        .context("invalid base64 encoding")?;

    if bytes.len() % 4 != 0 {
        anyhow::bail!(
            "PCM data length ({}) is not a multiple of 4 (f32 samples)",
            bytes.len()
        );
    }

    let samples: Vec<f32> = bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();

    if sample_rate == ASR_SAMPLE_RATE {
        return Ok(samples);
    }

    let mut resampler = AudioResampler::new(sample_rate, ASR_SAMPLE_RATE)
        .context("failed to create resampler")?;
    resampler
        .process(&samples)
        .with_context(|| format!("failed to resample from {sample_rate} Hz to {ASR_SAMPLE_RATE} Hz"))
}

/// Run the full ASR + LLM pipeline on pre-loaded audio samples.
///
/// Clones the ASR engine and LLM processor from VoxState (Arc-based,
/// cheap to clone), transcribes the audio, then post-processes the
/// raw transcript through the LLM. Returns the combined result
/// without injecting any text.
pub fn run(samples: &[f32], state: &VoxState) -> Result<InjectionResult> {
    let asr = state
        .clone_asr_engine()
        .context("ASR engine not loaded — is the app ready?")?;
    let llm = state
        .clone_llm_processor()
        .context("LLM processor not loaded — is the app ready?")?;

    let start = Instant::now();

    let raw_transcript = asr
        .transcribe(samples)
        .context("ASR transcription failed")?;

    if raw_transcript.is_empty() {
        return Ok(InjectionResult {
            raw_transcript: String::new(),
            polished_text: String::new(),
            is_command: false,
            latency_ms: start.elapsed().as_millis() as u64,
        });
    }

    let hints = state.dictionary().top_hints(50);
    let output = llm
        .process(&raw_transcript, &hints, "diagnostics")
        .context("LLM post-processing failed")?;

    let (polished_text, is_command) = match output {
        ProcessorOutput::Text(text) => (text, false),
        ProcessorOutput::Command(cmd) => {
            let json = serde_json::to_string(&cmd)
                .unwrap_or_else(|_| format!("{{\"cmd\":\"{}\"}}", cmd.cmd));
            (json, true)
        }
    };

    Ok(InjectionResult {
        raw_transcript,
        polished_text,
        is_command,
        latency_ms: start.elapsed().as_millis() as u64,
    })
}

/// Read WAV samples into mono f32 format regardless of source format.
fn read_wav_to_mono_f32(
    reader: hound::WavReader<std::io::BufReader<std::fs::File>>,
    spec: hound::WavSpec,
) -> Result<Vec<f32>> {
    let channels = spec.channels as usize;

    match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Float, 32) => {
            let all_samples: Vec<f32> = reader
                .into_samples::<f32>()
                .collect::<Result<Vec<f32>, _>>()
                .context("failed to read f32 WAV samples")?;
            Ok(mix_to_mono(&all_samples, channels))
        }
        (hound::SampleFormat::Int, 16) => {
            let all_samples: Vec<f32> = reader
                .into_samples::<i16>()
                .collect::<Result<Vec<i16>, _>>()
                .context("failed to read i16 WAV samples")?
                .into_iter()
                .map(|s| s as f32 / i16::MAX as f32)
                .collect();
            Ok(mix_to_mono(&all_samples, channels))
        }
        (hound::SampleFormat::Int, 24) => {
            let all_samples: Vec<f32> = reader
                .into_samples::<i32>()
                .collect::<Result<Vec<i32>, _>>()
                .context("failed to read i24 WAV samples")?
                .into_iter()
                .map(|s| s as f32 / 8_388_607.0) // 2^23 - 1
                .collect();
            Ok(mix_to_mono(&all_samples, channels))
        }
        (fmt, bits) => {
            anyhow::bail!("unsupported WAV format: {fmt:?} {bits}-bit")
        }
    }
}

/// Mix interleaved multi-channel audio down to mono by averaging channels.
fn mix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_wav_speech_sample() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("speech_sample.wav");
        let samples = load_wav(&path).expect("should load speech_sample.wav");
        assert!(
            !samples.is_empty(),
            "loaded WAV should contain samples"
        );
        // speech_sample.wav is 16kHz mono — should have a reasonable number of samples
        assert!(
            samples.len() > 1000,
            "expected more than 1000 samples, got {}",
            samples.len()
        );
    }

    #[test]
    fn test_load_wav_nonexistent_path() {
        let path = Path::new("/nonexistent/path/to/audio.wav");
        let result = load_wav(path);
        assert!(result.is_err(), "should fail for nonexistent path");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("failed to open WAV"),
            "error should mention WAV open failure, got: {err_msg}"
        );
    }

    #[test]
    fn test_load_pcm_base64_roundtrip() {
        use base64::Engine;
        let original: Vec<f32> = vec![0.0, 0.5, -0.5, 1.0];
        let bytes: Vec<u8> = original
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);

        let decoded = load_pcm_base64(&encoded, ASR_SAMPLE_RATE)
            .expect("should decode valid base64 PCM");
        assert_eq!(decoded.len(), original.len());
        for (a, b) in decoded.iter().zip(original.iter()) {
            assert!(
                (a - b).abs() < f32::EPSILON,
                "sample mismatch: {a} vs {b}"
            );
        }
    }

    #[test]
    fn test_load_pcm_base64_invalid() {
        let result = load_pcm_base64("not-valid-base64!!!", 16000);
        assert!(result.is_err(), "should fail for invalid base64");
    }

    #[test]
    fn test_load_pcm_base64_wrong_length() {
        use base64::Engine;
        // 5 bytes — not a multiple of 4
        let bytes = vec![0u8; 5];
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let result = load_pcm_base64(&encoded, 16000);
        assert!(result.is_err(), "should fail for non-multiple-of-4 length");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not a multiple of 4"),
            "error should mention alignment, got: {err_msg}"
        );
    }

    #[test]
    fn test_mix_to_mono_stereo() {
        let stereo = vec![1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        let mono = mix_to_mono(&stereo, 2);
        assert_eq!(mono.len(), 3);
        assert!((mono[0] - 0.5).abs() < f32::EPSILON);
        assert!((mono[1] - 0.5).abs() < f32::EPSILON);
        assert!((mono[2] - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_mix_to_mono_passthrough() {
        let mono_input = vec![0.1, 0.2, 0.3];
        let result = mix_to_mono(&mono_input, 1);
        assert_eq!(result, mono_input);
    }
}
