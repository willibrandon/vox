# Tasks: Pipeline Orchestration

**Input**: Design documents from `/specs/007-pipeline-orchestration/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/

**Tests**: Integration tests are included — explicitly defined in the feature specification (Testing Requirements section). Per Constitution Principle VIII, all tests run unconditionally (no `#[ignore]`). Unit tests are inline with implementation tasks.

**Organization**: Tasks are grouped by user story. US3 (State Broadcasting) and US4 (Transcript History) are architecturally embedded in US1's Pipeline orchestrator — their data types live in the foundational phase, their runtime behavior is part of `Pipeline::run()`. Separate phases exist for subscriber edge cases (US3) and persistence verification (US4).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story this task belongs to (US1, US2, US3, US4)
- All source paths are under `crates/vox_core/src/` unless noted otherwise

---

## Phase 1: Setup

**Purpose**: Create the pipeline module directory structure. No functional changes.

- [ ] T001 Create `pipeline/` directory under `crates/vox_core/src/` with empty submodule files (`state.rs`, `orchestrator.rs`, `controller.rs`, `transcript.rs`). Update existing `crates/vox_core/src/pipeline.rs` from empty stub to module root declaring `pub mod state; pub mod orchestrator; pub mod controller; pub mod transcript;`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Data types, data-layer implementations, and modifications to existing modules that ALL user stories depend on. Pipeline::new() requires PipelineState, DictionaryCache, TranscriptStore, and ActivationMode to exist.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [ ] T002 [P] Implement `PipelineState` enum (Idle, Listening, Processing{raw_text: Option\<String\>}, Injecting{polished_text: String}, Error{message: String}) with derives (Clone, Debug, PartialEq) and `PipelineCommand` enum (Stop) with derive (Debug) in `crates/vox_core/src/pipeline/state.rs` per contracts/pipeline.md
- [ ] T003 [P] Implement `DictionaryEntry` struct and `DictionaryCache` struct in `crates/vox_core/src/dictionary.rs` per contracts/dictionary.md and research R-004. Methods: `load(db_path)` (create table if needed, build HashMap + phrase Vec + hints Vec from SQLite), `empty()`, `apply_substitutions(text)` (two-pass: phrase replacement longest-first then single-word HashMap lookup; empty replacement removes matched text; all-empty result returns empty string), `top_hints(n)` (format "term → replacement" string), `reload(db_path)`, `len()`, `is_empty()`. Include inline unit tests for: two-pass substitution ordering, case-insensitive matching, empty replacement removal, phrase longest-first priority, round-trip load from temp SQLite DB
- [ ] T004 [P] Implement `TranscriptEntry` struct (id: String, raw_text, polished_text, target_app, duration_ms: u32, latency_ms: u32, created_at: String) and `TranscriptStore` struct using `parking_lot::Mutex<rusqlite::Connection>` in `crates/vox_core/src/pipeline/transcript.rs` per contracts/transcript.md and research R-005. Methods: `open(db_path)` (create table + index if needed, auto-prune records >30 days), `save(entry)`, `list(limit, offset)` (newest first), `prune_older_than(days)` (return count deleted), `count()`. Use `uuid::Uuid::new_v4().to_string()` for IDs, ISO 8601 strings for timestamps
- [ ] T005 [P] Implement `ActivationMode` enum (HoldToTalk, Toggle, HandsFree) with derives (Clone, Debug, PartialEq, Serialize, Deserialize) and `impl Default` returning HoldToTalk in `crates/vox_core/src/pipeline/controller.rs` per contracts/pipeline.md. Place enum at top of file — PipelineController implementation comes in Phase 4
- [ ] T006 [P] Add `take_consumer(&mut self) -> Option<HeapCons<f32>>` method to `AudioCapture` in `crates/vox_core/src/audio/capture.rs` per research R-002. Takes ownership of the ring buffer consumer via `Option::take()`. Returns None if already taken. Add `///` doc comment explaining this splits the consumer from the producer for cross-thread use
- [ ] T007 [P] Change `segment_tx.try_send(segment)` to `segment_tx.blocking_send(segment).ok()` at both call sites (main emit ~line 287 and flush ~line 296) in `run_vad_loop()` in `crates/vox_core/src/vad.rs` per research R-008 and FR-017. `blocking_send` blocks the VAD thread until channel has space, guaranteeing no segment drops under backpressure. The `.ok()` discards the SendError that only occurs if the receiver is dropped (normal shutdown)
- [ ] T008 [P] Implement `get_focused_app_name_impl() -> String` for Windows in `crates/vox_core/src/injector/windows.rs` per contracts/focused_app.md and research R-003. Flow: `GetForegroundWindow()` → `GetWindowThreadProcessId()` → `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → `QueryFullProcessImageNameW()` → extract filename stem. Return `"Unknown"` on any failure. All Win32 features already in Cargo.toml
- [ ] T009 [P] Implement `get_focused_app_name_impl() -> String` for macOS in `crates/vox_core/src/injector/macos.rs` per contracts/focused_app.md and research R-003. Flow: `NSWorkspace.shared().frontmostApplication()?.localizedName()`. Return `"Unknown"` on any failure. objc2 already in Cargo.toml
- [ ] T010 Add public `get_focused_app_name() -> String` function to `crates/vox_core/src/injector.rs` per contracts/focused_app.md. Dispatches to platform impl via `#[cfg(target_os = "windows")]` / `#[cfg(target_os = "macos")]` calling the `get_focused_app_name_impl()` from T008/T009. Add `///` doc comment per contract (depends on T008, T009)

**Checkpoint**: All data types, data stores, and module modifications ready — user story implementation can begin

---

## Phase 3: User Story 1 — End-to-End Dictation (Priority: P1) 🎯 MVP

**Goal**: Activate dictation, speak naturally, see polished text appear in the focused application. Voice commands execute as actions, not typed text. Dictionary substitutions applied before LLM. State broadcasting (US3) and transcript saving (US4) are integral to the run loop and implemented here.

**Independent Test**: Hold hotkey, speak "hello world", verify "Hello, world." appears in a text editor. Speak "delete that", verify deletion executes.

### Implementation for User Story 1

- [ ] T011 [US1] Implement `Pipeline` struct with fields (asr: AsrEngine, llm: PostProcessor, dictionary: DictionaryCache, transcript_store: TranscriptStore, state_tx: broadcast::Sender\<PipelineState\>, command_rx: mpsc::Receiver\<PipelineCommand\>, stop_flag: Arc\<AtomicBool\>, segment_rx: Option\<mpsc::Receiver\<Vec\<f32\>\>\>, vad_handle: Option\<JoinHandle\<Result\<()\>\>\>, vad_model_path: PathBuf, vad_config: VadConfig) and `new()` constructor storing all fields in `crates/vox_core/src/pipeline/orchestrator.rs` per contracts/pipeline.md
- [ ] T012 [US1] Implement `Pipeline::start(&mut self, consumer: HeapCons<f32>, native_sample_rate: u32) -> Result<()>` in `crates/vox_core/src/pipeline/orchestrator.rs` per research R-001/R-002/R-008. Create mpsc segment channel (capacity 32). Clone stop_flag. Spawn std::thread that creates SileroVad, AudioResampler (if rate != 16000), VadStateMachine, SpeechChunker on-thread, then calls run_vad_loop(). Store segment_rx and vad_handle. Broadcast PipelineState::Listening
- [ ] T013 [US1] Implement `Pipeline::run(&mut self) -> Result<()>` in `crates/vox_core/src/pipeline/orchestrator.rs` per research R-010. Main loop: `tokio::select!` on `segment_rx.recv()` → call process_segment, `command_rx.recv()` → match PipelineCommand::Stop → break (current segment already complete per FR-018), `else` → break (both channels closed). After loop: set stop_flag, join vad_handle, broadcast Idle
- [ ] T014 [US1] Implement `process_segment(&mut self, segment: Vec<f32>) -> Result<()>` in `crates/vox_core/src/pipeline/orchestrator.rs` per spec FR-001/FR-010/FR-011/FR-012a. Flow: (1) record start_time via Instant::now(), (2) silent pre-check (call T015 helper), (3) broadcast Processing{raw_text: None}, (4) spawn_blocking ASR transcribe, (5) if empty text → broadcast Listening → return, (6) broadcast Processing{raw_text: Some(raw)}, (7) dictionary.apply_substitutions, (8) if substituted empty → broadcast Listening → return, (9) get_focused_app_name(), (10) dictionary.top_hints(50), (11) spawn_blocking LLM process, (12) match ProcessorOutput::Text → broadcast Injecting{polished} → inject_text → save transcript (T016), match ProcessorOutput::Command → execute_command (no transcript per FR-016), (13) broadcast Listening
- [ ] T015 [US1] Implement silent segment pre-check helper `is_silent(segment: &[f32]) -> bool` in `crates/vox_core/src/pipeline/orchestrator.rs` per spec FR-012. Compute RMS energy: `sqrt(sum(s*s) / len)`. Return true if RMS < 1e-3 (0.001). Called at top of process_segment — if silent, broadcast Listening and return early (skip ASR/LLM/injection). Include inline unit test with all-zero samples and known-energy samples
- [ ] T016 [US1] Implement transcript saving in process_segment after successful text injection in `crates/vox_core/src/pipeline/orchestrator.rs` per spec FR-014/FR-016 and data-model. Compute: duration_ms = `(segment.len() as u32) * 1000 / 16000` (multiply first to avoid integer truncation for sub-second segments), latency_ms = start_time.elapsed().as_millis() as u32. Create TranscriptEntry with Uuid::new_v4().to_string(), raw_text, polished_text, get_focused_app_name() result, duration_ms, latency_ms, Utc::now() ISO 8601. Call self.transcript_store.save(&entry). Skip entirely for ProcessorOutput::Command
- [ ] T017 [US1] Implement per-segment error recovery in `Pipeline::run()` in `crates/vox_core/src/pipeline/orchestrator.rs` per spec FR-019 and research R-009. Wrap process_segment() in match: Ok(()) → continue, Err(e) → broadcast Error{message: e.to_string()} → broadcast Listening (pipeline continues). Handle spawn_blocking JoinError identically (R-009 table). For VAD thread unexpected exit: when segment_rx returns None outside of Stop command, drain any remaining buffered segments, broadcast Error("VAD processing thread exited unexpectedly"), transition to Idle
- [ ] T018 [US1] Implement `Pipeline::subscribe(&self) -> broadcast::Receiver<PipelineState>` and `Pipeline::state(&self) -> PipelineState` in `crates/vox_core/src/pipeline/orchestrator.rs` per contracts/pipeline.md. subscribe() calls self.state_tx.subscribe(). state() tracks latest broadcast state in an internal field updated on each broadcast call
- [ ] T019 [US1] Wire pipeline module root in `crates/vox_core/src/pipeline.rs` — ensure submodule declarations (`pub mod state; pub mod orchestrator; pub mod controller; pub mod transcript;`) and add re-exports of key public types: `pub use state::{PipelineState, PipelineCommand}; pub use orchestrator::Pipeline; pub use controller::{PipelineController, ActivationMode}; pub use transcript::{TranscriptEntry, TranscriptStore};`

**Checkpoint**: Full pipeline operational — audio → VAD → ASR → dictionary → LLM → inject/execute works end-to-end. State transitions broadcast to subscribers. Transcripts saved after injection. US1, US3, and US4 acceptance criteria satisfied.

---

## Phase 4: User Story 2 — Activation Modes (Priority: P2)

**Goal**: Three mutually exclusive activation modes (hold-to-talk, toggle, hands-free) controlling how hotkey events translate to pipeline start/stop commands via the mpsc command channel.

**Independent Test**: Configure each mode, verify hotkey press/release/double-press behavior matches mode description. For hands-free, verify multiple utterances produce separate injections without manual intervention.

### Implementation for User Story 2

- [ ] T020 [US2] Implement `PipelineController` struct (command_tx: mpsc::Sender\<PipelineCommand\>, mode: ActivationMode, is_active: bool, last_press_time: Option\<Instant\>) and `new(command_tx) -> Self` initializing mode to HoldToTalk default, is_active false, last_press_time None in `crates/vox_core/src/pipeline/controller.rs` per contracts/pipeline.md
- [ ] T021 [US2] Implement `on_hotkey_press(&mut self)` in `crates/vox_core/src/pipeline/controller.rs` per research R-006. Match on self.mode: HoldToTalk → set is_active=true (caller responsible for starting pipeline externally), Toggle → if is_active send Stop + set false, else set is_active=true (caller starts pipeline), HandsFree → call double-press detection (T023), if double-press detected → set is_active=true, if already active and single press → send Stop + set false
- [ ] T022 [US2] Implement `on_hotkey_release(&mut self)` in `crates/vox_core/src/pipeline/controller.rs` per research R-006. Match on self.mode: HoldToTalk → if is_active, send PipelineCommand::Stop via command_tx, set is_active=false. Toggle/HandsFree → no-op
- [ ] T023 [US2] Implement double-press detection for HandsFree mode in `crates/vox_core/src/pipeline/controller.rs` per spec FR-006 and research R-006. On each press, check `last_press_time`: if Some and elapsed < 300ms (exclusive — exactly 300ms is single press) → double-press detected, clear last_press_time. Otherwise → single press, set last_press_time = Some(Instant::now()). Include inline unit test: presses at 200ms apart = double, presses at 300ms apart = two singles, presses at 301ms apart = two singles
- [ ] T024 [US2] Implement `force_stop(&mut self)` (send PipelineCommand::Stop unconditionally, set is_active=false), `is_active(&self) -> bool`, `mode(&self) -> ActivationMode` in `crates/vox_core/src/pipeline/controller.rs` per contracts/pipeline.md
- [ ] T025 [US2] Implement `set_mode(&mut self, mode: ActivationMode)` in `crates/vox_core/src/pipeline/controller.rs` per contracts/pipeline.md. If is_active, call force_stop() first (sends Stop, pipeline completes current segment per FR-018 before exiting). Update self.mode. Persist to SQLite settings table: ensure `CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)` exists (per data-model.md schema), then INSERT OR REPLACE into settings (key="activation_mode", value=mode string). Include inline unit test for mode persistence round-trip

**Checkpoint**: All three activation modes work correctly. Mode changes stop active sessions cleanly. Mode persists across restarts.

---

## Phase 5: User Story 3 — State Broadcasting (Priority: P3)

**Goal**: All UI subscribers receive real-time pipeline state transitions via push. Lagged subscribers recover gracefully.

**Note**: State broadcasting is implemented within `Pipeline::run()` (Phase 3, T014/T017). This phase handles the subscriber-side edge case (lagged receiver) and broadcast-send error handling not addressed in the hot path.

**Independent Test**: Subscribe multiple receivers before pipeline start, process a segment, verify all receivers observe identical transition sequence: Listening → Processing{None} → Processing{Some} → Injecting → Listening.

- [ ] T026 [US3] Handle `broadcast::send()` return value in Pipeline — Err means zero receivers, log at debug level and continue (not an error). Document `RecvError::Lagged(n)` recovery strategy in `subscribe()` doc comment: subscriber calls recv() again to get the most recent state, missed intermediate states are acceptable (latest-wins semantics per spec edge case "Broadcast subscriber overflow"). Add a broadcast helper method `fn broadcast(&mut self, state: PipelineState)` that sends + updates internal latest-state field + handles Err gracefully, in `crates/vox_core/src/pipeline/orchestrator.rs`

**Checkpoint**: Subscribers receive all state transitions. Zero-receiver scenario handled gracefully. Lagged recovery documented.

---

## Phase 6: User Story 4 — Transcript History (Priority: P4)

**Goal**: Verify transcript persistence, pruning, and command-exclusion behavior via automated tests.

**Note**: TranscriptStore is implemented in Phase 2 (T004). Transcript saving is implemented in Phase 3 (T016). This phase adds dedicated unit tests validating the acceptance criteria.

**Independent Test**: Dictate phrases into different apps, verify each creates a transcript with all fields populated. Restart app, verify records persist. Verify 30-day pruning removes old records on startup.

- [ ] T027 [US4] Add unit tests for TranscriptStore in `crates/vox_core/src/pipeline/transcript.rs`: (1) save + list round-trip verifies all fields preserved, (2) list ordering is newest-first, (3) list pagination (limit/offset) works correctly, (4) count returns accurate total, (5) prune_older_than deletes only records exceeding threshold and returns correct count, (6) open() auto-prunes records >30 days old (insert old record with past timestamp, re-open, verify pruned), (7) concurrent save+list via Arc\<TranscriptStore\> from two threads verifies no deadlock

**Checkpoint**: TranscriptStore persistence, pruning, ordering, and thread safety verified via automated tests.

---

## Phase 7: Polish & Integration Tests

**Purpose**: Documentation completeness, zero-warning compilation, end-to-end integration tests from spec, and quickstart validation

- [ ] T028 Add `///` doc comments to all `pub` items in new and modified files per Constitution Principle VII. Files: `pipeline/state.rs`, `pipeline/orchestrator.rs`, `pipeline/controller.rs`, `pipeline/transcript.rs`, `dictionary.rs`, additions to `injector.rs`, `injector/windows.rs`, `injector/macos.rs`, `audio/capture.rs`. Module-level `//!` docs on `pipeline/state.rs`, `pipeline/orchestrator.rs`, `pipeline/controller.rs`, `pipeline/transcript.rs`, `dictionary.rs`
- [ ] T029 Run `cargo build -p vox_core --features cuda` (Windows) or `--features metal` (macOS) and fix any compiler warnings to achieve zero-warning build per CLAUDE.md requirement
- [ ] T030 [P] Implement integration test `test_full_pipeline_hello_world` in `crates/vox_core/src/pipeline/orchestrator.rs` — construct Pipeline with real AsrEngine + PostProcessor from test fixture models, create DictionaryCache::empty(), create TranscriptStore in temp dir, feed speech_sample.wav through pipeline via segment channel, assert polished output is non-empty and transcript is saved
- [ ] T031 [P] Implement integration tests in `crates/vox_core/src/pipeline/orchestrator.rs` per spec Testing Requirements: `test_pipeline_empty_audio` (all-zero samples → no injection, RMS check triggers), `test_pipeline_multiple_segments` (3 sequential segments → 3 separate transcript entries in FIFO order). Additionally implement `test_pipeline_filler_removal`, `test_pipeline_course_correction`, and `test_pipeline_command` — these require specific WAV fixtures with known ASR output. Create WAV fixtures by recording or synthesizing test audio for each case (filler-laden speech, self-correction speech, command phrase) and place them in `crates/vox_core/tests/fixtures/`. If hardware recording is unavailable, construct synthetic PCM samples programmatically using known audio patterns that produce deterministic ASR output
- [ ] T032 Validate quickstart.md — run all listed build and test commands from the quickstart, verify each passes. Fix any discrepancies between quickstart documentation and actual build/test behavior

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 (directory must exist) — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Phase 2 completion — core pipeline implementation
- **US2 (Phase 4)**: Depends on Phase 2 completion — can run **in parallel with Phase 3** (different file: controller.rs vs orchestrator.rs)
- **US3 (Phase 5)**: Depends on Phase 3 (extends Pipeline with broadcast helper)
- **US4 (Phase 6)**: Depends on Phase 2 (T004 provides TranscriptStore) — tests can run after T004
- **Polish (Phase 7)**: Depends on Phases 3-6 — integration tests need full pipeline

### User Story Dependencies

- **US1 (P1)**: Depends on all foundational types and modifications. The MVP deliverable.
- **US2 (P2)**: Code-level independent of US1 (controller.rs vs orchestrator.rs). Logically builds on US1's pipeline.
- **US3 (P3)**: Broadcasting is implemented within US1; Phase 5 adds broadcast helper and subscriber docs.
- **US4 (P4)**: TranscriptStore created in Phase 2 (T004), saving integrated in Phase 3 (T016), verification tests in Phase 6 (T027).

### Within Each Phase

- **Phase 2**: All tasks marked [P] run in parallel (different files). T010 depends on T008+T009 completing first.
- **Phase 3**: Sequential within orchestrator.rs — T011 (struct) → T012 (start) → T013 (run) → T014 (process_segment) → T015-T016 (helpers) → T017 (error recovery) → T018 (subscribe/state) → T019 (module wiring)
- **Phase 4**: Sequential within controller.rs — T020 (struct) → T021-T023 (hotkey methods) → T024 (utility methods) → T025 (set_mode with persistence)

### Parallel Opportunities

```
Phase 2 — up to 9 tasks in parallel (different files):
  T002 (state.rs) | T003 (dictionary.rs) | T004 (transcript.rs) | T005 (controller.rs)
  T006 (capture.rs) | T007 (vad.rs) | T008 (windows.rs) | T009 (macos.rs)
  Then T010 (injector.rs) after T008+T009

Phase 3 + Phase 4 — two parallel streams:
  Stream A (US1): T011 → T012 → T013 → T014 → T015 → T016 → T017 → T018 → T019
  Stream B (US2): T020 → T021 → T022 → T023 → T024 → T025
  (orchestrator.rs and controller.rs have no cross-dependencies)

Phase 6 can overlap with Phase 3:
  T027 (TranscriptStore tests) only depends on T004, not on Pipeline

Phase 7 — partial parallel:
  T028 (docs) | T030 + T031 (integration tests) — different concerns
  T029 (zero warnings) after all code written
  T032 (quickstart) last
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (minutes)
2. Complete Phase 2: Foundational (all tasks in parallel)
3. Complete Phase 3: User Story 1 (sequential in orchestrator.rs)
4. **STOP and VALIDATE**: Full pipeline end-to-end — speak → polished text appears
5. Continue to Phase 4-7

### Incremental Delivery

1. Phase 1+2 → Foundation ready (types, data layer, module modifications)
2. Phase 3 → End-to-end dictation works → **Testable MVP**
3. Phase 4 → All activation modes functional → **Full user experience**
4. Phase 5+6 → Subscriber edge cases + transcript verification → **Robust**
5. Phase 7 → Integration tests + docs + quickstart validation → **Ship-ready**

### Parallel Team Strategy

With two agents working concurrently after Phase 2:

```
Agent A: Phase 3 (US1 — orchestrator.rs) → Phase 5 (US3) → Phase 7 (integration tests)
Agent B: Phase 4 (US2 — controller.rs) → Phase 6 (US4 — transcript tests) → Phase 7 (docs)
```

---

## Notes

- [P] tasks = different files, no dependencies on incomplete tasks
- [Story] label maps tasks to specific user stories for traceability
- All source paths under `crates/vox_core/src/` unless noted otherwise
- Constitution Principle III: Pipeline requires ALL 6 components — no fallbacks, no optional
- Constitution Principle VII: All `pub` items need `///` doc comments (T028)
- Constitution Principle VIII: No `#[ignore]` — all tests run unconditionally
- Integration tests (T030-T031) require model fixtures in `crates/vox_core/tests/fixtures/`
- `parking_lot = "0.12"` already in workspace Cargo.toml — no new dependencies needed
- Existing `run_vad_loop()` returns `anyhow::Result<()>` — compatible with JoinHandle error handling
