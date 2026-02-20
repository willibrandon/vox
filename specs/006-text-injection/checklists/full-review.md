# Full Review Checklist: Text Injection

**Purpose**: Formal requirements quality validation across all dimensions — completeness, clarity, consistency, coverage, and measurability. For both author self-review and peer PR review.
**Created**: 2026-02-20
**Feature**: [spec.md](../spec.md) | [plan.md](../plan.md) | [contracts/injector-api.md](../contracts/injector-api.md)

## Requirement Completeness

- [x] CHK001 — Is the injection queue/serialization mechanism specified as a requirement, or only mentioned in an edge case? The edge case says "queued and executed sequentially" but no FR mandates serialization. [Gap, Edge Cases §4] → **Resolved**: Added FR-016 mandating serialization. Edge case now references FR-016.
- [x] CHK002 — Is the maximum queue depth or backpressure behavior defined for rapid sequential injections? [Gap] → **Resolved**: FR-016 specifies queue depth is bounded by the caller's concurrency mechanism, not the injector.
- [x] CHK003 — Is the source of focus-change events for retry (FR-011) specified? The requirement says "on focus change events" but does not define who detects or provides these events. [Completeness, Spec §FR-011] → **Resolved**: FR-011 rewritten to clarify the injector is stateless; focus-change detection is the pipeline/UI layer's responsibility. Also captured in ASM-002.
- [x] CHK004 — Is the interface between the injector and the UI layer defined for buffer display (FR-010)? The requirement says "provide it to the UI layer" without specifying the handoff mechanism. [Gap, Spec §FR-010] → **Resolved**: FR-010 rewritten to specify the injector returns the blocked text and reason via its function return value; the caller handles display.
- [x] CHK005 — Are macOS Accessibility permission failure detection and user guidance defined as functional requirements? The edge case describes guidance text, but no FR covers permission detection. Windows UIPI has FR-009; macOS has no equivalent. [Gap, Edge Cases §3 vs Spec §FR-009] → **Resolved**: Added FR-017 for macOS Accessibility permission detection, symmetric with FR-009. Edge case updated to reference FR-017. US-4 scenario 6 added.
- [x] CHK006 — Is the behavior defined for when a buffered retry also fails? FR-011 specifies retry on focus change and clear on success, but not what happens if retry fails repeatedly. [Gap, Spec §FR-011] → **Resolved**: FR-011 now explicitly states "buffer is preserved — it is never discarded on failure. Only cleared on successful injection or explicit user dismissal."
- [x] CHK007 — Is the maximum text length for a single injection call specified? Is there a defined behavior for very long text (e.g., 10,000+ characters)? [Gap] → **Resolved**: New edge case defines no maximum text length; very long text processed identically with linear latency scaling. ASM-001 covers the SendInput assumption.
- [x] CHK008 — Is the clipboard copy operation referenced in FR-010 ("copy-to-clipboard option") defined as its own requirement, or is it assumed to be a UI-layer concern? [Completeness, Spec §FR-010] → **Resolved**: FR-010 explicitly states "The clipboard copy operation itself is a UI-layer concern, not the injector's responsibility."

## Requirement Clarity

- [x] CHK009 — Is "typical utterance (sentence-length text)" in FR-013 quantified with a specific character count? The 30ms budget needs a defined input size to be measurable. [Clarity, Spec §FR-013] → **Resolved**: FR-013 now specifies "benchmark input of 50 ASCII characters" with explicit measurement methodology.
- [x] CHK010 — Is the text encoding of the input parameter specified? FR-001 says "arbitrary text" — is this assumed to be UTF-8 `&str` or could it be another encoding? [Clarity, Spec §FR-001] → **Resolved**: FR-001 now reads "arbitrary UTF-8 encoded text (Rust `&str`)".
- [x] CHK011 — Is "informative message" in User Story 4 acceptance scenario 1 defined with specific content or constraints? [Clarity, US-4 §1] → **Resolved**: US-4 scenario 1 now specifies the message content: "the target application is running as administrator and keyboard simulation is blocked by Windows security policy." Scenario 6 specifies macOS guidance content.
- [x] CHK012 — Is FR-006 sufficiently decomposed? It combines modifier key selection, per-command exceptions, backward deletion semantics, and cross-platform intent alignment into a single requirement. [Clarity, Spec §FR-006] → **Resolved**: FR-006 decomposed into FR-006a (standard modifier), FR-006b (delete_last exception with rationale), FR-006c (no-modifier commands).
- [x] CHK013 — Does FR-015 ("platform-agnostic interface") specify whether the abstraction is a trait, a module-level function, or conditional compilation dispatch? Or is this intentionally left to the plan phase? [Clarity, Spec §FR-015] → **Resolved**: FR-015 now explicitly states "The abstraction mechanism (compile-time conditional compilation vs. trait dispatch) is an implementation decision documented in the plan."
- [x] CHK014 — Is the term "character" used consistently? User Story 5 says "20 characters" while FR-004 says "20 UTF-16 code units" — these differ for emoji (2 code units = 1 character). [Clarity, US-5 vs Spec §FR-004] → **Resolved**: US-5 title, description, and acceptance scenarios now consistently say "20 UTF-16 code units" instead of "20 characters."

## Requirement Consistency

- [x] CHK015 — Is the "20 characters" phrasing in User Story 5 consistent with "20 UTF-16 code units" in FR-004? For surrogate pairs (emoji), one character = two code units. The acceptance scenario 3 correctly says "UTF-16 code units" but the story title/description says "20 characters." [Consistency, US-5 vs Spec §FR-004] → **Resolved**: All references in US-5 now use "UTF-16 code units" consistently.
- [x] CHK016 — Is the macOS `delete_last` key described consistently across all artifacts? The design doc says "Option+Delete", the spec clarification says "Option+Backspace", and the data model says "Option + Delete (0x33)". Mac keycode 0x33 is the Backspace/Delete key — are all references aligned? [Consistency, Clarification §1 vs data-model.md] → **Resolved**: Clarification updated with keycode note. data-model.md command table changed to "Backspace (0x33)". Spec command mapping table uses "Backspace (keycode 0x33)". FR-006b uses "Option+Backspace (macOS keycode 0x33)".
- [x] CHK017 — Are failure handling requirements symmetric across platforms? FR-009 (UIPI detection) is Windows-only, but there is no parallel FR for macOS Accessibility permission failure detection. Is this intentional asymmetry documented? [Consistency, Spec §FR-009] → **Resolved**: Added FR-017 for macOS Accessibility permission detection, explicitly symmetric with FR-009.
- [x] CHK018 — Does the API contract's `InjectionResult` enum align with the spec's requirements? The contract defines `NoFocusedWindow` as a variant, but the spec does not have a dedicated FR for no-focus detection (it's only implied by User Story 4 §2). [Consistency, Contract vs Spec §FR-010] → **Resolved**: Added FR-018 requiring no-focused-window detection on both platforms.
- [x] CHK019 — Are the performance latency targets in the spec (FR-013, FR-014) consistent with the performance contracts in `injector-api.md`? The contract adds two targets not in the spec: "UIPI check < 5ms" and "per-chunk < 2ms." [Consistency, Spec §FR-013/014 vs Contract] → **Resolved**: Added NFR-005 with performance sub-budgets matching the contract: UIPI check < 5ms, per-chunk < 2ms.

## Cross-Platform Parity

- [x] CHK020 — Are all 8 voice commands explicitly mapped to both Windows AND macOS key sequences in the spec's requirements, or only in the plan's data model? [Coverage, Spec §FR-005/006/007] → **Resolved**: Added Voice Command Mapping table to spec with all 8 commands mapped to both platforms including key codes.
- [x] CHK021 — Is the absence of a macOS text length limit documented as a contrast to the 20 UTF-16 code unit chunking requirement? (Windows has no chunking limit.) [Completeness] → **Resolved**: FR-004 now explicitly states "Windows has no per-call text length limit and handles the full text in a single API call."
- [x] CHK022 — Are threading/concurrency constraints specified for both platforms? The plan notes CGEvent/CGEventSource are NOT Send/Sync, but this constraint is absent from the spec's requirements. [Gap, Plan §D4 vs Spec] → **Resolved**: Added NFR-001 specifying macOS thread affinity requirement and Windows threading guidance.
- [x] CHK023 — Is the macOS event source configuration (HIDSystemState vs Private vs CombinedSessionState) specified as a requirement, or left as an implementation detail? [Completeness, Plan §Research] → **Resolved**: Added ASM-004 explicitly documenting that event source configuration is an implementation detail, not a functional requirement.

## Acceptance Criteria Quality

- [x] CHK024 — Is the latency measurement methodology for SC-001 ("within 30ms") defined? From when to when? From function call entry to function return? From LLM output to SendInput completion? [Measurability, Spec §SC-001] → **Resolved**: SC-001 and FR-013 now specify "wall-clock time from function entry to return" for a "benchmark input of 50 ASCII characters."
- [x] CHK025 — Is there a defined Unicode test corpus for SC-003 ("injects correctly without corruption")? Which specific characters, scripts, and emoji are required to pass? [Measurability, Spec §SC-003] → **Resolved**: SC-003 now defines a specific test corpus: ASCII, accented, CJK, emoji, typographic, ZWJ sequence, and mixed scripts.
- [x] CHK026 — Is SC-005 ("zero data loss") practically testable? What constitutes proof of zero loss — a test that verifies every failed injection is recoverable via clipboard? [Measurability, Spec §SC-005] → **Resolved**: SC-005 now defines "zero data loss" as "the full original text is preserved in the blocked result — byte-for-byte identical to the input. Every character of every failed injection is recoverable."
- [x] CHK027 — Does SC-006 ("both platforms") imply CI/CD testing on both Windows and macOS, or manual verification? Is the testing infrastructure a requirement? [Measurability, Spec §SC-006] → **Resolved**: SC-006 now specifies `#[cfg(target_os)]` gating and CI running platform-specific tests on platform-specific runners.
- [x] CHK028 — Are acceptance scenarios for User Story 4 (failure recovery) testable without an actual elevated process? Is the UIPI scenario automatable, or only manual? [Measurability, US-4] → **Resolved**: Added SC-007 specifying UIPI/Accessibility detection is unit-testable by isolation; full scenarios validated via manual testing.

## Scenario Coverage

- [x] CHK029 — Is the scenario defined for when the user switches focus mid-injection of a long text? (e.g., 500-character text being injected character-by-character, user clicks another window halfway through) [Coverage, Gap] → **Resolved**: New edge case: "Remaining text is delivered to whichever window now has focus. Atomicity is best-effort at the OS level."
- [x] CHK030 — Is the scenario defined for rapid-fire voice commands? (e.g., "undo that undo that undo that" producing three consecutive commands) [Coverage, Gap] → **Resolved**: New edge case referencing FR-016 serialization.
- [x] CHK031 — Is the scenario defined for the pipeline delivering text to the injector while a previous injection is still in progress? [Coverage, Gap] → **Resolved**: New edge case referencing FR-016 serialization. FR-016 itself covers concurrent request handling.
- [x] CHK032 — Are recovery scenarios defined for macOS Accessibility permission denial at the same level as Windows UIPI? User Story 4 focuses on Windows; macOS permission failure is only an edge case bullet. [Coverage, Edge Cases §3 vs US-4] → **Resolved**: Added US-4 acceptance scenario 6 for macOS Accessibility permission failure with specific guidance text. Independent test updated.
- [x] CHK033 — Is the scenario defined for what happens when the pipeline delivers a VoiceCommand but no text-capable field has focus? (e.g., user is on the desktop with no window focused) [Coverage, Gap] → **Resolved**: New edge case: injector detects no focused window (FR-018) and returns blocked result. FR-018 added.

## Edge Case Coverage

- [x] CHK034 — Is behavior defined for text containing null bytes, control characters (ASCII 0-31), or other non-printable characters? [Edge Case, Gap] → **Resolved**: New edge case: null bytes stripped to avoid C API truncation; other control characters injected as-is.
- [x] CHK035 — Is behavior defined for right-to-left (RTL) text (Arabic, Hebrew) or bidirectional text mixing? FR-008 mentions Unicode but not directionality. [Edge Case, Spec §FR-008] → **Resolved**: FR-008 updated to include "bidirectional text" and state the injector "does not interpret, reorder, or filter text based on script or directionality." New edge case added.
- [x] CHK036 — Is behavior defined for emoji sequences (skin tone modifiers, ZWJ sequences like 👨‍👩‍👧) that are multiple Unicode code points but render as a single glyph? Are these handled by UTF-16 encoding alone? [Edge Case, Spec §FR-008] → **Resolved**: FR-008 updated to include "multi-codepoint sequences such as ZWJ and skin tone modifiers." New edge case explains chunking may split ZWJ across chunks but OS reassembles. SC-003 test corpus includes ZWJ sequence.
- [x] CHK037 — Is the behavior defined for text containing tab characters (`\t`) vs the `tab` voice command? Could injecting text with embedded tabs conflict with the tab command shortcut? [Edge Case, Spec §FR-007 vs FR-001] → **Resolved**: New edge case: tab characters in text use Unicode injection (FR-001), tab command uses virtual key code (FR-007) — distinct code paths, no conflict.

## Non-Functional Requirements

- [x] CHK038 — Are threading requirements specified for which thread the injection functions execute on? The plan notes macOS CGEvent is not Send/Sync but the spec has no thread affinity requirement. [Gap, Plan §D1] → **Resolved**: Added NFR-001 specifying macOS single-thread requirement and Windows threading guidance.
- [x] CHK039 — Is a memory budget or lifetime defined for `InjectionBuffer`? Does buffered text persist indefinitely, or is there an expiry/eviction policy? [Gap, Spec §FR-010/011] → **Resolved**: Added NFR-002 specifying buffer persists until successful injection or user dismissal, no automatic expiry.
- [x] CHK040 — Are logging or observability requirements defined for injection events? (e.g., tracing injection latency, logging UIPI detections, tracking buffer usage) [Gap] → **Resolved**: Added NFR-003 specifying structured logging: attempts at DEBUG, failures at WARN, with specific fields.
- [x] CHK041 — Are security implications of unrestricted keystroke simulation addressed? Could an attacker feed malicious text through the pipeline to execute arbitrary key sequences in a privileged context? [Gap, Security] → **Resolved**: Added NFR-004 documenting the trust boundary: injector receives text only from the trusted pipeline, voice commands are a closed set, and text injection cannot execute keyboard shortcuts.

## Dependencies & Assumptions

- [x] CHK042 — Is the dependency on `VoiceCommand` from `llm::processor` explicitly documented in the spec or only in the plan? The spec mentions it in Key Entities but doesn't reference the source module. [Dependency, Spec §Key Entities] → **Resolved**: Key Entities VoiceCommand entry now includes full path `vox_core::llm::processor::VoiceCommand` and field types. DEP-001 added to Dependencies section.
- [x] CHK043 — Is the assumption that `SendInput` can handle arbitrarily large input arrays validated? Is there a Windows system limit on the number of INPUT structs per call? [Assumption] → **Resolved**: Added ASM-001 documenting the assumption with a fallback strategy if a limit is discovered.
- [x] CHK044 — Is the assumption that focus-change events are available to the injector validated? FR-011 requires retry on focus change, but the event source is undefined. Is this a pipeline responsibility? [Assumption, Spec §FR-011] → **Resolved**: FR-011 rewritten to clarify injector is stateless. ASM-002 explicitly assigns focus-change detection to the pipeline/UI layer.
- [x] CHK045 — Is the assumption that macOS Accessibility permission can be checked programmatically documented? The edge case says "show guidance" but doesn't specify detection. [Assumption, Edge Cases §3] → **Resolved**: Added ASM-003 documenting `AXIsProcessTrusted()` as the detection mechanism with a CGEvent-failure fallback.

## Notes

All 45 items resolved on 2026-02-20.

**Key structural changes to spec**:
- Added 3 new functional requirements: FR-016 (injection serialization), FR-017 (macOS Accessibility detection), FR-018 (no-focus detection)
- Decomposed FR-006 into FR-006a/b/c sub-requirements
- Added Voice Command Mapping table with all 8 commands mapped to both platforms
- Added Non-Functional Requirements section (NFR-001 through NFR-005)
- Added Dependencies & Assumptions section (DEP-001, ASM-001 through ASM-004)
- Added SC-007 for UIPI/Accessibility testability
- Added 9 new edge cases (mid-injection focus, rapid-fire commands, concurrent injection, no-focus commands, control characters, RTL text, emoji sequences, tab conflict, very long text)
- Fixed "20 characters" → "20 UTF-16 code units" throughout User Story 5
- Fixed "Delete" → "Backspace" (0x33) in data-model.md command table

**Artifacts updated**: spec.md, data-model.md
