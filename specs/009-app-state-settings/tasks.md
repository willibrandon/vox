# Tasks: Application State & Settings

**Input**: Design documents from `/specs/009-app-state-settings/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/public-api.md, quickstart.md

**Tests**: Included — spec.md Testing Requirements section explicitly lists 9 unit tests.

**Organization**: Tasks grouped by user story. US2 (Settings) precedes US1 (VoxState) because `VoxState::new()` calls `Settings::load()`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup

**Purpose**: Add the gpui dependency required by Constitution Principle XI

- [ ] T001 Add `gpui.workspace = true` (non-optional) to `crates/vox_core/Cargo.toml` and verify compilation with `cargo check -p vox_core --features cuda`

**Checkpoint**: vox_core compiles with gpui as a required dependency

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Define the core types that all user stories depend on

**CRITICAL**: No user story work can begin until this phase is complete

- [ ] T002 [P] Implement `OverlayPosition` enum (5 variants), `ThemeMode` enum (3 variants), and `Settings` struct (23 fields across 7 categories) with `Default` impl, `#[serde(default)]`, `Serialize`, `Deserialize`, `Clone`, `Debug` derives in `crates/vox_core/src/config.rs`
- [ ] T003 [P] Implement `AppReadiness` enum with `Downloading { vad_progress, whisper_progress, llm_progress }`, `Loading { stage }`, and `Ready` variants using `crate::models::DownloadProgress` in `crates/vox_core/src/state.rs`

**Checkpoint**: Foundation types compile — Settings struct with 23 fields and AppReadiness enum exist

---

## Phase 3: User Story 2 - Settings Persistence (Priority: P1)

**Goal**: Settings load from JSON, save to JSON with atomic write, handle corrupt/missing files gracefully

**Independent Test**: Create a tempdir, save non-default settings, reload, verify all 23 fields round-trip. Write corrupt JSON, load, verify defaults returned without crash.

### Implementation for User Story 2

- [ ] T004 [US2] Implement `Settings::load(data_dir: &Path) -> Result<Self>` with corrupt-file recovery (log warning via `tracing::warn!`, return defaults) in `crates/vox_core/src/config.rs`
- [ ] T005 [US2] Implement `Settings::save(&self, data_dir: &Path) -> Result<()>` with atomic write pattern (write to `settings.json.tmp`, rename to `settings.json`) in `crates/vox_core/src/config.rs`
- [ ] T006 [US2] Write tests in `crates/vox_core/src/config.rs`: `test_settings_default` (sane defaults), `test_settings_roundtrip` (save then load preserves all 23 fields), `test_settings_corrupt_file` (corrupt JSON resets to defaults), `test_settings_missing_file` (missing file creates defaults), `test_settings_forward_backward_compat` (extra fields ignored, missing fields get defaults)

**Checkpoint**: Settings persistence fully functional — load, save, corrupt recovery, forward/backward compatibility all tested

---

## Phase 4: User Story 1 - Application Initializes with Persistent State (Priority: P1)

**Goal**: VoxState initializes as GPUI Global, creates data directory, loads settings, opens database with schema

**Independent Test**: Create VoxState from tempdir. Verify settings.json exists with defaults, vox.db exists with schema, readiness starts at Downloading, `impl Global` compiles.

**Depends on**: US2 (Settings::load called by VoxState::new)

### Implementation for User Story 1

- [ ] T007 [US1] Implement `data_dir() -> Result<PathBuf>` (using `dirs::data_local_dir()` on Windows, `dirs::data_dir()` on macOS, appending `com.vox.app`) and `ensure_data_dirs() -> Result<PathBuf>` (creates data dir + models subdir) in `crates/vox_core/src/state.rs`
- [ ] T008 [US1] Implement `init_database(data_dir: &Path) -> Result<TranscriptStore>` that opens `vox.db`, creates transcripts table (via TranscriptStore) and dictionary table (`CREATE TABLE IF NOT EXISTS dictionary` with existing schema from dictionary.rs) in `crates/vox_core/src/state.rs`
- [ ] T009 [US1] Implement `VoxState` struct (fields: `settings: RwLock<Settings>`, `transcript_store: Arc<TranscriptStore>`, `readiness: RwLock<AppReadiness>`, `pipeline_state: RwLock<PipelineState>`, `tokio_runtime: Runtime`, `data_dir: PathBuf`), `impl gpui::Global for VoxState {}`, and `VoxState::new(data_dir: &Path) -> Result<Self>` constructor in `crates/vox_core/src/state.rs`
- [ ] T010 [US1] Implement VoxState accessor methods: `settings()`, `update_settings()`, `data_dir()`, `tokio_runtime()`, `transcript_store()` in `crates/vox_core/src/state.rs`
- [ ] T011 [US1] Refactor `Pipeline::new()` to accept `Arc<TranscriptStore>` instead of owned `TranscriptStore`, update all internal usages and test helpers in `crates/vox_core/src/pipeline/orchestrator.rs`
- [ ] T012 [US1] Write tests in `crates/vox_core/src/state.rs`: `test_vox_state_init` (settings file + db created, readiness is Downloading), `test_data_dir_platform` (correct platform path), `test_vox_state_existing_data` (loads existing settings and db)

**Checkpoint**: VoxState initializes correctly — GPUI Global impl compiles, data directory created, settings loaded, database schema created

---

## Phase 5: User Story 3 - Transcript History (Priority: P2)

**Goal**: Full transcript CRUD with search by text content, single delete, and secure clear (overwrite + VACUUM)

**Independent Test**: Save 5 transcripts, search for one by text, delete one by ID, verify 4 remain. Clear history, verify 0 remain and database is vacuumed.

**Depends on**: US1 (VoxState with TranscriptStore)

### Implementation for User Story 3

- [ ] T013 [US3] Implement `TranscriptStore::search(query: &str) -> Result<Vec<TranscriptEntry>>` with SQL LIKE matching on both `raw_text` and `polished_text`, ordered by `created_at DESC` in `crates/vox_core/src/pipeline/transcript.rs`
- [ ] T014 [US3] Implement `TranscriptStore::delete(id: &str) -> Result<()>` for single record deletion in `crates/vox_core/src/pipeline/transcript.rs`
- [ ] T015 [US3] Implement `TranscriptStore::clear_secure() -> Result<()>` that executes UPDATE (overwrite text fields with empty strings) → DELETE all rows → VACUUM in `crates/vox_core/src/pipeline/transcript.rs`
- [ ] T016 [US3] Implement VoxState transcript wrapper methods: `save_transcript()`, `get_transcripts()`, `search_transcripts()`, `delete_transcript()`, `clear_history()` that delegate to `Arc<TranscriptStore>` in `crates/vox_core/src/state.rs`
- [ ] T017 [US3] Write tests in `crates/vox_core/src/pipeline/transcript.rs`: `test_transcript_search` (finds matching transcripts in raw_text and polished_text), `test_transcript_delete` (single record removed, others intact), `test_transcript_clear_secure` (all records gone after overwrite + DELETE + VACUUM)

**Checkpoint**: Full transcript CRUD operational — search, delete, and secure clear all tested

---

## Phase 6: User Story 4 - Application Readiness Tracking (Priority: P2)

**Goal**: VoxState tracks readiness (Downloading -> Loading -> Ready) and pipeline state, queryable at each stage

**Independent Test**: Create VoxState, verify initial state is Downloading. Transition to Loading, verify. Transition to Ready, verify. Same for pipeline state transitions.

**Depends on**: US1 (VoxState struct exists)

### Implementation for User Story 4

- [ ] T018 [US4] Implement VoxState methods: `readiness() -> AppReadiness` (clone from RwLock), `set_readiness(state)`, `pipeline_state() -> PipelineState` (clone from RwLock), `set_pipeline_state(state)` in `crates/vox_core/src/state.rs`
- [ ] T019 [US4] Write tests in `crates/vox_core/src/state.rs`: `test_app_readiness_transitions` (Downloading -> Loading -> Ready), `test_pipeline_state_transitions` (Idle -> Listening -> Processing -> Injecting -> Idle)

**Checkpoint**: Readiness and pipeline state tracking fully functional with tested transitions

---

## Phase 7: User Story 5 - Audio Data Privacy (Priority: P1)

**Goal**: save_transcript is no-op when save_history is false, clear_history performs secure delete

**Independent Test**: Disable save_history in settings, save a transcript, verify 0 entries in database. Enable save_history, save transcripts, clear history, verify database is empty and vacuumed.

**Depends on**: US1 (VoxState), US2 (Settings with save_history field), US3 (clear_history)

### Implementation for User Story 5

- [ ] T020 [US5] Add `save_history` check to `VoxState::save_transcript()` — return `Ok(())` immediately when `self.settings().save_history` is false in `crates/vox_core/src/state.rs`
- [ ] T021 [US5] Write tests in `crates/vox_core/src/state.rs`: `test_save_history_disabled` (no transcript saved when save_history=false), `test_clear_history_vacuum` (database file size decreases after clear_history with VACUUM)

**Checkpoint**: Privacy controls verified — transcript saving respects save_history setting, secure delete leaves no recoverable data

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across all user stories

- [ ] T022 [P] Verify zero compiler warnings with `cargo build -p vox_core --features cuda` and `cargo clippy -p vox_core --features cuda`
- [ ] T023 Run all quickstart.md verification scenarios (7 scenarios) and confirm all pass
- [ ] T024 Verify all `pub` items in state.rs, config.rs, and extended transcript.rs have `///` doc comments per Constitution Principle VII

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup (gpui must be available) — BLOCKS all user stories
- **US2 (Phase 3)**: Depends on Foundational (Settings struct must exist)
- **US1 (Phase 4)**: Depends on US2 (VoxState::new calls Settings::load)
- **US3 (Phase 5)**: Depends on US1 (VoxState with TranscriptStore)
- **US4 (Phase 6)**: Depends on US1 (VoxState struct exists)
- **US5 (Phase 7)**: Depends on US1, US2, US3 (needs save_history setting, transcript wrappers, clear_history)
- **Polish (Phase 8)**: Depends on all user stories complete

### User Story Dependencies

```
Phase 1 (Setup)
    │
Phase 2 (Foundational: Settings struct + AppReadiness enum)
    │
Phase 3 (US2: Settings load/save)
    │
Phase 4 (US1: VoxState + data dir + database + Global impl)
    ├─────────────┐
Phase 5 (US3)   Phase 6 (US4)     ← Can run in parallel
    │             │
    └──────┬──────┘
           │
    Phase 7 (US5: Privacy controls)
           │
    Phase 8 (Polish)
```

### Within Each User Story

- Types and enums before methods that use them
- load/save before constructor that calls them
- TranscriptStore extensions before VoxState wrappers
- Implementation before tests
- Tests validate the story independently

### Parallel Opportunities

- T002 and T003 are [P] — config.rs and state.rs are different files
- T022 and T024 are [P] — build check and doc check are independent
- Phase 5 (US3) and Phase 6 (US4) can run in parallel after US1 completes,
  but T016 (US3 state.rs wrappers) and T018 (US4 state.rs methods) must be
  serialized since both modify state.rs. The transcript.rs tasks (T013-T015)
  can truly parallelize with US4.
- Within US3: T013 and T014 add independent methods but to the same file — sequential

---

## Parallel Example: Foundational Phase

```
# These can be launched together (different files):
Agent A: "T002 — Implement Settings struct, OverlayPosition, ThemeMode in config.rs"
Agent B: "T003 — Implement AppReadiness enum in state.rs"
```

## Parallel Example: After US1 Completes

```
# US3 and US4 can proceed in parallel:
Agent A: "T013-T017 — Transcript History (pipeline/transcript.rs + state.rs)"
Agent B: "T018-T019 — Readiness Tracking (state.rs)"
```

---

## Implementation Strategy

### MVP First (US2 + US1)

1. Complete Phase 1: Setup (add gpui dependency)
2. Complete Phase 2: Foundational (Settings struct + AppReadiness enum)
3. Complete Phase 3: US2 — Settings load/save
4. Complete Phase 4: US1 — VoxState initialization
5. **STOP and VALIDATE**: VoxState creates data dir, loads settings, opens database

### Incremental Delivery

1. Setup + Foundational → Types compile
2. US2 → Settings persistence works → Validate independently
3. US1 → VoxState initializes as GPUI Global → Validate independently
4. US3 → Transcript CRUD operational → Validate independently
5. US4 → Readiness tracking works → Validate independently
6. US5 → Privacy controls verified → Validate independently
7. Polish → Zero warnings, all docs, quickstart passes

---

## File Change Summary

| File | Action | Tasks |
|------|--------|-------|
| `crates/vox_core/Cargo.toml` | MODIFY | T001 |
| `crates/vox_core/src/config.rs` | POPULATE (empty stub) | T002, T004, T005, T006 |
| `crates/vox_core/src/state.rs` | POPULATE (empty stub) | T003, T007-T010, T012, T016, T018-T021 |
| `crates/vox_core/src/pipeline/transcript.rs` | EXTEND | T013-T015, T017 |
| `crates/vox_core/src/pipeline/orchestrator.rs` | REFACTOR | T011 |

## Notes

- T001 is critical: gpui must be **required** (not optional) per Constitution Principle XI
- T011 (Arc<TranscriptStore> refactor) must update existing orchestrator tests
- T002 Settings struct needs `#[serde(default)]` on the struct (NOT `#[serde(deny_unknown_fields)]`)
- T008 init_database creates BOTH transcripts and dictionary tables
- T015 clear_secure order matters: UPDATE (overwrite) → DELETE → VACUUM
- T020 save_history check: read lock on settings, check field, early return if false
