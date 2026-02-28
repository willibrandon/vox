# Feature Specification: Audio Debug Tap

**Feature Branch**: `016-audio-debug-tap`
**Created**: 2026-02-27
**Status**: Draft
**Input**: User description: "Audio Debug Tap — save WAV files of captured audio at key pipeline stages for debugging audio quality, VAD boundary detection, resampling artifacts, and ASR input issues."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Diagnose VAD Segment Boundaries (Priority: P1)

A developer notices that Vox is cutting off the beginning of spoken words or merging separate utterances into one. They need to hear exactly what the VAD chunker emitted as a segment to determine whether the boundaries are correct, and compare it against what the speech recognizer actually received (which includes silence padding).

**Why this priority**: VAD boundary issues are the most common audio pipeline debugging scenario. Without hearing the actual segments, developers are blind to whether the problem is VAD configuration, audio quality, or ASR input formatting.

**Independent Test**: Can be fully tested by enabling segment-level recording, speaking a few sentences, and verifying that per-utterance WAV files appear on disk with audible content matching what was spoken.

**Acceptance Scenarios**:

1. **Given** debug audio is set to "Segments", **When** the user speaks a sentence and Vox processes it, **Then** a WAV file containing the VAD-segmented audio is saved to the debug audio directory.
2. **Given** debug audio is set to "Segments", **When** the user speaks a sentence, **Then** a separate WAV file containing the exact audio buffer sent to the speech recognizer (including silence padding) is saved, correlated to the same recording session and utterance.
3. **Given** debug audio is set to "Off" (the default), **When** the user speaks and Vox processes it, **Then** no audio files are written to disk.

---

### User Story 2 - Diagnose Raw Capture and Resampling Quality (Priority: P2)

A developer suspects that the microphone is not capturing cleanly (clipping, wrong device, low volume) or that the resampler is introducing artifacts (clicks at chunk boundaries, frequency distortion). They need continuous recordings of the raw microphone input and the post-resampler output to compare side-by-side.

**Why this priority**: Raw capture and resampling issues are less frequent than VAD boundary problems but are harder to diagnose without audio evidence. These are continuous streams (not per-utterance), so they produce significantly more data and are separated into a higher debug level.

**Independent Test**: Can be tested by enabling full-level recording, speaking for 10 seconds, and verifying that two continuous WAV files (raw and resampled) are produced for the entire recording session, playable in any audio tool.

**Acceptance Scenarios**:

1. **Given** debug audio is set to "Full", **When** the user records a 10-second session, **Then** a single continuous WAV file of the raw microphone capture (at the device's native sample rate) is saved.
2. **Given** debug audio is set to "Full", **When** the user records a 10-second session, **Then** a single continuous WAV file of the post-resampler output (at 16 kHz) is saved alongside the raw capture file.
3. **Given** debug audio is set to "Segments", **When** the user records a session, **Then** no raw capture or post-resampler files are produced (only per-utterance segment files).

---

### User Story 3 - Configure Debug Audio Level via Settings (Priority: P3)

A user or developer wants to enable or disable debug audio recording through the settings panel without restarting the application. They want to choose between three levels: Off (no recording), Segments (per-utterance files only), or Full (continuous raw + resampled streams plus per-utterance).

**Why this priority**: The configuration UI is necessary for usability but is not the core diagnostic capability. A developer could theoretically modify the settings file directly. The UI makes it accessible and allows mid-session toggling.

**Independent Test**: Can be tested by opening the settings panel, changing the debug audio dropdown, and verifying the new level takes effect on the next recording session.

**Acceptance Scenarios**:

1. **Given** the settings panel is open, **When** the user changes "Debug Audio Recording" from "Off" to "Segments Only", **Then** the next recording session produces per-utterance WAV files.
2. **Given** the settings panel is open and debug audio is "Full", **When** the user changes it to "Off" mid-recording, **Then** the current streaming WAV files are finalized and no new audio files are produced.
3. **Given** the settings file does not contain a debug audio setting, **When** the application starts, **Then** debug audio defaults to "Off" and no audio files are written.

---

### User Story 4 - Automatic Storage Management (Priority: P4)

A developer who has been debugging for several hours does not want debug audio files to accumulate indefinitely and fill their disk. The system should automatically clean up old files and cap total storage usage.

**Why this priority**: Storage management is a housekeeping concern that prevents the debug feature from becoming a liability. Without it, a developer running at "Full" level for extended periods could consume gigabytes of disk space.

**Independent Test**: Can be tested by creating old debug audio files, restarting the app, and verifying that files older than 24 hours are deleted and total storage stays under the cap.

**Acceptance Scenarios**:

1. **Given** the debug audio directory contains files older than 24 hours, **When** the application starts, **Then** those files are automatically deleted.
2. **Given** the debug audio directory exceeds 500 MB, **When** new files are written, **Then** the oldest files are deleted until the total is under 400 MB.
3. **Given** the disk is full or the debug audio directory is not writable, **When** the system attempts to write a debug audio file, **Then** the user is notified of the error via the overlay and no pipeline processing is disrupted.

---

### Edge Cases

- What happens when the user toggles debug audio on mid-recording? Audio capture starts from the next buffer read, with no retroactive capture of audio that already passed through. The writer thread auto-creates a session when it receives a segment or streaming message without a preceding StartSession, using a default session ID and timestamp.
- What happens when the user toggles debug audio off mid-recording? Current streaming WAV files are finalized normally; 1-2 additional files may appear as buffered messages drain.
- What happens when the disk write is slower than audio production (slow USB drive, antivirus scanning)? Excess audio data is dropped silently (gap in WAV file); the pipeline is never blocked or slowed.
- What happens if the application crashes during a recording session? Streaming WAV files will have incorrect headers but the audio data is recoverable by standard tools (Audacity, ffmpeg).
- What happens when the session counter resets? Session counter resets on each app launch; files are self-describing via embedded timestamp.
- How are recording sessions correlated across tap points? All files from the same recording share a session identifier; per-utterance files share a segment index across VAD and ASR tap points.
- What happens if the debug audio directory is deleted while the app is running? The system attempts to recreate the directory on the next StartSession. If recreation fails, the user is notified via the overlay and writing is suspended until the next session.
- What happens when debug audio is set to "Segments" but the user records silence (VAD never triggers)? Zero segment files are produced. The system logs an info-level message on session end indicating no segments were detected, preempting developer confusion about whether the feature is working.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST support four audio tap points in the processing pipeline: raw microphone capture (native sample rate), post-resampler output (16 kHz), VAD-segmented utterance (16 kHz), and ASR input with silence padding (16 kHz). All tap points record mono (single-channel) 32-bit float PCM.
- **FR-002**: System MUST provide three debug audio levels: Off (default, no files written), Segments (per-utterance files only), and Full (continuous raw + resampled streams plus per-utterance files).
- **FR-003**: System MUST write audio data as standard WAV files playable by any audio tool.
- **FR-004**: System MUST produce one continuous WAV file per recording session for each streaming tap (raw capture, post-resample), not one file per buffer read.
- **FR-005**: System MUST produce one WAV file per utterance for each per-segment tap (VAD segment, ASR input).
- **FR-006**: System MUST correlate all files from the same recording session via a shared session identifier in filenames.
- **FR-007**: System MUST correlate VAD segment and ASR input files for the same utterance via a shared segment index in filenames. File naming convention: `session-{NNN}_{ISO-timestamp}_{tap-type}[-{segment-NNN}].wav` (e.g., `session-001_2026-02-27T10-30-45_vad-segment-001.wav`).
- **FR-008**: System MUST never block or slow the audio processing pipeline when writing debug audio files. If the writer cannot keep up, excess data is dropped.
- **FR-009**: System MUST default to debug audio Off, ensuring no audio files are written during normal operation (preserving existing security constraints on audio persistence).
- **FR-010**: System MUST allow the debug audio level to be changed at runtime through the settings panel without restarting the application.
- **FR-011**: System MUST automatically delete debug audio files older than 24 hours on application startup. Note: age-based cleanup only runs at startup, not periodically. During long-running sessions, old files may persist beyond 24 hours until either the 500 MB cap triggers deletion or the application restarts. This is a known limitation acceptable for a debug feature.
- **FR-012**: System MUST enforce a 500 MB storage cap on the debug audio directory, deleting oldest files first when the cap is exceeded.
- **FR-013**: System MUST notify the user via the overlay when a write error occurs (disk full, permission denied), without disrupting pipeline processing.
- **FR-014**: System MUST persist the debug audio level setting to the settings file.
- **FR-015**: System MUST support both VAD mode (chunked utterances) and passthrough mode (hold-to-talk accumulation) for all four tap points.
- **FR-016**: System MUST use a bounded buffer for debug audio messages with a hard memory ceiling of 1 MB, preventing unbounded memory growth regardless of disk speed.
- **FR-017**: System MUST display the debug audio directory path in the settings panel when debug audio is not Off, with a Copy button that copies the path to the clipboard.
- **FR-018**: System MUST produce WAV files that can be successfully round-tripped through a standard WAV reader without error (valid RIFF headers, correct sample count, correct format metadata).
- **FR-019**: System MUST log an info-level session summary when a recording session ends, including: session ID, number of streaming files written, number of segment files written, total bytes written, and number of dropped samples (due to backpressure).
- **FR-020**: System SHOULD log the total number of dropped audio samples due to backpressure in the session summary, making writer-falling-behind conditions observable without inspecting WAV files for gaps.
- **FR-021**: System MUST log each VAD segment's duration at debug level when the vad_segment tap fires (e.g., segment duration in seconds and sample count), enabling boundary diagnosis without opening individual WAV files.
- **FR-022**: System MUST log an info-level message on session end if zero segments were emitted (VAD never triggered), preempting developer confusion about whether the feature is working.
- **FR-023**: When the debug audio directory is deleted or becomes inaccessible while the application is running, the system MUST attempt to recreate the directory on the next StartSession. If recreation fails, the system MUST notify the user via the overlay and set the write error flag.

### Key Entities

- **Debug Audio Session**: A recording session initiated by a hotkey press and ended by release/toggle. Groups all audio files from a single recording. Attributes: session ID (monotonic counter), timestamp, native sample rate.
- **Debug Audio Segment**: A single utterance within a session, identified by a segment index. Correlates the VAD-emitted segment with the silence-padded ASR input.
- **Debug Audio Level**: A three-valued setting (Off, Segments, Full) controlling which tap points are active. Persisted in user settings.
- **Debug Audio File**: A WAV file on disk, named with session ID, timestamp, tap type, and optional segment index. Subject to automatic age-based and size-based cleanup.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Each VAD-emitted segment produces exactly 2 WAV files (one vad-segment, one asr-input), verified with controlled synthetic audio input. A session that produces zero VAD segments (silence only) produces zero segment files — this is valid behavior, not an error.
- **SC-002**: Enabling "Full" level for a recording session produces exactly 2 continuous files (raw + resampled) regardless of the number of utterances, plus 2 files per VAD-emitted segment. A session with no detected speech produces only the 2 streaming files and zero segment files.
- **SC-003**: End-to-end pipeline latency with debug audio at Full does not increase by more than 1 ms compared to Off, measured over 100 utterances.
- **SC-004**: With debug audio set to Off (default), no audio files exist in the debug audio directory after any number of recording sessions (security constraint preserved).
- **SC-005**: Total debug audio storage never exceeds 500 MB, verified by running at "Full" level for 30+ minutes and confirming automatic cleanup.
- **SC-006**: Debug audio files older than 24 hours are deleted on startup. Cleanup runs on the writer thread (asynchronously from the main thread) and does not block the application from reaching a usable state. Cleanup completes within 5 seconds for up to 10,000 files on a standard disk.
- **SC-007**: When disk writes fail, the user sees an error notification in the overlay within 2 seconds, and the pipeline continues processing without interruption.
- **SC-008**: Memory used by the debug audio buffering system never exceeds 1 MB regardless of recording duration or disk speed.

## Assumptions

- Debug audio is a developer/power-user feature, not intended for end users. The UI placement in the Advanced settings section reflects this.
- WAV format (uncompressed PCM float32) is acceptable for debug audio. Compression is not needed since files are temporary and auto-cleaned.
- 24-hour file retention and 500 MB storage cap are reasonable defaults for debugging sessions. These are not user-configurable.
- The existing security tests verify that no audio files are persisted during normal operation. These tests remain valid because debug audio defaults to Off.
- Vox targets Windows and macOS only. File creation time (not modification time) is the reliable timestamp for cleanup on both platforms — NTFS and APFS both support creation time via `std::fs::Metadata::created()`. Linux is not a supported target; if it were, a fallback to modification time would be needed since ext4 creation time (`btime`) requires kernel 4.11+ with `statx(2)` and `Metadata::created()` returns `Err` on older kernels.
