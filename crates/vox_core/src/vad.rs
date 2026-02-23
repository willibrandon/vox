//! Voice Activity Detection (VAD) subsystem for the Vox dictation engine.
//!
//! Determines when the user is speaking by analyzing 512-sample audio windows
//! (32ms at 16 kHz) through Silero VAD v5 via ONNX Runtime. A streaming state
//! machine converts speech probabilities into discrete boundary events, and a
//! chunker accumulates audio into padded segments dispatched to the ASR engine.
//!
//! All VAD processing runs on the processing thread, never the audio callback.

/// Speech segment accumulator with context padding for ASR delivery.
pub mod chunker;
/// Silero VAD v5 ONNX model wrapper for speech probability detection.
pub mod silero;

pub use chunker::SpeechChunker;
pub use silero::SileroVad;

use anyhow::Result;
use ringbuf::traits::{Consumer, Observer};
use ringbuf::HeapCons;

/// Configuration parameters for the voice activity detection subsystem.
///
/// All timing values have sensible defaults tuned for real-time dictation.
/// Passed by reference to components — immutable after construction.
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Speech probability threshold. Values at or above trigger the Speaking state.
    pub threshold: f32,
    /// Minimum speech duration (ms) to accept as a valid utterance.
    /// Shorter bursts are silently discarded as noise.
    pub min_speech_ms: u32,
    /// Consecutive silence duration (ms) required to trigger a SpeechEnd event.
    pub min_silence_ms: u32,
    /// Maximum speech duration (ms) before force-segmenting to cap memory usage.
    pub max_speech_ms: u32,
    /// Audio pre-padding (ms) captured before speech onset via a circular buffer.
    /// The VAD may fire SpeechStart several hundred milliseconds after the actual
    /// start of speech (soft onsets like nasals /m/, /n/). A larger pre-buffer
    /// recovers the missed beginning, preventing ASR hallucinations.
    pub pre_pad_ms: u32,
    /// Audio post-padding (ms) collected after SpeechEnd before emitting the
    /// segment. Captures trailing sounds and gives the ASR engine end-of-speech
    /// context without adding excessive latency.
    pub post_pad_ms: u32,
    /// Silero VAD input window size in samples. Fixed at 512 for 16 kHz (32ms).
    pub window_size_samples: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech_ms: 250,
            min_silence_ms: 500,
            max_speech_ms: 30_000,
            pre_pad_ms: 300,
            post_pad_ms: 100,
            window_size_samples: 512,
        }
    }
}

/// Current state of the VAD streaming state machine.
///
/// Tracks whether the system is waiting for speech or actively accumulating
/// a speech segment, along with timing metadata for the current segment.
#[derive(Debug, Clone, PartialEq)]
pub enum VadState {
    /// Waiting for speech. No audio is being accumulated.
    Silent,
    /// Speech detected and audio is being accumulated.
    Speaking {
        /// Sample index at which this speech segment began.
        start_sample: usize,
        /// Elapsed speech duration in milliseconds since segment start.
        speech_duration_ms: u32,
    },
}

/// Events emitted by the VAD state machine on state transitions.
///
/// Downstream consumers (the SpeechChunker) use these events to know when
/// to start/stop accumulating audio and when to emit complete segments.
#[derive(Debug, Clone, PartialEq)]
pub enum VadEvent {
    /// Speech detected — begin accumulating audio samples.
    SpeechStart,
    /// Speech ended after sufficient silence — the accumulated segment is ready.
    SpeechEnd {
        /// Duration of the speech segment in milliseconds.
        duration_ms: u32,
    },
    /// Speech exceeded the maximum duration and was force-segmented.
    /// More speech may follow immediately.
    ForceSegment {
        /// Duration of the force-segmented speech in milliseconds.
        duration_ms: u32,
    },
}

/// Streaming state machine that converts speech probabilities into discrete
/// speech boundary events.
///
/// Fed one speech probability per 512-sample window, it tracks transitions
/// between Silent and Speaking states using configurable timing thresholds.
/// Pure logic — no audio data, no I/O, no allocations on the hot path.
pub struct VadStateMachine {
    config: VadConfig,
    state: VadState,
    silence_duration_ms: u32,
    total_samples_processed: usize,
}

impl VadStateMachine {
    /// Create a new state machine starting in the Silent state.
    pub fn new(config: VadConfig) -> Self {
        Self {
            config,
            state: VadState::Silent,
            silence_duration_ms: 0,
            total_samples_processed: 0,
        }
    }

    /// Feed a speech probability from the VAD model and receive an optional event.
    ///
    /// Call this once per 512-sample window. The window duration is derived from
    /// `config.window_size_samples / 16000 * 1000` (32ms at default settings).
    pub fn update(&mut self, speech_prob: f32) -> Option<VadEvent> {
        let window_ms =
            (self.config.window_size_samples as f32 / 16000.0 * 1000.0) as u32;

        let event = match &self.state {
            VadState::Silent => {
                if speech_prob >= self.config.threshold {
                    self.state = VadState::Speaking {
                        start_sample: self.total_samples_processed,
                        speech_duration_ms: window_ms,
                    };
                    self.silence_duration_ms = 0;
                    Some(VadEvent::SpeechStart)
                } else {
                    None
                }
            }
            VadState::Speaking {
                start_sample,
                speech_duration_ms,
            } => {
                let start_sample = *start_sample;
                let speech_duration_ms = *speech_duration_ms;

                if speech_prob >= self.config.threshold {
                    // Still speaking — increment duration, reset silence counter
                    let new_duration = speech_duration_ms + window_ms;
                    self.silence_duration_ms = 0;

                    // Check for force-segment
                    if new_duration >= self.config.max_speech_ms {
                        // Force-segment: emit event, reset duration but stay Speaking
                        self.state = VadState::Speaking {
                            start_sample: self.total_samples_processed
                                + self.config.window_size_samples as usize,
                            speech_duration_ms: 0,
                        };
                        Some(VadEvent::ForceSegment {
                            duration_ms: new_duration,
                        })
                    } else {
                        self.state = VadState::Speaking {
                            start_sample,
                            speech_duration_ms: new_duration,
                        };
                        None
                    }
                } else {
                    // Silence while speaking — accumulate silence duration
                    self.silence_duration_ms += window_ms;

                    if self.silence_duration_ms >= self.config.min_silence_ms {
                        // Enough silence — transition to Silent
                        self.state = VadState::Silent;
                        self.silence_duration_ms = 0;

                        if speech_duration_ms >= self.config.min_speech_ms {
                            Some(VadEvent::SpeechEnd {
                                duration_ms: speech_duration_ms,
                            })
                        } else {
                            // Too short — discard as noise
                            None
                        }
                    } else {
                        // Brief pause — stay in Speaking, keep current duration
                        self.state = VadState::Speaking {
                            start_sample,
                            speech_duration_ms,
                        };
                        None
                    }
                }
            }
        };

        self.total_samples_processed += self.config.window_size_samples as usize;
        event
    }

    /// Returns a reference to the current state.
    pub fn state(&self) -> &VadState {
        &self.state
    }

    /// Reset the state machine to its initial Silent state with all counters zeroed.
    ///
    /// Call this when starting a new dictation session.
    pub fn reset(&mut self) {
        self.state = VadState::Silent;
        self.silence_duration_ms = 0;
        self.total_samples_processed = 0;
    }
}

/// Run the VAD processing loop on a dedicated thread.
///
/// Reads audio from the ring buffer consumer, optionally resamples to 16 kHz,
/// extracts 512-sample windows, runs Silero VAD inference, feeds results
/// through the state machine and chunker, and dispatches complete speech
/// segments via the channel. Exits when the `stop` flag is set.
///
/// This function is synchronous and should be called from `std::thread::spawn`,
/// not from an async runtime. The only async boundary is `segment_tx.try_send`.
pub fn run_vad_loop(
    consumer: &mut HeapCons<f32>,
    mut resampler: Option<&mut crate::audio::AudioResampler>,
    vad: &mut SileroVad,
    state_machine: &mut VadStateMachine,
    chunker: &mut SpeechChunker,
    segment_tx: &tokio::sync::mpsc::Sender<Vec<f32>>,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<()> {
    let window_size = 512;
    let mut accumulation_buffer: Vec<f32> = Vec::with_capacity(window_size * 4);
    let mut read_buffer = vec![0.0f32; window_size * 2];

    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        // Read all available samples from the ring buffer
        let available = consumer.occupied_len();
        if available == 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }

        let to_read = available.min(read_buffer.len());
        let read_count = consumer.pop_slice(&mut read_buffer[..to_read]);
        if read_count == 0 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            continue;
        }

        // Optionally resample to 16 kHz
        let samples = if let Some(ref mut resampler) = resampler {
            match resampler.process(&read_buffer[..read_count]) {
                Ok(resampled) => resampled,
                Err(error) => {
                    tracing::warn!("Resample error, skipping batch: {error}");
                    continue;
                }
            }
        } else {
            read_buffer[..read_count].to_vec()
        };

        accumulation_buffer.extend_from_slice(&samples);

        // Process all complete 512-sample windows
        while accumulation_buffer.len() >= window_size {
            let window: Vec<f32> =
                accumulation_buffer.drain(..window_size).collect();

            let speech_prob = match vad.process(&window) {
                Ok(prob) => prob,
                Err(error) => {
                    tracing::warn!("VAD inference error, skipping window: {error}");
                    continue;
                }
            };

            let event = state_machine.update(speech_prob);
            let segment = chunker.feed(&window, event.as_ref());

            if let Some(segment) = segment {
                // blocking_send blocks the VAD thread until channel has space,
                // guaranteeing no segment drops under backpressure (FR-017).
                // The .ok() discards the SendError that only occurs if the
                // receiver is dropped (normal shutdown).
                segment_tx.blocking_send(segment).ok();
            }
        }
    }

    // Flush any remaining audio on stop
    if let Some(segment) = chunker.flush() {
        segment_tx.blocking_send(segment).ok();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_machine_silent_to_speaking() {
        let config = VadConfig::default();
        let mut sm = VadStateMachine::new(config);

        assert_eq!(*sm.state(), VadState::Silent);

        let event = sm.update(0.8);
        assert_eq!(event, Some(VadEvent::SpeechStart));
        assert!(matches!(sm.state(), VadState::Speaking { .. }));
    }

    #[test]
    fn test_state_machine_speaking_to_silent() {
        let config = VadConfig::default();
        let mut sm = VadStateMachine::new(config);

        // Enter speaking
        sm.update(0.8);

        // Speak for 300ms (enough to pass min_speech_ms of 250ms)
        // Each window is 32ms, so ~10 windows = 320ms
        for _ in 0..9 {
            let event = sm.update(0.8);
            assert_eq!(event, None);
        }

        // Now send silence for 500ms+ (~16 windows at 32ms = 512ms)
        let mut speech_end_event = None;
        for _ in 0..16 {
            if let Some(event) = sm.update(0.1) {
                speech_end_event = Some(event);
                break;
            }
        }

        assert!(
            matches!(speech_end_event, Some(VadEvent::SpeechEnd { duration_ms }) if duration_ms >= 250)
        );
        assert_eq!(*sm.state(), VadState::Silent);
    }

    #[test]
    fn test_state_machine_force_segment() {
        let config = VadConfig {
            max_speech_ms: 30_000,
            ..VadConfig::default()
        };
        let mut sm = VadStateMachine::new(config);

        // Enter speaking
        sm.update(0.8);

        // Speak for 30s+ (938 windows at 32ms = 30,016ms)
        let mut force_event = None;
        for _ in 0..938 {
            if let Some(event) = sm.update(0.8)
                && matches!(event, VadEvent::ForceSegment { .. })
            {
                force_event = Some(event);
                break;
            }
        }

        assert!(
            matches!(force_event, Some(VadEvent::ForceSegment { duration_ms }) if duration_ms >= 30_000)
        );
        // Should still be in Speaking state after force segment
        assert!(matches!(sm.state(), VadState::Speaking { .. }));
    }

    #[test]
    fn test_state_machine_min_speech() {
        let config = VadConfig::default();
        let mut sm = VadStateMachine::new(config);

        // Enter speaking
        sm.update(0.8);

        // Speak for only 4 windows (~128ms, less than min_speech_ms of 250ms)
        for _ in 0..3 {
            sm.update(0.8);
        }

        // Now silence for 500ms+ to trigger end
        let mut got_speech_end = false;
        for _ in 0..16 {
            if let Some(event) = sm.update(0.1)
                && matches!(event, VadEvent::SpeechEnd { .. })
            {
                got_speech_end = true;
            }
        }

        // Should NOT get SpeechEnd because speech was too short
        assert!(!got_speech_end);
        assert_eq!(*sm.state(), VadState::Silent);
    }

    #[test]
    fn test_state_machine_brief_pause() {
        let config = VadConfig::default();
        let mut sm = VadStateMachine::new(config);

        // Enter speaking
        sm.update(0.8);

        // Speak for 300ms
        for _ in 0..9 {
            sm.update(0.8);
        }

        // Brief silence: 8 windows (~256ms, less than min_silence_ms of 500ms)
        for _ in 0..8 {
            let event = sm.update(0.1);
            assert!(
                !matches!(event, Some(VadEvent::SpeechEnd { .. })),
                "should not emit SpeechEnd during brief pause"
            );
        }

        // Resume speech
        let event = sm.update(0.8);
        assert!(
            !matches!(event, Some(VadEvent::SpeechStart)),
            "should not re-emit SpeechStart after brief pause"
        );
        assert!(matches!(sm.state(), VadState::Speaking { .. }));
    }

    #[test]
    fn test_vad_end_to_end() {
        use crate::audio::AudioRingBuffer;
        use ringbuf::traits::Producer;
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let fixture_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let model_path = fixture_dir.join("silero_vad_v5.onnx");
        let wav_path = fixture_dir.join("speech_sample.wav");

        // Load speech audio, truncated to 3 seconds (48000 samples at 16 kHz).
        // The full WAV is ~169 seconds — using it all causes a deadlock because
        // max_speech_ms force-segments produce more segments than the channel
        // capacity, and blocking_send stalls the loop thread while the test
        // waits on join().
        let mut reader =
            hound::WavReader::open(&wav_path).expect("Failed to open WAV");
        let speech_samples: Vec<f32> =
            if reader.spec().sample_format == hound::SampleFormat::Float {
                reader
                    .samples::<f32>()
                    .filter_map(|s| s.ok())
                    .take(48000)
                    .collect()
            } else {
                reader
                    .samples::<i16>()
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 / 32768.0)
                    .take(48000)
                    .collect()
            };

        // Build the pipeline
        let config = VadConfig::default();
        let mut vad_model =
            SileroVad::new(&model_path).expect("Failed to load VAD model");
        let mut state_machine = VadStateMachine::new(config.clone());
        let mut chunker = SpeechChunker::new(config);
        let (segment_tx, mut segment_rx) = tokio::sync::mpsc::channel(4);
        let (mut producer, mut consumer) =
            AudioRingBuffer::new(AudioRingBuffer::capacity_for_rate(16000));

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        // Run processing loop in a thread
        let loop_handle = std::thread::spawn(move || {
            run_vad_loop(
                &mut consumer,
                None,
                &mut vad_model,
                &mut state_machine,
                &mut chunker,
                &segment_tx,
                &stop_flag,
            )
        });

        // Push: 1s silence + speech + 1s silence
        let silence_1s = vec![0.0f32; 16000];
        producer.push_slice(&silence_1s);
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Push speech in chunks to simulate real-time
        for chunk in speech_samples.chunks(512) {
            producer.push_slice(chunk);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }

        producer.push_slice(&silence_1s);
        std::thread::sleep(std::time::Duration::from_millis(800));

        // Stop the loop
        stop.store(true, Ordering::Relaxed);
        loop_handle.join().expect("loop panicked").expect("loop error");

        // Check that at least one segment was emitted
        let mut segments = Vec::new();
        while let Ok(seg) = segment_rx.try_recv() {
            segments.push(seg);
        }

        assert!(
            !segments.is_empty(),
            "expected at least 1 speech segment, got 0"
        );
    }

    #[test]
    fn test_vad_multiple_utterances() {
        use crate::audio::AudioRingBuffer;
        use ringbuf::traits::Producer;
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        let fixture_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let model_path = fixture_dir.join("silero_vad_v5.onnx");
        let wav_path = fixture_dir.join("speech_sample.wav");

        // Truncate to 3 seconds per utterance to avoid channel deadlock
        // (see test_vad_end_to_end comment for details)
        let mut reader =
            hound::WavReader::open(&wav_path).expect("Failed to open WAV");
        let speech_samples: Vec<f32> =
            if reader.spec().sample_format == hound::SampleFormat::Float {
                reader
                    .samples::<f32>()
                    .filter_map(|s| s.ok())
                    .take(48000)
                    .collect()
            } else {
                reader
                    .samples::<i16>()
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 / 32768.0)
                    .take(48000)
                    .collect()
            };

        let config = VadConfig::default();
        let mut vad_model =
            SileroVad::new(&model_path).expect("Failed to load VAD model");
        let mut state_machine = VadStateMachine::new(config.clone());
        let mut chunker = SpeechChunker::new(config);
        let (segment_tx, mut segment_rx) = tokio::sync::mpsc::channel(16);
        let (mut producer, mut consumer) =
            AudioRingBuffer::new(AudioRingBuffer::capacity_for_rate(16000));

        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();

        let loop_handle = std::thread::spawn(move || {
            run_vad_loop(
                &mut consumer,
                None,
                &mut vad_model,
                &mut state_machine,
                &mut chunker,
                &segment_tx,
                &stop_flag,
            )
        });

        let silence_1s = vec![0.0f32; 16000];

        // Push 3 utterances separated by 1s silence
        for utterance_idx in 0..3 {
            producer.push_slice(&silence_1s);
            std::thread::sleep(std::time::Duration::from_millis(100));

            for chunk in speech_samples.chunks(512) {
                producer.push_slice(chunk);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }

            if utterance_idx < 2 {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        // Trailing silence to let the last utterance end
        producer.push_slice(&silence_1s);
        std::thread::sleep(std::time::Duration::from_millis(800));

        stop.store(true, Ordering::Relaxed);
        loop_handle.join().expect("loop panicked").expect("loop error");

        let mut segments = Vec::new();
        while let Ok(seg) = segment_rx.try_recv() {
            segments.push(seg);
        }

        assert!(
            segments.len() >= 3,
            "expected at least 3 speech segments for 3 utterances, got {}",
            segments.len()
        );
    }
}
