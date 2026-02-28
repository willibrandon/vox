# Quickstart: Audio Debug Tap Implementation

**Feature**: 016-audio-debug-tap
**Date**: 2026-02-27

## Implementation Order

Follow this order strictly. Each step compiles and tests independently.

### Step 1: Promote hound dependency (2 lines)

**File**: `crates/vox_core/Cargo.toml`

Move `hound = "3.5"` from `[dev-dependencies]` to `[dependencies]`. Delete the line from dev-dependencies.

**Verify**: `cargo check -p vox_core --features cuda` compiles.

### Step 2: Add DebugAudioLevel enum + Settings field (~20 lines)

**File**: `crates/vox_core/src/config.rs`

1. Define `DebugAudioLevel` enum with `Serialize`, `Deserialize`, `Clone`, `Copy`, `Debug`, `Default`, `PartialEq`, `Eq`. Use `#[serde(rename_all = "kebab-case")]` and `#[default] Off`.
2. Add `#[serde(default)] pub debug_audio: DebugAudioLevel` to the `Settings` struct in the Advanced section.
3. Update the doc comment header to mention Debug (1 field) as an 8th category.

**Verify**: `cargo test -p vox_core --features cuda` — existing tests still pass. Settings round-trip with missing field defaults to Off.

### Step 3: Create DebugAudioTap module (~250 lines including writer + tests)

**File**: `crates/vox_core/src/audio/debug_tap.rs` (NEW)
**Also modify**: `crates/vox_core/src/audio.rs` — add `pub mod debug_tap;`

This is the largest step. Contains:
- `DebugAudioMessage` enum (6 variants)
- `DebugAudioTap` struct with all public methods
- `writer_thread()` private function (WAV I/O, cleanup, error handling, auto-session on orphaned messages)
- `startup_cleanup()` private function (age-based + size-based file deletion) — called by writer thread as first action, not in `new()`
- `compute_dir_size()` helper
- Unit tests (9 tests — all defined here, though some exercise Full-level flows wired in later steps; they test the DebugAudioTap API directly, not pipeline integration)

**Key implementation details**:
- Channel: `std::sync::mpsc::sync_channel::<DebugAudioMessage>(256)`
- Level check: `self.level.load(Ordering::Relaxed)` — returns 0/1/2
- try_send: `self.sender.try_send(msg)` — on `Err(TrySendError::Full(_))`, increment `drop_count`
- Writer: `hound::WavWriter::new(BufWriter::new(File::create(path)?), spec)` for streaming taps
- Cleanup: runs on writer thread before recv loop (async from caller), uses `std::fs::Metadata::created()` for file age, `std::fs::metadata().len()` for size
- Auto-session: writer thread auto-creates a session when receiving VadSegment/AsrInput without preceding StartSession (mid-recording toggle-on scenario). AppendRaw/AppendResampled in Idle state are dropped (no streaming writers, no sample rate) — logged once at debug level. Streaming taps resume on next proper recording session.

**Verify**: `cargo test -p vox_core --features cuda -- debug_tap` — all 9 unit tests pass.

### Step 4: Pipeline integration — segment channel type change (~15 sites)

**Files**: `crates/vox_core/src/vad.rs`, `crates/vox_core/src/pipeline/orchestrator.rs`

Change segment channel type from `Vec<f32>` to `(Vec<f32>, u32)`. This is a mechanical change across ~15 sites (see research.md R3 for full site list). This step also adds `segment_index: u32` parameter to `process_segment()` signature (callee-before-caller pattern — Step 6 later adds the `debug_tap` field and `tap_asr_input()` call inside `process_segment()`).

For now, all sends pass `0u32` as the segment index (debug tap not yet wired). This step ensures the type change compiles and existing tests pass before adding tap calls.

**Verify**: `cargo test -p vox_core --features cuda` — all existing VAD and orchestrator tests pass with updated channel type.

### Step 5: Wire tap calls into VAD thread (~35 lines)

**File**: `crates/vox_core/src/vad.rs`

Add `debug_tap: &Arc<DebugAudioTap>` parameter to `run_vad_loop` and `run_passthrough_loop`.

Insert tap calls at verified locations:
- `start_session()` before main loop
- `tap_raw()` after ring buffer read (vad.rs ~line 346)
- `tap_resampled()` after resample (vad.rs ~line 363)
- `tap_vad_segment()` when chunker emits segment — use returned `seg_idx` in `blocking_send`
- `end_session()` after drain loop

For passthrough mode:
- `tap_raw()` incrementally during accumulation
- Chunk resampled buffer into 1-second slices for `tap_resampled()`

**Verify**: Compile check. Unit tests need `Arc<DebugAudioTap>` in test setup.

### Step 6: Wire tap calls into orchestrator (~15 lines)

**File**: `crates/vox_core/src/pipeline/orchestrator.rs`

1. Add `debug_tap: Arc<DebugAudioTap>` to `Pipeline` struct and `Pipeline::new()`.
2. In `process_segment()` (signature already updated in Step 4 with `segment_index: u32`): add `self.debug_tap.tap_asr_input(segment_index, &padded_segment);` after building `padded_segment` (~line 337).
3. Update `make_pipeline()` test helper to accept and store a `DebugAudioTap` (Off level).

**Verify**: `cargo test -p vox_core --features cuda` — all orchestrator tests pass.

### Step 7: Initialization wiring (~20 lines)

**Files**: `crates/vox_core/src/state.rs`, `crates/vox/src/main.rs`

1. Add `debug_tap: Arc<DebugAudioTap>` field to `VoxState`.
2. Create `DebugAudioTap` in `VoxState::new()` (or `run_app()`) — read `settings.debug_audio` for initial level.
3. In `start_recording()`: pass `Arc::clone(&debug_tap)` to `Pipeline::new()` and to VAD thread spawn.
4. In `start_recording()`: call `debug_tap.set_state_tx(state_tx.clone())` so error notifications reach the overlay.

**Verify**: `cargo run -p vox --features vox_core/cuda` — app launches, debug audio defaults to Off, no files written.

### Step 8: Settings panel dropdown (~25 lines)

**File**: `crates/vox_ui/src/settings_panel.rs`

1. Add `debug_audio_select: Entity<Select>` field to settings panel struct.
2. In `new()`: create `Select::new()` with options `["Off", "Segments Only", "Full"]`, following the Activation Mode pattern (lines 221-250).
3. In callback: parse value, call `update_settings(|s| s.debug_audio = level)`, then call `debug_tap.set_level(level)` via `cx.global::<VoxState>()`.
4. Add `.child(self.debug_audio_select.clone())` to `render_advanced_section()`.
5. Conditionally show debug audio directory path below the dropdown when level != Off, using `.when()`. Display path in a monospace-styled div with a sibling "Copy" clickable div that calls `cx.write_to_clipboard(ClipboardItem::new_string(path))` (GPUI has no native text selection — follow the established Copy button pattern from history_panel.rs).

**Verify**: Open settings, change dropdown, verify files appear/stop appearing on next recording.

## Build & Test Commands

```bash
# Full test suite
cargo test -p vox_core --features cuda

# Debug tap tests only
cargo test -p vox_core --features cuda -- debug_tap

# Single test
cargo test -p vox_core --features cuda -- test_streaming_wav_session --nocapture

# Run the app
cargo run -p vox --features vox_core/cuda
```

## Files Modified (Summary)

| File | Change | Lines |
|------|--------|-------|
| `crates/vox_core/Cargo.toml` | Move hound to deps | ~2 |
| `crates/vox_core/src/config.rs` | Add enum + field | ~20 |
| `crates/vox_core/src/audio.rs` | Add `pub mod debug_tap;` | 1 |
| `crates/vox_core/src/audio/debug_tap.rs` | **NEW** — entire module | ~250 |
| `crates/vox_core/src/vad.rs` | Add tap calls + channel type | ~35 |
| `crates/vox_core/src/pipeline/orchestrator.rs` | Add debug_tap field + channel type | ~15 |
| `crates/vox_core/src/state.rs` | Add debug_tap to VoxState | ~10 |
| `crates/vox/src/main.rs` | Create + wire DebugAudioTap | ~15 |
| `crates/vox_ui/src/settings_panel.rs` | Add dropdown + path display + Copy button | ~25 |
| **Total** | 1 new + 7 modified | **~375** |
