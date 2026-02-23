//! Speech segment accumulator with context padding.
//!
//! The [`SpeechChunker`] buffers audio samples during detected speech segments
//! and emits complete padded segments when the VAD signals speech end. It
//! maintains a circular pre-buffer so that audio context before speech onset
//! is included, and continues accumulating briefly after speech end for
//! post-padding.

use super::{VadConfig, VadEvent};

/// Accumulates audio samples during speech and emits complete padded segments
/// ready for ASR transcription.
///
/// Maintains a circular pre-buffer of recent audio so that when speech starts,
/// the preceding context (300ms by default) is prepended to the segment. After
/// speech ends, continues collecting post-padding samples (100ms) before emitting.
/// Force-segmented long speech includes a 1-second overlap for ASR stitching.
pub struct SpeechChunker {
    config: VadConfig,
    /// Accumulated speech samples for the current segment.
    speech_buffer: Vec<f32>,
    /// Circular buffer holding the last `pre_pad_ms` of audio for pre-padding.
    pre_buffer: Vec<f32>,
    /// Current write position in the circular pre-buffer.
    pre_buffer_pos: usize,
    /// Whether currently accumulating speech samples.
    is_accumulating: bool,
    /// Remaining samples to collect for post-padding after SpeechEnd.
    post_pad_remaining: u32,
}

impl SpeechChunker {
    /// Create a new chunker with the given configuration.
    ///
    /// Initializes the circular pre-buffer to hold `pre_pad_ms` worth of
    /// samples (4,800 samples at 300ms × 16 kHz) filled with silence.
    pub fn new(config: VadConfig) -> Self {
        let pre_buffer_capacity =
            (config.pre_pad_ms as usize * 16000) / 1000;
        Self {
            speech_buffer: Vec::new(),
            pre_buffer: vec![0.0f32; pre_buffer_capacity],
            pre_buffer_pos: 0,
            is_accumulating: false,
            post_pad_remaining: 0,
            config,
        }
    }

    /// Feed audio samples and an optional VAD event.
    ///
    /// Returns a complete speech segment when one is ready (on SpeechEnd after
    /// post-padding is collected, or on ForceSegment). The caller should check
    /// the return value each call and dispatch any returned segment to the ASR
    /// engine.
    pub fn feed(
        &mut self,
        samples: &[f32],
        event: Option<&VadEvent>,
    ) -> Option<Vec<f32>> {
        let mut emitted_segment = None;

        // Keep pre-padding context updated regardless of current VAD state.
        self.write_pre_buffer(samples);

        // First, continue any active accumulation with this window.
        if self.is_accumulating {
            self.speech_buffer.extend_from_slice(samples);

            // If we're collecting trailing pad after SpeechEnd, count down.
            if self.post_pad_remaining > 0 {
                self.post_pad_remaining = self
                    .post_pad_remaining
                    .saturating_sub(samples.len() as u32);

                if self.post_pad_remaining == 0 {
                    emitted_segment = Some(std::mem::take(&mut self.speech_buffer));
                    self.is_accumulating = false;
                }
            }
        }

        // Then handle boundary/control events for this window.
        if let Some(event) = event {
            match event {
                VadEvent::SpeechStart => {
                    if !self.is_accumulating {
                        self.is_accumulating = true;
                        self.post_pad_remaining = 0;
                        self.speech_buffer.clear();
                        self.speech_buffer.extend(self.read_pre_buffer_ordered());
                    }
                }
                VadEvent::SpeechEnd { .. } => {
                    if self.is_accumulating {
                        self.post_pad_remaining =
                            (self.config.post_pad_ms * 16000) / 1000;

                        // If no post-padding is configured, emit immediately.
                        if self.post_pad_remaining == 0 && emitted_segment.is_none() {
                            emitted_segment =
                                Some(std::mem::take(&mut self.speech_buffer));
                            self.is_accumulating = false;
                        }
                    }
                }
                VadEvent::ForceSegment { .. } => {
                    if self.is_accumulating
                        && !self.speech_buffer.is_empty()
                        && emitted_segment.is_none()
                    {
                        let overlap_samples = self.speech_buffer.len().min(16_000);
                        let overlap_start =
                            self.speech_buffer.len() - overlap_samples;
                        let overlap =
                            self.speech_buffer[overlap_start..].to_vec();

                        emitted_segment =
                            Some(std::mem::take(&mut self.speech_buffer));
                        self.speech_buffer = overlap;
                        self.post_pad_remaining = 0;
                        self.is_accumulating = true;
                    }
                }
            }
        }

        emitted_segment
    }

    /// Flush any buffered audio as a final segment.
    ///
    /// Call this when recording stops mid-utterance to ensure no audio is lost.
    /// Returns `None` if no speech was being accumulated.
    pub fn flush(&mut self) -> Option<Vec<f32>> {
        if self.is_accumulating && !self.speech_buffer.is_empty() {
            let segment = std::mem::take(&mut self.speech_buffer);
            self.is_accumulating = false;
            self.post_pad_remaining = 0;
            Some(segment)
        } else {
            None
        }
    }

    /// Write samples into the circular pre-buffer, overwriting oldest data.
    fn write_pre_buffer(&mut self, samples: &[f32]) {
        if self.pre_buffer.is_empty() {
            return;
        }

        for &sample in samples {
            self.pre_buffer[self.pre_buffer_pos] = sample;
            self.pre_buffer_pos = (self.pre_buffer_pos + 1) % self.pre_buffer.len();
        }
    }

    /// Read the pre-buffer contents in chronological order (oldest to newest).
    fn read_pre_buffer_ordered(&self) -> Vec<f32> {
        let len = self.pre_buffer.len();
        if len == 0 {
            return Vec::new();
        }

        let mut ordered = Vec::with_capacity(len);
        for i in 0..len {
            ordered.push(self.pre_buffer[(self.pre_buffer_pos + i) % len]);
        }
        ordered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_chunker() -> SpeechChunker {
        SpeechChunker::new(VadConfig::default())
    }

    /// Helper: number of samples in `ms` milliseconds at 16 kHz.
    fn ms_to_samples(ms: u32) -> usize {
        (ms as usize * 16000) / 1000
    }

    #[test]
    fn test_chunker_accumulates() {
        let mut chunker = default_chunker();
        let window = vec![0.5f32; 512];

        // Start speech
        let result = chunker.feed(&window, Some(&VadEvent::SpeechStart));
        assert!(result.is_none(), "should not emit on SpeechStart");

        // Feed 3 more windows — should accumulate without emitting
        for _ in 0..3 {
            let result = chunker.feed(&window, None);
            assert!(result.is_none(), "should not emit while speaking");
        }

        // On SpeechStart: is_accumulating is false, so extend_from_slice skips.
        // SpeechStart handler prepends pre_buffer (4800 samples at 300ms).
        // Subsequent 3 feeds each add 512 via extend_from_slice.
        // Total = 4800 + 3*512 = 6336.
        // Just verify it's growing by flushing.
        let segment = chunker.flush();
        assert!(segment.is_some());
        let seg = segment.unwrap();
        assert!(
            seg.len() > 512 * 3,
            "should have accumulated more than 3 windows, got {} samples",
            seg.len()
        );
    }

    #[test]
    fn test_chunker_emits_on_end() {
        let mut chunker = default_chunker();
        let window = vec![1.0f32; 512];

        // Start speech
        chunker.feed(&window, Some(&VadEvent::SpeechStart));

        // Speak for 10 windows (~320ms)
        for _ in 0..10 {
            assert!(chunker.feed(&window, None).is_none());
        }

        // Signal speech end — post-padding begins (100ms = 1600 samples)
        let result = chunker.feed(&window, Some(&VadEvent::SpeechEnd { duration_ms: 320 }));
        assert!(result.is_none(), "should not emit immediately — needs post-padding");

        // Feed enough windows to satisfy post-padding: 1600 / 512 ≈ 4 windows
        let mut emitted = None;
        for _ in 0..5 {
            if let Some(seg) = chunker.feed(&window, None) {
                emitted = Some(seg);
                break;
            }
        }

        assert!(emitted.is_some(), "should emit after post-padding collected");
    }

    #[test]
    fn test_chunker_padding() {
        let mut chunker = default_chunker();
        let pad_samples = ms_to_samples(chunker.config.pre_pad_ms); // 4800

        // Fill pre-buffer with recognizable data by feeding silence before speech
        let pre_fill = vec![0.1f32; 512];
        for _ in 0..(pad_samples / 512 + 1) {
            chunker.feed(&pre_fill, None);
        }

        // Start speech
        let speech = vec![0.9f32; 512];
        chunker.feed(&speech, Some(&VadEvent::SpeechStart));

        // 10 windows of speech
        let speech_windows = 10;
        for _ in 0..speech_windows {
            chunker.feed(&speech, None);
        }

        // End speech — starts post-padding countdown
        chunker.feed(&speech, Some(&VadEvent::SpeechEnd { duration_ms: 320 }));

        // Collect post-padding
        let post_fill = vec![0.05f32; 512];
        let mut segment = None;
        for _ in 0..10 {
            if let Some(seg) = chunker.feed(&post_fill, None) {
                segment = Some(seg);
                break;
            }
        }

        let seg = segment.expect("should have emitted a segment");

        // The segment should be longer than just the raw speech windows
        // because it includes pre-padding + speech + post-padding.
        // Raw speech = SpeechStart window + 10 windows + SpeechEnd window = 12 × 512 = 6144
        // With padding ≈ pre(4800) + speech(6144) + post(~1600) = ~12544
        let raw_speech_samples = (speech_windows + 2) * 512; // +2 for start/end windows
        assert!(
            seg.len() > raw_speech_samples,
            "padded segment ({}) should be longer than raw speech ({})",
            seg.len(),
            raw_speech_samples
        );

        // Verify padding adds approximately pre_pad + post_pad samples
        let padding_added = seg.len() - raw_speech_samples;
        let post_pad_samples = ms_to_samples(chunker.config.post_pad_ms);
        let expected_padding = pad_samples + post_pad_samples;
        // Allow tolerance since post-pad countdown works in window-sized chunks
        assert!(
            padding_added > expected_padding / 2,
            "padding ({padding_added} samples) should be roughly {expected_padding}"
        );
    }

    #[test]
    fn test_chunker_flush() {
        let mut chunker = default_chunker();
        let window = vec![0.7f32; 512];

        // No speech — flush should return None
        assert!(chunker.flush().is_none());

        // Start speech and accumulate
        chunker.feed(&window, Some(&VadEvent::SpeechStart));
        chunker.feed(&window, None);
        chunker.feed(&window, None);

        // Flush mid-speech
        let segment = chunker.flush();
        assert!(segment.is_some(), "flush should return buffered audio");
        assert!(
            !segment.unwrap().is_empty(),
            "flushed segment should not be empty"
        );

        // After flush, another flush returns None
        assert!(chunker.flush().is_none());
    }

    #[test]
    fn test_chunker_force_segment_overlap() {
        let mut chunker = default_chunker();

        // Start speech
        let window = vec![0.8f32; 512];
        chunker.feed(&window, Some(&VadEvent::SpeechStart));

        // Feed enough windows for >10 seconds of speech (10s = 10_000ms = 160_000 samples)
        // 160_000 / 512 ≈ 313 windows
        let windows_for_10s = 160_000 / 512;
        for _ in 0..windows_for_10s {
            chunker.feed(&window, None);
        }

        // Force segment
        let segment = chunker.feed(
            &window,
            Some(&VadEvent::ForceSegment { duration_ms: 30_000 }),
        );

        assert!(segment.is_some(), "should emit on ForceSegment");
        let seg = segment.unwrap();

        // The emitted segment should contain all accumulated speech
        assert!(
            seg.len() > 160_000,
            "segment should contain >10s of audio, got {} samples",
            seg.len()
        );

        // After force-segment, chunker should still be accumulating
        // with the 1s overlap (16_000 samples) already in the buffer
        assert!(chunker.is_accumulating);

        // Flush to verify overlap is present
        let remainder = chunker.flush();
        assert!(remainder.is_some());
        let rem = remainder.unwrap();
        assert_eq!(
            rem.len(), 16_000,
            "overlap should be exactly 16,000 samples (1s at 16kHz), got {}",
            rem.len()
        );
    }
}
