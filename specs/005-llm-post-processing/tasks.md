# Tasks: LLM Post-Processing

**Input**: Design documents from `/specs/005-llm-post-processing/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/post-processor.md

**Tests**: Explicitly requested in the feature specification. Unit tests (no model) and integration tests (#[ignore], require model file) are included.

**Organization**: Tasks grouped by user story. US3 (Tone Adaptation) and US5 (Dictionary Hints) are primarily implemented via the system prompt and `process()` parameters in earlier phases, with integration tests in their own phases to verify LLM behavior.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- All file paths relative to repository root

---

## Phase 1: Setup

**Purpose**: Add dependency, populate module root, create prompt constants

- [X] T001 Add `encoding_rs = "0.8"` to `[dependencies]` in crates/vox_core/Cargo.toml
- [X] T002 [P] Populate crates/vox_core/src/llm.rs with `mod processor; mod prompts;` declarations, `pub use processor::{PostProcessor, ProcessorOutput, VoiceCommand};` re-exports, and `//!` module-level doc comment
- [X] T003 [P] Create crates/vox_core/src/llm/prompts.rs with `SYSTEM_PROMPT` constant (all 8 rules from spec: filler removal, punctuation, course correction, formatting, tone adaptation, command detection, voice preservation, output-only constraint) and `build_user_message(active_app: &str, dictionary_hints: &str, raw_text: &str) -> String` helper that formats the user message block

---

## Phase 2: Foundational (Core PostProcessor)

**Purpose**: Implement the PostProcessor struct, output types, and inference loop that ALL user stories depend on

- [X] T004 Implement `PostProcessor` struct with `backend: Arc<LlamaBackend>`, `model: Arc<LlamaModel>`, `system_prompt_tokens: Arc<Vec<LlamaToken>>`, `chat_template: Arc<String>` fields, `new(model_path: &Path, use_gpu: bool) -> Result<Self>` constructor (LlamaBackend::init, void_logs, model load with GPU layers, chat template extraction, system prompt tokenization via apply_chat_template + str_to_token with AddBos::Never), and `Clone` impl via Arc::clone in crates/vox_core/src/llm/processor.rs
- [X] T005 Implement `ProcessorOutput` enum (Text(String), Command(VoiceCommand)), `VoiceCommand` struct (cmd: String, args: Option<Value> with #[serde(default)], derive Deserialize), and `parse_output(raw: &str) -> ProcessorOutput` helper (trim, check starts_with '{', attempt serde_json::from_str, fall back to Text on invalid JSON) in crates/vox_core/src/llm/processor.rs
- [X] T006 Implement private `run_inference(&self, prompt_tokens: &[LlamaToken]) -> Result<String>` method in crates/vox_core/src/llm/processor.rs — create fresh LlamaContext with 2048-token context window, build LlamaBatch, encode prompt tokens (logits=true only for last token), sample in loop with LlamaSampler::chain_simple([temp(0.1), top_p(0.9, 1), dist(seed)]), decode tokens via model.token_to_piece with encoding_rs UTF_8 decoder, stop on model.is_eog_token() or newline or 512 max tokens

**Checkpoint**: PostProcessor can load a model and run inference. All user story implementations build on this.

---

## Phase 3: User Story 1 — Polish Raw Transcript (Priority: P1) MVP

**Goal**: Raw ASR transcripts are cleaned up — fillers removed, punctuation fixed, course corrections applied, numbers/dates/emails formatted.

**Independent Test**: Feed raw transcript text into `process()`, verify polished output with correct formatting and no fillers.

### Tests for User Story 1

- [X] T007 [P] [US1] Write unit tests in crates/vox_core/src/llm/processor.rs: `test_output_parsing_text` (plain text string → `ProcessorOutput::Text`), `test_output_parsing_invalid_json` (string starting with `{` but malformed → `ProcessorOutput::Text`), `test_empty_input` (empty string → `ProcessorOutput::Text("")`)
- [X] T008 [P] [US1] Write unit test `test_prompt_construction` in crates/vox_core/src/llm/processor.rs — call `build_user_message` with sample active_app, dictionary_hints, and raw_text, assert the returned string contains all three fields in expected format

### Implementation for User Story 1

- [X] T009 [US1] Implement `process(&self, raw_text: &str, dictionary_hints: &str, active_app: &str) -> Result<ProcessorOutput>` in crates/vox_core/src/llm/processor.rs — return Text("") for empty input, build user message via `build_user_message()`, format full prompt via `model.apply_chat_template()` with system + user messages and add_ass=true, tokenize with `AddBos::Never`, call `run_inference()`, pass result to `parse_output()`

### Error & Integration Tests for User Story 1

- [X] T010 [P] [US1] Write error test `test_llm_model_load_error` in crates/vox_core/src/llm/processor.rs — call `PostProcessor::new()` with nonexistent path, assert Err with descriptive message
- [X] T011 [US1] Write integration tests (#[ignore]) in crates/vox_core/src/llm/processor.rs: `test_llm_model_loads` (load model from fixtures, assert Ok), `test_llm_filler_removal` ("um uh let's meet" → no fillers), `test_llm_course_correction` ("tuesday no wait wednesday" → correction only), `test_llm_number_formatting` ("twenty five dollars" → "$25"), `test_llm_email_formatting` ("john at outlook dot com" → "john@outlook.com"), `test_llm_empty_input` (empty string with model → Text(""))

**Checkpoint**: Core text polishing works end-to-end. All unit tests pass without model. Integration tests pass with model file.

---

## Phase 4: User Story 2 — Detect and Route Voice Commands (Priority: P2)

**Goal**: Voice commands like "delete that" return structured `ProcessorOutput::Command` instead of text. Wake word "hey vox" prefix triggers command-emphasis routing.

**Independent Test**: Feed command phrases and wake-word-prefixed input into `process()`, verify structured command output.

### Tests for User Story 2

- [X] T012 [P] [US2] Write unit tests in crates/vox_core/src/llm/processor.rs: `test_output_parsing_command` (valid JSON `{"cmd":"delete_last"}` → `ProcessorOutput::Command`), `test_wake_word_detection` ("hey vox delete that" → wake word detected, prefix stripped), `test_wake_word_case_insensitive` ("Hey Vox delete that" → detected), `test_wake_word_not_in_middle` ("I said hey vox" → NOT detected)

### Implementation for User Story 2

- [X] T013 [US2] Add `detect_wake_word(text: &str) -> Option<&str>` function (case-insensitive "hey vox" prefix check, returns remaining text after prefix) and `build_user_message_with_command_emphasis(active_app, dictionary_hints, raw_text)` variant to crates/vox_core/src/llm/prompts.rs. Integrate into `process()` in crates/vox_core/src/llm/processor.rs — when wake word detected, strip prefix and use command-emphasis prompt

### Integration Tests for User Story 2

- [X] T014 [US2] Write integration test (#[ignore]) `test_llm_command_detection` in crates/vox_core/src/llm/processor.rs — "delete that" → `ProcessorOutput::Command { cmd: "delete_last", .. }`

**Checkpoint**: Voice commands are detected and returned as structured data. Wake word routing works.

---

## Phase 5: User Story 3 — Tone Adaptation (Priority: P3)

**Goal**: Output style adapts to the active application — formal for email, casual for chat, technical for code editors.

**Implementation**: Tone adaptation is implemented by `SYSTEM_PROMPT` rule 5 (T003), `active_app` parameter in `build_user_message()` (T003), and `process()` (T009). `test_prompt_construction` (T008) verifies `active_app` appears in the prompt.

### Integration Test for User Story 3

- [X] T015 [US3] Write integration test (#[ignore]) `test_llm_tone_adaptation` in crates/vox_core/src/llm/processor.rs — process the same transcript "hey how are you doing" with active_app "Outlook" and active_app "Slack", assert both return Text output (no crash, valid output). Verify outputs differ or at minimum the prompt correctly includes the active_app context.

**Checkpoint**: Tone adaptation is verified via the LLM producing valid output for different app contexts.

---

## Phase 6: User Story 4 — Stream Tokens for Low Perceived Latency (Priority: P4)

**Goal**: Tokens are delivered via callback as generated for text output. Commands are NOT streamed — collected in full before returning.

**Independent Test**: Process a transcript with a streaming callback, verify tokens arrive incrementally. Verify commands are not streamed.

### Implementation for User Story 4

- [X] T016 [US4] Implement `process_streaming(&self, raw_text: &str, dictionary_hints: &str, active_app: &str, on_token: impl FnMut(&str)) -> Result<ProcessorOutput>` in crates/vox_core/src/llm/processor.rs — same setup as `process()`, but during token generation: if first token starts with `{`, switch to command accumulation mode (no callback); otherwise call `on_token(&piece)` for each decoded token. Return collected output via `parse_output()`

### Integration Tests for User Story 4

- [X] T017 [US4] Write integration tests (#[ignore]) in crates/vox_core/src/llm/processor.rs: `test_llm_streaming` (text output → callback invoked with tokens, verify non-empty tokens received), `test_llm_command_not_streamed` ("delete that" → callback NOT invoked, command returned directly)

**Checkpoint**: Streaming works for text output, commands bypass streaming.

---

## Phase 7: User Story 5 — Dictionary Hints for Domain Terms (Priority: P5)

**Goal**: Domain-specific dictionary hints improve post-processing accuracy for specialized terms.

**Implementation**: Dictionary hints are implemented by `SYSTEM_PROMPT` (T003), `dictionary_hints` parameter in `build_user_message()` (T003), and `process()` (T009). `test_prompt_construction` (T008) verifies `dictionary_hints` appear in the prompt.

### Integration Test for User Story 5

- [X] T018 [US5] Write integration test (#[ignore]) `test_llm_dictionary_hints` in crates/vox_core/src/llm/processor.rs — process a transcript with dictionary_hints containing domain terms (e.g., "Kubernetes\nPrometheus"), verify the output is valid Text (no crash). Optionally assert the hints influence output when transcript contains a phonetically similar term.

**Checkpoint**: Dictionary hints are passed to the LLM and influence post-processing.

---

## Phase 8: Polish & Cross-Cutting Concerns

**Purpose**: Build verification, test validation, quickstart verification

- [X] T019 Verify `cargo build -p vox_core --features cuda` produces zero warnings
- [X] T020 Run `cargo test -p vox_core --features cuda` and verify all non-ignored tests pass (unit + error tests)
- [X] T021 Run quickstart.md verification: `cargo test -p vox_core --features cuda -- llm --ignored --nocapture` to verify all integration tests pass with model file. Observe inference timing in output to confirm post-processing completes well under 100ms for short transcripts on RTX 4090 (manual proxy for NFR-001/NFR-002 — formal benchmarks deferred to pipeline integration)

**Note on NFR-003/004/005**: Model memory (~2.2 GB), combined memory (<6 GB), and load time (<5s) are properties of the Qwen 2.5 3B Q4_K_M model and hardware, not implementation tasks. They are validated by model selection (research.md R-001) and confirmed during T021 quickstart verification.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup (T001 for encoding_rs, T002 for module structure, T003 for prompts)
- **US1 (Phase 3)**: Depends on Foundational (T004-T006 provide PostProcessor and inference)
- **US2 (Phase 4)**: Depends on US1 (T009 provides process() to integrate wake word into)
- **US3 (Phase 5)**: Depends on US1 (T009 provides process() — test verifies tone behavior)
- **US4 (Phase 6)**: Depends on US1 (T009 provides process() pattern to extend for streaming)
- **US5 (Phase 7)**: Depends on US1 (T009 provides process() — test verifies hints behavior)
- **Polish (Phase 8)**: Depends on all implementation phases complete

### Within Each Phase

- Unit tests (T007, T008, T012) can be written before implementation — they test helpers directly
- Integration tests (T011, T014, T015, T017, T018) depend on implementation being complete
- Error test (T010) is independent of implementation

### Parallel Opportunities

- **Phase 1**: T002 || T003 (different files: llm.rs vs llm/prompts.rs)
- **Phase 3**: T007 || T008 || T010 (independent test functions)
- **Phase 4**: T012 can run parallel with T007/T008 (independent test functions)
- **Phase 5-7**: US3, US4, US5 can all start after US1, independent of each other
- **Phase 6**: US4 can start after US1, independent of US2

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001-T003)
2. Complete Phase 2: Foundational (T004-T006)
3. Complete Phase 3: US1 — Polish Raw Transcript (T007-T011)
4. **STOP and VALIDATE**: Run `cargo test -p vox_core --features cuda` — unit tests pass
5. Run `cargo test -p vox_core --features cuda -- llm --ignored` — integration tests pass with model

### Incremental Delivery

1. Setup + Foundational → Core infrastructure ready
2. Add US1 → Text polishing works → **MVP!**
3. Add US2 → Voice commands and wake word → Command routing works
4. Add US3 → Tone adaptation integration test → Verified
5. Add US4 → Token streaming → Lower perceived latency
6. Add US5 → Dictionary hints integration test → Verified
7. Polish → Zero warnings, all tests pass

---

## Notes

- All source files are in `crates/vox_core/src/llm/` (processor.rs, prompts.rs)
- All tests are in the same files as implementation (`#[cfg(test)] mod tests`)
- Integration tests use `#[ignore]` — require model file at `crates/vox_core/tests/fixtures/qwen2.5-3b-instruct-q4_k_m.gguf`
- Model file is ~1.6 GB, gitignored via `*.gguf` pattern
- US3 and US5 implementation is inherent in the system prompt and `process()` parameters — their phases add integration tests to verify LLM behavior
