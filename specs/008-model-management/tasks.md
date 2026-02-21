# Tasks: Model Management

**Input**: Design documents from `/specs/008-model-management/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/models-api.md, quickstart.md

**Tests**: Included — spec.md explicitly defines unit and integration test requirements.

**Organization**: Tasks grouped by user story. Three source files: `models.rs` (module root), `models/downloader.rs` (download engine), `models/format.rs` (format validation).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup

**Purpose**: Add dependencies and create module file structure

- [ ] T001 Add `sha2 = "0.10"` and `dirs = "5"` dependencies to `crates/vox_core/Cargo.toml` under `[dependencies]`
- [ ] T002 Set up models module structure: add `//!` module docs and `pub mod downloader;` / `pub mod format;` declarations to `crates/vox_core/src/models.rs`, create `crates/vox_core/src/models/` directory with skeleton files `downloader.rs` and `format.rs` (each with `//!` module docs only)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core types and utility functions shared by all user stories

**CRITICAL**: No user story work can begin until this phase is complete

- [ ] T003 [P] Implement `ModelInfo` struct (`name`, `filename`, `url`, `sha256`, `size_bytes` — all `&'static str`/`u64`) and `MODELS: &[ModelInfo]` constant with 3 entries (Silero VAD, Whisper Large V3 Turbo, Qwen 2.5 3B) using verified SHA-256 hashes from `scripts/download-models.sh` in `crates/vox_core/src/models.rs`
- [ ] T004 [P] Implement `model_dir() -> Result<PathBuf>` using `dirs::data_local_dir()` (Windows) / `dirs::data_dir()` (macOS) with `com.vox.app/models/` suffix and `std::fs::create_dir_all`, and `model_path(filename: &str) -> Result<PathBuf>` in `crates/vox_core/src/models.rs`
- [ ] T005 Implement `check_missing_models() -> Result<Vec<&'static ModelInfo>>` (filter MODELS by file existence) and `all_models_present() -> Result<bool>` in `crates/vox_core/src/models.rs`
- [ ] T006 [P] Implement `verify_checksum(path: &Path, expected_sha256: &str) -> Result<bool>` using `sha2::Sha256` with `std::io::copy` for streaming hash in `crates/vox_core/src/models.rs`
- [ ] T007 [P] Implement `cleanup_tmp_files() -> Result<()>` that deletes all `*.tmp` files in model_dir() in `crates/vox_core/src/models.rs`
- [ ] T008 Write foundational unit tests in `#[cfg(test)] mod tests` in `crates/vox_core/src/models.rs`: `test_model_dir_platform` (path ends with `com.vox.app/models`), `test_check_models_all_present` (returns empty when files exist in tempdir), `test_check_models_missing` (returns missing models), `test_sha256_verification` (known file matches hash), `test_sha256_mismatch` (corrupt file returns false)

**Checkpoint**: Foundation ready — model registry, path resolution, checksum verification, and tmp cleanup all functional with passing tests

---

## Phase 3: User Story 1 — Zero-Click First Launch (Priority: P1)

**Goal**: On first launch, detect missing models, download all three concurrently with streaming and progress reporting, verify SHA-256 checksums, and use atomic .tmp→rename writes. Auto-retry once on checksum failure.

**Independent Test**: Clear model directory and invoke `download_missing()` — all three models download, checksums verify, and files appear at final paths. No user interaction required.

### Tests for User Story 1

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T009 [P] [US1] Write unit tests `test_atomic_write` (.tmp file renamed to final on success) and `test_atomic_write_cleanup` (.tmp file deleted on verification failure) in `crates/vox_core/src/models/downloader.rs` `#[cfg(test)] mod tests`
- [ ] T010 [P] [US1] Write integration test `test_download_small_model` in `crates/vox_core/tests/models_download.rs`: download VAD model (~1.1 MB) to a tempdir, verify checksum matches, verify file exists at final path (not .tmp)
- [ ] T011 [P] [US1] Write integration test `test_concurrent_download` in `crates/vox_core/tests/models_download.rs`: verify `download_missing()` spawns concurrent tasks by downloading VAD model + subscribing to broadcast events, confirming Started and Complete events arrive for the model

### Implementation for User Story 1

- [ ] T012 [US1] Implement `DownloadEvent` enum (Started, Progress, Complete, Failed, VerificationFailed, DetectedOnDisk — all with `model: String` field) and `DownloadProgress` enum (Pending, InProgress, Complete, Failed) with `Clone + Debug` derives in `crates/vox_core/src/models/downloader.rs`
- [ ] T013 [US1] Implement `ModelDownloader` struct holding `reqwest::Client` and `broadcast::Sender<DownloadEvent>` (capacity 16), with `pub fn new() -> Self` and `pub fn subscribe(&self) -> broadcast::Receiver<DownloadEvent>` in `crates/vox_core/src/models/downloader.rs`
- [ ] T014 [US1] Implement private `download_model(&self, model: &ModelInfo) -> Result<()>`: emit Started, stream response via `response.chunk()` loop sending chunks through bounded `tokio::sync::mpsc` to a `tokio::task::spawn_blocking` writer that does `std::fs::File` writes + inline `Sha256::update()` per chunk, emit Progress throttled to 500ms via `Instant::elapsed()`, `file.sync_all()` before close, compare final hash, `std::fs::rename` from .tmp to final path, emit Complete — in `crates/vox_core/src/models/downloader.rs`
- [ ] T015 [US1] Implement `pub async fn download_missing(&self, missing: &[&ModelInfo]) -> Result<()>`: `tokio::spawn` one task per model calling `download_model()`, collect `Vec<JoinHandle>`, await all, on VerificationFailed delete .tmp and retry once then emit Failed — in `crates/vox_core/src/models/downloader.rs`
- [ ] T016 [US1] Re-export `DownloadEvent`, `DownloadProgress`, `ModelDownloader` from `crates/vox_core/src/models.rs` via `pub use downloader::{...};`

**Checkpoint**: First launch auto-download works end-to-end — missing models detected, downloaded concurrently, SHA-256 verified, atomic writes, progress events broadcast. All US1 tests pass.

---

## Phase 4: User Story 2 — Download Failure Recovery (Priority: P2)

**Goal**: When downloads fail (no internet, server error, corruption after retry), show manual download instructions with model directory path and direct URLs. Provide "Open Folder" action. Poll model directory every 5 seconds to detect manually-placed files.

**Independent Test**: Block network, verify failure events and manual URLs are available. Place file in model directory during poll, verify DetectedOnDisk event within 5 seconds.

### Tests for User Story 2

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T017 [P] [US2] Write integration test `test_resume_after_failure` in `crates/vox_core/tests/models_download.rs`: simulate download failure (invalid URL), verify Failed event with error message, then place file manually and verify detection

### Implementation for User Story 2

- [ ] T018 [US2] Implement `pub fn open_model_directory() -> Result<()>` using `std::process::Command::new("explorer.exe").arg(path).spawn()` (Windows) / `std::process::Command::new("open").arg(path).spawn()` (macOS) in `crates/vox_core/src/models.rs`
- [ ] T019 [US2] Implement `pub async fn poll_until_ready(&self) -> Result<()>` on `ModelDownloader`: `tokio::time::interval(Duration::from_secs(5))` loop calling `check_missing_models()`, emit `DetectedOnDisk` for newly-found models via broadcast channel, return `Ok(())` when all models present — in `crates/vox_core/src/models/downloader.rs`
- [ ] T020 [US2] Write unit test for `poll_until_ready()` in `crates/vox_core/src/models/downloader.rs`: create tempdir, spawn poll task, place file after 1 second, verify DetectedOnDisk event received within 6 seconds

**Checkpoint**: Failure recovery works — open folder launches explorer, polling detects manual file placement, DetectedOnDisk events broadcast. US2 tests pass.

---

## Phase 5: User Story 3 — Model Swapping (Priority: P3)

**Goal**: Validate model file format by checking magic bytes (GGUF, GGML, ONNX) and map detected format to the correct model slot (VAD/ASR/LLM). This enables safe model swapping at the app layer.

**Independent Test**: Create test files with known magic byte headers, verify `detect_format()` returns correct `ModelFormat` and `format_to_slot()` maps to the right index.

**Note**: FR-017 (benchmark inference after swap) is implemented at the pipeline/app layer, not in `vox_core::models`. This phase provides the format validation infrastructure that the app layer uses before benchmarking.

### Tests for User Story 3

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [ ] T021 [P] [US3] Write tests in `crates/vox_core/src/models/format.rs` `#[cfg(test)] mod tests`: test GGUF magic bytes (`0x47475546`), test GGML/GGMF/GGJT variants, test ONNX protobuf byte (`0x08`), test Unknown for random bytes, test error for file smaller than 4 bytes

### Implementation for User Story 3

- [ ] T022 [US3] Implement `ModelFormat` enum (Gguf, Ggml, Onnx, Unknown) with `Debug, Clone, Copy, PartialEq, Eq` derives, `pub fn detect_format(path: &Path) -> Result<ModelFormat>` (read first 4 bytes, match magic bytes per research.md Decision 9), and `pub fn format_to_slot(format: ModelFormat) -> Option<usize>` (Onnx→0, Ggml→1, Gguf→2, Unknown→None) in `crates/vox_core/src/models/format.rs`
- [ ] T023 [US3] Re-export `ModelFormat`, `detect_format`, `format_to_slot` from `crates/vox_core/src/models.rs` via `pub use format::{...};`

**FR-011 note**: The "Retry Download" action (FR-011) is satisfied by re-invoking `download_missing()` with the current `check_missing_models()` result. No additional models-module code is needed — the retry is triggered at the UI/app layer.

**FR-017 note**: Benchmark inference (FR-017) requires loading models into inference engines (whisper-rs, llama-cpp-2, ort) and running a forward pass. This lives at the pipeline/app layer, not in `vox_core::models`. The models module provides `detect_format()` and `format_to_slot()` as the validation infrastructure that the app layer uses *before* benchmarking. A pipeline-level task list should cover the benchmark integration.

**Checkpoint**: Format validation works — magic byte detection for GGUF/GGML/ONNX, slot mapping, all US3 tests pass.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across all user stories

- [ ] T024 Verify all `pub` items in `models.rs`, `models/downloader.rs`, `models/format.rs` have `///` doc comments and all three modules have `//!` module-level docs (Constitution VII)
- [ ] T025 [P] Validate SC-003 (SHA-256 < 5s): add a unit test that times `verify_checksum()` on a large temp file (>= 100 MB) and asserts completion under 5 seconds; validate SC-004 (detection < 100ms): time `check_missing_models()` and assert under 100ms — in `crates/vox_core/src/models.rs` `#[cfg(test)]`
- [ ] T026 Run full test suite `cargo test -p vox_core --features cuda` (Windows) / `--features metal` (macOS), verify zero warnings, all tests pass unconditionally (Constitution VIII)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup completion — BLOCKS all user stories
- **User Stories (Phase 3–5)**: All depend on Foundational phase completion
  - US2 depends on US1 (uses download infrastructure: ModelDownloader, broadcast channel)
  - US3 is independent of US1 and US2 (standalone format module)
- **Polish (Phase 6)**: Depends on all user stories being complete

### User Story Dependencies

- **User Story 1 (P1)**: Can start after Foundational (Phase 2) — no dependencies on other stories
- **User Story 2 (P2)**: Depends on US1 — adds polling and recovery to ModelDownloader
- **User Story 3 (P3)**: Can start after Foundational (Phase 2) — independent of US1/US2 (different file: `format.rs`)

### Within Each User Story

- Tests written FIRST (TDD) — they will fail until implementation
- Types/enums before struct implementations
- Struct construction before methods
- Private methods before public API methods
- Re-exports after types are implemented

### Parallel Opportunities

**Phase 2** (4 parallel tasks):
```
T003 (ModelInfo + MODELS)  ←── parallel ──→  T004 (model_dir + model_path)
T006 (verify_checksum)     ←── parallel ──→  T007 (cleanup_tmp_files)
```
Then T005 (check_missing) depends on T003+T004, T008 (tests) depends on T003–T007.

**Phase 3** (3 parallel test tasks):
```
T009 (atomic write tests)
T010 (download integration test)    ← all parallel, different files
T011 (concurrent download test)
```

**Phase 3+5 cross-story parallel** (if staffed):
```
US1 implementation (T012–T016 in downloader.rs)
US3 implementation (T021–T023 in format.rs)         ← parallel, different files
```

---

## Parallel Example: User Story 1

```bash
# Launch all US1 tests in parallel (TDD - they fail first):
Task: "T009 — atomic write tests in models/downloader.rs"
Task: "T010 — download integration test in tests/models_download.rs"
Task: "T011 — concurrent download test in tests/models_download.rs"

# Then sequential implementation:
Task: "T012 — DownloadEvent + DownloadProgress types"
Task: "T013 — ModelDownloader struct"
Task: "T014 — download_model() streaming engine"
Task: "T015 — download_missing() concurrent orchestrator"
Task: "T016 — re-exports"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (add deps, create files)
2. Complete Phase 2: Foundational (registry, paths, checksum, cleanup)
3. Complete Phase 3: User Story 1 (download engine)
4. **STOP and VALIDATE**: `cargo test -p vox_core --features cuda` — all download tests pass
5. Models auto-download on first launch with progress and verification

### Incremental Delivery

1. Setup + Foundational → Core types ready
2. Add US1 → Test independently → Auto-download works (MVP)
3. Add US2 → Test independently → Failure recovery works
4. Add US3 → Test independently → Format validation works
5. Polish → Zero warnings, full doc coverage

### File-Level Isolation

Each user story primarily touches one file, minimizing merge conflicts:
- **US1**: `models/downloader.rs` (download engine) + `models.rs` (re-exports)
- **US2**: `models/downloader.rs` (add poll_until_ready) + `models.rs` (add open_model_directory)
- **US3**: `models/format.rs` (standalone) + `models.rs` (re-exports)

---

## Notes

- [P] tasks = different files, no dependencies — safe to run in parallel
- [Story] label maps task to specific user story for traceability
- All `pub` items get `///` doc comments inline during implementation (Constitution VII)
- No `#[ignore]` or conditional test guards (Constitution VIII)
- Integration tests use the real VAD model (~1.1 MB) to keep test time reasonable
- FR-017 (benchmark inference) is at the pipeline/app layer, not in `vox_core::models`
- Verified SHA-256 hashes sourced from `scripts/download-models.sh`, not the original spec's "TBD" values
- reqwest 0.12 (not 0.13) per CLAUDE.md pinned versions
- Qwen URL uses official `Qwen/` org (not `bartowski/` community mirror) per research.md Decision 10
