# Tasks: Voice Activity Detection

**Input**: Design documents from `/specs/003-voice-activity-detection/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/vad-api.md, quickstart.md

**Tests**: Included — explicitly defined in the feature specification Testing Requirements section.

**Organization**: Tasks grouped by user story. US1 (SileroVad), US2 (VadStateMachine), and US3 (SpeechChunker) are in separate files and can run in parallel after foundational types are in place. US4 (processing loop) depends on all three.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1, US2, US3, US4)
- All paths relative to repository root

---

## Phase 1: Setup

**Purpose**: Create the vad module directory structure and download test fixtures

- [x] T001 Create vad module structure: edit existing `crates/vox_core/src/vad.rs` to add `pub mod silero;` and `pub mod chunker;` declarations plus placeholder re-exports. Create `crates/vox_core/src/vad/` directory with empty `silero.rs` and `chunker.rs` files. Verify `cargo build -p vox_core --features cuda` compiles with zero warnings.
- [x] T002 [P] Download test fixtures to `crates/vox_core/tests/fixtures/`. Create the directory if it does not exist. (1) Download Silero VAD v5 ONNX model to `silero_vad_v5.onnx` from `https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx` (~1.1 MB). (2) Download a short speech WAV to `speech_sample.wav` from `https://github.com/snakers4/silero-vad/raw/master/files/en_example.wav` — this is Silero's own English test recording (~3s, 16 kHz mono). This fixture is required by `test_vad_speech_audio` and the end-to-end integration tests.

---

## Phase 2: Foundational (Shared Types)

**Purpose**: Core types needed by ALL user stories — VadConfig, VadState, VadEvent

**CRITICAL**: No user story work can begin until this phase is complete

- [x] T003 Implement `VadConfig` struct with all 6 fields (threshold, min_speech_ms, min_silence_ms, max_speech_ms, speech_pad_ms, window_size_samples) and `Default` trait in `crates/vox_core/src/vad.rs`. Derive `Clone`, `Debug`. Default values: threshold=0.5, min_speech_ms=250, min_silence_ms=500, max_speech_ms=30_000, speech_pad_ms=100, window_size_samples=512. Add `///` doc comments to struct and every field. See data-model.md for field types and descriptions.
- [x] T004 Implement `VadState` enum (Silent, Speaking { start_sample: usize, speech_duration_ms: u32 }) and `VadEvent` enum (SpeechStart, SpeechEnd { duration_ms: u32 }, ForceSegment { duration_ms: u32 }) in `crates/vox_core/src/vad.rs`. Derive `Debug`, `Clone`, `PartialEq` on both. Add `///` doc comments to enums and every variant. See data-model.md for variant definitions.

**Checkpoint**: Shared types compile. `cargo build -p vox_core --features cuda` passes with zero warnings.

---

## Phase 3: User Story 1 — Speech Detection from Microphone Stream (Priority: P1) MVP

**Goal**: Load Silero VAD v5 via ONNX Runtime and produce speech probabilities for 512-sample audio windows.

**Independent Test**: Feed silence (all zeros) and verify speech_prob < 0.1. Feed the model with consecutive windows and verify hidden state persists. Reset and verify state returns to zeros.

### Implementation for User Story 1

- [x] T005 [US1] Implement `SileroVad` struct in `crates/vox_core/src/vad/silero.rs`: fields are `session: Session`, `hidden_state: Vec<f32>` (256 elements), `sample_rate: i64` (16000). Implement `new(model_path: &Path) -> Result<Self>` that creates an ort `Session` via `Session::builder()?.with_optimization_level(Level3)?.with_intra_threads(1)?.commit_from_file(model_path)?`, initializes hidden state to zeros, and logs model input/output names for verification. See research.md R-001 for ort 2.0 API patterns. Add `///` doc comments to struct and all methods.
- [x] T006 [US1] Implement `SileroVad::process(&mut self, audio: &[f32]) -> Result<f32>` in `crates/vox_core/src/vad/silero.rs`: validate input length equals `window_size_samples` (512), create input tensors (audio [1, 512], sr [1] = 16000, state [2, 1, 128] from self.hidden_state), run `session.run(ort::inputs!{...})`, extract speech probability from output tensor `"output"`, copy updated hidden state from `"stateN"` output back to self.hidden_state. Return speech_prob clamped to [0.0, 1.0]. See research.md R-002 for tensor names.
- [x] T007 [US1] Implement `SileroVad::reset(&mut self)` in `crates/vox_core/src/vad/silero.rs`: fill `self.hidden_state` with 0.0. Add re-export `pub use silero::SileroVad;` in `crates/vox_core/src/vad.rs`.

### Tests for User Story 1

- [x] T008 [US1] Write integration tests (all `#[ignore]` since they require fixture files) in `crates/vox_core/src/vad/silero.rs`: (1) `test_vad_model_loads` — load model from fixtures path, assert Ok. (2) `test_vad_silent_audio` — feed 10 windows of zeros, assert all speech_prob < 0.1. (3) `test_vad_speech_audio` — load `speech_sample.wav` from fixtures, extract 512-sample windows, feed to VAD, assert speech_prob > 0.5 for at least 50% of windows (the sample contains speech). (4) `test_vad_hidden_state_persistence` — feed 3 consecutive windows, verify hidden_state is not all zeros after processing. (5) `test_vad_reset` — process a window, call reset(), verify hidden_state is all zeros. Use path `env!("CARGO_MANIFEST_DIR")` + `/tests/fixtures/` for both model and WAV files. Add `hound = "3.5"` as a dev-dependency in Cargo.toml for WAV file reading in tests.

**Checkpoint**: SileroVad loads model and produces speech probabilities. Integration tests pass with `cargo test -p vox_core --features cuda -- vad::silero --ignored --nocapture`.

---

## Phase 4: User Story 2 — Utterance Segmentation via State Machine (Priority: P1)

**Goal**: Convert a stream of speech probabilities into discrete SpeechStart, SpeechEnd, and ForceSegment events with configurable timing thresholds.

**Independent Test**: Feed synthetic probability sequences (no model needed) and verify correct state transitions and events.

### Implementation for User Story 2

- [x] T009 [US2] Implement `VadStateMachine` struct in `crates/vox_core/src/vad.rs`: fields are `config: VadConfig`, `state: VadState`, `silence_duration_ms: u32`, `total_samples_processed: usize`. Implement `new(config: VadConfig) -> Self` starting in Silent state with zero counters. Add `///` doc comments to struct and all methods.
- [x] T010 [US2] Implement `VadStateMachine::update(&mut self, speech_prob: f32) -> Option<VadEvent>` in `crates/vox_core/src/vad.rs`. Window duration is `config.window_size_samples as f32 / 16000.0 * 1000.0` ms (32ms). Transition logic per data-model.md state transitions: (1) Silent + prob >= threshold → emit SpeechStart, enter Speaking. (2) Speaking + prob >= threshold → increment speech_duration_ms, reset silence_duration_ms. Check force-segment at max_speech_ms. (3) Speaking + prob < threshold → increment silence_duration_ms. If >= min_silence_ms: if speech_duration_ms >= min_speech_ms emit SpeechEnd, else discard silently. Return to Silent. Increment total_samples_processed by window_size_samples on every call.
- [x] T011 [US2] Implement `VadStateMachine::state(&self) -> &VadState` and `VadStateMachine::reset(&mut self)` in `crates/vox_core/src/vad.rs`. Reset restores Silent state and zeroes all counters.

### Tests for User Story 2

- [x] T012 [US2] Write unit tests in `crates/vox_core/src/vad.rs`: (1) `test_state_machine_silent_to_speaking` — feed prob 0.8, verify SpeechStart emitted and state is Speaking. (2) `test_state_machine_speaking_to_silent` — enter Speaking, feed ~16 windows of prob 0.1 (>500ms of silence), verify SpeechEnd emitted with correct duration. (3) `test_state_machine_force_segment` — enter Speaking, feed ~938 windows of prob 0.8 (>30s), verify ForceSegment emitted. (4) `test_state_machine_min_speech` — enter Speaking for 4 windows (~128ms < 250ms), then silence, verify no SpeechEnd event (discarded as noise). (5) `test_state_machine_brief_pause` — enter Speaking, feed 8 windows of silence (~256ms < 500ms), then speech again, verify no SpeechEnd (stayed in Speaking).

**Checkpoint**: State machine passes all 5 unit tests. `cargo test -p vox_core --features cuda -- vad::test_state_machine`.

---

## Phase 5: User Story 3 — Speech Segment Delivery with Context Padding (Priority: P2)

**Goal**: Accumulate audio during speech, apply pre/post padding, handle force-segment overlap, and emit complete segments.

**Independent Test**: Feed synthetic samples + VadEvents and verify output segment lengths, padding inclusion, and overlap behavior.

### Implementation for User Story 3

- [x] T013 [US3] Implement `SpeechChunker` struct in `crates/vox_core/src/vad/chunker.rs`: fields are `config: VadConfig`, `speech_buffer: Vec<f32>`, `pre_buffer: Vec<f32>` (circular, capacity = speech_pad_ms * 16 samples = 1600), `pre_buffer_pos: usize`, `is_accumulating: bool`, `post_pad_remaining: u32`. Implement `new(config: VadConfig) -> Self` initializing pre_buffer to capacity with zeros. Add `///` doc comments.
- [x] T014 [US3] Implement `SpeechChunker::feed(&mut self, samples: &[f32], event: Option<&VadEvent>) -> Option<Vec<f32>>` in `crates/vox_core/src/vad/chunker.rs`. Logic: (1) Always write samples to circular pre_buffer. (2) On SpeechStart: set is_accumulating=true, prepend pre_buffer contents (ordered from oldest to newest) to speech_buffer. (3) While accumulating: append samples to speech_buffer. (4) On SpeechEnd: continue accumulating post_pad_remaining samples, then emit speech_buffer and reset. (5) On ForceSegment: copy last 16000 samples (1s overlap) as start of next buffer, emit current speech_buffer, start new accumulation with overlap. Add re-export `pub use chunker::SpeechChunker;` in vad.rs.
- [x] T015 [US3] Implement `SpeechChunker::flush(&mut self) -> Option<Vec<f32>>` in `crates/vox_core/src/vad/chunker.rs`: if is_accumulating and speech_buffer is non-empty, return speech_buffer contents and reset state. Otherwise return None.

### Tests for User Story 3

- [x] T016 [US3] Write unit tests in `crates/vox_core/src/vad/chunker.rs`: (1) `test_chunker_accumulates` — feed SpeechStart + 3 batches of samples, verify speech_buffer grows. (2) `test_chunker_emits_on_end` — feed SpeechStart, samples, SpeechEnd + post-pad samples, verify Some(segment) returned with correct length. (3) `test_chunker_padding` — verify emitted segment is longer than raw speech by approximately 2 × pad_ms of samples (pre + post padding). (4) `test_chunker_flush` — feed SpeechStart + samples, call flush(), verify partial segment returned. (5) `test_chunker_force_segment_overlap` — feed SpeechStart + >10s of samples + ForceSegment, verify emitted segment and that the next accumulation starts with 1s overlap (16000 samples).

**Checkpoint**: SpeechChunker passes all 5 unit tests. `cargo test -p vox_core --features cuda -- vad::chunker::test_chunker`.

---

## Phase 6: User Story 4 — End-to-End VAD Processing Loop (Priority: P2)

**Goal**: Wire SileroVad + VadStateMachine + SpeechChunker into a processing loop that reads from the ring buffer and dispatches segments via channel.

**Independent Test**: Feed a synthetic multi-utterance audio stream through the full pipeline and verify correct segment count.

### Implementation for User Story 4

- [x] T017 [US4] Implement the VAD processing loop function in `crates/vox_core/src/vad.rs`. Signature: `pub fn run_vad_loop(consumer: &mut HeapCons<f32>, resampler: Option<&mut AudioResampler>, vad: &mut SileroVad, state_machine: &mut VadStateMachine, chunker: &mut SpeechChunker, segment_tx: &tokio::sync::mpsc::Sender<Vec<f32>>, stop: &std::sync::atomic::AtomicBool) -> Result<()>`. Loop: read available samples from consumer via pop_slice, optionally resample, extract 512-sample windows from accumulation buffer, for each window call vad.process() then state_machine.update() then chunker.feed(), if segment ready call segment_tx.try_send(). Sleep 5ms when < 512 samples available. Exit when stop flag is set. Log errors from vad.process() and skip window on failure.

### Tests for User Story 4

- [x] T018 [US4] Write integration test in `crates/vox_core/src/vad.rs` (`#[ignore]`, requires fixture files): `test_vad_end_to_end` — load `speech_sample.wav` from fixtures via `hound`, create a ring buffer, push 1s of silence + the speech WAV samples + 1s of silence into producer, run the processing loop in a thread with stop flag, verify at least 1 segment is received on segment_rx within 5 seconds. Use real speech audio (not sine waves) because Silero VAD is trained on human voice spectra and may not detect pure tones.
- [x] T019 [US4] Write integration test in `crates/vox_core/src/vad.rs` (`#[ignore]`, requires fixture files): `test_vad_multiple_utterances` — load `speech_sample.wav` from fixtures, push 3 copies of the speech samples each separated by 1s silence into the ring buffer producer, run processing loop, verify exactly 3 segments received on segment_rx.

**Checkpoint**: Full VAD pipeline processes audio and dispatches segments. Integration tests pass with `cargo test -p vox_core --features cuda -- vad --ignored --nocapture`.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Final verification across all components

- [x] T020 Verify all `pub` items in `crates/vox_core/src/vad.rs`, `crates/vox_core/src/vad/silero.rs`, and `crates/vox_core/src/vad/chunker.rs` have `///` doc comments per Constitution Principle VII. Add module-level `//!` doc to `crates/vox_core/src/vad.rs`.
- [x] T021 Run `cargo build -p vox_core --features cuda` and verify zero warnings. Fix any warnings.
- [x] T022 Run `cargo test -p vox_core --features cuda -- vad` and verify all unit tests pass. Run `cargo test -p vox_core --features cuda -- vad --ignored` and verify all integration tests pass. Document results.
- [x] T023 Validate quickstart.md: follow all build and test commands in `specs/003-voice-activity-detection/quickstart.md` and verify they produce expected results. Fix any discrepancies.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on T001 (module structure exists)
- **US1, US2, US3 (Phases 3-5)**: All depend on Phase 2 completion. **Can run in parallel** — each is in a separate file:
  - US1: `vad/silero.rs` (SileroVad)
  - US2: `vad.rs` (VadStateMachine)
  - US3: `vad/chunker.rs` (SpeechChunker)
- **US4 (Phase 6)**: Depends on US1 + US2 + US3 all being complete (wires them together)
- **Polish (Phase 7)**: Depends on all user stories complete

### User Story Dependencies

- **US1 (P1)**: Depends only on Phase 2. Independent of US2, US3.
- **US2 (P1)**: Depends only on Phase 2. Independent of US1, US3.
- **US3 (P2)**: Depends only on Phase 2 (uses VadEvent from foundational types). Independent of US1, US2.
- **US4 (P2)**: Depends on US1 + US2 + US3 (integrates all three components).

### Within Each User Story

- Implementation tasks before test tasks
- Core struct/constructor before methods
- Methods before integration

### Parallel Opportunities

Within Phase 1:
- T001 and T002 can run in parallel (directory setup vs model download)

After Phase 2, these phases can run in **full parallel**:
- Phase 3 (US1: SileroVad in silero.rs)
- Phase 4 (US2: VadStateMachine in vad.rs)
- Phase 5 (US3: SpeechChunker in chunker.rs)

---

## Parallel Example: User Stories 1-3 (after Foundational)

```text
# All three user stories in parallel (different files, no dependencies):
Agent A: T005, T006, T007, T008  → SileroVad complete
Agent B: T009, T010, T011, T012  → VadStateMachine complete
Agent C: T013, T014, T015, T016  → SpeechChunker complete

# Then sequentially:
Any:     T017, T018, T019        → Processing loop (needs all three)
Any:     T020, T021, T022, T023  → Polish
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001-T002)
2. Complete Phase 2: Foundational types (T003-T004)
3. Complete Phase 3: SileroVad (T005-T008)
4. **STOP and VALIDATE**: Model loads, produces probabilities, integration tests pass
5. This proves ONNX Runtime integration works end-to-end

### Incremental Delivery

1. Setup + Foundational → Module compiles
2. Add US1 (SileroVad) → ONNX inference works
3. Add US2 (VadStateMachine) → Speech boundaries detected
4. Add US3 (SpeechChunker) → Padded segments produced
5. Add US4 (Processing Loop) → Full pipeline operational
6. Each story adds testable value

---

## Notes

- [P] tasks = different files, no dependencies on in-progress tasks
- [Story] label maps task to specific user story for traceability
- US1/US2/US3 are in separate files (silero.rs, vad.rs, chunker.rs) — true parallel opportunity
- Integration tests require the model file and speech WAV in `crates/vox_core/tests/fixtures/`
- Use `env!("CARGO_MANIFEST_DIR")` in tests to locate fixture files portably
- The ort 2.0 API uses `state`/`stateN` tensor names (not `h`/`hn` from spec) — see research.md R-002
- Commit after each phase checkpoint
