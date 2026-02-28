# Research: Audio Debug Tap

**Feature**: 016-audio-debug-tap
**Date**: 2026-02-27
**Status**: Complete — all decisions resolved, no NEEDS CLARIFICATION remaining

## Research Tasks

### R1: hound crate promotion from dev-dependencies

**Decision**: Move `hound = "3.5"` from `[dev-dependencies]` to `[dependencies]` in `crates/vox_core/Cargo.toml`.

**Rationale**: hound is currently at line 65 in `[dev-dependencies]` (version 3.5). The debug tap writer thread needs `hound::WavWriter` in production code. Per constitution Principle XI, this must be a required dependency, not optional.

**Alternatives considered**:
- Raw WAV writing without hound (~80 lines of RIFF header management). Rejected: hound is 3.5 KB compiled, handles endianness, header finalization, and seek-back for data size patching. Re-implementing this is error-prone and saves negligible binary size.
- Using a different WAV crate (e.g., wav, wavers). Rejected: hound is already a dev-dependency, proven in existing tests, and the most mature Rust WAV crate.

**Impact**: hound adds ~8 KB to binary. Total feature binary impact: ~15-20 KB (0.1% of 15 MB budget).

---

### R2: Channel type — std::sync::mpsc vs tokio::sync::mpsc

**Decision**: Use `std::sync::mpsc::sync_channel(256)` for the debug tap channel.

**Rationale**: The producers are:
1. VAD thread — a plain `std::thread` (confirmed in `vad.rs` lines 319-328)
2. Orchestrator — a Tokio async task (confirmed in `orchestrator.rs` line 163, inside `tokio::select!`)

`SyncSender::try_send` is non-blocking from both contexts. The writer thread is a plain `std::thread::spawn` — it calls `receiver.recv()` in a blocking loop (no Tokio runtime needed). The existing segment channel between VAD and orchestrator uses `tokio::sync::mpsc` because the orchestrator needs it inside `tokio::select!`. The debug tap channel has no such requirement.

**Alternatives considered**:
- `tokio::sync::mpsc` — would require a Tokio runtime in the writer thread or `blocking_recv()` which doesn't exist. `try_send` exists but the writer would need `tokio::select!` or a runtime to block on receive. Unnecessary complexity.
- `crossbeam::channel` — adds a new dependency for no benefit over std.
- `flume` — same argument as crossbeam.

---

### R3: Segment channel type change blast radius

**Decision**: Change segment channel from `mpsc::Sender<Vec<f32>>` to `mpsc::Sender<(Vec<f32>, u32)>` to carry the segment index for ASR input correlation.

**Rationale**: The segment index must travel from the VAD thread (where `tap_vad_segment` is called) to the orchestrator (where `tap_asr_input` is called). The simplest approach is to bundle it with the segment audio data in the existing channel.

**Verified impact sites** (confirmed via codebase exploration):

| Site | File | Line(s) | Change |
|------|------|---------|--------|
| `run_vad_loop` signature | vad.rs | 319-328 | Add `debug_tap` parameter |
| Segment sends (VAD main loop) | vad.rs | 388 | `blocking_send((segment, seg_idx))` |
| Segment sends (VAD drain) | vad.rs | 440, 462, 469 | Same pattern |
| `run_passthrough_loop` send | vad.rs | 302 | `blocking_send((audio_buffer, seg_idx))` |
| Test `test_vad_end_to_end` | vad.rs | 651, 691-694 | Channel type + destructure |
| Test `test_vad_multiple_utterances` | vad.rs | 740, 784-793 | Channel type + destructure |
| `Pipeline` struct `segment_rx` | orchestrator.rs | 43 | Type change to `(Vec<f32>, u32)` |
| Channel creation | orchestrator.rs | ~118 | Type annotation update |
| `select!` arm | orchestrator.rs | 163-164 | Destructure `(segment, seg_idx)` |
| Drain loops | orchestrator.rs | 212-218, 241-247 | Destructure `(segment, seg_idx)` |
| `process_segment` signature | orchestrator.rs | 295 | Add `segment_index: u32` parameter |
| Orchestrator tests | orchestrator.rs | ~769+ | Add `0u32` parameter to `process_segment` calls |

Total: ~15 sites across 2 files. All mechanical — no logic changes.

**Alternatives considered**:
- Separate channel for segment indices. Rejected: adds synchronization complexity and a second channel to manage.
- Store segment index in DebugAudioTap and have orchestrator read it. Rejected: race condition between VAD thread incrementing and orchestrator reading.

---

### R4: WAV writer BufWriter + seek correctness

**Decision**: Use `hound::WavWriter::create(BufWriter::new(File::create(path)?))` for streaming taps.

**Rationale**: hound's `WavWriter::finalize()` seeks back to byte 4 to patch the RIFF chunk size. `BufWriter::seek()` flushes its internal buffer before seeking — this is guaranteed by the `std::io::BufWriter` implementation of `Seek`. The data-then-seek-then-patch sequence is correct. Verified by test_streaming_wav_session which round-trips through `hound::WavReader`.

**Alternatives considered**:
- Direct `File` without `BufWriter`. Rejected: streaming taps write ~100 times/second. Without buffering, each write is a syscall. `BufWriter` batches to ~8KB writes (default buffer size).
- Manual WAV header writing with raw `File`. Rejected: reimplements hound's finalization logic, error-prone.

---

### R5: File creation time reliability

**Decision**: Use `std::fs::Metadata::created()` for cleanup file age sorting. Scoped to Windows (NTFS) and macOS (APFS) only.

**Rationale**: Both target platforms (Windows NTFS, macOS APFS) support reliable file creation time via `Metadata::created()`. Linux ext4 requires kernel 4.11+ with `statx(2)` — `Metadata::created()` returns `Err` on older kernels. Since Vox targets Windows + macOS only, this is not a concern.

**Alternatives considered**:
- Modification time (`Metadata::modified()`). Rejected: unreliable on Windows due to antivirus tools and indexing services touching files, updating mtime.
- Encoding timestamp in filename and parsing for cleanup. Viable but adds parsing complexity when `created()` is reliable on both target platforms.

---

### R6: Storage cap enforcement strategy

**Decision**: In-memory `cumulative_bytes: u64` counter in writer thread, full directory scan on startup + every 50 writes.

**Rationale**: O(1) per-write tracking. Full scan on startup initializes the counter. Periodic re-scan (every 50 writes) corrects for external file deletions. When `cumulative_bytes > 500 MB`: delete oldest files (by creation time) until under 400 MB (20% hysteresis prevents delete-write-delete thrashing).

**Alternatives considered**:
- Full directory scan on every write. Rejected: O(n) per write, degrades as file count grows.
- No periodic re-scan (only startup). Rejected: counter drifts if user manually deletes files.
- Database tracking of files. Rejected: overkill for a debug feature.

---

### R7: Writer thread error notification path

**Decision**: First error per session broadcasts `PipelineState::Error { message }` via the existing `state_tx: broadcast::Sender<PipelineState>`. Subsequent errors log at `warn` level only.

**Rationale**: The `state_tx` broadcast channel already exists in the pipeline (confirmed in `orchestrator.rs` line 40). The overlay already subscribes to `PipelineState` updates and renders error messages (confirmed in `main.rs` state forwarding at lines 241-442). Using the existing error path avoids any new notification infrastructure.

**Alternatives considered**:
- Separate error channel for debug audio. Rejected: overlay already handles PipelineState::Error.
- Log-only errors (no UI). Rejected: FR-013 requires overlay notification on write errors.
- Error on every write failure. Rejected: would spam the overlay during sustained disk issues.

---

### R8: Settings panel integration pattern

**Decision**: Follow the Activation Mode Select pattern (settings_panel.rs lines 221-250) — extract enum to string, create `Select::new()` with options, parse back in callback, call `update_settings()`.

**Rationale**: Three existing dropdown patterns in the settings panel (Activation Mode, Theme, Overlay Position) all follow the same structure. The Debug Audio dropdown is a simple 3-variant enum, matching Pattern A (Activation Mode) exactly.

**Additional requirement**: FR-017 specifies displaying the debug audio directory path when debug audio is not Off, with a Copy button. GPUI has no native text selection in divs and no read-only text input primitive. The established Vox pattern (used in history_panel.rs and overlay_hud.rs) is a monospace-styled div showing the path with a sibling "Copy" clickable div that calls `cx.write_to_clipboard(ClipboardItem::new_string(path))`. Placed below the dropdown, conditionally shown via `.when()`.

---

### R9: VoxState integration and lifecycle

**Decision**: Create `DebugAudioTap` once in `run_app()` after VoxState creation. Store as `Arc<DebugAudioTap>` in `VoxState`. Swap `state_tx` per recording session via `set_state_tx()`.

**Rationale**: The DebugAudioTap must be a singleton for the app lifetime because:
- Startup cleanup should run once, not per recording session
- The writer thread should persist between recordings (avoid thread spawn/join overhead per session)
- Session lifecycle is managed via `start_session()`/`end_session()` messages, not by creating/destroying the tap

The `state_tx` broadcast channel is created per-recording session in `start_recording()`. Since it changes each session, the `DebugAudioTap` stores it as `Mutex<Option<broadcast::Sender<PipelineState>>>`, updated via `set_state_tx()` at each `start_recording()` call. The writer thread uses this sender for error notifications (FR-013). Between recording sessions, errors are log-only (no broadcast receiver exists).

**Integration pattern**:
1. Create `DebugAudioTap::new(data_dir, initial_level)` in `run_app()` after VoxState is set as global
2. Store as `Arc<DebugAudioTap>` in `VoxState`
3. In `start_recording()`: call `debug_tap.set_state_tx(state_tx.clone())`, pass `Arc::clone(&debug_tap)` to `Pipeline::new()` and VAD thread spawn
4. Call `debug_tap.shutdown()` on app exit

**Alternatives considered**:
- Creating `DebugAudioTap` per-session in `start_recording()`. Rejected: duplicates startup cleanup, spawns/joins a thread per session, loses cross-session drop count.
- Single long-lived `broadcast::channel` for errors. Rejected: overlay subscribes per-session, would need a second subscription for debug audio errors.
- Storing `state_tx` in `DebugAudioTap::new()`. Rejected: `state_tx` doesn't exist at construction time (created per-session).

---

### R10: Passthrough mode tap integration

**Decision**: Tap raw samples incrementally during accumulation (same `AppendRaw` pattern as VAD mode). Chunk resampled buffer into 1-second slices for `AppendResampled`.

**Rationale**: `run_passthrough_loop` (vad.rs lines 238-310) accumulates raw samples in `raw_buffer` (line 265) and resamples once at end (lines 290-294). Tapping raw samples incrementally during the accumulation loop avoids memory doubling. The final resampled buffer can be large for long recordings — chunking into 16000-sample (1-second) slices keeps individual messages under the channel bound.

**Verified**: The accumulation loop is at lines 251-266 (main) and 269-282 (drain). The single resample call is at lines 290-294. The single segment send is at line 302.
