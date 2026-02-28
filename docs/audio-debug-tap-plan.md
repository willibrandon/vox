# Audio Debug Tap — Design & Implementation Plan

## Problem Statement

When debugging audio quality, VAD boundary detection, resampling artifacts, or ASR input issues, there is no way to hear what Vox actually captured or what was sent to the speech recognition engine. All audio lives in memory only (enforced by security constraint SC-005/SC-006) and is discarded after processing. A developer or advanced user needs the ability to play back captured audio at key pipeline stages to diagnose issues.

## Architecture Overview

### Current Audio Flow (Reference)

```
┌──────────────────┐     ring buffer     ┌──────────────────┐     mpsc(32)     ┌──────────────────┐
│  cpal callback   │ ──── (SPSC) ──────→ │   VAD Thread     │ ─── channel ───→ │   Orchestrator   │
│  f32 mono        │                     │  resample→16kHz  │                  │  silence-pad     │
│  native rate     │                     │  512-sample wins │                  │  ASR → LLM →     │
│  (e.g. 48kHz)    │                     │  chunker→segment │                  │  inject           │
└──────────────────┘                     └──────────────────┘                  └──────────────────┘
```

### Proposed Debug Tap Architecture

```
┌──────────────────┐     ring buffer     ┌──────────────────────────────┐     mpsc(32)     ┌────────────────┐
│  cpal callback   │ ──── (SPSC) ──────→ │        VAD Thread            │ ─── channel ───→ │  Orchestrator  │
│                  │                     │                              │                  │                │
└──────────────────┘                     │  read → [TAP 1: raw]        │                  │  silence-pad   │
                                         │  resample → [TAP 2: resamp] │                  │  [TAP 4: asr]  │
                                         │  chunker → [TAP 3: segment] │                  │                │
                                         └──────────┬───────────────────┘                  └───────┬────────┘
                                                    │                                              │
                                                    ▼                                              ▼
                                         ┌──────────────────────────────────────────────────────────┐
                                         │                   DebugAudioTap                          │
                                         │  bounded mpsc(256) → background writer thread            │
                                         │  try_send — drops taps on backpressure, never blocks    │
                                         │                                                          │
                                         │  Streaming taps (raw, resampled):                        │
                                         │    StartSession → AppendSamples → EndSession             │
                                         │    One WAV file per session, samples appended in flight   │
                                         │                                                          │
                                         │  Per-segment taps (vad_segment, asr_input):              │
                                         │    One WAV file per segment, correlated by session_id     │
                                         │                                                          │
                                         │  WAV files → data_dir/debug_audio/                       │
                                         └──────────────────────────────────────────────────────────┘
```

## Design Decisions

### Tap Points (4 stages)

| Tap ID | Location | Sample Rate | Firing Pattern | Diagnostic Value |
|--------|----------|-------------|----------------|------------------|
| `raw_capture` | VAD thread, after ring buffer read, before resample | Native (e.g. 48kHz) | Continuous while recording (~100 msgs/s) | Mic working? Correct device? Audio quality? |
| `post_resample` | VAD thread, after resample, before chunker | 16 kHz | Continuous while recording (~100 msgs/s) | Resampler introducing artifacts? FFT boundary glitches? |
| `vad_segment` | VAD thread, segment emitted from chunker | 16 kHz | Per-utterance (~1-5/min) | VAD cutting correctly? Pre-buffer capturing onsets? |
| `asr_input` | Orchestrator, after 200ms silence-pad prepend | 16 kHz | Per-utterance (~1-5/min) | Exact buffer Whisper receives. Silence padding correct? |

**Why 4 taps, not 3:**

The original plan omitted a post-resample tap, leaving a diagnostic blind spot. If `vad_segment` sounds bad but `raw_capture` sounds clean, you can't tell whether the problem is resampling or the chunker's segment boundary stitching. `post_resample` directly isolates the resampler output before any VAD/chunker processing touches it.

**Two structural categories:**

| Category | Taps | File pattern | Data volume |
|----------|------|--------------|-------------|
| **Streaming** | `raw_capture`, `post_resample` | 1 WAV per recording session (appended to continuously) | ~192 KB/s raw (48kHz), ~64 KB/s resampled (16kHz) |
| **Per-segment** | `vad_segment`, `asr_input` | 1 WAV per utterance | ~5-50 KB per segment |

Streaming taps produce fundamentally different I/O patterns than per-segment taps. Streaming taps use a single open `WavWriter` per session with samples appended incrementally. Per-segment taps create a new WAV file for each utterance. This distinction prevents the file-per-ring-buffer-read problem — `raw_capture` produces one file for a 30-minute session, not 180,000 files.

### Three-Level Debug Audio Setting

A single boolean is insufficient — continuous streaming taps generate orders of magnitude more data than per-segment taps. Users debugging VAD boundaries don't need the raw capture flood, and users checking mic quality don't need per-segment files.

```rust
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DebugAudioLevel {
    /// No debug audio recording (default).
    #[default]
    Off,
    /// Record per-segment taps only (vad_segment + asr_input).
    /// Low volume: ~1-5 small WAV files per minute of active dictation.
    Segments,
    /// Record all taps including continuous raw capture and post-resample.
    /// High volume: ~256 KB/s of WAV data while recording.
    Full,
}
```

Settings UI: a dropdown with three options in the Advanced section.

### Bounded Channel with Backpressure Drop

The writer channel is **bounded at 256 messages** using `std::sync::mpsc::SyncSender` with `try_send`. If the writer thread falls behind (slow disk, antivirus scanning, USB drive), messages are dropped rather than accumulating unbounded memory.

**Why `std::sync::mpsc` and not `tokio::sync::mpsc`?** The primary producers are the VAD thread (a plain OS `std::thread`) and the orchestrator (a Tokio async task). `std::sync::mpsc::SyncSender::try_send` is non-blocking regardless of calling context — it returns immediately with `Ok` or `Err(TrySendError::Full)`. This means it is safe to call from both a regular thread and an async task without stalling the Tokio runtime. Using `std::sync::mpsc` also avoids pulling Tokio into the writer thread (which is a plain `std::thread::spawn` — no runtime needed for sequential disk I/O). The existing segment channel between VAD and orchestrator remains `tokio::sync::mpsc` because the orchestrator uses `segment_rx.recv()` inside `tokio::select!`, which requires a Tokio-aware receiver. The two channel types serve different roles and coexist cleanly:

| Channel | Type | Producer | Consumer | Why |
|---------|------|----------|----------|-----|
| Segment (VAD → orchestrator) | `tokio::sync::mpsc` | VAD thread (`blocking_send`) | Orchestrator (`select!` + `recv`) | Needs Tokio `select!` integration |
| Debug tap (VAD/orchestrator → writer) | `std::sync::mpsc` | VAD thread + orchestrator (`try_send`) | Writer thread (`recv` blocking loop) | Non-blocking from any context, no Tokio runtime needed in writer |

**Why 256?** Each streaming message carries ~512-1024 f32 samples (~2-4 KB). Worst-case channel memory: 256 × 4 KB = 1 MB. This is a hard ceiling — OOM is impossible regardless of disk speed.

**Drop behavior:**
- On `try_send` failure (channel full), the tap call returns immediately — no allocation, no blocking.
- A `drop_count: AtomicU64` counter on `DebugAudioTap` tracks total drops.
- The writer thread logs a warning on first drop per session: `"debug audio writer falling behind — {n} samples dropped"`.
- No UI notification for drops — they indicate the writer is slow, not broken. Dropped taps result in a gap in the streaming WAV (audible as a skip), which is an acceptable degradation for a debug tool.

### Session-Based Correlation

Each recording session (hotkey press/release cycle) gets a monotonic `session_id: u64` incremented on each `StartSession` message. All files from the same session share the same ID.

Per-segment taps track a `segment_index: u32` per session, incremented when a `vad_segment` is emitted. Both `vad_segment` and `asr_input` for the same utterance share the same segment index (the orchestrator receives the segment index alongside the audio data).

**Mid-session toggle edge case:** If the user toggles level from Off → Segments mid-recording, `set_level` sends a `StartSession` for the new session. However, a segment already in-flight in the orchestrator's pipeline still carries `segment_index = 0` from the old (Off) state. When it arrives as an `AsrInput` message, its `session_id` comes from the message payload itself (set at the time `tap_vad_segment` was called), not from the writer thread's current state. Since the writer keys filenames off the `session_id` embedded in each message, a stale `AsrInput` with `session_id = N-1` simply writes to the old session's filename. If no streaming files exist for that old session (because level was Off), the per-segment file is still valid standalone. No special handling needed — the message-embedded session ID makes this self-consistent.

**File naming convention:**
```
debug_audio/
  session-001_2026-02-27T10-30-45_raw-capture.wav
  session-001_2026-02-27T10-30-45_post-resample.wav
  session-001_2026-02-27T10-30-45_vad-segment-001.wav
  session-001_2026-02-27T10-30-45_asr-input-001.wav
  session-001_2026-02-27T10-30-45_vad-segment-002.wav
  session-001_2026-02-27T10-30-45_asr-input-002.wav
  session-002_2026-02-27T10-31-12_raw-capture.wav
  ...
```

Session ID is the primary grouping key. Within a session, segment indices match across `vad_segment` and `asr_input`. Streaming taps (`raw_capture`, `post_resample`) have no segment index — they cover the entire session.

### Message Protocol

```rust
enum DebugAudioMessage {
    /// Open streaming WAV files for a new recording session.
    StartSession {
        session_id: u64,
        raw_sample_rate: u32,
        timestamp: String,
    },
    /// Append raw capture samples to the session's streaming WAV.
    AppendRaw(Vec<f32>),
    /// Append post-resample samples to the session's streaming WAV.
    AppendResampled(Vec<f32>),
    /// Write a complete VAD segment as a standalone WAV file.
    VadSegment {
        session_id: u64,
        segment_index: u32,
        samples: Vec<f32>,
    },
    /// Write the exact ASR input as a standalone WAV file.
    AsrInput {
        session_id: u64,
        segment_index: u32,
        samples: Vec<f32>,
    },
    /// Close streaming WAV files for the current session.
    EndSession,
}
```

**Streaming WAV lifecycle:** `hound::WavWriter` is created on `StartSession` and held open by the writer thread. `AppendRaw`/`AppendResampled` write samples to the open writer. `EndSession` finalizes and drops the writer (which seeks back to update the RIFF data-size header). If the app crashes mid-session, the WAV file will have an incorrect size header but most audio tools (Audacity, ffmpeg) can recover the samples.

### Passthrough Mode Handling

In passthrough mode (hold-to-talk), raw samples are accumulated in a single `Vec<f32>` and resampled once on stop. The naive approach — tapping the entire buffer as one message — doubles memory usage momentarily for long recordings.

**Fix:** Tap raw samples incrementally during the accumulation loop (same `AppendRaw` messages as VAD mode). For the final resampled buffer, chunk it into 1-second slices (16,000 samples each) and send as multiple `AppendResampled` messages. The segment tap sends the resampled buffer as a single `VadSegment` (same as the mpsc segment channel — bounded at 32 so this is already tolerated).

### Storage & Cleanup

- **Directory:** `data_dir/debug_audio/` (e.g. `%LOCALAPPDATA%/com.vox.app/debug_audio/`)
- **Auto-cleanup on startup:** Delete files older than 24 hours using **creation time** (not mtime). On Windows/NTFS, mtime can be unreliable due to antivirus tools or indexing services touching files. Creation time is set once at file creation and never modified.
- **Max storage cap:** 500 MB total. Enforced by tracking cumulative bytes in memory (a `u64` counter in the writer thread, incremented after each write). Full directory scan only happens on startup (to initialize the counter) and every 50 writes (to correct for drift from external deletions or cleanup). When cap is exceeded, delete oldest files (sorted by creation time) until under 400 MB (20% hysteresis to avoid delete-write-delete thrashing).
- **Session counter resets** on each app launch. Files are self-describing via timestamp.

### Enable/Disable Semantics

`DebugAudioLevel` is stored in an `AtomicU8` (cast from enum discriminant) for lock-free reads on the hot path.

**Toggling off:** Messages already in the bounded channel will still be written. This is documented and expected — a user toggling off may see 1-2 more files appear as the channel drains. Streaming WAV files are finalized normally (EndSession sent on toggle-off).

**Toggling on:** Resets the session counter to 0 for clean numbering in the new debug session.

**Toggling mid-recording:** If debug audio is toggled on while a recording session is active, the tap starts from the next ring buffer read — there is no retroactive capture of audio that already passed through. If toggled off mid-recording, an `EndSession` is sent to finalize the streaming WAV.

### Writer Thread Error Handling

The writer thread must not silently swallow errors. Failures to write (disk full, permission denied, path too long on Windows) are handled as follows:

1. **First error per session:** Logged at `error` level via tracing AND a one-time error notification is sent through the `PipelineState` broadcast channel: `PipelineState::Error { message: "Debug audio write failed: {reason}" }`. The overlay shows this to the user.
2. **Subsequent errors in same session:** Logged at `warn` level only — no repeated UI notifications.
3. **Error flag:** An `AtomicBool` is set on first write error, checked by tap calls. When set, new `AppendRaw`/`AppendResampled` messages are suppressed (no point filling the channel with data that will fail to write). Per-segment taps still attempt writes since the failure may be transient (e.g., antivirus released the file lock).
4. **Error flag reset:** Cleared on next `StartSession` (new recording session = fresh attempt).

### Shutdown Semantics

`shutdown()` is idempotent:
1. Drops the `SyncSender` (disconnects channel).
2. Takes the `JoinHandle` from `Option` via `.take()` — second call is a no-op.
3. Joins the writer thread with a 2-second timeout. If the thread panicked, logs the panic payload but does not propagate.

**Crash behavior:** If the app crashes, pending messages in the channel are lost. Streaming WAV files will have incorrect RIFF headers but the audio data is intact — most tools recover this. This is acceptable for a debug feature.

### Security Constraint Compatibility

SC-005/SC-006 test that **no audio files are written during normal pipeline processing**. Since `debug_audio` defaults to `Off`, the test passes without modification. When `debug_audio` is explicitly `Segments` or `Full`, the user has opted in — audio persistence is intentional. The security test verifies default behavior, not debug behavior.

### Thread Safety

| Component | Thread | Safety Model |
|-----------|--------|-------------|
| `DebugAudioTap` | Shared across VAD thread + async pipeline | `Arc<DebugAudioTap>` — interior `SyncSender` is `Send + Sync` |
| Writer thread | Own `std::thread` | Owns `Receiver`, holds open `WavWriter` handles, no shared state |
| Level check | Each tap call | `AtomicU8` — lock-free, updated when settings change |
| Drop counter | Tap calls on channel-full | `AtomicU64` — lock-free increment |
| Write error flag | Writer thread sets, tap calls read | `AtomicBool` — lock-free |

### Binary Size

The `DebugAudioTap` module, channel infrastructure, writer thread, and tap call sites add ~300 lines of Rust. This compiles to roughly 5-10 KB of machine code. Combined with hound's ~8 KB, total impact is ~15-20 KB against the 15 MB budget (0.1%). The code is always compiled — no `#[cfg]` guards. This is intentional per the constitution ("No Optional Compilation"). The runtime cost when disabled is 1 atomic load per tap call (~1 ns). Enabling `#[cfg]` would save 15 KB at the cost of requiring recompilation to debug audio issues, which defeats the purpose.

## Implementation Plan

### Task 1: Promote hound dependency

**File:** `crates/vox_core/Cargo.toml`

Move `hound = "3.5"` from `[dev-dependencies]` to `[dependencies]`.

**Estimated size:** 2 lines changed

### Task 2: Settings — DebugAudioLevel enum and field

**File:** `crates/vox_core/src/config.rs`

- Define `DebugAudioLevel` enum (`Off`, `Segments`, `Full`) with `Serialize`/`Deserialize`, `#[serde(rename_all = "kebab-case")]`, `Default` (→ `Off`)
- Add `pub debug_audio: DebugAudioLevel` field to `Settings` (with `#[serde(default)]`)
- Update doc comment header to mention Debug (1 field) category

**Estimated size:** ~20 lines

### Task 3: DebugAudioTap module — core struct and message protocol

**File:** `crates/vox_core/src/audio/debug_tap.rs`

Create the `DebugAudioTap` struct and `DebugAudioMessage` enum:

```
DebugAudioTap
├── level: Arc<AtomicU8>                — lock-free level check (Off=0, Segments=1, Full=2)
├── sender: std::sync::mpsc::SyncSender<DebugAudioMessage>  — bounded(256)
├── session_counter: AtomicU64          — monotonic, reset on level change
├── segment_counter: AtomicU32          — per-session, reset on StartSession
├── drop_count: AtomicU64              — total try_send failures
├── write_error: Arc<AtomicBool>       — set by writer on first I/O failure
├── writer_handle: Mutex<Option<JoinHandle<()>>>  — background thread
└── data_dir: PathBuf                  — for debug_audio/ subdirectory
```

Public methods:
- `new(data_dir: &Path, state_tx: broadcast::Sender<PipelineState>) -> Self`
  - Creates `debug_audio/` dir
  - Runs startup cleanup (delete files older than 24h by creation time, enforce 500 MB cap)
  - Spawns writer thread with `Receiver` + `state_tx` clone
- `start_session(&self, native_sample_rate: u32)`
  - Increments session counter, resets segment counter
  - Sends `StartSession` (or no-op if level == Off)
- `tap_raw(&self, samples: &[f32])`
  - No-op if level != Full, or write_error is set
  - Clones samples, `try_send(AppendRaw(...))`
  - Increments drop_count on `TrySendError::Full`
- `tap_resampled(&self, samples: &[f32])`
  - Same gating as `tap_raw`
  - `try_send(AppendResampled(...))`
- `tap_vad_segment(&self, samples: &[f32]) -> u32`
  - No-op if level == Off
  - Increments and returns segment_index (for orchestrator correlation)
  - `try_send(VadSegment { ... })`
- `tap_asr_input(&self, segment_index: u32, samples: &[f32])`
  - No-op if level == Off
  - `try_send(AsrInput { ... })`
- `end_session(&self)`
  - Sends `EndSession` (or no-op if level == Off)
- `set_level(&self, level: DebugAudioLevel)`
  - Updates AtomicU8
  - Resets session_counter to 0
  - Clears write_error flag
  - If transitioning from non-Off to Off mid-session: sends `EndSession`
- `shutdown(&self)`
  - Takes JoinHandle from Mutex, drops sender (implicitly — sender is cloned on construction, original dropped here), joins with 2s timeout
  - Idempotent (second call is no-op)

**Estimated size:** ~130 lines

### Task 4: Writer thread — WAV file I/O and cleanup

**File:** `crates/vox_core/src/audio/debug_tap.rs` (same file, private function)

Writer thread main loop:

```rust
fn writer_thread(
    receiver: std::sync::mpsc::Receiver<DebugAudioMessage>,
    data_dir: PathBuf,
    write_error: Arc<AtomicBool>,
    state_tx: broadcast::Sender<PipelineState>,
) {
    // State: open WavWriters for streaming taps
    let mut raw_writer: Option<hound::WavWriter<BufWriter<File>>> = None;
    let mut resample_writer: Option<hound::WavWriter<BufWriter<File>>> = None;
    let mut cumulative_bytes: u64 = compute_dir_size(&data_dir);
    let mut writes_since_scan: u32 = 0;
    let mut error_notified_this_session: bool = false;

    loop {
        match receiver.recv() {
            Ok(msg) => handle_message(msg, ...),
            Err(_) => break, // channel disconnected (shutdown)
        }
    }
    // Finalize any open writers on exit
}
```

Message handling:
- `StartSession`: Create `raw_writer` and `resample_writer` via `hound::WavWriter::create` with `BufWriter<File>` for buffered I/O. Reset `error_notified_this_session`. **Note on BufWriter + seek:** hound's `WavWriter::finalize()` seeks back to byte 4 to patch the RIFF chunk size. `BufWriter` flushes its internal buffer before any seek operation (guaranteed by `std::io::BufWriter::seek` impl), so the data-then-seek-then-patch sequence is correct. Verified in test 3 (test_streaming_wav_session) by round-tripping through `hound::WavReader`.
- `AppendRaw`: Write samples to `raw_writer`. Update `cumulative_bytes`.
- `AppendResampled`: Write samples to `resample_writer`. Update `cumulative_bytes`.
- `VadSegment`: Create new WAV, write all samples, finalize immediately. Update `cumulative_bytes`.
- `AsrInput`: Same as VadSegment.
- `EndSession`: Finalize and drop `raw_writer` and `resample_writer` (triggers hound's header fixup seek).

Error handling:
- On any `hound::Error` or `std::io::Error`:
  - Set `write_error` AtomicBool
  - Log at `error` level with the error details
  - If `!error_notified_this_session`: broadcast `PipelineState::Error { message }` and set flag
  - Close the failed writer (drop it), skip remaining messages for that tap type until next session

Size tracking:
- `cumulative_bytes` incremented by `samples.len() * 4 + 44` (f32 size + WAV header) after each write
- Every 50 writes (`writes_since_scan % 50 == 0`): re-scan directory to correct for external deletions
- When `cumulative_bytes > 500 MB`: delete oldest files (by creation time) until under 400 MB (20% hysteresis)

**Estimated size:** ~120 lines

### Task 5: Pipeline integration — VAD thread taps

**File:** `crates/vox_core/src/vad.rs`

Add `debug_tap: &DebugAudioTap` parameter to `run_vad_loop` and `run_passthrough_loop`.

**VAD mode (`run_vad_loop`):**

```rust
// Before the main loop:
debug_tap.start_session(native_sample_rate);

// After ring buffer read, before resample (line ~346):
debug_tap.tap_raw(&read_buffer[..read_count]);

// After resample, before accumulation (line ~365):
debug_tap.tap_resampled(&samples);

// When chunker emits a segment (line ~383):
if let Some(ref segment) = segment {
    let seg_idx = debug_tap.tap_vad_segment(segment);
    // Pass seg_idx alongside segment to orchestrator (see Task 6)
    segment_tx.blocking_send((segment, seg_idx)).ok();
}

// After the drain loop exits:
debug_tap.end_session();
```

**Passthrough mode (`run_passthrough_loop`):**

```rust
// Before the accumulation loop:
debug_tap.start_session(native_sample_rate);

// Inside the accumulation loop, after each ring buffer read (line ~265):
debug_tap.tap_raw(&read_buffer[..read_count]);

// After the drain loop, after resampling (line ~290-294):
// Chunk the resampled buffer into 1-second slices for the channel:
for chunk in audio_buffer.chunks(16000) {
    debug_tap.tap_resampled(chunk);
}
let seg_idx = debug_tap.tap_vad_segment(&audio_buffer);
segment_tx.blocking_send((audio_buffer, seg_idx)).ok();

debug_tap.end_session();
```

**Channel type change:** The segment channel changes from `mpsc::Sender<Vec<f32>>` to `mpsc::Sender<(Vec<f32>, u32)>` to carry the segment index for ASR input correlation. When debug audio is Off, the segment index is always 0 (ignored by the orchestrator).

**Blast radius of channel type change** (scoped via grep):

| Site | File | Lines | Change |
|------|------|-------|--------|
| Channel creation | `orchestrator.rs` | 118 | `mpsc::channel::<(Vec<f32>, u32)>(32)` |
| `segment_rx` field type | `orchestrator.rs` | 43 | `Option<mpsc::Receiver<(Vec<f32>, u32)>>` |
| `run()` select arm | `orchestrator.rs` | 164-167 | Destructure `(audio_segment, seg_idx)` |
| Two drain loops | `orchestrator.rs` | 212, 241 | Destructure `(segment, seg_idx)` |
| `process_segment` signature | `orchestrator.rs` | 295 | Add `segment_index: u32` parameter |
| `run_vad_loop` sends | `vad.rs` | 388, 440, 462, 469 | `blocking_send((segment, seg_idx))` |
| `run_passthrough_loop` send | `vad.rs` | 302 | `blocking_send((audio_buffer, seg_idx))` |
| VAD test: `test_vad_end_to_end_with_speech` | `vad.rs` | 651, 692 | Channel type + destructure in `try_recv` |
| VAD test: `test_vad_multiple_utterances` | `vad.rs` | 740, 785 | Channel type + destructure in `try_recv` |
| Orchestrator tests: `make_pipeline` | `orchestrator.rs` | 769 | No change (tests call `process_segment` directly, don't use segment channel) |

The orchestrator tests (`test_full_pipeline_hello_world`, `test_pipeline_empty_audio`, etc.) call `process_segment()` directly — they never construct a segment channel, so they only need the new `segment_index` parameter added (always 0 in tests). The VAD tests construct their own `tokio::sync::mpsc::channel` and need the type updated + destructuring in `try_recv`.

Total: ~15 sites across 2 files. Straightforward mechanical change.

**Estimated size:** ~35 lines changed across both functions + signature updates + test fixtures

### Task 6: Pipeline integration — Orchestrator ASR input tap

**File:** `crates/vox_core/src/pipeline/orchestrator.rs`

1. Add `debug_tap: Arc<DebugAudioTap>` field to `Pipeline` struct.
2. Update `segment_rx` type from `mpsc::Receiver<Vec<f32>>` to `mpsc::Receiver<(Vec<f32>, u32)>`.
3. In `process_segment`, accept `segment_index: u32` parameter.
4. After building `padded_segment` (line ~337):

```rust
self.debug_tap.tap_asr_input(segment_index, &padded_segment);
```

**Estimated size:** ~15 lines changed

### Task 7: Initialization wiring

**File:** `crates/vox_core/src/state.rs` (VoxState) and `crates/vox/src/main.rs`

1. Create `DebugAudioTap` during `VoxState` initialization:
   - Read `settings.debug_audio` for initial level
   - Pass `data_dir` and pipeline `state_tx` to constructor
2. Store as `Arc<DebugAudioTap>` in `VoxState`.
3. Pass `Arc` clone to `Pipeline::new()`.
4. Pass `Arc` clone (or `&DebugAudioTap`) to VAD thread spawn.
5. On app shutdown, call `debug_tap.shutdown()`.

**Estimated size:** ~20 lines changed

### Task 8: Settings panel UI

**File:** `crates/vox_ui/src/settings_panel.rs`

Add a dropdown (Select entity) in the Advanced section:
- Label: "Debug Audio Recording"
- Description: "Save WAV files of captured audio for debugging. Files auto-delete after 24 hours."
- Options: "Off", "Segments Only", "Full (includes raw capture)"
- Callback: `update_settings(|s| s.debug_audio = level)` + `debug_tap.set_level(level)`

**Estimated size:** ~20 lines changed

### Task 9: Tests

**File:** `crates/vox_core/src/audio/debug_tap.rs` (unit tests)

1. **test_wav_written_when_segments_level** — Enable Segments, send VadSegment, verify WAV file exists with correct sample count.
2. **test_no_wav_when_off** — Default Off, send VadSegment, verify no files.
3. **test_streaming_wav_session** — Enable Full, StartSession → AppendRaw × N → EndSession, verify single WAV file (not N files). Re-read the WAV with `hound::WavReader` to verify: (a) RIFF header is valid (WavReader::open succeeds), (b) spec matches expected sample rate and format, (c) total sample count equals sum of all appended samples. This catches BufWriter seek/flush issues in hound's finalize.
4. **test_bounded_channel_drops_on_backpressure** — Fill channel with 256+ messages (mock slow writer), verify tap calls don't block and drop_count > 0.
5. **test_cleanup_deletes_old_files** — Create files with old creation time, construct DebugAudioTap, verify deleted.
6. **test_storage_cap_enforced** — Write files totaling > 500 MB, verify oldest deleted to reach < 400 MB.
7. **test_session_correlation** — Send StartSession + VadSegment + AsrInput with same session_id, verify filenames share session ID and segment indices match.
8. **test_shutdown_idempotent** — Call shutdown() twice, verify no panic.
9. **test_writer_error_sets_flag** — Point data_dir to read-only path, verify write_error flag is set and subsequent raw taps are suppressed.

**File:** `crates/vox_core/src/pipeline/orchestrator.rs`

10. **Verify existing SC-005/SC-006 test still passes** — debug_audio defaults to Off.

**Estimated size:** ~120 lines

## Task Dependency Graph

```
Task 1 (hound dep) ──────────────────────────────────────────────────────────┐
                                                                              │
Task 2 (Settings enum) ──┐                                                   │
                          │                                                   │
                          ▼                                                   ▼
                    Task 3 (DebugAudioTap struct) ──→ Task 4 (Writer thread)
                          │                                   │
                          ├──→ Task 5 (VAD taps) ────────────┤
                          │                                   │
                          ├──→ Task 6 (Orchestrator tap) ────┤
                          │                                   │
                          ├──→ Task 7 (Init wiring) ─────────┤
                          │       │                           │
                          │       ▼                           │
                          ├──→ Task 8 (UI dropdown) ─────────┤
                          │                                   │
                          └───────────────────────────────────▼
                                                        Task 9 (Tests)
```

## Addressed Review Feedback

| # | Issue | Resolution |
|---|-------|------------|
| 1 | Unbounded channel OOM risk | Bounded channel (256), `try_send`, hard 1 MB memory ceiling |
| 2 | raw_capture volume vastly higher | Three-level setting (Off/Segments/Full); streaming taps only enabled at Full |
| 3 | O(n) dir scan per write | In-memory `cumulative_bytes` counter, full scan only on startup + every 50 writes |
| 4 | File-per-ring-buffer-read | Streaming WAV pattern: one file per session, samples appended via open `WavWriter` |
| 5 | Sequence doesn't correlate | `session_id` groups all taps per recording; `segment_index` shared between `vad_segment` and `asr_input` |
| 6 | AtomicBool toggle gap | Documented drain behavior; session counter reset on level change; EndSession sent on toggle-off |
| 7 | No flush/sync on shutdown | Idempotent shutdown (Mutex + Option + take); 2s join timeout; panic logged not propagated; crash behavior documented |
| 8 | Windows mtime unreliable | Uses creation time for cleanup, not modification time |
| 9 | Passthrough mode memory spike | Raw samples tapped incrementally during loop; resampled buffer chunked into 1s slices |
| 10 | No resample isolation tap | Added 4th tap point: `post_resample` (immediately after resampler, before chunker) |
| 11 | Binary size concern | ~15-20 KB total, 0.1% of budget; always compiled per constitution; runtime cost when Off: 1 atomic load (~1 ns) |
| 12 | Silent writer failures | First error: tracing error + PipelineState broadcast to UI; subsequent: warn-level log only; write_error flag suppresses further raw taps |

### Follow-Up Review (Round 2)

| # | Issue | Resolution |
|---|-------|------------|
| R2-1 | `std::sync::mpsc` vs `tokio::sync::mpsc` mixing | Intentional. Documented with comparison table in "Bounded Channel" section. `SyncSender::try_send` is non-blocking from async context. Writer thread needs no Tokio runtime. |
| R2-2 | Segment channel type change blast radius | Scoped via grep: ~15 sites across 2 files (orchestrator.rs + vad.rs). Orchestrator tests call `process_segment` directly (no channel), only need new parameter. VAD tests need channel type + destructuring update. Detailed site table in Task 5. |
| R2-3 | Mid-session toggle `session_id` mismatch | Session ID embedded in message payload, not derived from writer's current state. Stale `AsrInput` with old session_id writes to old session filename — self-consistent. Documented in "Session-Based Correlation" section. |
| R2-4 | `BufWriter<File>` + hound finalize seek correctness | `BufWriter::seek` flushes before seeking (std guarantee). Test 3 (test_streaming_wav_session) round-trips through `hound::WavReader` to verify header correctness end-to-end. |

## Future Extensions (Not In Scope)

These are natural extension points the architecture supports but are **not part of this implementation:**

- **In-app playback:** Add cpal output stream + play/pause controls in a "Debug Audio" panel. The WAV files on disk serve as the playback source. No architectural changes needed — just a new UI panel that reads from `debug_audio/`.
- **Real-time audio monitor:** Stream tap audio to a loopback output device for live monitoring during recording.
- **Transcript-to-audio linking:** Embed segment_index in `TranscriptEntry` so the history panel can link each transcript to its audio files.
- **Waveform visualization:** Render WAV file waveforms in the debug panel using the existing RMS computation infrastructure.

## Estimated Total Impact

| Metric | Value |
|--------|-------|
| New files | 1 (`audio/debug_tap.rs`) |
| Modified files | 5 (`config.rs`, `vad.rs`, `orchestrator.rs`, `state.rs`, `settings_panel.rs`) + 1 (`Cargo.toml`) |
| New lines (approx) | ~340 production + ~130 test |
| New dependencies | 0 (hound promoted from dev → regular) |
| Binary size impact | ~15-20 KB |
| Runtime overhead (Off) | 1 atomic load per tap call (~1 ns) |
| Runtime overhead (Segments) | Vec clone + try_send per segment (~1-5 μs, ~1-5×/min) |
| Runtime overhead (Full) | Vec clone + try_send per ring buffer read (~1-5 μs, ~100×/s) |
| Memory ceiling (channel) | 1 MB (bounded 256 × ~4 KB max message) |
| Latency budget impact | None — all I/O on background thread, try_send never blocks |
