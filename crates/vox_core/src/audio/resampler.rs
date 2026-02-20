use anyhow::Result;
use audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};

/// Converts audio from a device's native sample rate to the 16 kHz mono f32
/// format required by the downstream VAD and ASR stages.
///
/// Uses an FFT-based resampler from rubato. When the device already captures
/// at 16 kHz, no resampler is needed — [`AudioResampler::new`] returns `None`.
pub struct AudioResampler {
    resampler: Fft<f32>,
    #[allow(dead_code)] // stored for diagnostics and potential future use
    input_rate: u32,
    #[allow(dead_code)]
    output_rate: u32,
}

impl AudioResampler {
    /// Create a resampler that converts from `input_rate` to `output_rate`.
    ///
    /// Returns `None` when the rates are equal (no resampling needed).
    /// The resampler processes in fixed 1024-frame input chunks internally.
    pub fn new(input_rate: u32, output_rate: u32) -> Option<Self> {
        if input_rate == output_rate {
            return None;
        }
        let resampler = Fft::<f32>::new(
            input_rate as usize,
            output_rate as usize,
            1024,
            1,
            1,
            FixedSync::Input,
        )
        .ok()?;
        Some(Self {
            resampler,
            input_rate,
            output_rate,
        })
    }

    /// Resample a buffer of mono f32 audio samples.
    ///
    /// Accepts an arbitrary-length input slice at the input rate and returns
    /// the corresponding samples at the output rate. Called on the processing
    /// thread, never from the real-time audio callback.
    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>> {
        let input_frames = input.len();
        let output_len = self.resampler.process_all_needed_output_len(input_frames);

        let input_data = vec![input.to_vec()];
        let input_adapter = SequentialSliceOfVecs::new(&input_data, 1, input_frames)
            .map_err(|e| anyhow::anyhow!("input adapter error: {e}"))?;

        let mut output_data = vec![vec![0.0f32; output_len]];
        let mut output_adapter = SequentialSliceOfVecs::new_mut(&mut output_data, 1, output_len)
            .map_err(|e| anyhow::anyhow!("output adapter error: {e}"))?;

        let (_input_read, output_written) = self
            .resampler
            .process_all_into_buffer(&input_adapter, &mut output_adapter, input_frames, None)
            .map_err(|e| anyhow::anyhow!("resample error: {e}"))?;

        let mut result = output_data.into_iter().next().expect("single channel");
        result.truncate(output_written);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_sine(sample_rate: u32, frequency: f32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (sample_rate as f32 * duration_secs) as usize;
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * frequency * t).sin()
            })
            .collect()
    }

    fn estimate_frequency(samples: &[f32], sample_rate: u32) -> f32 {
        // Count zero crossings to estimate frequency
        let mut crossings = 0u32;
        for window in samples.windows(2) {
            if (window[0] >= 0.0) != (window[1] >= 0.0) {
                crossings += 1;
            }
        }
        // Each full cycle has 2 zero crossings
        let duration = samples.len() as f32 / sample_rate as f32;
        crossings as f32 / (2.0 * duration)
    }

    #[test]
    fn test_resampler_48000_to_16000() {
        let input = generate_sine(48000, 440.0, 1.0);
        assert_eq!(input.len(), 48000);

        let mut resampler = AudioResampler::new(48000, 16000).expect("resampler should be created");
        let output = resampler.process(&input).expect("resample should succeed");

        // Output length should be approximately 16000 (±5%)
        let expected = 16000;
        let tolerance = expected / 20; // 5%
        assert!(
            output.len().abs_diff(expected) <= tolerance,
            "expected ~{expected} samples, got {}",
            output.len()
        );

        // Verify frequency is preserved (~440 Hz)
        let detected_freq = estimate_frequency(&output, 16000);
        assert!(
            (detected_freq - 440.0).abs() < 20.0,
            "expected ~440 Hz, got {detected_freq} Hz"
        );
    }

    #[test]
    fn test_resampler_44100_to_16000() {
        let input = generate_sine(44100, 440.0, 1.0);
        assert_eq!(input.len(), 44100);

        let mut resampler = AudioResampler::new(44100, 16000).expect("resampler should be created");
        let output = resampler.process(&input).expect("resample should succeed");

        let expected = 16000;
        let tolerance = expected / 20;
        assert!(
            output.len().abs_diff(expected) <= tolerance,
            "expected ~{expected} samples, got {}",
            output.len()
        );

        let detected_freq = estimate_frequency(&output, 16000);
        assert!(
            (detected_freq - 440.0).abs() < 20.0,
            "expected ~440 Hz, got {detected_freq} Hz"
        );
    }

    #[test]
    fn test_resampler_16000_bypass() {
        assert!(
            AudioResampler::new(16000, 16000).is_none(),
            "resampler should return None when rates match"
        );
    }
}
