# Tasks: Speech Recognition (ASR)

**Input**: Design documents from `/specs/004-speech-recognition/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/asr-engine.md, quickstart.md

**Tests**: Included. The spec defines 6 ASR unit tests (#[ignore], require model), 1 ASR error test (no model required), and 4 stitcher tests (no model required). All test names from quickstart.md.

**Organization**: Tasks grouped by user story. US3 (stitcher) is fully parallelizable with US1/US2 since it operates on a separate file with no shared dependencies.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3)
- All file paths relative to repository root

---

## Phase 1: Setup

**Purpose**: Create ASR module file structure. No new dependencies needed — whisper-rs 0.15.1 and hound 3.5 are already in Cargo.toml, `pub mod asr;` is already declared in vox_core.rs.

- [x] T001 Create asr module file structure — populate `crates/vox_core/src/asr.rs` with module-level `//!` docs, whisper-rs imports (`WhisperContext`, `WhisperContextParameters`, `FullParams`, `SamplingStrategy`, `WhisperError`), `std::sync::{Arc, Mutex}`, `std::path::Path`, `anyhow::Result`, and `pub mod stitcher;` submodule declaration. Create `crates/vox_core/src/asr/stitcher.rs` with module-level `//!` docs placeholder.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: AsrEngine struct and model loading. MUST complete before any user story — nothing works without a loaded model.

- [x] T002 Implement `AsrEngine` struct with field `ctx: Arc<Mutex<WhisperContext>>`, derive `Clone` via manual impl (Arc::clone per research R-003), and implement `AsrEngine::new(model_path: &Path, use_gpu: bool) -> Result<AsrEngine>` — create `WhisperContextParameters`, call `use_gpu()`, load model via `WhisperContext::new_with_params()`, wrap in `Arc::new(Mutex::new())`. Add `///` doc comments on struct and `new()`. File: `crates/vox_core/src/asr.rs`

**Checkpoint**: `cargo build -p vox_core --features cuda` compiles with zero warnings. AsrEngine struct exists and model can be loaded.

---

## Phase 3: User Story 1 — Transcribe a Single Utterance (Priority: P1) MVP

**Goal**: Feed a speech audio segment to AsrEngine, get transcribed text back. Handle silence and empty audio gracefully.

**Independent Test**: Load model, feed known speech WAV → verify text. Feed silence → verify empty string. Feed empty slice → verify empty string.

### Implementation for User Story 1

- [x] T003 [US1] Implement `AsrEngine::transcribe(&self, audio_pcm: &[f32]) -> Result<String>` — lock mutex, create fresh `WhisperState` via `ctx.create_state()`, configure `FullParams` per data-model.md (Greedy best_of=1, language "en", no_speech_thold 0.6, set_suppress_nst(true), single_segment true, no_context true, n_threads 4, set_print_progress(false)). Handle empty audio: if `audio_pcm.is_empty()` return `Ok(String::new())` before calling `state.full()`. Call `state.full(params, audio_pcm)` — catch `WhisperError::NoSamples` and return `Ok(String::new())`. Iterate segments via `state.full_n_segments()` + `state.get_segment(i)` + `segment.to_str()`, collect text, trim, return. Add `///` doc comments. File: `crates/vox_core/src/asr.rs`

- [x] T004 [US1] Write `#[cfg(test)] mod tests` with `#[ignore]` unit tests in `crates/vox_core/src/asr.rs`: (1) `test_asr_model_loads` — load model from `tests/fixtures/ggml-large-v3-turbo-q5_0.bin` with use_gpu=true, assert Ok; (2) `test_asr_transcribe_speech` — load model, read `tests/fixtures/speech_sample.wav` via hound, transcribe, assert non-empty text; (3) `test_asr_empty_audio` — transcribe empty slice `&[]`, assert `Ok("")`; (4) `test_asr_silent_audio` — transcribe 16000 zero-valued f32 samples (1 second of silence), assert `Ok("")`; (5) `test_asr_short_segment` — transcribe first 8000 samples (~0.5s) from speech WAV, assert Ok (no panic); (6) `test_asr_model_load_error` (NOT #[ignore]) — call `AsrEngine::new` with a nonexistent path, assert returns Err with descriptive message.

**Checkpoint**: All 5 #[ignore] US1 tests pass with `cargo test -p vox_core --features cuda -- asr --ignored`. The non-ignored `test_asr_model_load_error` passes with `cargo test -p vox_core --features cuda -- model_load_error`. Model loads, speech transcribes, empty/silent audio returns empty string, bad path returns error.

---

## Phase 4: User Story 2 — Sequential Transcriptions (Priority: P2)

**Goal**: Verify the engine handles repeated transcription calls correctly with no state leakage between utterances.

**Independent Test**: Transcribe 5 segments sequentially, verify each produces correct independent results.

**Note**: No new implementation needed — US1's `transcribe()` already creates fresh `WhisperState` per call (FR-004/FR-008). This phase adds the sequential validation test.

### Implementation for User Story 2

- [x] T005 [US2] Write `#[ignore]` test `test_asr_sequential` in `crates/vox_core/src/asr.rs` — load model, read speech WAV, transcribe the same audio 5 times sequentially, assert all 5 results are identical and non-empty. Additionally, clone the engine via `engine.clone()`, transcribe once more with the clone, assert identical result — validates FR-004 (fresh state) and FR-005 (thread-safe sharing via clone).

**Checkpoint**: `test_asr_sequential` passes. 5 sequential transcriptions produce identical, correct results.

---

## Phase 5: User Story 3 — Force-Segmented Long Speech Stitching (Priority: P3)

**Goal**: Combine transcriptions from force-segmented audio, deduplicating the 1-second overlap region so text reads naturally.

**Independent Test**: Feed two text strings with known overlapping words at the boundary → verify deduplicated output.

**Note**: US3 is fully parallelizable with US1/US2 — stitcher.rs is a separate file operating on pure text with no whisper-rs dependency.

### Implementation for User Story 3

- [x] T006 [P] [US3] Implement `pub fn stitch_segments(previous: &str, next: &str) -> String` in `crates/vox_core/src/asr/stitcher.rs` — split both texts into word tokens (whitespace-split), find longest common subsequence between tail of `previous` and head of `next` using word-level LCS per research R-005, trim duplicate words from `next`, concatenate with space. Handle edge cases: empty previous (return next), empty next (return previous), both empty (return empty), no overlap found (concatenate with space). Add `pub use stitcher::stitch_segments;` re-export in `crates/vox_core/src/asr.rs`. Add `///` doc comments.

- [x] T007 [US3] Write `#[cfg(test)] mod tests` with unit tests in `crates/vox_core/src/asr/stitcher.rs` (no #[ignore] — these don't need the model): (1) `test_stitch_no_overlap` — stitch "hello world" + "foo bar" → "hello world foo bar"; (2) `test_stitch_with_overlap` — stitch "the quick brown fox" + "brown fox jumps over" → "the quick brown fox jumps over"; (3) `test_stitch_empty_inputs` — stitch "" + "hello" → "hello", "hello" + "" → "hello", "" + "" → ""; (4) `test_stitch_identical` — stitch "hello world" + "hello world" → "hello world".

**Checkpoint**: All 4 stitcher tests pass with `cargo test -p vox_core -- stitch` (no --ignored needed). Stitcher correctly deduplicates overlap regions.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Final validation across all stories

- [x] T008 Verify zero warnings with `cargo build -p vox_core --features cuda`, run all non-ignored tests (stitcher + model_load_error) with `cargo test -p vox_core --features cuda`, run all #[ignore] ASR tests with `cargo test -p vox_core --features cuda -- asr --ignored`, validate all quickstart.md verification steps pass, and observe transcription timing via `--nocapture` to confirm transcription completes well under 1 second (manual proxy for SC-001/SC-002 — formal benchmarks deferred to pipeline integration)

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — BLOCKS US1 and US2
- **US1 (Phase 3)**: Depends on Phase 2 — core transcription
- **US2 (Phase 4)**: Depends on US1 (tests validate US1's transcribe impl)
- **US3 (Phase 5)**: Depends on Phase 1 ONLY — can run in parallel with US1/US2
- **Polish (Phase 6)**: Depends on all user stories complete

### User Story Dependencies

- **US1 (P1)**: Blocked by Phase 2 (needs AsrEngine::new). MVP story.
- **US2 (P2)**: Blocked by US1 (sequential test uses transcribe()). Thin phase — test only.
- **US3 (P3)**: Blocked by Phase 1 ONLY. Different file, pure text logic, no whisper-rs.

### Within Each User Story

- Implementation before tests (tests call the implementation)
- US1: transcribe() impl (T003) → tests (T004)
- US2: just test (T005), depends on T003
- US3: stitch impl (T006) → tests (T007)

### Parallel Opportunities

- **T006 + T007 (US3)** can run in parallel with **T003 + T004 + T005 (US1/US2)** — different files, no shared dependencies
- After Phase 2, a second agent can start on US3 while the first works through US1 → US2

---

## Parallel Example: US1 + US3 Concurrent

```text
Agent A (asr.rs):                    Agent B (asr/stitcher.rs):
  T003: Implement transcribe()         T006: Implement stitch_segments()
  T004: Write US1 tests                T007: Write stitcher tests
  T005: Write US2 sequential test
```

Both agents start after Phase 2 (T002) completes. Agent B only needs Phase 1 (T001).

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001)
2. Complete Phase 2: Foundational — AsrEngine::new (T002)
3. Complete Phase 3: US1 — transcribe + tests (T003, T004)
4. **STOP and VALIDATE**: `cargo test -p vox_core --features cuda -- asr --ignored`
5. Single utterance transcription works end-to-end

### Incremental Delivery

1. T001 → T002 → Foundation ready
2. T003 → T004 → US1 complete (MVP — single utterance works)
3. T005 → US2 complete (sequential validation)
4. T006 → T007 → US3 complete (stitching for long speech)
5. T008 → Full validation, zero warnings

### Parallel Strategy

With 2 agents after T001 + T002:
- **Agent A**: T003 → T004 → T005 (US1 + US2, asr.rs)
- **Agent B**: T006 → T007 (US3, stitcher.rs)
- Both complete → T008 (polish)

---

## Notes

- All ASR tests are `#[ignore]` (require ~900 MB model file at `crates/vox_core/tests/fixtures/ggml-large-v3-turbo-q5_0.bin`)
- Stitcher tests do NOT require model — run without `--ignored`
- whisper-rs 0.15.1 API uses `set_suppress_nst()` not `set_suppress_non_speech_tokens()` (research R-002)
- `set_print_progress(false)` must be set explicitly — defaults to true (research R-002)
- `state.full()` returns `Result<c_int>` not `Result<()>` — handle return value (research R-002)
- Empty audio: check `audio_pcm.is_empty()` before calling `state.full()` to avoid `WhisperError::NoSamples` (research R-004)
- Model path in tests: use `env!("CARGO_MANIFEST_DIR")` to build path to `tests/fixtures/`
