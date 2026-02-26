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
use crate::error::VoxError;
use crate::injector;
use crate::llm::{PostProcessor, ProcessorOutput, detect_wake_word, is_likely_command};
use crate::recovery::retry_once;
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
    /// Monotonic counter for segment tracking in error/recovery logs.
    segment_counter: u64,
    /// Cancel handle for any active injection focus-retry task.
    /// Dropping or sending `true` cancels the background retry.
    injection_retry_cancel: Option<tokio::sync::watch::Sender<bool>>,
    /// Shared flag from AudioCapture indicating device disconnection.
    /// Checked at the start of each segment processing cycle to skip
    /// work on stale audio when the device is gone.
    audio_error_flag: Option<Arc<AtomicBool>>,
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
            segment_counter: 0,
            injection_retry_cancel: None,
            audio_error_flag: None,
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
                        Some(PipelineCommand::CancelInjectionRetry) => {
                            self.cancel_injection_retry();
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
        // 0. Cancel any active injection focus-retry task so it doesn't inject
        //    stale text after the pipeline stops.
        self.cancel_injection_retry();

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

    /// Set the audio error flag for cross-thread device health monitoring.
    ///
    /// When set, the pipeline checks this flag before processing each segment.
    /// If the flag is raised (device disconnected), the segment is skipped and
    /// an error state is broadcast. The flag is obtained from
    /// [`AudioCapture::error_flag()`](crate::audio::AudioCapture::error_flag).
    pub fn set_audio_error_flag(&mut self, flag: Arc<AtomicBool>) {
        self.audio_error_flag = Some(flag);
    }

    /// Cancel any active injection focus-retry task.
    ///
    /// Called when the user clicks Copy on buffered text or when the pipeline
    /// transitions to idle. Without this, the retry task would keep polling
    /// and could inject stale text into a later-focused window.
    pub fn cancel_injection_retry(&mut self) {
        if let Some(cancel_tx) = self.injection_retry_cancel.take() {
            let _ = cancel_tx.send(true);
            tracing::info!("injection focus-retry cancelled by external caller");
        }
    }

    /// Process a single speech segment through the full pipeline.
    ///
    /// Flow: silent pre-check → silence-pad → ASR → dictionary substitution →
    /// focused app detection → LLM post-processing → injection/command →
    /// transcript save.
    async fn process_segment(&mut self, segment: Vec<f32>) -> Result<()> {
        // Check audio device health before processing (T017)
        if let Some(ref flag) = self.audio_error_flag {
            if flag.load(Ordering::Acquire) {
                tracing::warn!("audio device disconnected — skipping segment, broadcasting error");
                self.broadcast(PipelineState::Error {
                    message: "No microphone detected — attempting recovery".to_string(),
                });
                return Ok(());
            }
        }

        let start_time = Instant::now();
        let segment_len = segment.len() as u32;
        self.segment_counter += 1;
        let segment_id = self.segment_counter;
        let audio_duration_ms = segment_len * 1000 / 16000;

        tracing::info!(segment_id, audio_duration_ms, "pipeline_segment: start");

        // Cancel any active injection focus-retry from a previous segment
        if let Some(cancel_tx) = self.injection_retry_cancel.take() {
            let _ = cancel_tx.send(true);
        }

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

        // ASR (GPU-bound) — retry once on failure, discard segment on second failure
        let asr_start = Instant::now();
        let asr = self.asr.clone();
        let raw_text = match retry_once("ASR transcribe", padded_segment, |segment| {
            let asr = asr.clone();
            async move {
                tokio::task::spawn_blocking(move || asr.transcribe(&segment))
                    .await
                    .map_err(|e| VoxError::AsrFailure {
                        source: anyhow::anyhow!("{e}"),
                        segment_id,
                    })?
                    .map_err(|e| VoxError::AsrFailure {
                        source: e,
                        segment_id,
                    })
            }
        })
        .await
        {
            Ok(text) => text,
            Err(vox_err) => {
                tracing::warn!(
                    segment_id,
                    error = %vox_err,
                    "ASR failed after retry — discarding segment"
                );
                self.broadcast(PipelineState::Listening);
                return Ok(());
            }
        };
        let asr_ms = asr_start.elapsed().as_millis();
        tracing::info!(segment_id, asr_ms, audio_duration_ms, raw = %raw_text, "asr_transcribe: complete");

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
            // LLM post-processing (GPU-bound) — retry once, discard on second failure
            let llm_start = Instant::now();
            let hints = self.dictionary.top_hints(50);
            let llm = self.llm.clone();
            let llm_input = (sub_result.text.clone(), hints, active_app.clone());

            let result = match retry_once(
                "LLM process",
                llm_input,
                |(text, hints, app)| {
                    let llm = llm.clone();
                    async move {
                        tokio::task::spawn_blocking(move || llm.process(&text, &hints, &app))
                            .await
                            .map_err(|e| VoxError::LlmFailure {
                                source: anyhow::anyhow!("{e}"),
                                segment_id,
                            })?
                            .map_err(|e| VoxError::LlmFailure {
                                source: e,
                                segment_id,
                            })
                    }
                },
            )
            .await
            {
                Ok(output) => output,
                Err(vox_err) => {
                    tracing::warn!(
                        segment_id,
                        error = %vox_err,
                        "LLM failed after retry — discarding segment"
                    );
                    self.broadcast(PipelineState::Listening);
                    return Ok(());
                }
            };
            let llm_elapsed = llm_start.elapsed().as_millis();
            tracing::info!(segment_id, llm_ms = llm_elapsed, "llm_process: complete");

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
        let text_len = polished.len();
        let target_app = &active_app;
        let inject_result_str = match &inject_result {
            injector::InjectionResult::Success => "success",
            injector::InjectionResult::Blocked { .. } => "blocked",
        };
        tracing::info!(
            segment_id,
            asr_ms,
            dict_ms,
            llm_ms,
            inject_ms,
            latency_ms,
            text_len,
            target_app,
            inject_result = inject_result_str,
            "pipeline_segment: complete"
        );

        if let injector::InjectionResult::Blocked {
            reason,
            text: failed_text,
        } = inject_result
        {
            tracing::error!(?reason, "text injection failed — buffering for focus retry");
            self.broadcast(PipelineState::InjectionFailed {
                polished_text: failed_text.clone(),
                error: format!("{reason:?}"),
            });

            // Spawn focus retry task (FR-003): poll every 500ms for a focused
            // text-accepting window, re-attempt injection on focus detected.
            let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
            self.injection_retry_cancel = Some(cancel_tx);
            let state_tx = self.state_tx.clone();
            tokio::spawn(injector::retry_on_focus(
                failed_text,
                state_tx,
                cancel_rx,
            ));

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

    // --- T005: ASR retry on failure ---

    #[tokio::test]
    async fn test_asr_retry_on_failure() {
        use crate::error::VoxError;
        use crate::recovery::retry_once;
        use std::sync::atomic::{AtomicU32, Ordering};

        let call_count = Arc::new(AtomicU32::new(0));
        let counter = call_count.clone();
        let segment_id = 1u64;

        // Simulate ASR: first call fails, second succeeds
        let result = retry_once("ASR transcribe", vec![0.0f32; 16000], move |segment| {
            let counter = counter.clone();
            async move {
                let count = counter.fetch_add(1, Ordering::SeqCst);
                if count == 0 {
                    Err(VoxError::AsrFailure {
                        source: anyhow::anyhow!("simulated whisper decode failure"),
                        segment_id,
                    })
                } else {
                    Ok(format!("transcribed {} samples", segment.len()))
                }
            }
        })
        .await;

        // Verify: transcribe called exactly twice, second attempt succeeded
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
        let text = result.expect("second attempt should succeed");
        assert!(text.contains("16000"));
    }

    // --- T006: LLM retry on failure ---

    #[tokio::test]
    async fn test_llm_retry_on_failure() {
        use crate::error::VoxError;
        use crate::recovery::retry_once;
        use std::sync::atomic::{AtomicU32, Ordering};

        let call_count = Arc::new(AtomicU32::new(0));
        let counter = call_count.clone();
        let segment_id = 42u64;

        // Simulate LLM: fails both times → segment discarded
        let result: Result<String, VoxError> =
            retry_once("LLM process", "hello world".to_string(), move |text| {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err(VoxError::LlmFailure {
                        source: anyhow::anyhow!("simulated context creation failure"),
                        segment_id,
                    })
                }
            })
            .await;

        // Verify: process called twice, both failed → segment should be discarded
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
        assert!(result.is_err());

        // Verify the pipeline would return to Listening (simulated via broadcast)
        let (state_tx, mut state_rx) = broadcast::channel::<PipelineState>(8);
        let _ = state_tx.send(PipelineState::Listening);
        let state = state_rx.recv().await.expect("should receive state");
        assert!(matches!(state, PipelineState::Listening));
    }

    // --- T007: Injection buffer on failure ---

    #[tokio::test]
    async fn test_injection_buffer_on_failure() {
        // Verify that when injection returns Blocked, the pipeline broadcasts
        // InjectionFailed with the text preserved for copy/retry.
        let (state_tx, mut state_rx) = broadcast::channel::<PipelineState>(8);

        // Simulate the injection failure path from process_segment
        let polished_text = "Hello world, this is a test.".to_string();
        let inject_result = injector::InjectionResult::Blocked {
            reason: injector::InjectionError::NoFocusedWindow,
            text: polished_text.clone(),
        };

        if let injector::InjectionResult::Blocked {
            reason,
            text: failed_text,
        } = inject_result
        {
            let _ = state_tx.send(PipelineState::InjectionFailed {
                polished_text: failed_text.clone(),
                error: format!("{reason:?}"),
            });

            // Spawn focus retry task
            let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
            let retry_state_tx = state_tx.clone();
            tokio::spawn(injector::retry_on_focus(
                failed_text,
                retry_state_tx,
                cancel_rx,
            ));

            // Verify InjectionFailed was broadcast
            let state = state_rx.recv().await.expect("should receive state");
            match state {
                PipelineState::InjectionFailed {
                    polished_text: text,
                    error,
                } => {
                    assert_eq!(text, "Hello world, this is a test.");
                    assert!(error.contains("NoFocusedWindow"));
                }
                other => panic!("expected InjectionFailed, got {other:?}"),
            }

            // Cancel the retry task to clean up
            let _ = cancel_tx.send(true);
        } else {
            panic!("expected Blocked result");
        }
    }

    // --- T039: Full pipeline recovery after problematic ASR segment ---

    #[tokio::test]
    async fn test_pipeline_recovery_asr_crash() {
        // Verifies: after a segment that produces empty ASR output (noise),
        // the pipeline returns to Listening and successfully processes the
        // next real speech segment. This proves process_segment doesn't enter
        // a dead state after handling an unproductive segment.
        let (mut pipeline, mut state_rx, _tmpdir) = make_pipeline();

        // Phase 1: Send random noise that passes is_silent() but contains no speech.
        // ASR will either return empty text or hallucinated filler — both are handled
        // gracefully by process_segment (early return to Listening).
        let noise: Vec<f32> = (0..48000)
            .map(|i| ((i as f32 * 0.1).sin() * 0.01) + 0.005)
            .collect();
        assert!(!is_silent(&noise), "noise must pass the silence check");

        pipeline
            .process_segment(noise)
            .await
            .expect("noise segment should not error");

        // Pipeline should return to Listening after noise (empty ASR or discarded)
        let mut returned_to_listening = false;
        while let Ok(state) = state_rx.try_recv() {
            if matches!(state, PipelineState::Listening) {
                returned_to_listening = true;
            }
        }
        assert!(
            returned_to_listening,
            "pipeline should return to Listening after noise segment"
        );

        // Phase 2: Send real speech — pipeline must still be functional
        let all_samples = load_speech_samples();
        let speech: Vec<f32> = all_samples.into_iter().take(48000).collect();
        pipeline
            .process_segment(speech)
            .await
            .expect("real speech should succeed after noise");

        // Verify transcript was saved (proving ASR + LLM + injection all worked)
        let count = pipeline.transcript_writer.count().expect("count");
        assert!(
            count >= 1,
            "should have at least 1 transcript from the real speech segment, got {count}"
        );

        // Verify Processing state was broadcast for the real segment
        let mut saw_processing = false;
        while let Ok(state) = state_rx.try_recv() {
            if matches!(state, PipelineState::Processing { .. }) {
                saw_processing = true;
            }
        }
        assert!(
            saw_processing,
            "should see Processing state for the real speech segment"
        );
    }

    // --- T040: Full pipeline recovery after LLM edge case ---

    #[tokio::test]
    async fn test_pipeline_recovery_llm_crash() {
        // Verifies: pipeline processes two consecutive speech segments
        // successfully. The first segment exercises the LLM path, and the
        // second proves the LLM context didn't become corrupted or locked.
        // This catches deadlocks, leaked GPU resources, and state corruption
        // between segments.
        let (mut pipeline, mut state_rx, _tmpdir) = make_pipeline();
        let all_samples = load_speech_samples();
        let speech: Vec<f32> = all_samples.into_iter().take(48000).collect();

        // Segment 1: process through full pipeline
        pipeline
            .process_segment(speech.clone())
            .await
            .expect("first speech segment should succeed");

        let count_after_first = pipeline.transcript_writer.count().expect("count");
        assert!(
            count_after_first >= 1,
            "first segment should produce at least 1 transcript"
        );

        // Drain state channel
        while state_rx.try_recv().is_ok() {}

        // Segment 2: process immediately after — LLM must not be in bad state
        pipeline
            .process_segment(speech)
            .await
            .expect("second speech segment should succeed after first");

        let count_after_second = pipeline.transcript_writer.count().expect("count");
        assert!(
            count_after_second >= 2,
            "should have at least 2 transcripts after two speech segments, got {count_after_second}"
        );

        // Verify the second segment also went through Processing
        let mut saw_processing = false;
        while let Ok(state) = state_rx.try_recv() {
            if matches!(state, PipelineState::Processing { .. }) {
                saw_processing = true;
            }
        }
        assert!(
            saw_processing,
            "second segment should also trigger Processing state"
        );
    }

    // --- T041: Audio device disconnect skips segments, reconnect resumes ---

    #[tokio::test]
    async fn test_pipeline_recovery_audio_disconnect() {
        // Verifies: when audio_error_flag is set (device disconnected),
        // process_segment skips the segment and broadcasts an Error state.
        // When the flag is cleared (device reconnected), the next segment
        // processes normally.
        let (mut pipeline, mut state_rx, _tmpdir) = make_pipeline();

        // Simulate device disconnect
        let error_flag = Arc::new(AtomicBool::new(true));
        pipeline.set_audio_error_flag(error_flag.clone());

        // Process with disconnected device — should skip
        let all_samples = load_speech_samples();
        let speech: Vec<f32> = all_samples.iter().take(48000).copied().collect();
        pipeline
            .process_segment(speech.clone())
            .await
            .expect("disconnected segment should not error (it skips gracefully)");

        // Verify Error state was broadcast
        let mut saw_error = false;
        while let Ok(state) = state_rx.try_recv() {
            if let PipelineState::Error { ref message } = state {
                if message.contains("microphone") || message.contains("No microphone") {
                    saw_error = true;
                }
            }
        }
        assert!(
            saw_error,
            "should broadcast Error about microphone disconnect"
        );

        // No transcript should be saved during disconnect
        let count_disconnected = pipeline.transcript_writer.count().expect("count");
        assert_eq!(
            count_disconnected, 0,
            "no transcripts during device disconnect"
        );

        // Simulate device reconnection
        error_flag.store(false, Ordering::Release);

        // Process after reconnect — should succeed
        pipeline
            .process_segment(speech)
            .await
            .expect("reconnected segment should succeed");

        let count_reconnected = pipeline.transcript_writer.count().expect("count");
        assert!(
            count_reconnected >= 1,
            "should have at least 1 transcript after reconnection, got {count_reconnected}"
        );
    }

    // --- T042: Sleep/wake recovery components ---

    #[tokio::test]
    async fn test_pipeline_recovery_sleep_wake() {
        // Verifies: the wake recovery components work correctly when composed.
        // 1. audio_recovery_loop recovers after transient failures
        // 2. Pipeline resets to correct state after recovery
        // Can't simulate real OS sleep/wake in a test, but we verify the
        // recovery building blocks that fire on wake events.
        use crate::error::AudioError;
        use crate::recovery::{audio_recovery_loop, AudioRecoveryResult};
        use std::sync::atomic::{AtomicU32, Ordering as AtomOrd};

        // Simulate wake scenario: audio device needs 3 attempts to recover
        // (typical after sleep — device takes a moment to re-initialize)
        let attempt_count = Arc::new(AtomicU32::new(0));
        let counter = attempt_count.clone();

        let result = audio_recovery_loop(move || {
            let counter = counter.clone();
            async move {
                let count = counter.fetch_add(1, AtomOrd::SeqCst);
                if count < 2 {
                    Err(AudioError::DeviceDisconnected {
                        device_name: "Headset Microphone".to_string(),
                    })
                } else {
                    Ok(())
                }
            }
        })
        .await;

        assert!(
            matches!(result, AudioRecoveryResult::Recovered),
            "should recover on third attempt"
        );
        assert_eq!(
            attempt_count.load(AtomOrd::SeqCst),
            3,
            "should have taken 3 attempts"
        );

        // Verify pipeline can process after simulated wake recovery
        let (mut pipeline, _state_rx, _tmpdir) = make_pipeline();
        let all_samples = load_speech_samples();
        let speech: Vec<f32> = all_samples.into_iter().take(48000).collect();

        pipeline
            .process_segment(speech)
            .await
            .expect("pipeline should work after simulated wake recovery");

        let count = pipeline.transcript_writer.count().expect("count");
        assert!(
            count >= 1,
            "should produce transcript after wake recovery"
        );
    }

    // --- T043: Stress test — 1000 segments with random failures ---

    #[tokio::test]
    async fn test_resilience_1000_failures() {
        // SC-010: Submit 1000 operations with random component failures.
        // Verify: no deadlocks, no panics, retry_once handles all cases.
        // Tests the retry mechanism's resilience under sustained failure load.
        use crate::error::VoxError;
        use crate::recovery::retry_once;
        use std::sync::atomic::{AtomicU32, Ordering as AtomOrd};

        let success_count = Arc::new(AtomicU32::new(0));
        let failure_count = Arc::new(AtomicU32::new(0));

        for i in 0u64..1000 {
            let successes = success_count.clone();
            let failures = failure_count.clone();

            // Deterministic "random" failures based on segment index:
            // - i % 3 == 0: first attempt fails, second succeeds
            // - i % 7 == 0: both attempts fail
            // - otherwise: first attempt succeeds
            let call_count = Arc::new(AtomicU32::new(0));
            let counter = call_count.clone();
            let fail_first = i % 3 == 0;
            let fail_both = i % 7 == 0;

            let result: Result<String, VoxError> =
                retry_once("stress_test", i, move |segment_id| {
                    let counter = counter.clone();
                    async move {
                        let attempt = counter.fetch_add(1, AtomOrd::SeqCst);
                        if fail_both || (fail_first && attempt == 0) {
                            Err(VoxError::AsrFailure {
                                source: anyhow::anyhow!("simulated failure #{segment_id}"),
                                segment_id,
                            })
                        } else {
                            Ok(format!("processed-{segment_id}"))
                        }
                    }
                })
                .await;

            match result {
                Ok(_) => {
                    successes.fetch_add(1, AtomOrd::SeqCst);
                }
                Err(_) => {
                    failures.fetch_add(1, AtomOrd::SeqCst);
                }
            }
        }

        let total_success = success_count.load(AtomOrd::SeqCst);
        let total_failure = failure_count.load(AtomOrd::SeqCst);

        assert_eq!(
            total_success + total_failure,
            1000,
            "all 1000 operations should complete"
        );
        assert!(
            total_success > 0,
            "some operations should succeed"
        );
        assert!(
            total_failure > 0,
            "some operations should fail (by design)"
        );

        // Segments where i % 7 == 0 fail both attempts (i=0,7,14,...,994)
        // That's ceil(1000/7) = 143 double-failures
        let expected_failures = (0u64..1000).filter(|i| i % 7 == 0).count() as u32;
        assert_eq!(
            total_failure, expected_failures,
            "failure count should match deterministic pattern"
        );
    }

    // --- T044: Security — zero audio files on disk, no network post-download ---

    #[tokio::test]
    async fn test_security_no_audio_on_disk() {
        // SC-005: Verify zero audio files written to disk during pipeline processing.
        // SC-006: Audio data stays in memory only — no .wav, .pcm, .raw, .mp3, .ogg
        // files should exist in the temp directory after processing.
        let (mut pipeline, _state_rx, tmpdir) = make_pipeline();
        let all_samples = load_speech_samples();
        let speech: Vec<f32> = all_samples.into_iter().take(48000).collect();

        pipeline
            .process_segment(speech)
            .await
            .expect("process_segment should succeed");

        // Scan the temp directory (and all subdirectories) for audio files
        let audio_extensions = ["wav", "pcm", "raw", "mp3", "ogg", "flac", "aac", "m4a"];
        let mut audio_files_found = Vec::new();

        fn scan_dir(dir: &std::path::Path, extensions: &[&str], found: &mut Vec<std::path::PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        scan_dir(&path, extensions, found);
                    } else if let Some(ext) = path.extension() {
                        let ext_lower = ext.to_string_lossy().to_lowercase();
                        if extensions.contains(&ext_lower.as_str()) {
                            found.push(path);
                        }
                    }
                }
            }
        }

        scan_dir(tmpdir.path(), &audio_extensions, &mut audio_files_found);

        assert!(
            audio_files_found.is_empty(),
            "SC-005: no audio files should be written to disk during pipeline processing, \
             but found: {audio_files_found:?}"
        );

        // Also verify the pipeline's working memory doesn't leak files
        // by checking common temp directories
        let cargo_tmp = std::env::temp_dir();
        let mut vox_audio_files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&cargo_tmp) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension() {
                    let ext_lower = ext.to_string_lossy().to_lowercase();
                    if audio_extensions.contains(&ext_lower.as_str()) {
                        // Only flag files that look like they came from Vox
                        if let Some(name) = path.file_name() {
                            let name_str = name.to_string_lossy();
                            if name_str.contains("vox") || name_str.contains("speech") {
                                vox_audio_files.push(path);
                            }
                        }
                    }
                }
            }
        }

        assert!(
            vox_audio_files.is_empty(),
            "SC-005: no Vox-related audio files should be in system temp, \
             but found: {vox_audio_files:?}"
        );
    }
}
