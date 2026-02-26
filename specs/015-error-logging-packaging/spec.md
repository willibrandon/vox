# Feature Specification: Error Handling, Logging & Packaging

**Feature Branch**: `015-error-logging-packaging`
**Created**: 2026-02-25
**Status**: Draft
**Input**: User description: "Implement error recovery, structured logging, security measures, and packaging. Core principle: never stop working."

## Clarifications

### Session 2026-02-25

- Coverage fix: Added FR-026 (MSI installer) — present in original input ("Single `.exe` (portable) + `.msi` installer") but missing from FR list.
- Coverage fix: Added FR-027 (model file read-only permissions) — present in original input security measures but missing from FR list.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Self-Healing Pipeline (Priority: P1)

A user is dictating an email when the speech recognition component encounters a corrupted audio segment and fails. Instead of the app going silent or crashing, the system automatically retries the failed segment once. If the retry also fails, the system discards that segment, logs the failure, and continues listening for the next utterance. The user sees a brief moment of missed text at most, but dictation never stops.

This same recovery applies to every pipeline stage: if the language model produces garbled output, it retries once then discards. If text injection fails because the target window lost focus, the text is buffered and shown in the overlay with a copy option. The pipeline never enters a dead state.

**Why this priority**: This is the core promise of the application — "never stop working." Without self-healing, any transient failure kills the dictation session and forces a manual restart. Users dictating in real-time (meetings, medical notes, legal transcription) cannot afford downtime.

**Independent Test**: Can be tested by simulating component failures during an active dictation session and verifying the pipeline continues accepting new audio segments after recovery.

**Acceptance Scenarios**:

1. **Given** the pipeline is actively transcribing, **When** the speech recognition component fails on a segment, **Then** the system retries that segment once, and if the retry fails, discards the segment, logs the error, and continues listening for the next utterance.
2. **Given** the pipeline is actively transcribing, **When** the language model produces garbled or invalid output for a segment, **Then** the system retries that segment once, and if the retry fails, discards the segment and continues listening.
3. **Given** the pipeline has just processed a segment, **When** text injection fails because the target application lost focus, **Then** the processed text is buffered and displayed in the overlay with a "Copy" button, and injection is reattempted on the next focus event.
4. **Given** the pipeline is running, **When** a model file becomes corrupted or deleted on disk, **Then** the pipeline stops, re-enters the downloading state, re-downloads the affected model, reloads it, and resumes the pipeline automatically.
5. **Given** the pipeline is running, **When** the GPU runs out of memory, **Then** the system displays an actionable error message guiding the user to close other GPU-intensive applications or adjust model quantization settings.
6. **Given** the pipeline is running, **When** a GPU driver crash or unrecoverable GPU error occurs, **Then** the system displays an error with application restart instructions.

---

### User Story 2 - Audio Device Recovery (Priority: P2)

A user is dictating with their USB headset when they accidentally unplug it. The system detects the device disconnection, automatically switches to the built-in microphone (default device), and continues listening. If no audio device is available at all, the system pauses the pipeline, shows a message in the overlay ("No microphone detected"), and retries every 2 seconds until a device becomes available.

If the microphone permission is denied (macOS), the system shows a clear message with the exact path to the system setting and does not crash or hang.

**Why this priority**: Audio device disconnection is the most common hardware failure during dictation. Laptop users switch between built-in mic, headsets, and external microphones regularly. Without automatic recovery, every device change interrupts the workflow.

**Independent Test**: Can be tested by disconnecting an audio input device during recording and verifying the system switches to the default device or enters a retry loop with user-visible feedback.

**Acceptance Scenarios**:

1. **Given** the pipeline is listening on a specific audio device, **When** that device is disconnected, **Then** the system automatically switches to the default audio device and continues listening.
2. **Given** the user's audio device was disconnected and no other device is available, **When** the system enters the retry loop, **Then** the overlay displays "No microphone detected" and retries every 2 seconds until a device becomes available.
3. **Given** a new audio device becomes available during the retry loop, **When** the system detects it on the next retry, **Then** the system connects to the new device and resumes listening.
4. **Given** the application is running on macOS, **When** microphone permission is denied, **Then** the overlay displays a message with the exact path to System Settings > Privacy & Security > Microphone, and the system does not crash.

---

### User Story 3 - System Sleep/Wake Resilience (Priority: P3)

A user closes their laptop lid, putting the system to sleep. When they reopen it, the dictation app recovers automatically: it re-checks the audio device (which may have changed or been lost), verifies that GPU resources are still accessible (GPU context may be invalidated after sleep), re-registers the global hotkey (which may have been lost), and resets the pipeline to idle state. The user can press the hotkey and start dictating again without restarting the app.

**Why this priority**: Laptop users frequently sleep/wake their machines. If the app requires a manual restart after every sleep cycle, it becomes a friction point that leads to users closing the app entirely.

**Independent Test**: Can be tested by putting the system to sleep and waking it, then verifying the hotkey works and dictation can start without restarting the application.

**Acceptance Scenarios**:

1. **Given** the application is running, **When** the system goes to sleep and wakes, **Then** the system re-checks the audio device and reconnects or enters the device recovery loop.
2. **Given** the application is running, **When** the system wakes from sleep, **Then** the system verifies GPU resources are still accessible and reloads models if the GPU context was invalidated.
3. **Given** the application is running, **When** the system wakes from sleep, **Then** the global hotkey registration is verified and re-registered if lost.
4. **Given** the application is running, **When** the system wakes from sleep, **Then** the pipeline resets to Idle state and the user can start dictating immediately via hotkey.

---

### User Story 4 - Diagnostic Logging (Priority: P4)

A user reports that dictation "seems slow" or "missed some words." A developer asks the user to send their log file. The log contains structured entries with timing information for each pipeline stage (voice detection latency, transcription duration, language model processing time, injection time), making it straightforward to identify which stage is the bottleneck. The log also records every recovery event, device switch, and error with enough context to diagnose the problem without reproducing it.

**Why this priority**: Without structured, timed logs, debugging production issues requires reproducing the exact conditions. Structured logs with per-stage latency breakdowns enable remote diagnosis from a single log file.

**Independent Test**: Can be tested by running a dictation session, examining the log file, and verifying that each pipeline stage has timing information and that error/recovery events are recorded with structured context fields.

**Acceptance Scenarios**:

1. **Given** the application is running, **When** a dictation segment is processed through the pipeline, **Then** the log contains a structured entry for each stage with timing information (detection latency, transcription duration, processing duration, injection time).
2. **Given** an error occurs in any pipeline component, **When** the error is logged, **Then** the log entry includes the component name, error description, recovery action taken, and whether the retry succeeded.
3. **Given** the application is running, **When** the user checks the log directory, **Then** logs are organized by date with files rotated daily, older than 7 days automatically deleted, and no single file exceeding 10 MB.
4. **Given** the application is running, **When** the user sets the log verbosity via environment variable, **Then** the log output respects the configured level (error, warning, info, debug, trace).

---

### User Story 5 - Security & Privacy Guarantees (Priority: P5)

A privacy-conscious user wants assurance that their voice data is not stored or transmitted. The application processes all audio in memory and immediately discards it after transcription — no audio is ever written to disk. After model download completes, the application makes zero network calls. Model files are verified via SHA-256 checksum on download. Transcript history is stored locally and can be permanently deleted with a single action that overwrites data before reclaiming space.

**Why this priority**: Voice data is sensitive. Users dictating medical notes, legal documents, or personal communications need guarantees that their audio never leaves the device and can be permanently erased.

**Independent Test**: Can be tested by monitoring file system writes during dictation (no audio files created), monitoring network traffic after model download (zero outbound connections), verifying SHA-256 checksums match expected values, and verifying history deletion leaves no recoverable data.

**Acceptance Scenarios**:

1. **Given** the user is dictating, **When** audio is captured and processed, **Then** no audio data is written to disk at any point during the pipeline.
2. **Given** all models have been downloaded, **When** the application is running normally, **Then** the application makes zero network calls (no telemetry, no analytics, no update checks).
3. **Given** a model is being downloaded, **When** the download completes, **Then** the system verifies the file's SHA-256 checksum against the expected value before accepting it.
4. **Given** a downloaded model fails checksum verification, **When** the verification fails, **Then** the corrupt file is deleted and re-downloaded automatically.
5. **Given** the user has transcript history, **When** the user selects "Clear history," **Then** the data is overwritten and storage space is reclaimed (secure delete), leaving no recoverable fragments.
6. **Given** the user prefers no history, **When** they disable history in settings, **Then** no transcripts are persisted to storage.

---

### User Story 6 - Distributable Application Package (Priority: P6)

A user downloads Vox and installs it. On Windows, they can either run the portable executable directly or use the installer. On macOS, they mount the disk image, drag the app to Applications, and launch it. The installed binary is under 15 MB (models are downloaded separately on first launch). On first launch, the overlay appears instantly, models download concurrently in the background with progress shown, and once loaded, the user can start dictating. No setup wizards, no configuration steps.

**Why this priority**: Distribution is the final step before users can actually use the application. Without proper packaging, users cannot install the app. However, this is lower priority because it doesn't affect functionality — developers can run from source during development.

**Independent Test**: Can be tested by building the release binary, verifying its size is under 15 MB, running it on a clean machine, and verifying the first-run experience completes without manual intervention.

**Acceptance Scenarios**:

1. **Given** the release build is complete, **When** the binary size is measured (excluding model files), **Then** it is under 15 MB.
2. **Given** a user launches the application for the first time on Windows, **When** the application starts, **Then** the overlay HUD appears within 100 milliseconds, and all models begin downloading concurrently with progress displayed.
3. **Given** a user launches the application for the first time on macOS, **When** the application starts, **Then** the overlay HUD appears within 100 milliseconds, and all models begin downloading concurrently with progress displayed.
4. **Given** the portable Windows executable is placed in any directory, **When** the user double-clicks it, **Then** the application launches without requiring an installer or runtime dependencies.
5. **Given** the macOS application bundle is in the Applications folder, **When** the user opens it, **Then** the application launches and macOS permission prompts (Accessibility, Input Monitoring) fire automatically when the relevant features are first used.

---

### User Story 7 - GPU Detection & Resource Monitoring (Priority: P7)

The system detects available GPU hardware at startup and verifies it meets minimum requirements. On Windows, it checks for an NVIDIA GPU with sufficient VRAM. On macOS, it queries available unified memory. Combined GPU memory usage across all loaded models stays under 6 GB. If the GPU is not detected on Windows (required), the system shows an actionable error with driver installation guidance rather than crashing with a cryptic message.

**Why this priority**: GPU detection is a startup prerequisite. Without it, users with missing or outdated drivers see opaque errors instead of actionable guidance.

**Independent Test**: Can be tested by running the application on machines with and without compatible GPUs, and verifying the correct guidance or initialization occurs.

**Acceptance Scenarios**:

1. **Given** the application launches on Windows, **When** no compatible GPU is detected, **Then** the system displays an error message with specific guidance on installing GPU drivers.
2. **Given** the application launches on macOS, **When** the system queries available memory, **Then** unified memory availability is reported and used for model loading decisions.
3. **Given** all models are loaded onto the GPU, **When** combined memory usage is measured, **Then** total GPU memory consumption is under 6 GB.

---

### User Story 8 - macOS Permission Handling (Priority: P8)

A macOS user launches Vox for the first time. When they first try to dictate, the Accessibility permission prompt fires automatically (required for text injection). The Input Monitoring permission prompt fires when the global hotkey system initializes. If either permission is denied, the overlay shows a clear message with the exact system settings path. The application polls for permission status and proceeds automatically when the user grants it — no restart required.

**Why this priority**: macOS permission handling is platform-specific but critical for macOS users. Without graceful handling, denied permissions cause silent failures that are difficult to diagnose.

**Independent Test**: Can be tested on macOS by denying permissions and verifying the overlay guidance appears, then granting permissions and verifying the app proceeds without restart.

**Acceptance Scenarios**:

1. **Given** the application is running on macOS without Accessibility permission, **When** the user attempts to dictate, **Then** the overlay shows "Accessibility permission required" with the path "System Settings > Privacy & Security > Accessibility."
2. **Given** the application is running on macOS without Input Monitoring permission, **When** the global hotkey system initializes, **Then** the overlay shows guidance for granting Input Monitoring permission.
3. **Given** the user grants a previously denied permission, **When** the system polls for permission status, **Then** the application detects the grant and proceeds automatically without requiring a restart.

---

### Edge Cases

- What happens when the user has no internet and models are not yet downloaded? The system shows manual download instructions with the URL and expected file location, and polls every 5 seconds for the model files to appear on disk.
- What happens when multiple pipeline components fail simultaneously on the same segment? Each component retries independently; if all retries fail, the entire segment is discarded and the pipeline continues.
- What happens when the GPU runs out of memory mid-session (e.g., another application allocates GPU memory)? The system detects the OOM condition and displays guidance to close other GPU-intensive applications.
- What happens when a model file is deleted while the pipeline is running? The pipeline detects the missing file, stops, re-enters the downloading state, and re-downloads the model.
- What happens when the system wakes from sleep but the network is not yet available for a model re-download? The system enters the polling loop (check every 5 seconds) and shows status in the overlay.
- What happens when the portable Windows executable is run from a read-only location? The application stores its data (models, logs, settings, database) in the platform-standard user data directory, not alongside the executable.
- What happens when log files accumulate beyond the retention period? Files older than 7 days are automatically deleted on application startup and during daily rotation.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST automatically retry any failed pipeline segment (speech recognition or language model) exactly once before discarding and continuing to the next segment.
- **FR-002**: System MUST continue listening for new audio segments after any single-segment failure, regardless of whether the retry succeeded or the segment was discarded. (Explicit continuation guarantee for FR-001's retry-then-discard flow.)
- **FR-003**: System MUST buffer text when injection into the target application fails, display the buffered text in the overlay with a "Copy" button, and reattempt injection on the next focus event.
- **FR-004**: System MUST detect audio device disconnection and automatically switch to the default system audio device.
- **FR-005**: System MUST retry audio device connection every 2 seconds when no device is available, displaying status in the overlay.
- **FR-006**: System MUST detect model file corruption (e.g., checksum mismatch) and automatically delete and re-download the corrupt file.
- **FR-007**: System MUST display actionable guidance (not crash or show cryptic errors) when the GPU is unavailable, out of memory, or experiencing a driver crash.
- **FR-008**: System MUST recover the audio device, GPU context, and hotkey registration after system sleep/wake without requiring a manual application restart.
- **FR-009**: System MUST write structured log entries for every pipeline stage with timing information (latency per stage in milliseconds).
- **FR-010**: System MUST rotate log files daily, retain them for 7 days, and cap individual log files at 10 MB.
- **FR-011**: System MUST support configuring log verbosity via an environment variable.
- **FR-012**: System MUST verify all downloaded model files via SHA-256 checksum before accepting them.
- **FR-013**: System MUST NOT write any audio data to disk at any point during processing.
- **FR-014**: System MUST NOT make any network calls after all models have been downloaded.
- **FR-015**: System MUST perform secure deletion (data overwrite followed by storage reclamation) when the user clears transcript history.
- **FR-016**: System MUST allow users to disable transcript history persistence entirely in settings.
- **FR-017**: System MUST produce a release binary under 15 MB (excluding model files) on both target platforms.
- **FR-018**: System MUST provide a portable executable on Windows that runs without an installer.
- **FR-019**: System MUST provide a standard application bundle on macOS that can be distributed via disk image.
- **FR-020**: System MUST detect GPU availability at startup and report GPU model and available memory.
- **FR-021**: System MUST keep combined GPU memory usage under 6 GB across all loaded models.
- **FR-022**: On macOS, system MUST display the exact system settings path when a required permission (Accessibility, Input Monitoring) is not granted.
- **FR-023**: On macOS, system MUST poll for permission status and proceed automatically when permissions are granted, without requiring a restart.
- **FR-024**: System MUST show manual download instructions with the file URL and expected disk location when models are needed but no internet is available, and poll every 5 seconds for the files to appear.
- **FR-025**: System MUST store all user data (models, logs, settings, database) in the platform-standard user data directory, independent of the executable location.
- **FR-026**: System MUST provide a Windows installer package (.msi) in addition to the portable executable, for users who prefer a standard installation flow.
- **FR-027**: System MUST set downloaded model files to read-only permissions after successful download and verification, preventing accidental modification.

### Key Entities

- **VoxError**: Represents a categorized failure in any pipeline stage, including the error category (audio, model, recognition, language model, injection, GPU), severity, and the recovery action to be taken.
- **RecoveryAction**: The automatic response to a pipeline error — retry segment, discard segment, switch device, re-download model, display guidance, or enter polling loop.
- **LogEntry**: Structured tracing spans emitted via the `tracing` crate, containing timestamp, severity level, component name, message, and typed fields (latency_ms, model name, device name, error details). Not a discrete struct — entries are produced by the span/event system.
- **GpuInfo**: Detected GPU hardware information including model name, available memory, and driver version.
- **PermissionStatus**: On macOS, the current state of each required system permission (Accessibility, Input Monitoring) — checked at runtime via `AXIsProcessTrusted()` (boolean) and hotkey registration success/failure. Polled every 2 seconds when denied.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The application continues accepting new dictation segments within 2 seconds of any single-component failure, with zero manual intervention required.
- **SC-002**: Audio device disconnection results in automatic recovery to the default device within 3 seconds, or a visible retry indicator if no device is available.
- **SC-003**: After system sleep/wake, the application returns to a functional idle state (hotkey responsive, audio device connected) within 5 seconds of system wake.
- **SC-004**: Every dictation session produces log entries with per-stage timing that can be used to identify the slowest pipeline stage without reproducing the session.
- **SC-005**: Zero audio bytes are written to disk during normal operation, verified by file system monitoring during a 10-minute dictation session.
- **SC-006**: Zero outbound network connections occur after model download completes, verified by network traffic monitoring during a 10-minute session.
- **SC-007**: Release binary size is under 15 MB on both target platforms (excluding model files).
- **SC-008**: Combined GPU memory usage across all loaded models stays under 6 GB on both target machines.
- **SC-009**: Users on macOS can go from denied permissions to functional dictation by granting permissions in System Settings without restarting the application.
- **SC-010**: The application survives 1000 consecutive simulated segment failures (alternating components) without entering a dead state or leaking memory beyond 2x baseline RSS.

## Assumptions

- GPU is required on Windows (NVIDIA with CUDA support). macOS always has Metal via Apple Silicon.
- The application targets two reference machines: NVIDIA RTX 4090 (24 GB VRAM) and Apple M4 Pro (24 GB unified memory).
- "Portable executable" on Windows means a single .exe file that can run from any directory without installation (data stored in the platform-standard user data directory).
- macOS distribution uses a standard .dmg disk image containing a .app bundle.
- The 15 MB binary size target applies to the stripped release binary only, not including debug symbols or model files.
- Existing logging infrastructure (daily rotation, platform directories, environment variable config, UI log sink) is already in place and will be extended, not replaced.
- Existing SHA-256 model verification during download is already in place and will be extended to cover runtime corruption detection.
- The retry-once-then-discard policy is a fixed behavior, not configurable by the user, to keep the recovery logic predictable.
- MSI installer creation is a build artifact step (not an in-app feature) using standard Windows packaging tools.
- DMG creation is a build artifact step using standard macOS packaging tools.
