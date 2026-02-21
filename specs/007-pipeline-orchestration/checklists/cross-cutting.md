# Cross-Cutting Requirements Quality Checklist: Pipeline Orchestration

**Purpose**: Deep cross-artifact requirements quality validation — concurrency, state machine, persistence, platform, and performance. Dual-audience: pre-implementation gate + PR/design review.
**Created**: 2026-02-20
**Feature**: [spec.md](../spec.md) | [plan.md](../plan.md) | [research.md](../research.md) | [data-model.md](../data-model.md)

## Requirement Completeness

- [x] CHK001 - Is the channel capacity for segment delivery (32) justified with a rationale tied to expected throughput, or is it an arbitrary constant? [Completeness, Research §R-008]
  - Fixed: Added throughput analysis justification in R-008 — 32× headroom over worst-case backlog, ~768 bytes overhead
- [x] CHK002 - Is the channel capacity for PipelineCommand (8) justified, given that only one command type (Stop) currently exists? [Completeness, Research §R-010]
  - Fixed: Added capacity justification in R-010 — commands consumed near-instantly, 8 provides room for future variants
- [x] CHK003 - Are requirements defined for the pipeline state between `start()` and `run()` — what PipelineState is the pipeline in after start() returns but before run()'s select loop begins? [Gap, Contract §Pipeline]
  - Fixed: Added doc comment on start() — pipeline is in Listening state, segments buffered in channel until run() drains
- [x] CHK004 - Are shutdown sequencing requirements specified — specifically the ordering of: (1) set stop flag, (2) join VAD thread, (3) stop AudioCapture, (4) close channels? [Gap, Research §R-002]
  - Fixed: Added 4-step shutdown sequence with ordering rationale in R-002 lifecycle
- [x] CHK005 - Is the "energy below threshold" criterion in FR-012 quantified with a specific numeric threshold, or does it rely on an undefined VAD-internal heuristic? [Completeness, Spec §FR-012]
  - Fixed: FR-012 now specifies RMS energy < 1e-3 threshold, explains Whisper hallucination on silent input
- [x] CHK006 - Are requirements specified for what happens when `spawn_blocking` fails (e.g., tokio blocking pool exhaustion during ASR or LLM)? [Gap, Research §R-001]
  - Fixed: Added spawn_blocking failure handling section in R-001 — JoinError treated as segment error per R-009
- [x] CHK007 - Is the database file location specified — are dictionary and transcript tables in the same SQLite file, and where is the file created (app data directory, but which one per platform)? [Completeness, Data-Model §Database Schema]
  - Fixed: Added Database Location section — single vox.db file, platform-specific paths documented
- [x] CHK008 - Are requirements defined for DictionaryCache reload — if the user adds a dictionary entry via a future UI, how does the running pipeline receive updated substitutions? [Gap, Contract §DictionaryCache]
  - Fixed: Added reload() method to DictionaryCache contract with Arc snapshot semantics
- [x] CHK009 - Are requirements specified for how `duration_ms` on TranscriptEntry is computed — from first speech detection to segment emission, or total audio length in the segment buffer? [Completeness, Data-Model §TranscriptEntry]
  - Fixed: Clarified duration_ms = segment.len() / 16000 * 1000, latency_ms = recv-to-injection time

## Requirement Clarity

- [x] CHK010 - Is "still activated" unambiguously defined in the Error→Listening vs Error→Idle transition? The spec says "returns to Listening (if still activated) or Idle (if not)" but doesn't specify how activation state is determined after an error. [Ambiguity, Spec §Edge Cases ¶Pipeline error mid-segment]
  - Fixed: Edge case now explicitly references PipelineController's `is_active` flag as the activation state determinant
- [x] CHK011 - FR-020 states "O(1) lookup for substitutions" but the two-pass algorithm in Research R-004 is O(p×n) for phrases. Is the FR-020 wording accurate, or should it say "O(1) for single-word lookups, O(p×n) for phrase substitutions"? [Conflict, Spec §FR-020 vs Research §R-004]
  - Fixed: FR-020 now specifies O(1) for single-word + O(p×n) for phrases, with typical-case sub-microsecond note
- [x] CHK012 - Is the 300ms double-press window boundary condition defined — is a press at exactly 300ms elapsed treated as a double-press or a single press? [Ambiguity, Spec §FR-006]
  - Fixed: FR-006 now specifies "exclusive — a press at exactly 300ms elapsed is treated as a single press"
- [x] CHK013 - Does "processes remaining audio" (used in FR-004, FR-005, US2 scenarios 1-3) clearly define what happens — does the VAD flush its internal buffer, or does it only emit whatever segments were already chunked? [Ambiguity, Spec §FR-004/FR-005]
  - Fixed: FR-004 now defines flush semantics — VAD force-emits buffered samples as final segment, processed through full pipeline
- [x] CHK014 - Is "human-readable message" in FR-009 sufficiently specified — are there message format requirements (max length, allowed characters, i18n considerations), or is any non-empty string acceptable? [Clarity, Spec §FR-009]
  - Fixed: FR-009 now specifies non-empty English string, no format constraints, UI handles truncation
- [x] CHK015 - Is "segment" consistently defined across artifacts — does it always mean the `Vec<f32>` output of the VAD chunker, or does it sometimes refer to the ASR transcript of that audio? [Ambiguity, Cross-artifact]
  - Fixed: Added Terminology section to spec — "segment" always means raw audio Vec<f32>, never transcript text

## Requirement Consistency

- [x] CHK016 - The spec lists 6 required components in FR-002 (audio, VAD, ASR, LLM, injector, dictionary) but Pipeline::new() in the contract takes only 4 plus config (asr, llm, dictionary, transcript_store) — injector and audio are absent from the constructor. Is this consistent or is the validation deferred to start()? [Consistency, Spec §FR-002 vs Contract §Pipeline::new]
  - Fixed: Pipeline::new() doc comment now explains which 4 of 6 components it takes and why (Send+Sync), and where the other 2 (AudioCapture, TextInjector) are provided
- [x] CHK017 - US1 acceptance scenario 6 says "the UI shows which component is not ready" but no PipelineState variant carries a list of missing components — Error only has a `message` string. Are these consistent? [Consistency, Spec §US1.6 vs Data-Model §PipelineState]
  - Fixed: US1.6 now specifies the Error message string carries component names (e.g., "Pipeline cannot start: ASR model not loaded")
- [x] CHK018 - The plan's quickstart says "No changes needed" for Cargo.toml, but research mentions parking_lot::Mutex for TranscriptStore — is parking_lot already a workspace dependency, or is this an undocumented new dependency? [Consistency, Quickstart vs Research §R-005]
  - Fixed: Verified parking_lot 0.12 is in workspace Cargo.toml and vox_core. Quickstart updated to list parking_lot explicitly.
- [x] CHK019 - DictionaryEntry data-model shows `frequency: u32` but no requirement specifies when or how frequency is incremented. Is frequency updated on each substitution hit, or is it purely user-managed? [Gap, Data-Model §DictionaryEntry]
  - Fixed: DictionaryEntry.frequency description now specifies pipeline increments on each substitution match, with async batched DB persistence
- [x] CHK020 - The PipelineController contract shows `set_mode(&mut self, mode: ActivationMode)` but US2 scenario 4 says "changes the mode in settings" — are these the same operation, or is there a settings-layer indirection not reflected in the contract? [Consistency, Contract §PipelineController vs Spec §US2.4]
  - Fixed: set_mode() doc comment now clarifies it's the same operation — UI calls this method, which updates both in-memory and SQLite persistence

## Acceptance Criteria Quality

- [x] CHK021 - SC-001 specifies latency from "utterance completion" — is "utterance completion" defined as the moment the user stops speaking, the moment VAD emits the segment, or the moment the segment arrives in the pipeline's channel? [Measurability, Spec §SC-001]
  - Fixed: SC-001 now defines "utterance completion" as moment VAD emits the segment, matching TranscriptEntry.latency_ms
- [x] CHK022 - SC-005 tests "3 separate utterances" for no-drop/no-contamination, but FR-017 requires FIFO for any number of segments. Are acceptance criteria defined for rapid bursts (>3 segments) or pathological ordering scenarios? [Coverage, Spec §SC-005 vs §FR-017]
  - Fixed: SC-005 now includes a 10-utterance rapid burst stress test in addition to the 3-utterance basic test
- [x] CHK023 - SC-009 requires 95% filler removal — is the test corpus, methodology, and sample size for measuring this percentage specified anywhere? [Measurability, Spec §SC-009]
  - Fixed: SC-009 now specifies 20-utterance test corpus, accuracy formula, and test methodology
- [x] CHK024 - SC-010 requires 95% voice command classification accuracy — is the command vocabulary exhaustively listed, or could new commands appear that aren't covered by the 95% target? [Measurability, Spec §SC-010]
  - Fixed: SC-010 now lists exhaustive closed set of 6 commands, specifies 10 tests per command (60 total), and notes new commands require spec update

## Scenario Coverage

- [x] CHK025 - Are requirements defined for the scenario where the user switches activation mode while a segment is mid-processing? US2.4 says "current dictation session stops cleanly first" but doesn't specify if the in-progress segment completes or is discarded. [Coverage, Spec §US2.4]
  - Fixed: Added "Mode switch during processing" edge case — current segment completes fully per FR-018 before mode change
- [x] CHK026 - Are requirements defined for what happens if the SQLite database file is corrupted or locked by another process on startup? [Coverage, Gap]
  - Fixed: Added "SQLite database corruption" edge case — open() returns Err, pipeline can't start, error message identifies issue
- [x] CHK027 - Are requirements defined for the scenario where `get_focused_app_name()` returns "Unknown" for an extended period (e.g., user dictating on a screen with no focused window on macOS)? Does the LLM receive "Unknown" repeatedly, and is this acceptable? [Coverage, Contract §focused_app]
  - Fixed: Added "Persistent Unknown focused app" edge case — LLM treats as generic context, no special handling needed
- [x] CHK028 - Is there a requirement for what happens when the user speaks a phrase that is simultaneously a valid dictionary substitution AND a voice command (e.g., if "delete" is a dictionary entry mapping to "remove")? Which takes precedence — dictionary substitution before or after command classification? [Coverage, Spec §FR-010 vs §FR-011]
  - Fixed: Added "Dictionary/command precedence conflict" edge case — dictionary runs first (before LLM), can override commands, no auto-detection

## Edge Case Coverage

- [x] CHK029 - Are requirements specified for the maximum number of broadcast subscribers, or is the assumption that subscriber count is always small (<10) documented? [Edge Case, Research §R-007]
  - Fixed: Added subscriber count analysis in R-007 — <10 in practice, ~5KB clone overhead per segment, no hard limit
- [x] CHK030 - The spec defines the "elevated process target" edge case for Windows but has no macOS equivalent — are macOS-specific edge cases (e.g., Accessibility permission revoked mid-session, sandboxed apps) addressed? [Edge Case, Spec §Edge Cases]
  - Fixed: Added two macOS edge cases — Accessibility permission revoked (with error message) and sandboxed app target
- [x] CHK031 - Are requirements specified for what happens if the VAD thread exits unexpectedly (not via stop flag) while segments are queued in the channel? Does the pipeline drain remaining segments or immediately transition to Idle? [Edge Case, Research §R-009]
  - Fixed: Added "VAD thread unexpected exit" edge case in spec + new error category row in R-009 — drains remaining segments then transitions to Idle
- [x] CHK032 - Is the behavior defined for a dictionary substitution that produces an empty string (e.g., term "um" → replacement "")? Does this propagate as empty text through the pipeline? [Edge Case, Contract §DictionaryCache]
  - Fixed: apply_substitutions() doc now specifies empty replacement removes matched text; all-empty result skips LLM and injection

## Non-Functional Requirements

- [x] CHK033 - Are memory requirements specified for the DictionaryCache — what's the maximum expected dictionary size, and is there a bound on memory consumed by the in-memory HashMap and phrase Vec? [Gap, Data-Model §DictionaryCache]
  - Fixed: Added memory bounds analysis — <1 MB for 10,000 entries, no enforced upper bound, graceful degradation
- [x] CHK034 - Are concurrent access requirements for TranscriptStore specified — what happens if a transcript save coincides with a UI list query? Is the Mutex contention budget quantified? [Clarity, Research §R-005]
  - Fixed: Added concurrent access budget in R-005 — worst-case ~100µs wait, well within latency budget
- [x] CHK035 - SC-004 specifies CPU < 2% idle, but does this account for the VAD thread's 5ms sleep loop (polling at ~200Hz)? Is the idle CPU budget validated against the known polling behavior? [Consistency, Spec §SC-004 vs Research §R-008]
  - Fixed: SC-004 now clarifies 2% idle = pipeline loaded but NOT listening. VAD polling during Listening is part of active dictation budget.

## Dependencies & Assumptions

- [x] CHK036 - Is the assumption that `parking_lot` is already a workspace dependency validated, given that TranscriptStore and potentially DictionaryCache rely on it? [Assumption, Research §R-005]
  - Fixed: Validated — `parking_lot = "0.12"` in workspace Cargo.toml, `parking_lot.workspace = true` in vox_core. R-005 updated with explicit verification.
- [x] CHK037 - The plan assumes tokio's blocking pool has capacity for concurrent ASR and LLM spawn_blocking calls. Is the default pool size sufficient, or is a custom pool size required? The default is 512 threads, but is this assumption documented? [Assumption, Research §R-001]
  - Fixed: Added explicit "Blocking pool capacity assumption" section in R-001 — 512 default, max 2 concurrent, 250× headroom
- [x] CHK038 - Are the existing `run_vad_loop()` function's error return types compatible with the pipeline's error handling strategy, or will adaptation be needed at the thread boundary? [Dependency, Research §R-008]
  - Fixed: Added error return type compatibility note in R-008 — `anyhow::Result<()>` matches JoinHandle<Result<()>>, no adaptation needed

## Cross-Artifact Traceability

- [x] CHK039 - Does every edge case in the spec have a corresponding error handling entry in research R-009's error category table? (Compare: spec lists 10 edge cases, R-009 lists 7 error categories.) [Traceability, Spec §Edge Cases vs Research §R-009]
  - Fixed: R-009 error table expanded from 7 to 14 rows with Spec Edge Case column. All 17 spec edge cases now have corresponding error handling entries (some edge cases share categories, e.g., "rapid utterances" is a FIFO ordering concern, not an error).
- [x] CHK040 - Are all 21 functional requirements (FR-001 through FR-020, plus FR-012a) traceable to at least one acceptance scenario in the user stories? [Traceability, Spec §Requirements vs §User Stories]
  - Fixed: Added US1 scenarios 10 (FR-012a batch delivery) and 11 (FR-018 deactivation during processing). All 21 FRs now traceable to at least one acceptance scenario.

## Notes

- Check items off as completed: `[x]`
- Add findings inline as sub-bullets under each item
- Items marked [Gap] indicate requirements that are missing entirely
- Items marked [Ambiguity] indicate requirements that exist but are unclear
- Items marked [Conflict] indicate requirements that contradict each other
- Items marked [Consistency] indicate cross-artifact alignment issues
- Items marked [Assumption] indicate undocumented assumptions that need validation
