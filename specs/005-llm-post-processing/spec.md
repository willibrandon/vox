# Feature Specification: LLM Post-Processing

**Feature Branch**: `005-llm-post-processing`
**Created**: 2026-02-19
**Status**: Draft
**Dependencies**: 004-speech-recognition (ASR engine must be complete)

## Overview

Raw speech-to-text output from the ASR engine contains filler words, missing punctuation, uncorrected mistakes, and unformatted special content (numbers, dates, emails). The LLM post-processor takes this raw transcript and produces polished, ready-to-inject text. It also detects voice commands embedded in speech and routes them for execution instead of text injection.

The post-processor runs on GPU alongside the ASR engine within the combined memory budget. Token output is streamed as generated to reduce perceived latency — users see text appearing progressively rather than waiting for the full response.

## User Scenarios & Testing

### User Story 1 — Polish Raw Transcript (Priority: P1)

A user dictates naturally and the system cleans up filler words, fixes punctuation and capitalization, applies course corrections when the speaker corrects themselves, and formats special content like numbers, dates, emails, and URLs.

**Why this priority**: This is the core value of the post-processor. Without it, users receive messy raw ASR output that requires manual editing, defeating the purpose of voice dictation.

**Independent Test**: Feed raw transcript text containing fillers and formatting issues into the post-processor, verify the output is polished text with correct punctuation, no fillers, and properly formatted special content.

**Acceptance Scenarios**:

1. **Given** a raw transcript "um let's um meet at uh three pm", **When** the post-processor runs, **Then** the output is "Let's meet at 3 PM." with fillers removed and time formatted.
2. **Given** a raw transcript "send it to john at gmail dot com wait no john at outlook dot com", **When** the post-processor runs, **Then** the output keeps only the correction: "Send it to john@outlook.com"
3. **Given** a raw transcript "twenty five dollars", **When** the post-processor runs, **Then** the output is "$25"
4. **Given** a raw transcript "january third twenty twenty six", **When** the post-processor runs, **Then** the output is "January 3, 2026"
5. **Given** a raw transcript "h t t p s colon slash slash github dot com", **When** the post-processor runs, **Then** the output is "https://github.com"
6. **Given** a raw transcript with no issues, **When** the post-processor runs, **Then** the output preserves the speaker's voice and intent without rephrasing.

---

### User Story 2 — Detect and Route Voice Commands (Priority: P2)

A user speaks a voice command like "delete that" or "new line" and the system detects it as a command rather than text to inject. The command is returned as structured data for the pipeline to execute. A configurable wake word prefix ("hey vox") enables command routing for longer instructions.

**Why this priority**: Voice commands are essential for hands-free editing. Without them, users must switch to keyboard/mouse to perform common operations like deleting or undoing, breaking the dictation flow.

**Independent Test**: Feed known command phrases into the post-processor, verify structured command output is returned instead of text. Feed the wake word prefix followed by a command, verify routing.

**Acceptance Scenarios**:

1. **Given** a raw transcript "delete that", **When** the post-processor runs, **Then** the output is a structured command `{"cmd": "delete_last"}`, not text.
2. **Given** a raw transcript "new paragraph", **When** the post-processor runs, **Then** the output is a structured command `{"cmd": "paragraph"}`.
3. **Given** a raw transcript "hey vox, delete the last sentence", **When** the wake word is detected, **Then** the full text is routed to command detection.
4. **Given** a raw transcript that contains regular speech (not a command), **When** the post-processor runs, **Then** polished text is returned, not a command.

**Standard Command Catalog**:

| Spoken Phrase | Command | Action |
|---|---|---|
| "delete that" | `delete_last` | Delete last injected text |
| "undo that" | `undo` | Undo |
| "new line" | `newline` | Line break |
| "new paragraph" | `paragraph` | Double line break |
| "select all" | `select_all` | Select all text |
| "copy that" | `copy` | Copy selection |
| "paste" | `paste` | Paste clipboard |
| "tab" | `tab` | Tab character |

---

### User Story 3 — Adapt Tone to Active Application (Priority: P3)

The system adjusts the formality and style of polished output based on the currently focused application. Email applications get formal, complete sentences. Chat applications get casual, shorter sentences. Code editors preserve technical terms exactly.

**Why this priority**: Tone adaptation makes dictation feel natural across different contexts. Without it, users would need to consciously adjust their speech style or manually edit the output to match the target application's conventions.

**Independent Test**: Feed the same raw transcript with different active application names, verify the output style adjusts appropriately.

**Acceptance Scenarios**:

1. **Given** active application is an email client (e.g., Outlook, Gmail), **When** the user dictates, **Then** output uses formal tone with complete sentences and proper grammar.
2. **Given** active application is a chat app (e.g., Slack, Discord), **When** the user dictates, **Then** output uses casual tone with shorter sentences and relaxed punctuation.
3. **Given** active application is a code editor (e.g., VS Code, terminal), **When** the user dictates, **Then** output preserves technical terms exactly without reformatting.
4. **Given** an unknown application, **When** the user dictates, **Then** output uses a neutral professional tone.

---

### User Story 4 — Stream Tokens for Low Perceived Latency (Priority: P4)

As the post-processor generates output tokens, they are streamed to the text injector progressively rather than waiting for the full response. The user sees text appearing character by character, reducing perceived wait time. Commands (JSON output) are NOT streamed — they are collected in full before execution.

**Why this priority**: Streaming is a latency optimization that improves perceived responsiveness. The system is functional without it (batch mode works), but streaming makes the experience feel instant.

**Independent Test**: Process a transcript with a streaming callback, verify tokens arrive incrementally. Verify commands are NOT streamed.

**Acceptance Scenarios**:

1. **Given** a raw transcript producing text output, **When** processing with streaming enabled, **Then** tokens are delivered via callback as they are generated.
2. **Given** a raw transcript producing a command, **When** processing with streaming enabled, **Then** the command is returned only after full generation (not streamed token-by-token).

---

### User Story 5 — Dictionary Hints for Domain Terms (Priority: P5)

The user provides domain-specific dictionary hints (names, acronyms, jargon) that the post-processor uses to improve accuracy. When the ASR output contains a close match to a dictionary term, the post-processor corrects it to the exact dictionary spelling.

**Why this priority**: Dictionary hints are a polish feature. The core post-processing works without them, but hints improve accuracy for specialized domains (medical, legal, company-specific jargon).

**Independent Test**: Feed a raw transcript with a misspelled domain term alongside dictionary hints, verify the output uses the correct dictionary spelling.

**Acceptance Scenarios**:

1. **Given** dictionary hints containing "Kubernetes" and a raw transcript "we need to deploy to cooper net ease", **When** the post-processor runs, **Then** the output contains "Kubernetes" instead of the misheard version.
2. **Given** empty dictionary hints, **When** the post-processor runs, **Then** normal post-processing occurs without errors.

---

### Edge Cases

- What happens when the raw transcript is empty? The post-processor returns an empty string without errors.
- What happens when the raw transcript is extremely long (>500 words)? The post-processor handles it within the context window limit, truncating input if necessary rather than failing.
- What happens when the LLM generates output that is neither clean text nor valid JSON? The system treats it as text output, not a command.
- What happens when the model file is missing or corrupted? The system returns a descriptive error at load time.
- What happens when the speaker mixes commands and regular speech? Only recognized command phrases (from the catalog) or wake-word-prefixed speech are treated as commands. Ambiguous phrases default to text output.
- What happens when the wake word appears in normal speech? Only "hey vox" at the start of the transcript triggers command routing.

## Requirements

### Functional Requirements

- **FR-001**: System MUST load the LLM model from disk with GPU acceleration enabled.
- **FR-002**: System MUST process raw transcript text and return either polished text or a structured voice command.
- **FR-003**: System MUST remove filler words (um, uh, like, you know, basically, literally, so, I mean) from transcripts.
- **FR-004**: System MUST fix punctuation and capitalization in transcripts.
- **FR-005**: System MUST apply course correction — when the speaker corrects themselves, only the correction is kept.
- **FR-006**: System MUST format numbers, dates, emails, and URLs naturally (e.g., "twenty five dollars" to "$25", "john at gmail dot com" to "john@gmail.com").
- **FR-007**: System MUST detect voice commands from the standard catalog and return them as structured data instead of text.
- **FR-008**: System MUST support a configurable wake word prefix ("hey vox") for command routing.
- **FR-009**: System MUST accept an active application name and adapt output tone accordingly (formal for email, casual for chat, technical for code editors, neutral by default).
- **FR-010**: System MUST accept dictionary hints and use them to improve domain-specific term accuracy.
- **FR-011**: System MUST support streaming token output via a callback, delivering tokens as they are generated.
- **FR-012**: System MUST NOT stream command output — commands are collected in full before returning.
- **FR-013**: System MUST preserve the speaker's voice and intent — no rephrasing or summarizing.
- **FR-014**: System MUST create a fresh inference context per call to prevent state leakage between transcriptions.
- **FR-015**: System MUST support sharing a single loaded model across concurrent background processing tasks without reloading.
- **FR-016**: System MUST return a descriptive error when the model file is missing or fails to load.
- **FR-017**: System MUST support persistent caching of the system prompt tokens across calls to reduce per-call latency.

### Key Entities

- **PostProcessor**: The LLM inference engine. Holds a loaded model and produces polished text or voice commands from raw transcripts. Cheaply cloneable for background task use. Creates a fresh inference context per call.
- **ProcessorOutput**: The result of post-processing — either polished text ready for injection or a structured voice command.
- **VoiceCommand**: A structured command with a command name and optional arguments, parsed from the LLM's JSON output.
- **System Prompt**: The instruction set that defines post-processing rules (filler removal, formatting, command detection, tone adaptation). Cached for performance.

## Non-Functional Requirements

- **NFR-001**: Post-processing of a typical dictation (15 output tokens) MUST complete in under 100ms on high-end GPU and under 275ms on mid-range GPU.
- **NFR-002**: Post-processing of a longer dictation (30 output tokens) MUST complete in under 200ms on high-end GPU and under 550ms on mid-range GPU.
- **NFR-003**: Model memory usage MUST stay under 2.2 GB.
- **NFR-004**: Combined memory usage with the ASR model MUST stay under the 6 GB total budget.
- **NFR-005**: Model MUST load from disk in under 5 seconds.
- **NFR-006**: Inference context MUST use a 2048-token context window.
- **NFR-007**: Output generation MUST use near-deterministic settings (temperature 0.1, top-p 0.9).
- **NFR-008**: Maximum output length MUST be capped at 512 tokens per call.
- **NFR-009**: All public interfaces MUST compile with zero warnings.

## Assumptions

- The LLM model file (~1.6 GB) is downloaded separately and placed in the test fixtures directory. It is gitignored.
- English language only for v1.0. Multi-language support is deferred.
- The active application name is provided by the calling code — the post-processor does not detect it itself.
- Wake word detection is string matching on the raw transcript, not a separate ML model.
- Wake word "hey vox" is hardcoded for v1.0. Configurable wake word prefix is deferred.
- The command catalog is fixed at compile time for v1.0. Extensible command registration is deferred.
- Flash attention is not used (disabled by default in the inference backend).
- Stop conditions: EOG tokens (model-detected end-of-generation) and `\n` (newline). The `"` stop sequence from early design was dropped — it would truncate legitimate quoted output in dictation.

## Success Criteria

### Measurable Outcomes

- **SC-001**: Users receive polished text within 100ms of dictation completion (15-token output on high-end GPU), perceived as instantaneous.
- **SC-002**: 95% of filler words are successfully removed from transcripts without altering meaning.
- **SC-003**: Course corrections are applied correctly — when a speaker says "wait no" or "I mean", only the correction appears in output.
- **SC-004**: Numbers, dates, emails, and URLs are formatted correctly in 90%+ of cases.
- **SC-005**: All 8 standard voice commands are detected and routed correctly when spoken.
- **SC-006**: Tone adaptation produces noticeably different output styles for email vs. chat vs. code editor contexts.
- **SC-007**: Token streaming reduces perceived latency — first token appears within 30ms of processing start.
- **SC-008**: Combined GPU memory (ASR + LLM) stays under 6 GB total budget.
