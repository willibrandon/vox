//! Pipeline orchestrator for the Vox dictation engine.
//!
//! Coordinates the full audio-to-text flow: receives speech segments from the
//! VAD thread, runs ASR and LLM via `spawn_blocking`, applies dictionary
//! substitutions, and injects polished text into the focused application.
//! State transitions are broadcast to UI subscribers via `tokio::sync::broadcast`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use anyhow::{Context, Result};
use tokio::sync::{broadcast, mpsc};

use crate::asr::AsrEngine;
use crate::dictionary::DictionaryCache;
use crate::injector;
use crate::llm::{PostProcessor, ProcessorOutput, detect_wake_word, is_likely_command};
use crate::vad::{self, SpeechChunker, VadConfig, VadStateMachine};

use super::state::{PipelineCommand, PipelineState};
use super::transcript::TranscriptEntry;
use crate::state::TranscriptWriter;

/// The pipeline orchestrator. Coordinates audio capture, VAD, ASR, LLM,
/// dictionary substitution, and text injection into a single async flow.
///
/// All components must be loaded and operational before constructing a Pipeline
/// (Constitution Principle III). The pipeline uses a command channel for control
/// instead of direct method calls during the run loop.
pub struct Pipeline {
    asr: AsrEngine,
    llm: PostProcessor,
    dictionary: DictionaryCache,
    transcript_writer: TranscriptWriter,
    state_tx: broadcast::Sender<PipelineState>,
    command_rx: mpsc::Receiver<PipelineCommand>,
    stop_flag: Arc<AtomicBool>,
    segment_rx: Option<mpsc::Receiver<Vec<f32>>>,
    vad_handle: Option<JoinHandle<Result<()>>>,
    vad_model_path: PathBuf,
    vad_config: VadConfig,
    latest_state: PipelineState,
}

impl Pipeline {
    /// Create a new pipeline with all Send+Sync components.
    ///
    /// FR-002 requires 6 components (audio, VAD, ASR, LLM, injector, dictionary).
    /// Of these, Pipeline::new() takes the 4 that are Send+Sync (ASR, LLM,
    /// dictionary, transcript_store) plus VAD config (model path + config for
    /// deferred creation on the VAD thread, since SileroVad is NOT Send).
    /// The remaining 2 non-Send components are provided later:
    /// - AudioCapture: NOT Send — stays on caller's thread. Its ring buffer
    ///   consumer (HeapCons<f32>, which IS Send) is passed to start().
    /// - TextInjector: NOT Send — injection calls are made from the async
    ///   orchestrator task on the main thread (not from spawn_blocking).
    ///
    /// `command_rx` receives control commands (Stop) from the PipelineController.
    #[allow(clippy::too_many_arguments)] // orchestrator needs all pipeline components wired in
    pub fn new(
        asr: AsrEngine,
        llm: PostProcessor,
        dictionary: DictionaryCache,
        transcript_writer: TranscriptWriter,
        state_tx: broadcast::Sender<PipelineState>,
        command_rx: mpsc::Receiver<PipelineCommand>,
        vad_model_path: PathBuf,
        vad_config: VadConfig,
    ) -> Self {
        Self {
            asr,
            llm,
            dictionary,
            transcript_writer,
            state_tx,
            command_rx,
            stop_flag: Arc::new(AtomicBool::new(false)),
            segment_rx: None,
            vad_handle: None,
            vad_model_path,
            vad_config,
            latest_state: PipelineState::Idle,
        }
    }

    /// Start the pipeline: spawn VAD thread using the provided audio consumer.
    ///
    /// The caller is responsible for creating and starting AudioCapture, then
    /// passing the owned ring buffer consumer here. AudioCapture itself stays
    /// on the caller's thread (it is NOT Send).
    ///
    /// On success, broadcasts PipelineState::Listening. Between start() returning
    /// and run() beginning its select loop, segments are buffered in the channel.
    pub fn start(
        &mut self,
        mut consumer: ringbuf::HeapCons<f32>,
        native_sample_rate: u32,
    ) -> Result<()> {
        self.stop_flag.store(false, Ordering::Release);

        let (segment_tx, segment_rx) = mpsc::channel::<Vec<f32>>(32);
        self.segment_rx = Some(segment_rx);

        let stop_flag = self.stop_flag.clone();
        let vad_model_path = self.vad_model_path.clone();
        let vad_config = self.vad_config.clone();

        let vad_handle = std::thread::spawn(move || -> Result<()> {
            let mut vad_model = vad::SileroVad::new(&vad_model_path)?;
            let mut resampler_opt =
                crate::audio::AudioResampler::new(native_sample_rate, 16000);
            let mut state_machine = VadStateMachine::new(vad_config.clone());
            let mut chunker = SpeechChunker::new(vad_config.clone());

            vad::run_vad_loop(
                &mut consumer,
                resampler_opt.as_mut(),
                &mut vad_model,
                &mut state_machine,
                &mut chunker,
                &segment_tx,
                &stop_flag,
                &vad_config,
            )
        });

        self.vad_handle = Some(vad_handle);
        self.broadcast(PipelineState::Listening);

        Ok(())
    }

    /// Main processing loop. Receives speech segments from the VAD thread
    /// and processes each through ASR → Dictionary → LLM → Injection.
    ///
    /// Uses `tokio::select!` to concurrently listen for speech segments and
    /// control commands. Segments are processed in strict FIFO order. A Stop
    /// command causes the loop to finish the current segment, then exit.
    pub async fn run(&mut self) -> Result<()> {
        let mut segment_rx = self
            .segment_rx
            .take()
            .context("pipeline not started — call start() before run()")?;

        loop {
            tokio::select! {
                segment = segment_rx.recv() => {
                    match segment {
                        Some(audio_segment) => {
                            match self.process_segment(audio_segment).await {
                                Ok(()) => {}
                                Err(error) => {
                                    self.broadcast(PipelineState::Error {
                                        message: error.to_string(),
                                    });
                                    self.broadcast(PipelineState::Listening);
                                }
                            }
                        }
                        None => {
                            // VAD thread exited — drain any remaining buffered segments
                            // This shouldn't happen during normal operation (only on unexpected exit)
                            break;
                        }
                    }
                }
                cmd = self.command_rx.recv() => {
                    match cmd {
                        Some(PipelineCommand::Stop) => {
                            break;
                        }
                        None => {
                            // Command channel closed — controller dropped
                            break;
                        }
                    }
                }
            }
        }

        // Shutdown sequence (R-002):
        // 1. Set stop flag — signals VAD thread to exit its while loop
        self.stop_flag.store(true, Ordering::Release);

        // 2. Drain buffered segments to free channel capacity. This unblocks
        //    any in-flight blocking_send inside the VAD while loop, allowing
        //    the thread to see the stop flag on the next iteration.
        while let Ok(segment) = segment_rx.try_recv() {
            if let Err(error) = self.process_segment(segment).await {
                self.broadcast(PipelineState::Error {
                    message: error.to_string(),
                });
            }
        }

        // 3. Join VAD thread. After the while loop exits, the thread calls
        //    chunker.flush() → blocking_send for any trailing speech. The
        //    receiver is still alive so the flush succeeds.
        if let Some(handle) = self.vad_handle.take() {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    self.broadcast(PipelineState::Error {
                        message: format!("VAD processing thread exited with error: {error}"),
                    });
                }
                Err(_) => {
                    self.broadcast(PipelineState::Error {
                        message: "VAD processing thread panicked".to_string(),
                    });
                }
            }
        }

        // 4. Drain again to pick up the flushed segment and any segments
        //    produced between step 2 and the thread exiting.
        while let Ok(segment) = segment_rx.try_recv() {
            if let Err(error) = self.process_segment(segment).await {
                self.broadcast(PipelineState::Error {
                    message: error.to_string(),
                });
            }
        }

        self.broadcast(PipelineState::Idle);
        Ok(())
    }

    /// Subscribe to pipeline state changes.
    ///
    /// Returns a broadcast receiver. Multiple subscribers are supported.
    /// If a subscriber falls behind, it receives `RecvError::Lagged(n)` and
    /// the next successful recv gets the most recent state (latest-wins
    /// semantics, no crash or deadlock).
    pub fn subscribe(&self) -> broadcast::Receiver<PipelineState> {
        self.state_tx.subscribe()
    }

    /// Current pipeline state (latest broadcast value).
    pub fn state(&self) -> PipelineState {
        self.latest_state.clone()
    }

    /// Process a single speech segment through the full pipeline.
    ///
    /// Flow: silent pre-check → silence-pad → ASR → dictionary substitution →
    /// focused app detection → LLM post-processing → injection/command →
    /// transcript save.
    async fn process_segment(&mut self, segment: Vec<f32>) -> Result<()> {
        let start_time = Instant::now();
        let segment_len = segment.len() as u32;

        // Silent pre-check (FR-012)
        if is_silent(&segment) {
            self.broadcast(PipelineState::Listening);
            return Ok(());
        }

        self.broadcast(PipelineState::Processing { raw_text: None });

        let segment_duration_ms = segment_len * 1000 / 16000;

        // Prepend 200ms of silence before the speech segment. This gives
        // Whisper's attention mechanism a "settle" window before the first
        // phoneme arrives, improving recognition of word-initial sounds
        // (especially soft onsets like nasals /m/, /n/).
        let silence_pad_samples = (200 * 16000) / 1000; // 3200 samples
        let mut padded_segment = Vec::with_capacity(silence_pad_samples + segment.len());
        padded_segment.resize(silence_pad_samples, 0.0f32);
        padded_segment.extend_from_slice(&segment);

        // ASR (GPU-bound) — run in spawn_blocking
        let asr_start = Instant::now();
        let raw_text = tokio::task::spawn_blocking({
            let asr = self.asr.clone();
            move || asr.transcribe(&padded_segment)
        })
        .await
        .context("ASR task panicked")??;
        let asr_ms = asr_start.elapsed().as_millis();
        tracing::info!(asr_ms, segment_duration_ms, raw = %raw_text, "ASR completed");

        if raw_text.is_empty() {
            self.broadcast(PipelineState::Listening);
            return Ok(());
        }

        self.broadcast(PipelineState::Processing {
            raw_text: Some(raw_text.clone()),
        });

        // Dictionary substitution (fast, in-process)
        let dict_start = Instant::now();
        let sub_result = self.dictionary.apply_substitutions(&raw_text);
        let dict_ms = dict_start.elapsed().as_millis();
        if sub_result.text.is_empty() {
            self.broadcast(PipelineState::Listening);
            return Ok(());
        }

        // Increment use counts for matched dictionary entries
        if !sub_result.matched_ids.is_empty() {
            if let Err(e) = self.dictionary.increment_use_counts(&sub_result.matched_ids) {
                tracing::warn!("failed to increment dictionary use counts: {e}");
            }
        }

        // Get focused application name for tone adaptation
        let active_app = injector::get_focused_app_name();

        // Fast-path: if the transcript is already clean (properly capitalized,
        // punctuated, no fillers, no corrections, no commands), bypass the LLM
        // entirely. Saves ~3 seconds of GPU inference for simple dictation.
        let (polished, llm_ms) = if transcript_is_clean(&sub_result.text) {
            tracing::info!(
                text = %sub_result.text,
                "LLM fast-path: transcript is clean, skipping LLM"
            );
            (sub_result.text.clone(), 0u128)
        } else {
            // LLM post-processing (GPU-bound) — run in spawn_blocking
            let llm_start = Instant::now();
            let hints = self.dictionary.top_hints(50);
            let result: ProcessorOutput = tokio::task::spawn_blocking({
                let llm = self.llm.clone();
                let text = sub_result.text.clone();
                let app = active_app.clone();
                move || llm.process(&text, &hints, &app)
            })
            .await
            .context("LLM task panicked")??;
            let llm_elapsed = llm_start.elapsed().as_millis();
            tracing::info!(llm_ms = llm_elapsed, "LLM post-processing completed");

            match result {
                ProcessorOutput::Text(polished) => (polished, llm_elapsed),
                ProcessorOutput::Command(cmd) => {
                    // Voice commands are executed, not saved as transcripts (FR-016)
                    if let Err(error) = injector::execute_command(&cmd) {
                        tracing::warn!("failed to execute voice command: {error}");
                    }
                    self.broadcast(PipelineState::Listening);
                    return Ok(());
                }
            }
        };

        self.broadcast(PipelineState::Injecting {
            polished_text: polished.clone(),
        });

        let inject_start = Instant::now();
        let inject_result = injector::inject_text(&polished);
        let inject_ms = inject_start.elapsed().as_millis();

        let latency_ms = start_time.elapsed().as_millis() as u32;
        tracing::info!(
            asr_ms,
            dict_ms,
            llm_ms,
            inject_ms,
            latency_ms,
            polished = %polished,
            "segment processed"
        );

        if let injector::InjectionResult::Blocked { reason, text: failed_text } = inject_result {
            tracing::error!(?reason, "text injection failed");
            self.broadcast(PipelineState::InjectionFailed {
                polished_text: failed_text,
                error: format!("{reason:?}"),
            });
            return Ok(());
        }

        // Save transcript (FR-014)
        let entry = TranscriptEntry {
            id: uuid::Uuid::new_v4().to_string(),
            raw_text,
            polished_text: polished,
            target_app: active_app,
            duration_ms: segment_duration_ms,
            latency_ms,
            created_at: chrono_now_iso8601(),
        };
        if let Err(error) = self.transcript_writer.save(&entry) {
            tracing::warn!("failed to save transcript: {error}");
        }

        self.broadcast(PipelineState::Listening);
        Ok(())
    }

    /// Broadcast a state change to all subscribers and update the internal
    /// latest-state tracking.
    ///
    /// If no subscribers are listening (Err from send), this is not an error —
    /// it just means the UI hasn't subscribed yet.
    fn broadcast(&mut self, state: PipelineState) {
        self.latest_state = state.clone();
        // Err means zero receivers — not an error, just no UI subscribed yet
        let _ = self.state_tx.send(state);
    }
}

/// Check if a speech segment is silent (RMS energy below threshold).
///
/// Returns true if the RMS energy is below 1e-3 (0.001). Silent segments are
/// skipped to avoid feeding Whisper empty audio, which causes hallucinations.
fn is_silent(segment: &[f32]) -> bool {
    if segment.is_empty() {
        return true;
    }
    let sum_squares: f32 = segment.iter().map(|s| s * s).sum();
    let rms = (sum_squares / segment.len() as f32).sqrt();
    rms < 1e-3
}

/// Unambiguous filler words — always fillers in dictated speech, never
/// legitimate content words. Excludes "like" and "so" which have valid
/// non-filler uses ("I like pizza", "I think so").
const FILLER_WORDS: &[&str] = &["um", "uh", "er", "ah", "hmm", "hm"];

/// Multi-word filler phrases checked as substrings.
const FILLER_PHRASES: &[&str] = &["you know", "i mean"];

/// Course correction phrases that signal the speaker restarted mid-sentence.
const CORRECTION_PHRASES: &[&str] = &[
    "no wait",
    "no actually",
    "i meant",
    "or rather",
    "well actually",
    "i said",
];

/// Check whether a transcript is already clean and can bypass LLM processing.
///
/// Returns `true` only when ALL of these conditions hold:
/// 1. Not a voice command (would need JSON conversion)
/// 2. No wake word prefix (would need command-emphasis routing)
/// 3. First character is uppercase (proper capitalization)
/// 4. Ends with sentence-ending punctuation (. ! ?)
/// 5. Contains no unambiguous filler words (um, uh, er, ah, hmm)
/// 6. Contains no filler phrases (you know, I mean)
/// 7. Contains no course correction phrases (no wait, I meant, etc.)
/// 8. Contains no stuttered/repeated consecutive words
/// 9. Contains no self-correction dashes
/// 10. Contains no spelled-out email/URL patterns (at ... dot)
///
/// Intentionally conservative — when in doubt, sends to LLM. A false
/// negative (clean text goes to LLM) costs 3s latency. A false positive
/// (dirty text bypasses LLM) produces incorrect output.
fn transcript_is_clean(text: &str) -> bool {
    let trimmed = text.trim();

    // Fragments too short to evaluate — let LLM decide
    if trimmed.len() < 2 {
        return false;
    }

    // Strip trailing sentence punctuation for command/wake-word checks,
    // since Whisper adds periods to commands ("Delete that." → "delete that")
    let text_no_trailing_punct = trimmed.trim_end_matches(['.', '!', '?', ',']);

    // Voice commands must go through LLM for JSON conversion
    if is_likely_command(text_no_trailing_punct) {
        return false;
    }

    // Wake word prefix needs command-emphasis routing
    if detect_wake_word(trimmed).is_some() {
        return false;
    }

    // Must start with an uppercase letter (proper capitalization)
    match trimmed.chars().next() {
        Some(c) if c.is_uppercase() => {}
        _ => return false,
    }

    // Must end with sentence-ending punctuation
    if !trimmed.ends_with('.') && !trimmed.ends_with('!') && !trimmed.ends_with('?') {
        return false;
    }

    let lower = trimmed.to_lowercase();

    // Strip punctuation from each word for filler/stutter matching.
    // Whisper attaches commas/periods to words: "Um, I went" → ["um,", "i", "went"]
    let words: Vec<String> = lower
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation()).to_string())
        .collect();
    let word_refs: Vec<&str> = words.iter().map(|w| w.as_str()).collect();

    // Check for unambiguous single-word fillers
    for filler in FILLER_WORDS {
        if word_refs.contains(filler) {
            return false;
        }
    }

    // Check for multi-word filler phrases (check against punctuation-stripped text)
    let stripped_lower: String = words.join(" ");
    for phrase in FILLER_PHRASES {
        if stripped_lower.contains(phrase) {
            return false;
        }
    }

    // Check for course correction phrases
    for phrase in CORRECTION_PHRASES {
        if stripped_lower.contains(phrase) {
            return false;
        }
    }

    // Check for stuttered/repeated consecutive words ("I I went", "the the store")
    for window in word_refs.windows(2) {
        if window[0] == window[1] {
            return false;
        }
    }

    // Check for self-correction dashes ("I went to the- I walked")
    if trimmed.contains(" - ") || trimmed.contains("- ") || trimmed.ends_with('-') {
        return false;
    }

    // Check for spelled-out email/URL patterns ("john at outlook dot com")
    if stripped_lower.contains(" at ") && stripped_lower.contains(" dot ") {
        return false;
    }

    true
}

/// Generate an ISO 8601 timestamp for the current time in UTC.
fn chrono_now_iso8601() -> String {
    // Using manual formatting since rusqlite 0.38 has no FromSql for chrono.
    // This produces "2026-02-20T14:30:00Z" format.
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();

    // Calculate date/time components from epoch seconds
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to year/month/day (simplified — works for 2000-2099)
    let mut remaining_days = days as i64;
    let mut year = 1970i32;
    loop {
        let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
            366
        } else {
            365
        };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u32;
    for &md in &month_days {
        if remaining_days < md {
            break;
        }
        remaining_days -= md;
        month += 1;
    }
    let day = remaining_days + 1;

    format!(
        "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::transcript::TranscriptStore;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
    }

    fn load_speech_samples() -> Vec<f32> {
        let wav_path = fixtures_dir().join("speech_sample.wav");
        let reader = hound::WavReader::open(wav_path).expect("failed to open speech_sample.wav");
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16000, "expected 16 kHz WAV");
        assert_eq!(spec.channels, 1, "expected mono WAV");
        reader
            .into_samples::<i16>()
            .map(|s| s.expect("failed to read sample") as f32 / 32768.0)
            .collect()
    }

    /// Returns (Pipeline, state_rx, _tempdir). Caller MUST hold the TempDir
    /// or SQLite writes will silently fail when the directory is deleted.
    fn make_pipeline() -> (Pipeline, broadcast::Receiver<PipelineState>, tempfile::TempDir) {
        let asr_path = fixtures_dir().join("ggml-large-v3-turbo-q5_0.bin");
        let asr = crate::asr::AsrEngine::new(&asr_path, true).expect("failed to load ASR model");

        let llm_path = fixtures_dir().join("qwen2.5-3b-instruct-q4_k_m.gguf");
        let llm =
            crate::llm::PostProcessor::new(&llm_path, true).expect("failed to load LLM model");

        let dictionary = DictionaryCache::empty();

        let dir = tempfile::tempdir().expect("temp dir");
        let transcript_store = Arc::new(
            TranscriptStore::open(&dir.path().join("transcripts.db")).expect("open store"),
        );
        let save_history = Arc::new(AtomicBool::new(true));
        let transcript_writer = TranscriptWriter::new(
            Arc::clone(&transcript_store),
            Arc::clone(&save_history),
        );

        let (state_tx, state_rx) = broadcast::channel::<PipelineState>(64);
        let (_command_tx, command_rx) = mpsc::channel::<PipelineCommand>(8);

        let vad_model_path = fixtures_dir().join("silero_vad_v5.onnx");
        let vad_config = crate::vad::VadConfig::default();

        let pipeline = Pipeline::new(
            asr,
            llm,
            dictionary,
            transcript_writer,
            state_tx,
            command_rx,
            vad_model_path,
            vad_config,
        );

        (pipeline, state_rx, dir)
    }

    #[test]
    fn test_is_silent_zeros() {
        let silence = vec![0.0f32; 16000];
        assert!(is_silent(&silence));
    }

    #[test]
    fn test_is_silent_empty() {
        assert!(is_silent(&[]));
    }

    #[test]
    fn test_is_silent_loud() {
        let loud: Vec<f32> = (0..16000).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();
        assert!(!is_silent(&loud));
    }

    #[test]
    fn test_is_silent_threshold_boundary() {
        // RMS of 1e-3 exactly should be treated as silent
        // For N samples all equal to v, RMS = |v|
        // So v = 0.001 gives RMS = 0.001, which is NOT < 1e-3 (it equals it)
        let borderline = vec![0.001f32; 1000];
        assert!(!is_silent(&borderline)); // Equal to threshold, not below

        // Just below threshold
        let quiet = vec![0.0009f32; 1000];
        assert!(is_silent(&quiet));
    }

    // --- transcript_is_clean tests ---

    #[test]
    fn test_clean_simple_sentence() {
        assert!(transcript_is_clean("My name is Batman."));
        assert!(transcript_is_clean("Hello world!"));
        assert!(transcript_is_clean("Is this working?"));
        assert!(transcript_is_clean("The quick brown fox jumps over the lazy dog."));
    }

    #[test]
    fn test_clean_rejects_fillers() {
        assert!(!transcript_is_clean("Um, I went to the store."));
        assert!(!transcript_is_clean("I uh went to the store."));
        assert!(!transcript_is_clean("So I went er to the store."));
        assert!(!transcript_is_clean("I went to the store, ah yes."));
        assert!(!transcript_is_clean("Hmm that sounds good."));
    }

    #[test]
    fn test_clean_rejects_filler_phrases() {
        assert!(!transcript_is_clean("I went, you know, to the store."));
        assert!(!transcript_is_clean("I mean it was a good day."));
    }

    #[test]
    fn test_clean_rejects_corrections() {
        assert!(!transcript_is_clean("I went to the no wait I drove."));
        assert!(!transcript_is_clean("Send it to John, or rather Jane."));
        assert!(!transcript_is_clean("I meant the other one."));
        assert!(!transcript_is_clean("I said it was good."));
    }

    #[test]
    fn test_clean_rejects_missing_punctuation() {
        assert!(!transcript_is_clean("My name is Batman"));
        assert!(!transcript_is_clean("Hello world"));
    }

    #[test]
    fn test_clean_rejects_missing_capitalization() {
        assert!(!transcript_is_clean("my name is Batman."));
        assert!(!transcript_is_clean("hello world!"));
    }

    #[test]
    fn test_clean_rejects_stutters() {
        assert!(!transcript_is_clean("I I went to the store."));
        assert!(!transcript_is_clean("The the dog is here."));
    }

    #[test]
    fn test_clean_rejects_dashes() {
        assert!(!transcript_is_clean("I went to the- I walked to the store."));
        assert!(!transcript_is_clean("Send it - no wait."));
    }

    #[test]
    fn test_clean_rejects_commands() {
        assert!(!transcript_is_clean("Delete that."));
        assert!(!transcript_is_clean("Undo that."));
        assert!(!transcript_is_clean("New line."));
        assert!(!transcript_is_clean("Select all."));
    }

    #[test]
    fn test_clean_rejects_wake_word() {
        assert!(!transcript_is_clean("Hey Vox delete that."));
        assert!(!transcript_is_clean("Hey vox, undo."));
    }

    #[test]
    fn test_clean_rejects_email_patterns() {
        assert!(!transcript_is_clean("Send it to john at outlook dot com."));
    }

    #[test]
    fn test_clean_rejects_too_short() {
        assert!(!transcript_is_clean(""));
        assert!(!transcript_is_clean("A"));
        assert!(!transcript_is_clean(" "));
    }

    #[test]
    fn test_chrono_now_iso8601_format() {
        let ts = chrono_now_iso8601();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
        assert_eq!(ts.len(), 20, "timestamp should be 20 chars: {ts}");
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    // --- Integration tests (require model fixtures in tests/fixtures/) ---

    #[tokio::test]
    async fn test_full_pipeline_hello_world() {
        let (mut pipeline, mut state_rx, _tmpdir) = make_pipeline();
        let all_samples = load_speech_samples();

        // Take a realistic VAD-sized segment (~3 seconds = 48000 samples at 16kHz).
        // The full fixture is 169s of continuous speech; VAD would segment it into
        // 1-5 second utterances in practice.
        let samples: Vec<f32> = all_samples.into_iter().take(48000).collect();

        pipeline.process_segment(samples).await.expect("process_segment failed");

        // Verify transcript was saved
        let count = pipeline.transcript_writer.count().expect("count");
        assert!(count >= 1, "expected at least 1 transcript entry, got {count}");

        let entries = pipeline.transcript_writer.list(10, 0).expect("list");
        let entry = &entries[0];
        assert!(
            !entry.polished_text.is_empty(),
            "polished text should not be empty"
        );
        assert!(
            !entry.raw_text.is_empty(),
            "raw text should not be empty"
        );
        assert!(entry.duration_ms > 0, "duration should be positive");
        assert!(entry.latency_ms > 0, "latency should be positive");

        // Verify state transitions were broadcast
        let mut states = Vec::new();
        while let Ok(state) = state_rx.try_recv() {
            states.push(state);
        }
        assert!(
            states.iter().any(|s| matches!(s, PipelineState::Processing { .. })),
            "expected Processing state in broadcast"
        );
    }

    #[tokio::test]
    async fn test_pipeline_empty_audio() {
        let (mut pipeline, _state_rx, _tmpdir) = make_pipeline();

        // All-zero samples should be caught by is_silent and produce no transcript
        let silence = vec![0.0f32; 16000];
        pipeline.process_segment(silence).await.expect("process_segment failed");

        let count = pipeline.transcript_writer.count().expect("count");
        assert_eq!(count, 0, "silent audio should produce no transcript entry");
    }

    #[tokio::test]
    async fn test_pipeline_multiple_segments() {
        let (mut pipeline, _state_rx, _tmpdir) = make_pipeline();
        let all_samples = load_speech_samples();
        // Truncate to a realistic VAD-sized segment (~3 seconds)
        let samples: Vec<f32> = all_samples.into_iter().take(48000).collect();

        // Process the same speech segment 3 times
        for _ in 0..3 {
            pipeline
                .process_segment(samples.clone())
                .await
                .expect("process_segment failed");
        }

        let count = pipeline.transcript_writer.count().expect("count");
        assert_eq!(
            count, 3,
            "3 speech segments should produce 3 transcript entries, got {count}"
        );

        // Verify FIFO ordering (newest first in list)
        let entries = pipeline.transcript_writer.list(10, 0).expect("list");
        assert_eq!(entries.len(), 3);
        // Each entry should have unique IDs
        let ids: std::collections::HashSet<&str> =
            entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids.len(), 3, "all transcript entries should have unique IDs");
    }
}
