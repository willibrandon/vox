# Feature Specification: Diagnostics, CLI Tool, and MCP Server

**Feature Branch**: `017-diagnostics-cli-mcp`
**Created**: 2026-02-28
**Status**: Draft
**Input**: User description: "Vox Diagnostics, CLI Tool, and MCP Server — enabling AI coding assistants and developers to observe, control, and test a running Vox instance remotely"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Query Running Instance Status (Priority: P1)

An AI coding assistant (Claude Code, Cursor) or developer needs to understand the current state of a running Vox instance without manual interaction. They query status, read logs, inspect settings, and review transcript history through a programmatic interface.

**Why this priority**: Without observability, AI assistants are blind — they cannot diagnose issues, verify fixes, or understand runtime behavior. This is the foundation all other diagnostics capabilities build on.

**Independent Test**: Can be fully tested by connecting to a running Vox instance and querying its status, logs, settings, and transcripts. Delivers immediate value: AI assistants can read app state instead of asking the developer to describe it.

**Acceptance Scenarios**:

1. **Given** Vox is running and ready, **When** a client queries status, **Then** it receives a snapshot including: process ID, pipeline readiness state, current pipeline activity, activation mode, recording state, debug audio level, GPU info (name, VRAM, platform), model states with VRAM usage, audio device info (name, sample rate, current RMS level), and last transcription latency.
2. **Given** Vox is running, **When** a client queries all settings, **Then** it receives the complete current settings as a structured object.
3. **Given** Vox is running, **When** a client queries a single setting by key, **Then** it receives only that setting's value.
4. **Given** Vox is running, **When** a client requests recent logs with an optional count and minimum severity filter, **Then** it receives the requested log entries with timestamp, level, source, and message.
5. **Given** Vox is running with transcript history, **When** a client requests recent transcripts with an optional count, **Then** it receives entries with timestamp, raw transcript, polished text, and processing latency.
6. **Given** Vox is still downloading or loading models, **When** a client queries status, **Then** the response reflects the current readiness state (not "ready") and model download/loading progress.

---

### User Story 2 - Inject Test Audio and Receive Transcription (Priority: P2)

An AI coding assistant feeds a WAV audio file into the running Vox pipeline to test the full processing chain (VAD, ASR, LLM post-processing) without needing a physical microphone. The assistant receives the transcription result and processing latency back.

**Why this priority**: This is the highest-value diagnostic for AI-driven development. It turns pipeline testing from "developer records audio, describes output" into "assistant sends file, reads result" — enabling automated regression testing and rapid iteration on pipeline behavior.

**Independent Test**: Can be tested by sending a WAV file to a running Vox instance and verifying the returned transcript matches expected output. Delivers value immediately: AI assistants can verify pipeline correctness without human intervention.

**Acceptance Scenarios**:

1. **Given** Vox is running and ready, **When** a client sends a WAV file path for injection, **Then** the system processes the audio through VAD, ASR, and LLM, and returns the raw transcript, polished text, and end-to-end latency.
2. **Given** Vox is running and ready, **When** a client sends raw PCM audio data (encoded) with a sample rate, **Then** the system processes it identically to a WAV file and returns the transcript.
3. **Given** the injected audio is at a different sample rate than 16kHz, **When** the injection is processed, **Then** the system resamples to the pipeline's native rate before processing.
4. **Given** audio is injected for testing, **When** the pipeline produces a transcript, **Then** the result is returned to the diagnostics client only — no text is injected into any focused application via keystroke simulation.
5. **Given** Vox is still loading models (not ready), **When** a client attempts audio injection, **Then** it receives an error indicating the app is not ready.
6. **Given** a user is actively recording via the microphone, **When** audio injection is requested, **Then** both can operate concurrently without interfering (injection uses separate pipeline context from live recording).
7. **Given** audio is injected, **When** the system processes it, **Then** it operates in fast-forward mode by default (no wall-clock delay), returning results as quickly as processing allows.

---

### User Story 3 - Remote Recording Control (Priority: P3)

An AI coding assistant starts and stops recording sessions on the running Vox instance to test real-world microphone input flow without requiring the developer to press the hotkey.

**Why this priority**: Complements audio injection (P2) by enabling tests with live microphone input. Useful when testing audio capture, device switching, or real-world speech scenarios.

**Independent Test**: Can be tested by sending start/stop commands and verifying the recording state changes. Delivers value: AI assistants can trigger recording without requiring hotkey interaction.

**Acceptance Scenarios**:

1. **Given** Vox is ready and idle, **When** a client sends a start recording command, **Then** the system begins capturing from the active microphone and the pipeline state reflects "listening."
2. **Given** Vox is actively recording, **When** a client sends a stop recording command, **Then** the system stops recording, processes any pending audio, and the pipeline returns to idle.
3. **Given** Vox is already recording, **When** a client sends a start command, **Then** it receives an error indicating a recording is already active.
4. **Given** Vox is idle (not recording), **When** a client sends a stop command, **Then** it receives an error indicating no active recording.
5. **Given** Vox is not ready, **When** a client sends a start command, **Then** it receives a not-ready error.

---

### User Story 4 - Capture Window Screenshots (Priority: P4)

An AI coding assistant captures screenshots of Vox's overlay HUD or settings window to visually verify UI state, layout, and rendering without the developer manually taking screenshots and describing what they see.

**Why this priority**: Visual verification eliminates the "describe what you see" bottleneck. AI assistants can directly inspect overlay states (downloading, listening, processing, error) and settings panel layouts.

**Independent Test**: Can be tested by requesting a screenshot of a visible window and verifying a valid image is returned. Delivers value: AI assistants gain visual inspection capability.

**Acceptance Scenarios**:

1. **Given** the overlay HUD is visible, **When** a client requests a screenshot of the overlay window, **Then** the system returns the window's current visual content as a PNG image.
2. **Given** the settings window is open, **When** a client requests a screenshot of the settings window, **Then** the system returns the settings window's current visual content as a PNG image.
3. **Given** the requested window is not currently open/visible, **When** a screenshot is requested, **Then** the system returns an appropriate error.

---

### User Story 5 - Stream Real-Time Pipeline Events (Priority: P5)

An AI coding assistant subscribes to a live event stream from the running Vox instance, receiving pipeline state changes, audio level updates, and transcript events in real time.

**Why this priority**: Enables AI assistants to monitor live pipeline behavior during debugging — see exactly when state transitions happen, watch audio levels, and catch transcripts as they're produced.

**Independent Test**: Can be tested by subscribing to events, triggering a recording, and verifying the expected sequence of state change and transcript events arrives. Delivers value: real-time pipeline visibility.

**Acceptance Scenarios**:

1. **Given** a client subscribes to pipeline state events, **When** the pipeline transitions between states (idle, listening, processing), **Then** the client receives a notification for each transition.
2. **Given** a client subscribes to audio RMS events, **When** audio is being captured, **Then** the client receives periodic audio level updates.
3. **Given** a client subscribes to transcript events, **When** a transcription completes, **Then** the client receives the raw text, polished text, and latency.
4. **Given** a client is subscribed to audio RMS events, **When** no recording is active, **Then** no RMS events are pushed. Pipeline state events (idle → listening, processing → idle) implicitly signal recording start/stop, so the absence of RMS events unambiguously means no active recording.
5. **Given** a client has an active subscription, **When** the client disconnects, **Then** the subscription is cleaned up without affecting the Vox instance.

---

### User Story 6 - Modify Settings Remotely (Priority: P6)

An AI coding assistant changes Vox settings (VAD threshold, hotkey, debug audio level, etc.) without requiring the developer to open the settings panel and manually adjust values.

**Why this priority**: Enables AI-driven parameter tuning and configuration — e.g., enabling debug audio recording, adjusting VAD sensitivity during troubleshooting, or testing different hotkey configurations.

**Independent Test**: Can be tested by writing a setting value and reading it back to verify persistence. Delivers value: AI assistants can configure the app as needed during debugging.

**Acceptance Scenarios**:

1. **Given** Vox is running, **When** a client sets a valid setting key to a new value, **Then** the setting is persisted and the relevant component is notified of the change.
2. **Given** a client changes any setting that has a runtime-observable side effect (e.g., VAD threshold, activation mode, debug audio level), **When** the change is applied, **Then** the corresponding component reflects the new value immediately without requiring an application restart.
3. **Given** a client attempts to set an invalid key or value, **Then** the system returns an appropriate error.

---

### User Story 7 - CLI Tool for Human Developers (Priority: P7)

A developer uses a standalone command-line tool (`vox-tool`) to interact with the running Vox instance from their terminal. They can query status, read logs, inject audio, capture screenshots, and stream events — all from the shell.

**Why this priority**: While the primary audience is AI assistants (via MCP), human developers also benefit from shell-based diagnostics during development, debugging, and manual testing.

**Independent Test**: Can be tested by running CLI commands against a running Vox instance and verifying correct output. Delivers value: developer-friendly terminal interface to all diagnostics capabilities.

**Acceptance Scenarios**:

1. **Given** Vox is running, **When** a developer runs the status command, **Then** the CLI prints a formatted summary of the application state.
2. **Given** multiple Vox instances are running, **When** a developer runs any command without specifying a target, **Then** the CLI lists available instances and instructs the user to specify which one.
3. **Given** no Vox instance is running, **When** a developer runs any command, **Then** the CLI reports "No running Vox instance found."
4. **Given** Vox is running, **When** a developer runs the inject command with a WAV file path, **Then** the CLI sends the audio, waits for processing, and prints the transcript result.

---

### User Story 8 - MCP Server for AI Assistants (Priority: P8)

An AI coding assistant (Claude Code, Cursor, etc.) uses a Model Context Protocol (MCP) server to interact with the running Vox instance. The MCP server exposes all diagnostics capabilities as MCP tools that the assistant can call directly.

**Why this priority**: MCP is the standard protocol for AI assistant integrations. This makes Vox diagnostics natively accessible to any MCP-compatible tool without custom integration work.

**Independent Test**: Can be tested by launching the MCP server process, connecting via the MCP protocol, calling tools, and verifying correct responses. Delivers value: zero-friction AI assistant integration.

**Acceptance Scenarios**:

1. **Given** the MCP server is launched and Vox is running, **When** an AI assistant calls the status tool, **Then** it receives the current application status.
2. **Given** the MCP server is launched and Vox is running, **When** an AI assistant calls the inject audio tool with a file path, **Then** it receives the transcript result.
3. **Given** the MCP server is launched but no Vox instance is found, **When** any tool is called, **Then** it returns an error indicating no running instance.
4. **Given** the MCP server is running, **When** an AI assistant queries available tools, **Then** it receives descriptions for all diagnostics tools with their parameter schemas.

---

### Edge Cases

- What happens when the Vox process exits while a client is connected? The connection closes and the client receives an I/O error.
- What happens when injected audio contains no speech? The pipeline returns an empty transcript (same as live recording with silence).
- What happens when multiple clients send commands simultaneously? Each connection is handled independently; concurrent state reads are safe, and conflicting actions (e.g., two clients starting recording) return appropriate errors.
- What happens when a client sends malformed requests? The system returns a structured error with a descriptive message; the connection remains open for subsequent valid requests.
- What happens when the diagnostics endpoint is left over from a crash? Stale endpoints are detected (failed connection attempt) and cleaned up before creating a new one.
- What happens when the connection endpoint path exceeds OS limits? The path convention keeps paths well within platform limits.
- What happens when injected audio is invalid (file not found, corrupt WAV, unsupported format like 24-bit int or stereo, empty file with 0 samples)? The system returns a structured error with a descriptive message specific to the failure cause. The connection remains open for subsequent requests.
- What happens when audio injection is requested and the pipeline takes a long time to process? The injection is a blocking operation on that connection — the client should use a separate connection for concurrent queries if needed.
- What happens when a subscribe client wants to send an unsubscribe command while receiving events? The connection MUST support interleaved client-to-server messages during an active subscription (the server reads and writes concurrently on the same connection).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The running Vox application MUST expose a local inter-process communication interface for diagnostics queries and commands.
- **FR-002**: The diagnostics interface MUST support querying the full application status snapshot (readiness, pipeline state, GPU, models, audio device, recording state, latency).
- **FR-003**: The diagnostics interface MUST support reading all settings or individual settings by key.
- **FR-004**: The diagnostics interface MUST support writing individual settings by key, with immediate persistence and component notification. The value type MUST match the setting's expected type (e.g., numeric for thresholds, string for enum-like settings). A type mismatch MUST return a structured error indicating the expected type.
- **FR-005**: The diagnostics interface MUST support retrieving recent log entries with configurable count and minimum severity filter.
- **FR-006**: The diagnostics interface MUST support retrieving recent transcript history with configurable count. Each transcript entry MUST include timestamp, raw transcript text, polished text, and processing latency.
- **FR-007**: The diagnostics interface MUST support starting and stopping recording sessions remotely.
- **FR-008**: The diagnostics interface MUST support injecting audio into the full processing pipeline and returning the transcript result with latency. Audio input MUST be accepted as either a file path (WAV format) or raw PCM data encoded as base64 32-bit float mono samples with an accompanying sample rate.
- **FR-009**: Audio injection MUST process through the complete pipeline (VAD, ASR, LLM post-processing) but MUST NOT inject the resulting text into any application via keystroke simulation.
- **FR-010**: Audio injection MUST operate in fast-forward mode by default, processing as quickly as possible without wall-clock delays.
- **FR-011**: The diagnostics interface MUST support capturing screenshots of application windows (overlay HUD, settings window) and returning them as images.
- **FR-012**: The diagnostics interface MUST support subscribing to real-time event streams (pipeline state changes, audio levels, transcript completions).
- **FR-013**: The diagnostics interface MUST enforce a connection limit (maximum 4 concurrent connections) to prevent resource exhaustion. When the limit is reached, new connections MUST be accepted and immediately receive an error response indicating the limit has been reached, then be closed.
- **FR-014**: The diagnostics interface MUST be accessible only to processes running as the same OS user (no authentication tokens needed; OS-level filesystem/process permissions suffice).
- **FR-015**: Stale connection endpoints from previous crashes MUST be automatically detected and cleaned up on startup.
- **FR-016**: The diagnostics interface MUST return structured errors with error codes for: malformed requests, unknown methods, invalid parameters, internal errors, not-ready state, already-recording conflicts, not-recording conflicts, and connection limit reached.
- **FR-017**: A standalone CLI tool MUST expose all diagnostics capabilities as shell commands.
- **FR-018**: The CLI tool MUST auto-discover running Vox instances and connect without manual configuration. When multiple instances exist, it MUST require the user to specify a target.
- **FR-019**: A standalone MCP server MUST expose all request/response diagnostics capabilities as MCP tools compatible with the Model Context Protocol standard. Event streaming (subscribe) is excluded per FR-032.
- **FR-020**: The MCP server MUST operate as a subprocess launched by AI assistants, using standard I/O for protocol communication.
- **FR-021**: The diagnostics interface MUST work cross-platform (Windows and macOS) using the same protocol.
- **FR-022**: The diagnostics interface MUST NOT block the audio pipeline, UI render thread, or any latency-critical path.
- **FR-023**: The diagnostics listener MUST start during application initialization (before the pipeline is fully ready). Methods that require pipeline readiness (record, inject_audio) MUST return a not-ready error when the application is still loading. Read-only methods (status, settings, logs, transcripts) MUST work regardless of readiness state. The listener MUST shut down cleanly during application exit (removing its endpoint).
- **FR-024**: Audio injection MUST feed audio into the pipeline at the audio's original sample rate, letting the pipeline's own resampling stage handle conversion to the native processing rate. This exercises the same resampling code path as live microphone input.
- **FR-025**: The diagnostics protocol MUST use newline-delimited JSON messages with request/response correlation via integer IDs, following JSON-RPC error code conventions.
- **FR-026**: Audio injection MUST be able to operate concurrently with an active live recording session without interfering with the live session's audio capture, transcription, or text injection. Injection creates its own independent pipeline context.
- **FR-027**: Screenshot capture MUST return an error with a descriptive message when the target window does not exist, is not visible, or the capture fails for any platform-specific reason.
- **FR-028**: Event subscriptions MUST support client-initiated unsubscription while the subscription is active. The connection MUST handle concurrent reading (client commands) and writing (event push) on the same connection.
- **FR-029**: Audio injection MUST return structured errors for: file not found, unsupported audio format, corrupt file, and empty audio (0 samples).
- **FR-030**: Audio injection is a blocking operation on the connection — the client's connection is occupied for the duration of pipeline processing. Concurrent queries require a separate connection.
- **FR-031**: The CLI tool MUST return exit code 0 on success and non-zero on any error, with error messages printed to stderr. This enables scripting and CI integration.
- **FR-032**: Real-time event streaming (subscribe) is exposed via the CLI tool and direct diagnostics connections but is NOT available through the MCP server. MCP tool calls are request/response only. The MCP server MUST NOT expose a subscribe tool.

### Key Entities

- **Diagnostics Endpoint**: A local inter-process communication channel scoped to a single Vox process. Identified by the process ID. Created on app startup, removed on shutdown.
- **Diagnostics Request**: A structured message with an ID, method name, and optional parameters. Sent by clients (CLI tool, MCP server) to the running Vox instance.
- **Diagnostics Response**: A structured message echoing the request ID, containing either a result payload or an error with code and message.
- **Event Subscription**: A long-lived client connection that receives push notifications for selected event types (pipeline state, audio levels, transcripts).
- **Audio Injection**: A test operation that feeds audio samples into the processing pipeline, bypassing the microphone, and returns the transcript without keystroke injection.
- **CLI Tool**: A standalone command-line binary for human developers. Sends diagnostics requests, formats and prints responses.
- **MCP Server**: A standalone binary implementing the Model Context Protocol. Acts as a bridge between AI coding assistants and the Vox diagnostics interface.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An AI coding assistant can query the running Vox instance's status, settings, logs, and transcripts within 1 second of sending the request.
- **SC-002**: An AI coding assistant can inject a 10-second WAV audio file and receive a transcript result in under the pipeline's processing time plus 500ms of diagnostics overhead. In fast-forward mode, total wall-clock time for a 10-second file MUST be determined by ASR + LLM processing speed (typically 1-3 seconds), not by audio duration. A 10-second file MUST NOT take 10+ seconds.
- **SC-003**: An AI coding assistant can start and stop a recording session remotely with the state change confirmed within 1 second.
- **SC-004**: An AI coding assistant can capture a screenshot of any visible Vox window and receive the image within 2 seconds.
- **SC-005**: A developer can discover and connect to a running Vox instance from the CLI tool with zero manual configuration (auto-discovery).
- **SC-006**: All diagnostics operations work identically on both Windows and macOS.
- **SC-007**: The diagnostics listener introduces no more than 1ms of additional latency to the pipeline processing path, measured over 100 utterances with and without the listener active.
- **SC-008**: The MCP server is discoverable and usable by any MCP-compatible AI assistant without Vox-specific client code.
- **SC-009**: Stale endpoints from crashed instances are cleaned up automatically — a fresh Vox launch never fails due to leftover state from a previous run.
- **SC-010**: 100% of diagnostics error conditions return structured error responses (never silent failures or hangs).

## Assumptions

- The local inter-process communication mechanism is available on both Windows 10+ and macOS 12+.
- AI coding assistants support the MCP protocol (Claude Code, Cursor, and similar tools).
- The diagnostics interface is a developer/power-user tool — it is not exposed to end users and does not require a UI.
- Settings writes through the diagnostics interface have the same effect as changes made through the settings panel.
- Audio injection clones reference-counted model handles to create independent pipeline contexts. This is the same pattern used per-segment in normal pipeline operation (fresh inference contexts per segment). Results are identical to live pipeline output.
- Audio injection signals pipeline completion by closing the audio source after all samples are pushed. The pipeline drains remaining audio and exits naturally. If the audio source is not closed, the pipeline will wait indefinitely for more audio.
- Remote recording control dispatches commands to the UI thread via a command channel (not direct state mutation from the diagnostics thread). This ensures recording state changes follow the same code path as hotkey-triggered recording.
- Screenshot capture requires platform-specific unsafe FFI — the same pattern as existing text injection and hotkey code, not a new precedent.
- The diagnostics protocol (newline-delimited JSON over local IPC) is stable independent of any MCP SDK. The MCP server depends on a pre-1.0 MCP SDK; API changes in that SDK may require MCP server updates but do not affect the CLI tool or diagnostics protocol.
- The diagnostics client library (connection management, request/response serialization) lives in vox_core so both the CLI tool and MCP server depend on vox_core rather than duplicating protocol code.
- Event subscription connections require concurrent reading (client commands like unsubscribe) and writing (push events) on the same connection. This means a subscribe connection consumes two handler threads (one for reading, one for event pushing) rather than the one thread used by request/response connections. The connection limit of 4 therefore means at most 8 handler threads plus 1 listener thread.
- Audio RMS events are only pushed while recording is active — not polled continuously. Pipeline state events (which include transitions to/from "listening") implicitly signal recording start/stop, so the absence of RMS events during a subscription unambiguously means no active recording.
- The diagnostics interface is not a public API and has no stability guarantees across Vox versions.

## Dependencies

- Existing application state and pipeline infrastructure (features 009-016) provide the state and processing capabilities that diagnostics exposes.
- The log store (feature 015) provides log entries for the logs query.
- The transcript store (feature 009) provides transcript history.
- The overlay HUD (feature 012) and settings window (feature 013) provide the windows for screenshot capture.
- The recording session and pipeline orchestration (features 007, 012) provide the start/stop recording capability.
- The MCP Rust SDK (`D:\SRC\rust-sdk`, `rmcp` 0.17.0) provides the MCP server framework. Consult its examples and `crates/rmcp/` source for `#[tool]` macro usage, `ServerHandler` trait, and stdio transport patterns.
