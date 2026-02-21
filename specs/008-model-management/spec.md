# Feature Specification: Model Management

**Feature Branch**: `008-model-management`
**Created**: 2026-02-20
**Status**: Draft
**Input**: User description: "Model management subsystem for auto-download, SHA-256 verification, concurrent downloads, storage, and swapping of ML models"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Zero-Click First Launch (Priority: P1)

A user installs Vox and launches it for the first time. The application automatically detects that the three required ML models (VAD, ASR, LLM) are not present on disk, begins downloading all three concurrently, and shows real-time download progress. Once all downloads complete and pass integrity verification, the pipeline is ready. The user never sees a setup wizard, confirmation dialog, or "click to download" button.

**Why this priority**: This is the core user experience — Constitution Principle V (Zero-Click First Launch). Without automatic model provisioning, Vox cannot function at all. Every other feature depends on models being present.

**Independent Test**: Can be tested by clearing the model directory and launching the application. Delivers a fully-functional dictation engine without any manual setup steps.

**Acceptance Scenarios**:

1. **Given** a fresh installation with no models on disk, **When** the user launches Vox, **Then** all three models begin downloading concurrently with progress displayed for each model (name, bytes downloaded, total bytes).
2. **Given** downloads are in progress, **When** all three models finish downloading, **Then** each file is verified against its known SHA-256 checksum before being made available to the pipeline.
3. **Given** a download completes but the file is corrupt (checksum mismatch), **When** verification fails, **Then** the corrupt file is deleted and the download is retried automatically.
4. **Given** some models are already present on disk from a previous installation, **When** the user launches Vox, **Then** only missing models are downloaded; existing valid models are used immediately.
5. **Given** all three models are already present and verified, **When** the user launches Vox, **Then** no network calls are made and the pipeline starts immediately.

---

### User Story 2 - Download Failure Recovery (Priority: P2)

A user launches Vox without internet access, or a download fails due to a server error or timeout. The application displays manual download instructions including the model directory path and direct download URLs. The user can manually download models via their browser or copy them from a USB drive. The application continuously monitors the model directory and detects manually-placed files within 5 seconds.

**Why this priority**: Network failures are inevitable. Users must have a clear recovery path without the application becoming permanently stuck. This story also covers air-gapped deployment scenarios.

**Independent Test**: Can be tested by blocking network access and verifying the fallback UI appears with correct download URLs, then placing model files manually and confirming detection.

**Acceptance Scenarios**:

1. **Given** a download fails (no internet, server error, timeout), **When** the failure is detected, **Then** the application displays the model directory path and direct download URLs for each missing model.
2. **Given** the download failure UI is showing, **When** the user clicks "Open Folder", **Then** the model directory opens in the system file explorer.
3. **Given** the download failure UI is showing, **When** the user clicks "Retry Download", **Then** the failed model downloads are retried.
4. **Given** models are missing and the application is polling, **When** the user manually places a model file in the model directory, **Then** the file is detected within 5 seconds and loaded by the pipeline.
5. **Given** a re-download after corruption also fails, **When** both attempts have failed, **Then** manual download instructions are shown with direct URLs.

---

### User Story 3 - Model Swapping (Priority: P3)

A power user wants to swap one of the ML models for a different variant (e.g., a smaller Whisper model for faster inference, or a different LLM). They place a new model file in the model directory. Vox validates the file format, runs a quick benchmark to verify it works with GPU acceleration, and if validation passes, restarts the pipeline with the new model.

**Why this priority**: Model swapping is a power-user feature that enables experimentation. It depends on the core download and storage infrastructure from P1 and P2.

**Independent Test**: Can be tested by placing an alternative model file in the model directory and verifying the pipeline restarts with the new model after validation.

**Acceptance Scenarios**:

1. **Given** the user places a new model file in the model directory, **When** Vox detects the file, **Then** it validates the file format by checking the file header (GGUF, GGML, or ONNX magic bytes).
2. **Given** file format validation passes, **When** Vox loads the model, **Then** a quick benchmark inference is run to verify GPU acceleration works correctly.
3. **Given** benchmark validation passes, **When** the new model is ready, **Then** the pipeline restarts using the new model.
4. **Given** the user places a file with an invalid format, **When** validation fails, **Then** the user is informed that the file is not a recognized model format.

---

### Edge Cases

- What happens when disk space runs out during a download? The partial .tmp file is cleaned up and the error is reported to the user with the required disk space.
- What happens when two instances of Vox try to download simultaneously? The atomic write pattern (.tmp then rename) prevents file corruption; the first instance to complete the rename wins, the other detects the file already exists.
- What happens when a model file is deleted while the pipeline is running? The pipeline continues using the in-memory model; the model will be re-downloaded on next launch.
- What happens when the model directory does not exist? It is created automatically on first access.
- What happens when the model directory is on a read-only filesystem? The error is reported to the user with the expected directory path.
- What happens when a download is interrupted (application crash, system shutdown)? The .tmp file remains; on next launch, leftover .tmp files are cleaned up and the download restarts from scratch.
- What happens when the model URL returns a redirect (e.g., CDN redirect)? The HTTP client follows redirects automatically.
- What happens when the user places a model file with the correct name but wrong format? Header validation catches it and reports the mismatch to the user.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST maintain a built-in registry of required models with name, filename, download URL, expected SHA-256 checksum, and expected file size for each model.
- **FR-002**: System MUST store models in the platform-standard application data directory (`%LOCALAPPDATA%/com.vox.app/models/` on Windows, `~/Library/Application Support/com.vox.app/models/` on macOS).
- **FR-003**: System MUST detect which models are missing by checking for the existence of each expected filename in the model directory on startup.
- **FR-004**: System MUST download all missing models concurrently (not sequentially) with streaming (not buffering the entire file in memory).
- **FR-005**: System MUST verify the SHA-256 checksum of each downloaded file against the expected hash from the model registry.
- **FR-006**: System MUST use atomic file writes — download to a `.tmp` file, verify checksum, then rename to the final filename — to prevent partial files from being mistaken for valid models.
- **FR-007**: System MUST report download progress to the UI with model name, bytes downloaded, and total bytes. Progress updates MUST be throttled to no more than once every 500ms per model to avoid UI thrashing.
- **FR-008**: System MUST automatically re-download a model file if SHA-256 verification fails, deleting the corrupt file first.
- **FR-009**: System MUST display manual download instructions (model directory path and direct download URLs) via the overlay HUD when a download fails and the automatic retry also fails.
- **FR-010**: System MUST provide an "Open Folder" action that opens the model directory in the system file explorer.
- **FR-011**: System MUST provide a "Retry Download" action that retries downloading all failed models.
- **FR-012**: System MUST poll the model directory every 5 seconds when any models are missing, detecting manually-placed files.
- **FR-013**: System MUST create the model directory automatically if it does not exist.
- **FR-014**: System MUST NOT make any network calls once all models are present on disk. The only permitted network operation is model download during first-run setup.
- **FR-015**: System MUST clean up any leftover `.tmp` files from interrupted downloads on startup.
- **FR-016**: System MUST validate model file formats by checking file headers (GGUF, GGML, ONNX magic bytes) when a user places a new model file for swapping.
- **FR-017**: System MUST run a single benchmark inference (one forward pass with test input, completing within 30 seconds without error) when a new model is swapped to verify GPU acceleration works before restarting the pipeline.
- **FR-018**: Pipeline MUST NOT start until all three required models (VAD, ASR, LLM) are present and verified. No degraded modes, no fallbacks, no optional components.

### Key Entities

- **ModelInfo**: Represents a single ML model in the registry. Contains the model's human-readable name, filename on disk, download URL, expected SHA-256 hash, and expected file size in bytes. The registry is static and compiled into the application.
- **DownloadEvent**: Represents a state change during model download. Contains the model name and event type: started (with total bytes), progress (with bytes downloaded and total), complete, failed (with error message), or verification failed.
- **DownloadProgress**: Represents the current download state of a single model. One of: Pending (not started), InProgress (with byte counts), Complete (downloaded and verified), or Failed (with error message and manual download URL).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can go from first launch to a fully-functional dictation engine without any manual setup steps or configuration dialogs.
- **SC-002**: All three models download concurrently, achieving throughput limited only by the user's network connection (no artificial throttling).
- **SC-003**: SHA-256 verification of a 1.8 GB file completes in under 5 seconds.
- **SC-004**: Model directory detection and file existence checks complete in under 100 ms.
- **SC-005**: Manually-placed model files are detected within 5 seconds of being placed in the model directory.
- **SC-006**: After all models are downloaded, the application makes zero network calls during normal operation.
- **SC-007**: Total download of all models (~2.5 GB) completes in under 10 minutes on a 100 Mbps connection.
- **SC-008**: Recovery from download failure is self-service — the user can resolve the issue using the displayed instructions without external help.

## Assumptions

- The model download URLs are stable and publicly accessible without authentication. If URLs change, a Vox update would be required with the new URLs.
- The user has sufficient disk space for all three models (~2.5 GB total). If disk space is insufficient, the download will fail and the error will be reported like any other download failure.
- The model registry (names, URLs, checksums, sizes) is compiled into the application binary and does not require a remote configuration service.
- Model swapping (FR-016, FR-017) is limited to replacing existing model slots with compatible alternatives. Adding entirely new model types is out of scope.
- Resume/partial downloads are not supported. If a download is interrupted, it restarts from scratch on next launch. This is acceptable because the largest model is ~1.6 GB and modern connections can download this in a few minutes.

## Dependencies

- **001-workspace-scaffolding**: The three-crate workspace structure must be in place.
- Platform-standard application data directory resolution for model storage paths.
- HTTP client for streaming downloads with redirect support.
- SHA-256 hashing for file integrity verification.
