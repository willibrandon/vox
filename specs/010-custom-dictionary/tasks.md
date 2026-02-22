# Tasks: Custom Dictionary

**Input**: Design documents from `/specs/010-custom-dictionary/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/public-api.md, quickstart.md

**Tests**: 15 unit tests (13 from spec.md Testing Requirements + 2 added by analysis: test_list_by_category for FR-013, test_substitution_unicode for edge case 6).

**Organization**: Tasks grouped by user story. All code changes happen in 3 files: `dictionary.rs` (major refactor), `state.rs` (minor), `orchestrator.rs` (minor).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US6)
- All paths relative to `crates/vox_core/src/`

---

## Phase 1: Setup

**Purpose**: Verify environment — no code changes needed. All dependencies (rusqlite, parking_lot, serde, serde_json, anyhow, tracing) already in Cargo.toml. Dictionary module already exported from vox_core.rs.

- [X] T001 Verify all required dependencies present in crates/vox_core/Cargo.toml (rusqlite, parking_lot, serde, serde_json, anyhow, tracing, tempfile in dev-deps)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types, schema migration, and cache infrastructure that ALL user stories depend on. This is the major refactor of `crates/vox_core/src/dictionary.rs` — replacing immutable `Arc<HashMap>` internals with shared `Arc<RwLock<HashMap>>`, renaming fields, and adding new types.

**CRITICAL**: No user story work can begin until this phase is complete.

- [X] T002 Redefine DictionaryEntry struct with new fields (spoken, written, category, is_command_phrase, use_count, created_at) and add Serialize/Deserialize derives in crates/vox_core/src/dictionary.rs — replaces old fields (term→spoken, replacement→written, frequency→use_count as u64), adds category: String, is_command_phrase: bool
- [X] T003 Define new public types (SubstitutionResult, ImportResult, DictionaryExportEntry) and internal SubEntry struct in crates/vox_core/src/dictionary.rs — per contracts/public-api.md type definitions
- [X] T004 Update CREATE_TABLE_SQL constant with new schema (spoken TEXT UNIQUE COLLATE NOCASE, written, category DEFAULT 'general', is_command_phrase DEFAULT 0, use_count DEFAULT 0, created_at) and index statements (idx_dictionary_spoken, idx_dictionary_category) in crates/vox_core/src/dictionary.rs
- [X] T005 Implement migrate_schema(db_path: &Path) -> Result<()> free function in crates/vox_core/src/dictionary.rs — uses PRAGMA table_info to detect old columns (term, replacement, frequency), applies ALTER TABLE RENAME COLUMN + ADD COLUMN + CREATE INDEX per research.md RD-001, idempotent
- [X] T006 Refactor DictionaryCache struct to use Arc<RwLock<HashMap<String, DictionaryEntry>>> for entries, Arc<RwLock<HashMap<String, SubEntry>>> for word_subs, Arc<RwLock<Vec<(String, SubEntry)>>> for phrase_subs, and add db_path: Option<PathBuf> field in crates/vox_core/src/dictionary.rs — remove old word_substitutions/phrase_substitutions/hints fields
- [X] T007 Implement DictionaryCache::load(db_path: &Path) -> Result<Self> constructor — reads all rows from dictionary table with new column names, populates entries HashMap (keyed by spoken lowercase), calls rebuild_substitution_maps, stores db_path in crates/vox_core/src/dictionary.rs
- [X] T008 Implement DictionaryCache::empty() -> Self constructor and rebuild_substitution_maps(&self) internal method in crates/vox_core/src/dictionary.rs — rebuild partitions entries into word_subs (single-word, non-command) and phrase_subs (multi-word, non-command, sorted longest-first), empty() creates cache with no db_path and empty maps
- [X] T009 Implement len() and is_empty() accessors using entries HashMap count in crates/vox_core/src/dictionary.rs — len() returns entries.read().len(), is_empty() returns entries.read().is_empty()

**Checkpoint**: Foundation ready — types defined, cache infrastructure refactored, schema migration implemented. User story implementation can now begin.

---

## Phase 3: User Story 1 — Dictionary Entry Management (Priority: P1) MVP

**Goal**: Users can add, update, delete, list, and search dictionary entries with persistence to SQLite and in-memory cache synchronization.

**Independent Test**: Create fresh dictionary, add 3 entries (name, technical term, email expansion), verify persistence. Edit one, delete another, verify changes reflected. Search by partial text.

### Implementation for User Story 1

- [X] T010 [US1] Implement add(&self, spoken, written, category, is_command_phrase) -> Result<i64> method in crates/vox_core/src/dictionary.rs — validates spoken not empty, opens Connection from db_path, INSERT with datetime('now') for created_at, updates entries HashMap, calls rebuild_substitution_maps, returns new id. Error if spoken exists (COLLATE NOCASE uniqueness)
- [X] T011 [US1] Implement update(&self, id, spoken, written, category, is_command_phrase) -> Result<()> method in crates/vox_core/src/dictionary.rs — opens Connection, UPDATE by id, validates new spoken doesn't conflict with other entries, updates entries HashMap, calls rebuild_substitution_maps. Error if id not found
- [X] T012 [US1] Implement delete(&self, id) -> Result<()> method in crates/vox_core/src/dictionary.rs — opens Connection, DELETE by id, removes from entries HashMap, calls rebuild_substitution_maps. Error if id not found
- [X] T013 [US1] Implement list(&self, category: Option<&str>) -> Vec<DictionaryEntry> method in crates/vox_core/src/dictionary.rs — reads entries HashMap, filters by category if Some (case-sensitive match), returns sorted by spoken form
- [X] T014 [US1] Implement search(&self, query: &str) -> Vec<DictionaryEntry> method in crates/vox_core/src/dictionary.rs — reads entries HashMap, returns entries where query is substring of spoken or written (case-insensitive), sorted by spoken form

### Tests for User Story 1

- [X] T015 [US1] Write test_add_entry in crates/vox_core/src/dictionary.rs — add entry spoken="vox" written="Vox" category="name", verify returned id > 0, verify list(None) returns 1 entry with correct fields, verify duplicate spoken (case-insensitive) rejected. Validates FR-001, FR-002, FR-003, FR-004
- [X] T016 [US1] Write test_update_entry in crates/vox_core/src/dictionary.rs — add entry, update written and category, verify list returns updated fields, verify update with non-existent id returns error. Validates FR-003, FR-004
- [X] T017 [US1] Write test_delete_entry in crates/vox_core/src/dictionary.rs — add entry, delete by id, verify list returns empty, verify reloading from same db also shows empty, verify delete with non-existent id returns error. Validates FR-003, FR-004
- [X] T018 [US1] Write test_search_spoken in crates/vox_core/src/dictionary.rs — add "postgresql", "postgres", "python" entries, search "post" returns 2 entries, search "xyz" returns empty. Validates FR-014
- [X] T019 [US1] Write test_search_written in crates/vox_core/src/dictionary.rs — add entries with written "PostgreSQL" and "Python", search "Python" returns match (case-insensitive on written). Validates FR-014
- [X] T019b [US1] Write test_list_by_category in crates/vox_core/src/dictionary.rs — add entries with categories "name", "technical", "general", verify list(Some("technical")) returns only technical entries, verify list(Some("name")) returns only name entries, verify list(None) returns all entries. Validates FR-013

**Checkpoint**: CRUD operations fully functional. Dictionary entries persist to SQLite and stay synchronized with in-memory cache.

---

## Phase 4: User Story 2 — Text Substitution During Dictation (Priority: P1)

**Goal**: Pipeline applies dictionary substitutions to raw transcript text with case-insensitive whole-word matching, returning both the substituted text and matched entry IDs.

**Independent Test**: Add "vox" → "Vox" and "postgres" → "PostgreSQL". Apply substitutions to "I use vox and postgres daily". Verify output is "I use Vox and PostgreSQL daily". Verify "equinox" is NOT matched.

**Depends on**: Phase 3 (US1) for add() to create test entries

### Implementation for User Story 2

- [X] T020 [US2] Refactor apply_substitutions(&self, text: &str) -> SubstitutionResult method in crates/vox_core/src/dictionary.rs — change return type from String to SubstitutionResult, track matched SubEntry IDs during both phrase pass and word pass (one ID per match occurrence, duplicates for repeated matches), read from word_subs/phrase_subs via RwLock. Preserve existing two-pass algorithm (phrase longest-first, then single-word HashMap lookup)

### Tests for User Story 2

- [X] T021 [US2] Write test_substitution_basic in crates/vox_core/src/dictionary.rs — add "vox"→"Vox" and "postgres"→"PostgreSQL", apply to "I use vox and postgres daily", verify result.text == "I use Vox and PostgreSQL daily", verify result.matched_ids contains both entry IDs, verify matched_ids.len() == 2. Validates FR-005
- [X] T022 [US2] Write test_substitution_case_insensitive in crates/vox_core/src/dictionary.rs — add "vox"→"Vox", verify "VOX" and "Vox" both produce "Vox" in output. Validates FR-005
- [X] T023 [US2] Write test_substitution_whole_word in crates/vox_core/src/dictionary.rs — add "vox"→"Vox", apply to "equinox is great", verify result.text unchanged, verify result.matched_ids is empty. Validates FR-005
- [X] T023b [US2] Write test_substitution_unicode in crates/vox_core/src/dictionary.rs — add "cafe"→"Cafe" (ASCII), verify "CAFE" matches case-insensitively, add "naïve"→"Naïve" (Unicode), verify "NAÏVE" matches via Rust's to_lowercase() Unicode handling. Validates FR-005 edge case 6

**Checkpoint**: Text substitution returns SubstitutionResult with matched IDs. Whole-word, case-insensitive matching works correctly.

---

## Phase 5: User Story 3 — LLM Hint Integration (Priority: P2)

**Goal**: Top dictionary entries by use count are formatted as hints for the LLM post-processor's system prompt. All entries (including command phrases) are included.

**Independent Test**: Add 3 entries with varying use_counts (manually set via SQLite). Request top 2 hints. Verify correct ordering and format. Verify command phrases ARE included.

**Depends on**: Phase 2 (foundational types)

### Implementation for User Story 3

- [X] T024 [US3] Refactor top_hints(&self, n: usize) -> String method in crates/vox_core/src/dictionary.rs — read all entries from entries HashMap, sort by use_count descending, take at most n, format as "spoken1 → written1, spoken2 → written2, ...". Include ALL entries (including command phrases)

### Tests for User Story 3

- [X] T025 [US3] Write test_top_hints_format in crates/vox_core/src/dictionary.rs — add entries, verify output contains "→" separator, verify correct spoken/written values appear. Validates FR-008, FR-009
- [X] T026 [US3] Write test_top_hints_sorted_by_use in crates/vox_core/src/dictionary.rs — add 3 entries, manually set use_counts (10, 5, 1) via multiple increment_use_counts calls or direct SQLite, request top 2, verify highest use_count entry appears first, verify third entry excluded. Validates FR-009

**Checkpoint**: LLM hints correctly formatted and ordered by usage frequency.

---

## Phase 6: User Story 4 — Use Count Tracking (Priority: P2)

**Goal**: Each substitution match increments the entry's use count in both memory and SQLite. Use count drives hint priority ordering.

**Independent Test**: Add entry with use_count=0, apply substitution matching it twice, call increment_use_counts, verify use_count==2, verify persistence across reload.

**Depends on**: Phase 4 (US2) for apply_substitutions returning matched_ids

### Implementation for User Story 4

- [X] T027 [US4] Implement increment_use_counts(&self, ids: &[i64]) -> Result<()> method in crates/vox_core/src/dictionary.rs — opens Connection, wraps updates in single transaction, for each id occurrence increments use_count by 1 in both SQLite (UPDATE dictionary SET use_count = use_count + 1 WHERE id = ?1) and in-memory entries HashMap. Handles duplicate IDs (each occurrence increments separately)

### Tests for User Story 4

- [X] T028 [US4] Write test_use_count_increment in crates/vox_core/src/dictionary.rs — add entry "vox"→"Vox", apply_substitutions("vox is great vox"), verify matched_ids contains entry id twice, call increment_use_counts, verify list shows use_count == 2, reload from same db, verify use_count == 2 persisted. Validates FR-010

**Checkpoint**: Use counts track substitution frequency and persist to SQLite.

---

## Phase 7: User Story 5 — Import and Export (Priority: P3)

**Goal**: Users can export all dictionary entries as portable JSON and import entries from JSON, with duplicate handling and error reporting.

**Independent Test**: Add 5 entries with varying categories and command phrase flags. Export JSON. Import into fresh dictionary. Verify 5 added, 0 skipped. Import again — verify 0 added, 5 skipped.

**Depends on**: Phase 3 (US1) for add() used during import

### Implementation for User Story 5

- [X] T029 [US5] Implement export_json(&self) -> Result<String> method in crates/vox_core/src/dictionary.rs — reads all entries, maps to Vec<DictionaryExportEntry> (spoken, written, category, is_command_phrase only — excludes id, use_count, created_at), serializes via serde_json::to_string_pretty
- [X] T030 [US5] Implement import_json(&self, json: &str) -> Result<ImportResult> method in crates/vox_core/src/dictionary.rs — deserializes Vec<DictionaryExportEntry>, for each entry: skip if spoken form already exists (case-insensitive), report error if spoken is empty, otherwise call add() with use_count=0 and fresh created_at. Returns ImportResult { added, skipped, errors }

### Tests for User Story 5

- [X] T031 [US5] Write test_import_export_roundtrip in crates/vox_core/src/dictionary.rs — add 5 entries with varying categories and is_command_phrase values, export_json, verify JSON valid and contains 5 entries, verify JSON excludes id/use_count/created_at. Create new empty dictionary, import_json, verify added==5 skipped==0 errors empty, verify all 5 entries present with correct fields and use_count==0. Import same JSON again, verify added==0 skipped==5 errors empty. Validates FR-011, FR-012

**Checkpoint**: Dictionary entries can be backed up and restored via portable JSON format.

---

## Phase 8: User Story 6 — Command Phrase Handling (Priority: P3)

**Goal**: Entries marked as command phrases are excluded from text substitution but included in LLM hints. This allows voice command phrases to coexist with dictionary substitution.

**Independent Test**: Add "delete last" (command=true) and "vox" (command=false). Apply substitutions to "please delete last vox". Verify "delete last" unchanged, "vox" → "Vox". Verify both appear in top_hints.

**Depends on**: Phase 4 (US2) for substitution, Phase 5 (US3) for hints

### Implementation for User Story 6

- [X] T032 [US6] Verify command phrase filtering in rebuild_substitution_maps() in crates/vox_core/src/dictionary.rs — entries with is_command_phrase==true must NOT be added to word_subs or phrase_subs (this filtering was implemented in T008). Add integration verification: add a command phrase entry, apply_substitutions, confirm it is NOT substituted. Verify top_hints DOES include it

### Tests for User Story 6

- [X] T033 [US6] Write test_substitution_command_excluded in crates/vox_core/src/dictionary.rs — add entry spoken="delete last" written="" is_command_phrase=true, add entry spoken="vox" written="Vox" is_command_phrase=false. Apply substitutions to "please delete last vox", verify "delete last" unchanged in result, verify "Vox" substituted. Verify top_hints(50) includes both entries. Validates FR-007

**Checkpoint**: Command phrases correctly excluded from substitution but included in LLM hints.

---

## Phase 9: Integration & Polish

**Purpose**: Wire the refactored DictionaryCache into VoxState and Pipeline, update existing tests, verify zero warnings.

- [X] T034 Update init_database() in crates/vox_core/src/state.rs — replace direct CREATE_TABLE_SQL execution with call to dictionary::migrate_schema(db_path) which handles both fresh table creation and schema migration from old column names (term→spoken, replacement→written, frequency→use_count)
- [X] T035 Add DictionaryCache field to VoxState in crates/vox_core/src/state.rs — call migrate_schema() then DictionaryCache::load(db_path) during VoxState::new(), store as field, expose via pub fn dictionary(&self) -> &DictionaryCache accessor
- [X] T036 [P] Update Pipeline in crates/vox_core/src/pipeline/orchestrator.rs — change apply_substitutions usage to handle SubstitutionResult (use result.text instead of plain String), add increment_use_counts(&result.matched_ids) call after substitution with tracing::warn on error, no change to top_hints(50) call
- [X] T037 Update existing 10 tests in crates/vox_core/src/dictionary.rs for new API — update create_test_db helper to use new column names (spoken, written, use_count, category, is_command_phrase), update DictionaryEntry field references in assertions, update apply_substitutions assertions to use result.text from SubstitutionResult. Convert test_reload to test creating a fresh DictionaryCache::load() on the same db_path (reload() method is removed — Arc<RwLock> makes it unnecessary). Verify all 10 existing tests pass with new types
- [X] T038 Compile check — run `cargo build -p vox_core --features cuda` (Windows) / `--features metal` (macOS), verify zero warnings across all modified files (dictionary.rs, state.rs, orchestrator.rs)
- [X] T039 Run full test suite — `cargo test -p vox_core --features cuda` (Windows) / `--features metal` (macOS), verify all 25 tests pass (10 existing + 15 new)
- [X] T040 Run quickstart.md verification scenarios VS-001 through VS-015 against the implementation

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — verification only
- **Foundational (Phase 2)**: Depends on Phase 1 — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Phase 2 — CRUD is foundation for all other stories
- **US2 (Phase 4)**: Depends on Phase 3 — needs add() to create test entries
- **US3 (Phase 5)**: Depends on Phase 2 — can start after foundational, loosely coupled with US4
- **US4 (Phase 6)**: Depends on Phase 4 — needs SubstitutionResult.matched_ids from apply_substitutions
- **US5 (Phase 7)**: Depends on Phase 3 — needs add() for import
- **US6 (Phase 8)**: Depends on Phase 4 + Phase 5 — needs substitution and hints working
- **Integration (Phase 9)**: Depends on ALL user stories complete

### User Story Dependencies

```
Phase 2 (Foundational)
    ├── US1 (P1: CRUD) ─────────┬── US2 (P1: Substitution) ── US4 (P2: Use Count)
    │                           │                                      │
    │                           ├── US5 (P3: Import/Export)            │
    │                           │                                      │
    ├── US3 (P2: Hints) ───────┼── US6 (P3: Command Phrases) ────────┘
    │                           │
    └───────────────────────────┴── Phase 9 (Integration & Polish)
```

### Within Each User Story

- Implementation tasks before test tasks
- Tests validate the implementation within the same phase
- Story complete before dependent stories begin

### Parallel Opportunities

- T034 + T035 (state.rs, sequential) and T036 (orchestrator.rs) can run in parallel — different files
- US3 (hints) can start in parallel with US2 (substitution) if they don't share test setup — both only need foundational phase
- US5 (import/export) can start after US1 completes, independent of US2/US3/US4

---

## Parallel Example: Phase 9 Integration

```bash
# These three tasks touch different files and can run in parallel:
Task T034: "Update init_database() in state.rs"
Task T035: "Add DictionaryCache field to VoxState in state.rs"
Task T036: "Update Pipeline in orchestrator.rs"
# T034 and T035 both touch state.rs, so they're sequential with each other
# but parallel with T036 (orchestrator.rs)
```

---

## Implementation Strategy

### MVP First (User Stories 1 + 2)

1. Complete Phase 1: Setup (verification)
2. Complete Phase 2: Foundational (types, schema, cache refactor)
3. Complete Phase 3: US1 — CRUD operations
4. Complete Phase 4: US2 — Text substitution
5. **STOP and VALIDATE**: Dictionary creates entries and applies substitutions correctly

### Incremental Delivery

1. Setup + Foundational → Infrastructure ready
2. US1 → CRUD works → Dictionary is a data store
3. US2 → Substitution works → Dictionary improves dictation (MVP!)
4. US3 → Hints work → LLM biased toward user vocabulary
5. US4 → Use counts tracked → Hints adapt to usage
6. US5 → Import/export → Data portability
7. US6 → Command phrases → Voice commands coexist with substitution
8. Integration → Wired into VoxState + Pipeline → Production-ready

---

## Notes

- All 42 tasks (27 implementation + 15 test) target 3 files: dictionary.rs (~750 lines after refactor), state.rs, orchestrator.rs
- No new files created, no Cargo.toml changes needed
- The `reload()` method is removed — shared `Arc<RwLock>` makes all clones see mutations immediately
- Existing 10 tests in dictionary.rs must be updated (T037) for renamed fields and SubstitutionResult return type
- Performance targets from spec: cache load <50ms, substitution <5ms, CRUD <10ms, search <10ms
