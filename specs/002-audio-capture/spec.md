# Feature Specification: Audio Capture Pipeline

**Feature Branch**: `002-audio-capture`
**Created**: 2026-02-19
**Status**: Draft
**Dependencies**: 001-workspace-scaffolding
**Design Reference**: Section 4.1 (Audio Capture Pipeline)

## Clarifications

### Session 2026-02-19

- Q: Should the ring buffer use a fixed sample count (32768) or scale to hold 2 seconds at the device's native sample rate? → A: Scale to 2 seconds at native rate, rounded up to the next power of two for cheap index wrapping. At 48 kHz, 96000 samples rounds to 131072 (~2.73 seconds, ~512 KB).

## User Scenarios & Testing

### User Story 1 — Capture Microphone Audio (Priority: P1)

The system captures audio from the user's default microphone and makes it available to the downstream dictation pipeline. When the user speaks, raw audio samples flow into a fixed-size buffer that the voice activity detector and speech recognizer consume. This is the foundation — nothing downstream works without it.

**Why this priority**: Every other pipeline component depends on audio data. Without capture, there is no dictation. This is the minimum viable audio subsystem.

**Independent Test**: Start audio capture, speak into the microphone, verify that samples appear in the buffer within 100 ms and that capture stops cleanly without hanging threads or leaked resources.

**Acceptance Scenarios**:

1. **Given** the system has a working microphone, **When** audio capture starts, **Then** audio samples flow into the buffer continuously at the device's native sample rate.
2. **Given** audio capture is running, **When** capture is stopped, **Then** the audio stream closes cleanly with no resource leaks and no hanging threads.
3. **Given** the buffer is full, **When** new audio arrives, **Then** the oldest samples are silently dropped (no crash, no allocation, no blocking).
4. **Given** the audio callback is executing on the real-time OS thread, **Then** zero heap allocations, zero lock acquisitions, and zero blocking calls occur within the callback.

---

### User Story 2 — Resample to Pipeline Format (Priority: P1)

Most microphones capture at 44.1 kHz or 48 kHz, but the speech recognizer requires 16 kHz mono f32 PCM. The system resamples audio from the device's native rate to 16 kHz on the processing thread (never on the audio callback thread). When the device already captures at 16 kHz, no resampling occurs.

**Why this priority**: Without correct resampling, the speech recognizer receives garbled audio and produces garbage transcriptions. This is a hard dependency for any real device.

**Independent Test**: Feed a known sine wave at 44.1 kHz and 48 kHz into the resampler, verify the output at 16 kHz preserves the frequency. Confirm that 16 kHz input bypasses resampling entirely.

**Acceptance Scenarios**:

1. **Given** the device captures at 48 kHz, **When** audio reaches the processing thread, **Then** it is resampled to 16 kHz mono f32 with no audible artifacts.
2. **Given** the device captures at 44.1 kHz, **When** audio reaches the processing thread, **Then** it is resampled to 16 kHz mono f32 with no audible artifacts.
3. **Given** the device captures at 16 kHz, **When** audio reaches the processing thread, **Then** no resampling occurs (bypass path).
4. **Given** audio is being resampled, **Then** resampling throughput exceeds 10x real-time (1 second of audio processed in under 100 ms).

---

### User Story 3 — Switch Microphones (Priority: P2)

The user can change their input microphone in settings. When the device changes (or disconnects unexpectedly), the system stops the current stream, drains the buffer, starts a new stream on the new device, and reconfigures the resampler if the native sample rate changed. Device disconnection is detected and reported — it never fails silently.

**Why this priority**: Users with multiple microphones (headset, webcam, USB mic) need to select the right one. Device disconnection must be handled gracefully. Important for usability but not required for basic dictation with the default mic.

**Independent Test**: Start capture on device A, switch to device B in settings, verify capture resumes on device B. Disconnect the active device, verify the system reports the disconnection.

**Acceptance Scenarios**:

1. **Given** capture is running on device A, **When** the user selects device B in settings, **Then** capture stops on A, the buffer drains, capture starts on B, and the resampler updates if needed.
2. **Given** capture is running, **When** the active device disconnects, **Then** the system detects the disconnection and reports it to the pipeline (no silent failure, no crash).
3. **Given** the user opens settings, **When** they view device options, **Then** they see a list of all available input devices with the current default highlighted.

---

### User Story 4 — Enumerate Audio Devices (Priority: P2)

The settings UI displays all available input devices so the user can choose which microphone to use. Each device listing shows the device name and whether it is the system default.

**Why this priority**: Supports User Story 3 (device switching). Without enumeration, users cannot select a specific device.

**Independent Test**: Call device enumeration, verify at least one device is returned on any system with a microphone.

**Acceptance Scenarios**:

1. **Given** the system has one or more microphones, **When** device enumeration is requested, **Then** a list of available input devices is returned with name and default status.
2. **Given** the system has no microphones, **When** device enumeration is requested, **Then** an empty list is returned (no crash).

---

### Edge Cases

- **No microphone available**: System reports "no input device found" — does not crash, does not silently continue.
- **Buffer overflow**: When the processing thread cannot consume samples fast enough, excess samples are dropped (the SPSC ring buffer drops samples that don't fit on the producer side). No allocation, no panic, no blocking.
- **Device disconnection mid-capture**: Detected and reported to the pipeline. System is ready to resume when a new device becomes available.
- **Stereo/multi-channel input**: If the device provides stereo or multi-channel audio, only the first channel is used (downmix to mono).
- **Extremely high sample rates (96 kHz+)**: Resampler handles any common rate down to 16 kHz.
- **Resampler chunk boundaries**: Audio is processed in chunks matching the resampler's internal requirements. Partial chunks at stream boundaries are handled without data loss.

## Requirements

### Functional Requirements

- **FR-001**: System MUST capture audio from the system's default input device when no device is explicitly configured.
- **FR-002**: System MUST capture audio from a user-specified input device when one is configured in settings.
- **FR-003**: System MUST buffer captured audio in a lock-free single-producer single-consumer ring buffer sized to hold at least 2 seconds of audio at the device's native sample rate, with capacity rounded up to the next power of two.
- **FR-004**: System MUST resample audio from the device's native sample rate to 16 kHz mono f32 PCM on the processing thread (never on the audio callback thread).
- **FR-005**: System MUST bypass resampling entirely when the device natively captures at 16 kHz.
- **FR-006**: System MUST perform zero heap allocations and zero lock acquisitions within the audio callback.
- **FR-007**: System MUST enumerate all available input devices with name and default status.
- **FR-008**: System MUST support switching the active input device at runtime without restarting the application.
- **FR-009**: System MUST detect device disconnection and report it to the pipeline (no silent failure).
- **FR-010**: System MUST drop excess samples when the ring buffer overflows (no blocking, no crash, no allocation).
- **FR-011**: System MUST downmix multi-channel input to mono (single channel).

### Key Entities

- **AudioConfig**: Capture settings — target sample rate (16 kHz), channel count (1/mono), sample format (f32), optional device name override.
- **AudioCapture**: Active audio stream — manages the OS audio stream, writes raw samples into the ring buffer, tracks device name and native sample rate.
- **AudioRingBuffer**: Lock-free SPSC buffer — producer end on audio callback thread, consumer end on processing thread. Capacity: at least 2 seconds at the device's native rate, rounded up to the next power of two (e.g., 131072 samples at 48 kHz = ~512 KB, ~2.73 seconds).
- **AudioResampler**: Sample rate converter — converts from device native rate to 16 kHz. Operates on the processing thread only. Bypassed when native rate already matches.
- **AudioDeviceInfo**: Device metadata for the settings UI — name, default status.

## Success Criteria

### Measurable Outcomes

- **SC-001**: Audio samples appear in the buffer within 100 ms of capture starting.
- **SC-002**: Audio callback executes in under 5 ms with zero allocations and zero lock acquisitions (verified via code review).
- **SC-003**: Ring buffer read latency is under 1 ms.
- **SC-004**: Resampling throughput exceeds 10x real-time (1 second of audio processed in under 100 ms).
- **SC-005**: Ring buffer memory usage scales with device sample rate: approximately 512 KB at 48 kHz (131072 f32 samples at 4 bytes each), always rounded to the next power-of-two sample count.
- **SC-006**: CPU usage with an active audio stream is under 1% at idle.
- **SC-007**: Capture starts and stops cleanly with no resource leaks on repeated start/stop cycles.
- **SC-008**: Zero compiler warnings across all audio module code.

## Thread Safety Model

The audio subsystem uses two threads with a single lock-free channel between them:

- **Audio callback thread** (OS-managed, real-time priority): Writes raw samples into the ring buffer producer. No allocations, no locks, no blocking, no ML.
- **Processing thread** (application-managed): Reads samples from the ring buffer consumer, resamples if needed, feeds downstream pipeline. May allocate, may block briefly.

The ring buffer is the only communication channel between these two threads. No shared mutable state exists outside the buffer.

## Assumptions

- The system has at least one audio input device available (microphone, line-in, or virtual device).
- The OS audio subsystem is functional and accessible without elevated privileges.
- Common device sample rates include 16000, 44100, 48000, and 96000 Hz.
- The processing thread runs frequently enough to consume audio before the 2-second buffer fills.
- The audio library handles real-time thread priority for the audio callback automatically — no manual thread priority management is needed.
