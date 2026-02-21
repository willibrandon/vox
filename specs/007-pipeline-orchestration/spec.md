# Feature Specification: Pipeline Orchestration

**Feature Branch**: `007-pipeline-orchestration`
**Created**: 2026-02-20
**Status**: Draft
**Input**: User description: "Wire all pipeline components into a coordinated async system: Audio → VAD → ASR → LLM → Inject"

## Clarifications

### Session 2026-02-20

- Q: Should the LLM deliver output via streaming (tokens injected as generated) or batch (wait for complete output, inject all at once)? → A: Batch. The LLM must see its full output to classify text vs. voice command; streaming would inject command words before classification. At ~215ms LLM time on RTX 4090, batch latency is imperceptible.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - End-to-End Dictation (Priority: P1)

The user activates dictation with the hotkey, speaks naturally into their microphone, and sees polished text appear in their currently focused application. The complete pipeline processes audio through voice activity detection, speech recognition, intelligent post-processing (filler removal, punctuation, capitalization, course correction), and injects the result as keystrokes. Voice commands ("delete that", "undo", "new line") are recognized and executed as actions rather than typed as text. Dictionary substitutions (custom vocabulary, corrections) are applied before LLM processing to improve accuracy.

**Why this priority**: This is the core product — without end-to-end dictation working, nothing else matters. Every other story depends on this pipeline functioning correctly.

**Independent Test**: Activate dictation via hold-to-talk, speak "hello world", verify polished text (e.g., "Hello, world.") appears in a text editor. Speak "delete that", verify the text is deleted rather than "delete that" being typed.

**Acceptance Scenarios**:

1. **Given** the pipeline is fully loaded and idle, **When** the user holds the hotkey and says "hello world", **Then** polished text is injected into the focused application within 300ms of utterance completion (RTX 4090) or 750ms (M4 Pro).
2. **Given** the pipeline is listening, **When** the user pauses speaking for 500ms (the silence threshold), **Then** the accumulated speech segment is emitted for processing while the pipeline continues listening for the next utterance.
3. **Given** the pipeline is listening, **When** no speech is detected (silence only), **Then** no text is injected and no processing resources are consumed beyond VAD.
4. **Given** the pipeline is processing a segment, **When** ASR returns text with fillers like "um let's um meet tomorrow", **Then** dictionary substitutions are applied first, then LLM produces "Let's meet tomorrow."
5. **Given** the pipeline is processing, **When** ASR returns a voice command like "delete that", **Then** the LLM identifies this as a command and the delete action is executed (not typed as text).
6. **Given** any required component (audio, VAD, ASR, LLM, injector, dictionary) is not loaded, **When** the user attempts to activate dictation, **Then** the pipeline does not start and the UI shows an error message identifying the missing component(s) (e.g., "Pipeline cannot start: ASR model not loaded"). The Error state's `message` string carries this information — no separate component list is needed.
7. **Given** the pipeline is processing a segment, **When** the user speaks another utterance, **Then** the second utterance is captured and queued — it is processed after the first segment completes, in FIFO order.
8. **Given** the user has added "gonna" → "going to" in the dictionary, **When** the user says "I'm gonna leave", **Then** the dictionary substitution is applied before LLM processing (the LLM receives "I'm going to leave") and the final output reflects the substitution (FR-020).
9. **Given** the pipeline is processing a segment with Notepad in focus, **When** the LLM post-processes the text, **Then** the LLM receives "Notepad" as the active application name for context-aware tone adaptation (FR-013).
10. **Given** the pipeline is processing a segment, **When** the LLM completes, **Then** the full LLM output is available before any injection or command execution begins — no partial text is injected during LLM generation (FR-012a).
11. **Given** hold-to-talk mode is active and the pipeline is processing a segment, **When** the user releases the hotkey during processing, **Then** the current segment completes processing and injection fully before the pipeline transitions to Idle (FR-018). No audio is captured after release.

---

### User Story 2 - Activation Modes (Priority: P2)

The user can choose between three dictation activation modes based on their workflow preference. **Hold-to-talk** (default): user holds the hotkey while speaking, release stops recording and processes remaining audio. **Toggle**: press once to start recording, press again to stop. **Hands-free**: double-press the hotkey (two presses within 300ms) to enter continuous dictation mode where VAD automatically segments speech at natural boundaries; each segment flows through the full pipeline independently; single press exits hands-free mode. The active mode is persisted in user settings.

**Why this priority**: Different users and workflows require different activation patterns. Hold-to-talk is the safest default (no accidental dictation), but power users need toggle and hands-free for extended dictation sessions like writing emails or documents.

**Independent Test**: Configure each mode in settings, verify hotkey behavior matches the mode description. For hands-free mode, verify that multiple natural utterances each produce separate injections without any manual intervention between them.

**Acceptance Scenarios**:

1. **Given** hold-to-talk mode is active and the pipeline is idle, **When** the user presses and holds the hotkey, **Then** audio capture and VAD processing begin immediately. **When** the user releases the hotkey, **Then** any remaining buffered speech is processed through the full pipeline and the pipeline returns to idle.
2. **Given** toggle mode is active and the pipeline is idle, **When** the user presses the hotkey once, **Then** dictation starts. **When** the user presses the hotkey again, **Then** dictation stops and any remaining buffered audio is processed.
3. **Given** the pipeline is idle, **When** the user double-presses the hotkey (two presses within 300ms), **Then** hands-free continuous dictation begins. VAD automatically segments speech at natural boundaries. Each segment flows through the full pipeline independently. **When** the user single-presses the hotkey, **Then** hands-free mode exits after processing any remaining audio.
4. **Given** any activation mode is active, **When** the user changes the mode in settings, **Then** the new mode takes effect immediately (current dictation session, if any, stops cleanly first).

---

### User Story 3 - Pipeline State Broadcasting (Priority: P3)

The pipeline broadcasts its current state to all UI subscribers in real-time so the overlay HUD can display what is happening. States include: Idle (waiting for hotkey), Listening (microphone active, VAD processing), Processing (ASR or LLM working, with optional raw transcript preview once ASR completes), Injecting (typing polished text into target app), and Error (recoverable problem with human-readable message). The UI receives push notifications — it never polls for state.

**Why this priority**: Users need immediate visual feedback about what the pipeline is doing. Without state broadcasting, the overlay HUD is blind — users cannot tell if dictation is active, processing, or errored. This is essential for usability but depends on the core pipeline (US1) working first.

**Independent Test**: Subscribe to pipeline state changes, activate dictation, speak a phrase, and verify state transitions occur in the correct order: Idle → Listening → Processing (no raw text) → Processing (with raw text) → Injecting (with polished text) → Listening. Verify all subscribers receive every transition.

**Acceptance Scenarios**:

1. **Given** a UI subscriber is registered, **When** the pipeline transitions between any two states, **Then** the subscriber receives the new state within 1ms.
2. **Given** the pipeline is processing a segment, **When** ASR completes and returns raw text, **Then** the Processing state is re-broadcast with the raw transcript included.
3. **Given** the pipeline is about to inject, **When** the polished text is ready, **Then** the Injecting state is broadcast with the polished text included.
4. **Given** the pipeline encounters a recoverable error (e.g., ASR failure on one segment), **When** the error occurs, **Then** the Error state with a human-readable message is broadcast, the failed segment is discarded, and the pipeline returns to Listening (if still activated) or Idle (if not).
5. **Given** multiple UI subscribers exist, **When** a state change occurs, **Then** all subscribers receive the same update.

---

### User Story 4 - Transcript History (Priority: P4)

After each successful text injection, the pipeline saves a transcript record containing the raw ASR text, polished LLM output, name of the target application that received the text, audio segment duration, end-to-end processing latency, and a timestamp. This history enables users to review past dictations, track accuracy over time, and identify patterns for dictionary training. Voice command executions are NOT recorded as transcripts (they produce no text output). Records persist across application restarts and are automatically pruned after 30 days to prevent unbounded storage growth.

**Why this priority**: Transcript history is valuable for user review and system improvement, but the core dictation pipeline must work first. This is an enhancement that builds on top of working injection and can be shipped after the core pipeline is validated.

**Independent Test**: Dictate several phrases into different applications. Verify each produces a transcript record with all required fields populated. Verify records persist after restarting the application. Verify records older than 30 days are pruned.

**Acceptance Scenarios**:

1. **Given** a successful text injection, **When** the polished text is injected, **Then** a transcript record is created containing: unique ID, raw ASR text, polished text, target application name, audio segment duration in milliseconds, end-to-end latency in milliseconds, and ISO 8601 timestamp.
2. **Given** a voice command is executed (e.g., "delete that"), **When** the command completes, **Then** no transcript record is created.
3. **Given** transcript records exist, **When** the application restarts, **Then** all previous records are available (persisted to durable storage).
4. **Given** transcript records are accumulating, **When** records exceed 30 days old, **Then** they are automatically pruned on application startup.

---

### Edge Cases

- **Rapid successive utterances**: When the user speaks multiple short phrases with brief pauses, each segment MUST be processed independently and in FIFO order — no segment may be dropped, duplicated, or processed out of sequence.
- **Very long utterance**: If a single utterance exceeds the maximum speech duration (30 seconds default, configurable via `max_speech_ms` in VAD config), VAD forces a segment boundary and processing continues without dropping any audio.
- **Pipeline error mid-segment**: If ASR or LLM fails during processing of one segment, the error is broadcast, the failed segment is discarded, and the pipeline returns to Listening (if the PipelineController's `is_active` flag is still true — meaning the user has not deactivated dictation) or Idle (if `is_active` is false — meaning the user released the hotkey/toggled off during or before the error). Subsequent segments are unaffected.
- **Hotkey deactivation during processing**: If the user deactivates dictation (releases hotkey in hold-to-talk, or presses stop in toggle/hands-free) while a segment is being processed, the current segment completes processing and injection, then the pipeline goes Idle. No audio is captured after deactivation.
- **Target application focus change**: Text is injected into whatever application is focused at injection time (current-focus semantics). If focus changes between segment start and injection, text goes to the newly focused app.
- **Elevated process target (Windows)**: If the focused application runs elevated (admin) and Vox is not, injection is blocked. The pipeline broadcasts an Error state with guidance ("Target application is elevated — run Vox as administrator"), and returns to Listening so subsequent segments targeting non-elevated apps still work.
- **Accessibility permission revoked (macOS)**: If the macOS Accessibility permission is revoked while Vox is running, text injection via CGEvent fails and `get_focused_app_name()` via AX API returns errors. The pipeline broadcasts an Error state ("Accessibility permission required — grant access in System Preferences > Privacy & Security > Accessibility"), and returns to Listening. Subsequent segments will continue to fail until the permission is restored (the user must re-grant access; no automatic recovery).
- **Sandboxed app target (macOS)**: Some macOS apps restrict CGEvent injection. If injection fails for a sandboxed target, the pipeline broadcasts an Error state with the target app name and returns to Listening — same recovery path as elevated process on Windows.
- **Audio device disconnection**: If the configured audio device disconnects during dictation, the pipeline broadcasts an Error state ("Audio device disconnected") and returns to Idle.
- **Audio device reconnection**: When a previously configured device reconnects, the pipeline does NOT auto-resume — the user must re-activate dictation via the hotkey.
- **VAD thread unexpected exit**: If the VAD thread exits due to an error (not a panic and not the normal stop flag), the segment channel's sender is dropped, causing `segment_rx.recv()` to eventually return `None`. Before stopping, the pipeline drains any remaining segments already in the channel buffer, processing each through the full pipeline. After draining, the pipeline broadcasts Error("VAD processing thread exited unexpectedly") and transitions to Idle.
- **Empty ASR output**: If Whisper returns an empty string (e.g., background noise that passed VAD), the pipeline skips LLM processing and injection, broadcasts Listening, and waits for the next segment.
- **Broadcast subscriber overflow**: If a UI subscriber falls behind and misses state updates, the subscriber receives the most recent state on next read (latest-wins semantics, no crash or deadlock).
- **Mode switch during processing**: If the user changes activation mode (via settings UI) while a segment is mid-processing (ASR or LLM in progress), the current segment completes processing and injection fully before the mode change takes effect. The `set_mode()` method sends a Stop command first (if dictation is active), which waits for the current segment to finish per FR-018, then applies the new mode. The user must re-activate dictation with the new mode's gesture.
- **SQLite database corruption**: If the SQLite database file is corrupted or locked by another process on startup, `TranscriptStore::open()` and `DictionaryCache::load()` return `Err`. The pipeline cannot start without these components (Constitution Principle III). The error message identifies the database issue (e.g., "Database locked: vox.db is in use by another process"). Recovery: the user closes the other process, or deletes the corrupted file (auto-recreated on next launch with empty data).
- **Persistent "Unknown" focused app**: If `get_focused_app_name()` returns "Unknown" repeatedly (e.g., no focused window, screen locked, or AX-incompatible app on macOS), the LLM receives "Unknown" as the app name. This is acceptable — the LLM treats "Unknown" as a generic context and produces neutral tone output. No special handling or fallback is required.
- **Dictionary/command precedence conflict**: Dictionary substitutions are applied BEFORE LLM processing, and voice command classification happens DURING LLM processing. If a dictionary entry maps a term that is also a voice command (e.g., "delete" → "remove"), the substitution runs first, transforming the text before the LLM sees it. The LLM then classifies the substituted text. This means dictionary entries can effectively override voice commands — users should avoid creating dictionary entries for command trigger words. No automatic conflict detection is provided in this feature; a future dictionary editor UI may warn about such conflicts.

## Terminology

- **Segment** (audio segment): A `Vec<f32>` of PCM audio samples at 16kHz emitted by the VAD chunker when it detects a complete speech utterance (bounded by silence gaps or max duration). This is the unit of work that flows through the pipeline. The term "segment" always refers to raw audio data — never to the ASR transcript text. The transcript text is referred to as "raw text" (ASR output) or "polished text" (LLM output).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The pipeline MUST process audio through all stages in strict sequence: audio capture → resampling (if needed) → voice activity detection → speech recognition → dictionary substitution → LLM post-processing → text injection. No stage may be skipped or bypassed.
- **FR-002**: The pipeline MUST NOT activate dictation until ALL required components are loaded and operational: audio capture device, VAD model, ASR model, LLM model, text injector, and dictionary cache. No degraded modes, no optional components, no fallbacks (Constitution Principle III).
- **FR-003**: The pipeline MUST support three mutually exclusive activation modes: hold-to-talk (default), toggle, and hands-free. The active mode MUST be persisted in user settings.
- **FR-004**: Hold-to-talk: hotkey press starts audio capture, hotkey release stops capture and processes remaining audio. "Processes remaining audio" means: the VAD thread flushes its internal buffer — any accumulated samples that have not yet formed a complete segment are force-emitted as a final segment (via the existing flush-on-stop logic in `run_vad_loop`), then processed through the full ASR → Dictionary → LLM → Inject pipeline before the pipeline transitions to Idle.
- **FR-005**: Toggle: first hotkey press starts capture, second press stops capture and processes remaining audio (same flush semantics as FR-004).
- **FR-006**: Hands-free: double-press (two presses within 300ms, exclusive — a press at exactly 300ms elapsed is treated as a single press, not a double-press) enters continuous mode with VAD auto-segmentation. Single press exits.
- **FR-007**: The pipeline MUST broadcast state changes (Idle, Listening, Processing, Injecting, Error) to all registered subscribers using a push model. No polling.
- **FR-008**: The Processing state MUST include the raw transcript text once ASR completes. The Injecting state MUST include the polished text.
- **FR-009**: The Error state MUST include a human-readable message describing the problem. The message is a non-empty English string with no format constraints (no max length, no character restrictions, no i18n). Messages are generated from Rust error types via `.to_string()` or hand-crafted for specific scenarios (e.g., "Audio device disconnected", "Target application is elevated — run Vox as administrator"). The UI is responsible for display truncation if needed.
- **FR-010**: The pipeline MUST apply dictionary substitutions to ASR output before sending to LLM post-processing.
- **FR-011**: The pipeline MUST route voice commands to command execution and text output to text injection, based on LLM classification of the input.
- **FR-012**: The pipeline MUST skip ASR, LLM, and injection for empty or silent audio segments. A segment is considered silent when its RMS energy (root mean square of sample values) is below 1e-3 (0.001). This threshold is applied as a pre-check before sending audio to ASR, avoiding Whisper's hallucination behavior on near-silent input (Whisper produces phantom text like "Thank you." on all-zero PCM).
- **FR-012a**: The pipeline MUST use batch LLM delivery: wait for the complete LLM output before injecting or executing. Streaming token-by-token injection is not used, because the LLM must see its full output to classify it as text vs. voice command.
- **FR-013**: The pipeline MUST provide the name of the currently focused application to the LLM for context-aware tone adaptation.
- **FR-014**: The pipeline MUST save a transcript record after each successful text injection containing: unique ID, raw text, polished text, target application name, audio segment duration (ms), end-to-end latency (ms), and ISO 8601 timestamp.
- **FR-015**: Transcript records MUST persist across application restarts and auto-prune after 30 days.
- **FR-016**: Voice command executions MUST NOT create transcript records.
- **FR-017**: The pipeline MUST process speech segments in strict FIFO order.
- **FR-018**: If the user deactivates dictation during processing, the current segment MUST complete processing and injection before the pipeline goes Idle.
- **FR-019**: Recoverable errors (single segment failure) MUST NOT crash the pipeline — the failed segment is discarded and the pipeline continues.
- **FR-020**: The dictionary cache MUST be an in-memory structure loaded from persistent storage, providing O(1) lookup for single-word substitutions via HashMap and O(p×n) phrase substitution via longest-first string replacement (where p = number of multi-word entries, n = text length). For typical dictionaries (<100 phrases) and utterances (<50 words), the combined substitution time is sub-microsecond. The cache MUST also export top entries by frequency as LLM hints.

### Key Entities

- **PipelineState**: The current operational state of the pipeline. One of: Idle, Listening, Processing (with optional raw transcript), Injecting (with polished text), Error (with message). Broadcast to all UI subscribers on every transition.
- **ActivationMode**: The user's configured recording trigger behavior. One of: HoldToTalk, Toggle, HandsFree. Persisted in settings. Determines how hotkey events map to pipeline start/stop.
- **TranscriptEntry**: A historical record of a completed dictation. Contains: unique ID, raw ASR text, polished LLM text, target application name, audio segment duration, end-to-end processing latency, and creation timestamp.
- **Pipeline**: The orchestrator that owns all pipeline components (audio, VAD, ASR, LLM, injector, dictionary) and coordinates the audio-to-text flow. Runs the main processing loop.
- **PipelineController**: The hotkey-facing interface that translates activation events (press, release, double-press) into pipeline start/stop commands based on the active ActivationMode.
- **DictionaryCache**: In-memory cache of user-defined vocabulary substitutions and LLM hint entries. Loaded from persistent storage on startup.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can speak a phrase and see polished text appear in their active application within 300ms of utterance completion on RTX 4090, or within 750ms on M4 Pro. "Utterance completion" is defined as the moment the VAD emits the audio segment (i.e., after the silence threshold triggers segment boundary detection). Latency is measured from segment emission to injection completion, matching the `latency_ms` field in TranscriptEntry.
- **SC-002**: Combined GPU memory usage (VAD + ASR + LLM models loaded simultaneously) remains below 6 GB of VRAM (or unified memory on Apple Silicon).
- **SC-003**: System RAM usage remains below 500 MB during active dictation.
- **SC-004**: CPU usage remains below 2% when idle (pipeline loaded but not dictating) and below 15% on RTX 4090 / 20% on M4 Pro during active dictation. Note: during active dictation (Listening state), the VAD thread polls the ring buffer with a 5ms sleep loop (~200Hz). On modern CPUs, this polling consumes < 0.1% CPU, well within the 2% idle budget. The 2% idle measurement applies when the pipeline is loaded but NOT in Listening state (i.e., truly idle, waiting for hotkey). During Listening, the VAD polling is part of the "active dictation" budget.
- **SC-005**: Users can dictate 3 separate utterances in sequence and each produces a correctly polished, independently injected text segment — no dropped segments, no cross-contamination between segments. For stress testing, a burst of 10 rapid utterances (each ~1 second with minimal pauses) MUST also be processed correctly in FIFO order with no drops, validating the 32-slot channel buffer under load.
- **SC-006**: Users can switch between all three activation modes and each behaves correctly without restarting the application.
- **SC-007**: The overlay HUD reflects pipeline state changes synchronously — the broadcast completes before the pipeline proceeds to the next processing stage. Measurement: a test subscriber receives every state transition in order, and each transition is received before the subsequent transition is broadcast. The broadcast::send() call itself is sub-microsecond; the 1ms budget accounts for subscriber wake-up and HUD re-render scheduling.
- **SC-008**: Users can review their complete dictation history including raw text, polished text, target application, and timing data for each entry.
- **SC-009**: Filler words ("um", "uh", "like") are removed from final output in at least 95% of occurrences. Measured against a test corpus of 20 utterances containing known fillers, using the speech_sample.wav fixture and manually constructed test cases. Accuracy = (fillers correctly removed) / (total fillers in test corpus) × 100.
- **SC-010**: Voice commands are correctly classified and executed as actions (not typed as text) in at least 95% of occurrences. The command vocabulary is exhaustively defined: "delete that", "undo", "new line", "select all", "copy", "paste". This is a closed set — new commands require a spec update and corresponding test additions. The 95% target is measured against 10 test utterances per command (60 total).
- **SC-011**: The application binary (excluding model files) remains below 15 MB.
- **SC-012**: Incremental build time remains below 10 seconds.
