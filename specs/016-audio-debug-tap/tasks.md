# Tasks: Audio Debug Tap

**Input**: Design documents from `specs/016-audio-debug-tap/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/debug_tap_api.md, quickstart.md

**Tests**: Included — 9 unit tests total, all in `crates/vox_core/src/audio/debug_tap.rs`. Tests are spread across T009 (US1), T011 (US2), T013 (US4) by user story for traceability — but all test the DebugAudioTap API directly (not pipeline wiring) and can be written together during T004 if preferred.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)
- Include exact file paths in descriptions

## Path Conventions

Three-crate Rust workspace:
- `crates/vox_core/` — Backend (audio, VAD, pipeline, config, state)
- `crates/vox/` — Binary entry point (main.rs)
- `crates/vox_ui/` — GPUI UI components (settings panel)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Promote dependency, add config enum, declare new module

- [X] T001 [P] Promote `hound = "3.5"` from `[dev-dependencies]` to `[dependencies]` in `crates/vox_core/Cargo.toml` (delete the dev-dependencies line). Verify: `cargo check -p vox_core --features cuda`
- [X] T002 [P] Add `DebugAudioLevel` enum (Off/Segments/Full with serde kebab-case, Default→Off) and `#[serde(default)] pub debug_audio: DebugAudioLevel` field to Settings struct (Advanced section) in `crates/vox_core/src/config.rs`. Update doc comment header to mention Debug category. Verify: existing tests pass, missing field in JSON defaults to Off
- [X] T003 Add `pub mod debug_tap;` declaration in `crates/vox_core/src/audio.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Create the core DebugAudioTap module and update segment channel type — MUST complete before any user story wiring

**CRITICAL**: No user story work can begin until this phase is complete

- [X] T004 Create `crates/vox_core/src/audio/debug_tap.rs` (NEW file, ~250 lines) containing: `DebugAudioMessage` enum (6 variants: StartSession, AppendRaw, AppendResampled, VadSegment, AsrInput, EndSession), `DebugAudioTap` struct (9 fields per data-model.md: level AtomicU8, sender SyncSender, session_counter AtomicU64, segment_counter AtomicU32, drop_count AtomicU64, write_error Arc\<AtomicBool\>, writer_handle Mutex\<Option\<JoinHandle\>\>, debug_audio_dir PathBuf, state_tx Mutex\<Option\<broadcast::Sender\>\>), all public API methods per contracts/debug_tap_api.md — note: `new(data_dir: &Path, initial_level: DebugAudioLevel) -> Self` takes initial_level NOT state_tx (state_tx is set later per-session via set_state_tx()). Private writer_thread() function: runs startup_cleanup() as first action before entering recv loop (SC-006: cleanup does not block the caller), then blocking recv loop with WAV I/O via hound::WavWriter\<BufWriter\<File\>\>, streaming + per-segment message handling, auto-session creation when VadSegment/AsrInput arrives without preceding StartSession (mid-recording toggle-on — per-segment only, no streaming writers opened). AppendRaw/AppendResampled in Idle state are dropped with debug-level log (no sample rate available), directory recreation on StartSession when debug_audio_dir is missing (FR-023), error handling with write_error flag + PipelineState::Error broadcast, cumulative_bytes tracking with periodic rescan every 50 writes, storage cap enforcement at 500 MB with 20% hysteresis to 400 MB. Private startup_cleanup() function (delete files older than 24h by Metadata::created(), enforce 500 MB cap). compute_dir_size() helper. All logging (FR-019 session summary, FR-020 drop count, FR-021 segment duration, FR-022 zero-segment warning). Channel: std::sync::mpsc::sync_channel(256). File naming: `session-{NNN}_{ISO-timestamp}_{tap-type}[-{segment-NNN}].wav`
- [X] T005 [P] Change segment channel type from `Vec<f32>` to `(Vec<f32>, u32)` across ~15 sites in `crates/vox_core/src/vad.rs` and `crates/vox_core/src/pipeline/orchestrator.rs`. All sends pass `0u32` as placeholder segment index. This task also adds `segment_index: u32` parameter to `process_segment()` signature (callee-before-caller: T007 later adds the `debug_tap` field and `tap_asr_input()` call inside process_segment). Sites per research.md R3: segment sends in run_vad_loop (lines 388, 440, 462, 469), run_passthrough_loop send (line 302), VAD test channel types + destructuring (lines 651, 691-694, 740, 784-793), Pipeline segment_rx field type (line 43), channel creation (line ~118), select! arm destructure (lines 163-164), drain loops destructure (lines 212-218, 241-247), process_segment signature add segment_index: u32 (line 295), orchestrator test calls to process_segment add 0u32. Verify: `cargo test -p vox_core --features cuda` — all existing VAD and orchestrator tests pass

**Checkpoint**: Core module compiles with all public API. Channel type updated with placeholder indices. Foundation ready for pipeline wiring.

---

## Phase 3: User Story 1 — Diagnose VAD Segment Boundaries (Priority: P1) MVP

**Goal**: Enable per-segment recording (vad_segment + asr_input WAV files) at Segments or Full level, with session/segment correlation

**Independent Test**: Set debug audio to Segments, speak a few sentences, verify per-utterance WAV files appear on disk with correct session IDs and matching segment indices across vad-segment and asr-input files

### Implementation for User Story 1

- [X] T006 [US1] Add `debug_tap: &Arc<DebugAudioTap>` parameter to `run_vad_loop` and `run_passthrough_loop` in `crates/vox_core/src/vad.rs`. Wire: start_session() before main loop, tap_vad_segment() when chunker emits segment (replace 0u32 placeholder with returned seg_idx in blocking_send), end_session() after drain loop. In passthrough mode: tap_vad_segment() on resampled buffer. Update VAD test fixtures to create a DebugAudioTap (Off level) and pass to functions
- [X] T007 [P] [US1] Add `debug_tap: Arc<DebugAudioTap>` field to Pipeline struct and Pipeline::new() in `crates/vox_core/src/pipeline/orchestrator.rs`. Note: `segment_index: u32` parameter on process_segment() and channel type destructuring were already added by T005 — this task adds the debug_tap field and the `self.debug_tap.tap_asr_input(segment_index, &padded_segment)` call after building padded_segment (~line 337). Update make_pipeline() test helper to accept and store a DebugAudioTap (Off level)
- [X] T008 [US1] Add `debug_tap: Arc<DebugAudioTap>` field to VoxState in `crates/vox_core/src/state.rs`. Create DebugAudioTap in run_app() (after VoxState creation) reading settings.debug_audio for initial level in `crates/vox/src/main.rs`. In start_recording(): pass Arc::clone(&debug_tap) to Pipeline::new() and to VAD thread spawn, call debug_tap.set_state_tx(state_tx.clone()). Call debug_tap.shutdown() on app exit
- [X] T009 [US1] Write unit tests (completed in T004) in `crates/vox_core/src/audio/debug_tap.rs`: test_wav_written_when_segments_level (enable Segments, send VadSegment, verify WAV exists with correct sample count), test_no_wav_when_off (default Off, send VadSegment, verify no files), test_session_correlation (send StartSession + VadSegment + AsrInput with same session_id, verify filenames share session ID and segment indices match), test_shutdown_idempotent (call shutdown() twice, verify no panic)

**Checkpoint**: At Segments level, each utterance produces 2 WAV files (vad-segment + asr-input). Files are correlated by session ID and segment index. Default Off produces no files.

---

## Phase 4: User Story 2 — Diagnose Raw Capture and Resampling Quality (Priority: P2)

**Goal**: Enable continuous raw microphone and post-resampler WAV recording at Full level

**Independent Test**: Set debug audio to Full, record 10 seconds, verify 2 continuous WAV files (raw + resampled) exist for the session alongside any per-segment files. Verify WAV headers are valid via hound::WavReader round-trip.

### Implementation for User Story 2

- [X] T010 [US2] Wire tap_raw() and tap_resampled() calls in run_vad_loop() and run_passthrough_loop() in `crates/vox_core/src/vad.rs`. In VAD mode: tap_raw() after ring buffer read (~line 346), tap_resampled() after resample (~line 363). In passthrough mode: tap_raw() incrementally during accumulation loop, chunk resampled buffer into 1-second (16000-sample) slices for tap_resampled()
- [X] T011 [US2] Write unit tests (completed in T004) in `crates/vox_core/src/audio/debug_tap.rs`: test_streaming_wav_session (enable Full, StartSession → AppendRaw × N → EndSession, verify single WAV file not N files, round-trip through hound::WavReader verifying valid RIFF header + correct sample rate + total sample count equals sum of appended), test_bounded_channel_drops_on_backpressure (fill channel with 256+ messages without reading, verify tap calls don't block and drop_count > 0)

**Checkpoint**: At Full level, each recording session produces 2 continuous streaming WAV files plus per-segment files. Streaming WAVs have valid RIFF headers after finalization.

---

## Phase 5: User Story 3 — Configure Debug Audio Level via Settings (Priority: P3)

**Goal**: Add Settings panel dropdown to change debug audio level at runtime without restart, with directory path display

**Independent Test**: Open settings, change dropdown from Off to Segments, start recording, verify WAV files appear. Change to Off mid-recording, verify streaming files are finalized.

### Implementation for User Story 3

- [X] T012 [US3] Add Debug Audio Recording dropdown and conditional directory path display in `crates/vox_ui/src/settings_panel.rs`. Add `debug_audio_select: Entity<Select>` field. In new(): create Select::new() with options ["Off", "Segments Only", "Full"] following Activation Mode pattern (lines 221-250). Callback: parse value to DebugAudioLevel, call update_settings(|s| s.debug_audio = level), then call debug_tap.set_level(level) via cx.global::\<VoxState\>(). Add .child(self.debug_audio_select.clone()) to render_advanced_section(). Conditionally show debug audio directory path below dropdown when level != Off using .when(): monospace-styled div showing path + sibling "Copy" clickable div using cx.write_to_clipboard(ClipboardItem::new_string(path)) — GPUI has no native text selection, follow the Copy button pattern from history_panel.rs

**Checkpoint**: Debug audio level changeable at runtime via settings. Directory path visible when active.

---

## Phase 6: User Story 4 — Automatic Storage Management (Priority: P4)

**Goal**: Verify automatic cleanup of old files and storage cap enforcement (implementation is in T004)

**Independent Test**: Create old debug audio files, restart app, verify files older than 24h are deleted and total storage stays under 500 MB cap. Point data_dir to read-only path, verify write error notification.

### Verification for User Story 4

- [X] T013 [US4] Write unit tests (completed in T004) in `crates/vox_core/src/audio/debug_tap.rs`: test_cleanup_deletes_old_files (create files with old creation time, construct DebugAudioTap, verify old files deleted), test_storage_cap_enforced (write files totaling > 500 MB, verify oldest deleted to reach < 400 MB), test_writer_error_sets_flag (point data_dir to read-only path, verify write_error flag is set and subsequent streaming taps are suppressed while per-segment taps still attempt writes)

**Checkpoint**: Storage management works: 24h cleanup, 500 MB cap with 20% hysteresis, write errors propagate to overlay.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Full validation across all user stories and security constraint preservation

- [X] T014 Run full test suite validation: `cargo test -p vox_core --features cuda -- debug_tap` (all 9 unit tests pass), then `cargo test -p vox_core --features cuda` (all existing tests including SC-004/SC-005 security tests pass with debug_audio defaulting to Off). Verify no warnings introduced.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately. T001 and T002 are parallel (different files).
- **Foundational (Phase 2)**: Depends on Setup completion. T004 and T005 are parallel (different files: debug_tap.rs vs vad.rs/orchestrator.rs). BLOCKS all user stories.
- **US1 (Phase 3)**: Depends on Foundational. T006 and T007 are parallel (vad.rs vs orchestrator.rs). T008 depends on T006+T007. T009 depends on T004.
- **US2 (Phase 4)**: Depends on US1 (same file vad.rs, additive tap calls). T010 depends on T006. T011 depends on T004.
- **US3 (Phase 5)**: Depends on US1 T008 (VoxState has debug_tap). Can run in parallel with US2 (different file: settings_panel.rs).
- **US4 (Phase 6)**: Implementation complete in T004. Tests depend on T004 only — can run in parallel with US1/US2/US3.
- **Polish (Phase 7)**: Depends on all previous phases.

### User Story Dependencies

- **US1 (P1)**: Can start after Foundational (Phase 2) — no dependencies on other stories
- **US2 (P2)**: Depends on US1 T006 (vad.rs already has debug_tap parameter and session lifecycle)
- **US3 (P3)**: Depends on US1 T008 (init wiring provides VoxState.debug_tap). Can parallel with US2
- **US4 (P4)**: Tests only — can parallel with any story after Phase 2

### Within Each User Story

- Pipeline function signatures updated before caller wiring
- VAD thread wiring before orchestrator wiring (segment index flows VAD → orchestrator)
- Init wiring after both VAD and orchestrator signatures updated
- Tests after corresponding implementation

### Parallel Opportunities

- **Phase 1**: T001 ‖ T002 (different files)
- **Phase 2**: T004 ‖ T005 (debug_tap.rs ‖ vad.rs+orchestrator.rs)
- **Phase 3**: T006 ‖ T007 (vad.rs ‖ orchestrator.rs)
- **Cross-story**: US3 (T012) ‖ US2 (T010-T011) after US1 T008 completes
- **Cross-story**: US4 (T013) ‖ US1/US2/US3 after Phase 2 completes

---

## Parallel Example: Phase 2 (Foundational)

```
# These two tasks modify different files and can run concurrently:
Task T004: "Create DebugAudioTap module in crates/vox_core/src/audio/debug_tap.rs"
Task T005: "Change segment channel type in crates/vox_core/src/vad.rs and orchestrator.rs"
```

## Parallel Example: User Story 1

```
# These two tasks modify different files and can run concurrently:
Task T006: "Wire VAD tap calls in crates/vox_core/src/vad.rs"
Task T007: "Wire orchestrator tap calls in crates/vox_core/src/pipeline/orchestrator.rs"

# Then sequentially:
Task T008: "Init wiring in crates/vox_core/src/state.rs and crates/vox/src/main.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (3 tasks, ~25 lines)
2. Complete Phase 2: Foundational (2 tasks, ~265 lines — largest phase)
3. Complete Phase 3: User Story 1 (4 tasks, ~50 lines)
4. **STOP and VALIDATE**: Enable Segments, record speech, verify per-utterance WAV files appear with correct correlation
5. Per-segment debugging is functional — the most common debugging scenario works

### Incremental Delivery

1. Setup + Foundational → Core module ready
2. US1 → Per-segment WAV recording works → **MVP complete** (diagnose VAD boundaries)
3. US2 → Streaming raw/resampled recording works (diagnose mic/resampler issues)
4. US3 → Settings UI dropdown works (runtime toggling without restart)
5. US4 → Storage management verified (cleanup + cap + error notification)
6. Polish → Full validation, security test preservation

### Key Implementation Notes

- **Largest task**: T004 (~250 lines, one file). Contains the entire DebugAudioTap struct, writer thread, and cleanup logic. All other tasks are ≤35 lines.
- **Most mechanical task**: T005 (~15 sites). Pure type change from `Vec<f32>` to `(Vec<f32>, u32)` with 0u32 placeholder. See research.md R3 for exact line numbers.
- **Callee-before-caller**: T006/T007 update function signatures, T008 updates the callers. Standard Rust refactoring pattern.
- **All tests in one file**: The 9 unit tests (T009, T011, T013) are all in `crates/vox_core/src/audio/debug_tap.rs`. During implementation they can be written together.

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [Story] label maps task to specific user story for traceability
- Each user story is independently testable after its phase completes
- All 9 unit tests run unconditionally (Constitution Principle VIII)
- Total: ~375 lines across 8 files (1 new + 7 modified)
- Binary impact: ~15-20 KB (0.1% of 15 MB budget)
