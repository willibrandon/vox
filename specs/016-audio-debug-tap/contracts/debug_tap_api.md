# API Contract: DebugAudioTap

**Feature**: 016-audio-debug-tap
**Date**: 2026-02-27

## Public Interface

### DebugAudioTap

Thread-safe handle to the debug audio recording system. Shared via `Arc<DebugAudioTap>` across the VAD thread and async pipeline.

#### Construction

```rust
/// Create a new debug audio tap.
///
/// Creates the `debug_audio/` directory under `data_dir` if it doesn't exist.
/// Spawns the background writer thread. The writer thread runs startup cleanup
/// as its first action (deletes files older than 24 hours, enforces 500 MB cap)
/// before entering the message receive loop — cleanup does not block the caller.
pub fn new(data_dir: &Path, initial_level: DebugAudioLevel) -> Self
```

**Preconditions**: `data_dir` exists and is writable.
**Postconditions**: `debug_audio/` subdirectory exists. Writer thread is running. Cleanup runs asynchronously on writer thread.

#### Session Lifecycle

```rust
/// Begin a new recording session.
///
/// Increments the session counter, resets the segment counter, and sends
/// a StartSession message to the writer thread. No-op if level is Off.
pub fn start_session(&self, native_sample_rate: u32)

/// End the current recording session.
///
/// Sends EndSession to finalize streaming WAV files. No-op if level is Off.
/// The writer thread logs the session summary (FR-019) when it processes
/// the EndSession message — it has the file/byte counts. The drop count
/// is read from the shared AtomicU64 at that point.
pub fn end_session(&self)
```

#### Tap Points

```rust
/// Record raw microphone samples (before resampling).
///
/// No-op if level != Full or write_error is set.
/// Clones samples and sends via try_send. Increments drop_count on channel full.
pub fn tap_raw(&self, samples: &[f32])

/// Record post-resampler samples (16 kHz, before VAD/chunker).
///
/// No-op if level != Full or write_error is set.
/// Same backpressure behavior as tap_raw.
pub fn tap_resampled(&self, samples: &[f32])

/// Record a complete VAD segment.
///
/// No-op if level == Off. Returns the segment index for ASR correlation.
/// Logs segment duration at debug level (FR-021).
pub fn tap_vad_segment(&self, samples: &[f32]) -> u32

/// Record the exact audio buffer sent to ASR (with silence padding).
///
/// No-op if level == Off.
pub fn tap_asr_input(&self, segment_index: u32, samples: &[f32])
```

**Backpressure contract**: All tap methods use `try_send`. They never block. On channel full, they increment `drop_count` and return immediately. The caller's audio processing is never delayed.

#### Configuration

```rust
/// Change the debug audio level at runtime.
///
/// Updates the atomic level, resets session counter to 0, clears write_error.
/// If transitioning from non-Off to Off while a session is active, sends EndSession.
pub fn set_level(&self, level: DebugAudioLevel)

/// Set the pipeline state broadcast sender for error notifications.
///
/// Called each time a new recording session starts. The writer thread uses this
/// to notify the overlay on write failures (FR-013).
pub fn set_state_tx(&self, tx: broadcast::Sender<PipelineState>)
```

#### Shutdown

```rust
/// Shut down the writer thread.
///
/// Drops the channel sender, joins the writer thread with a 2-second timeout.
/// Idempotent — second call is a no-op.
pub fn shutdown(&self)
```

#### Diagnostics

```rust
/// Return the total number of dropped tap messages due to channel backpressure.
pub fn drop_count(&self) -> u64

/// Return the current debug audio level.
pub fn level(&self) -> DebugAudioLevel

/// Return the path to the debug audio directory.
pub fn debug_audio_dir(&self) -> &Path
```

### DebugAudioLevel

```rust
/// Debug audio recording level.
///
/// Controls which audio tap points are active. Persisted in settings.json.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DebugAudioLevel {
    /// No debug audio recording (default). Zero runtime overhead.
    #[default]
    Off,
    /// Record per-segment taps only (vad_segment + asr_input).
    /// Low data volume: ~1-5 small WAV files per minute.
    Segments,
    /// Record all taps including continuous raw capture and post-resample.
    /// High data volume: ~256 KB/s while recording.
    Full,
}
```

## Thread Safety Guarantees

| Method | Called from | Guarantee |
|--------|-----------|-----------|
| `tap_raw`, `tap_resampled` | VAD thread (std::thread) | Non-blocking, lock-free level check + try_send |
| `tap_vad_segment` | VAD thread | Non-blocking, atomic increment + try_send |
| `tap_asr_input` | Orchestrator (Tokio task) | Non-blocking, try_send safe from async context |
| `start_session`, `end_session` | VAD thread | Non-blocking, atomic ops + try_send |
| `set_level` | GPUI main thread (settings callback) | Non-blocking, atomic store |
| `set_state_tx` | GPUI main thread (start_recording) | Mutex lock (brief, no contention) |
| `shutdown` | GPUI main thread (app exit) | Mutex lock + thread join (blocking, 2s timeout) |

## Writer Thread Auto-Session

If the writer thread receives a `VadSegment` or `AsrInput` message without a preceding `StartSession` (e.g., when debug audio was toggled on mid-recording), it auto-creates a session: assigns the next session ID and processes the per-segment message. Streaming writers are NOT opened during auto-session (the native sample rate is unknown without a StartSession). `AppendRaw` and `AppendResampled` messages received in Idle state are dropped with a single debug-level log on first occurrence — streaming taps start working on the next proper recording session.

## Error Contract

| Condition | Behavior | User-visible |
|-----------|----------|-------------|
| Channel full (backpressure) | Drop message, increment counter, return immediately | No (gap in WAV file) |
| Disk full / permission denied | Set write_error flag, broadcast PipelineState::Error (once per session) | Yes (overlay notification) |
| Directory deleted mid-session | Attempt recreation on next StartSession; notify overlay if recreation fails | Yes (if recreation fails) |
| App crash during recording | WAV headers incorrect, audio data recoverable by standard tools | No |

## File Output Contract

| Level | Recording produces | Per utterance |
|-------|-------------------|---------------|
| Off | Nothing | Nothing |
| Segments | Nothing (streaming) | 2 files: `vad-segment-NNN.wav` + `asr-input-NNN.wav` |
| Full | 2 files: `raw-capture.wav` + `post-resample.wav` | 2 files: `vad-segment-NNN.wav` + `asr-input-NNN.wav` |
