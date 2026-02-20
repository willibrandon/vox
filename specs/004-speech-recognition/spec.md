# Feature Specification: Speech Recognition (ASR)

**Feature Branch**: `004-speech-recognition`
**Created**: 2026-02-19
**Status**: Draft
**Dependencies**: 003-voice-activity-detection
**Design Reference**: Section 4.3 (ASR Engine)

## User Scenarios & Testing

### User Story 1 - Transcribe a Single Utterance (Priority: P1)

As a user, I speak a sentence or phrase, and the system converts my speech into accurate text. The ASR engine receives a complete speech segment (already segmented by the VAD) and returns the transcribed words. This is the core value proposition — turning voice into text.

**Why this priority**: Without single-utterance transcription, the entire dictation pipeline has no output. Every other feature depends on this working correctly.

**Independent Test**: Can be fully tested by feeding a known speech audio segment to the ASR engine and verifying the returned text matches the spoken content.

**Acceptance Scenarios**:

1. **Given** a loaded speech recognition model, **When** a 5-second speech segment containing "hello world" is submitted, **Then** the returned text contains "hello world" (case-insensitive).
2. **Given** a loaded speech recognition model, **When** a segment of pure silence is submitted, **Then** the returned text is empty (not an error).
3. **Given** a loaded speech recognition model, **When** a very short speech segment (under 1 second) is submitted, **Then** the system returns whatever text it can recognize without erroring.

---

### User Story 2 - Sequential Transcriptions (Priority: P2)

As a user, I speak multiple utterances in succession during a dictation session. Each utterance is independently transcribed without state leaking between them. The ASR engine handles repeated calls correctly, producing accurate text for each segment.

**Why this priority**: Real dictation sessions involve many utterances. The engine must handle sequential calls without degradation, memory leaks, or state corruption.

**Independent Test**: Can be tested by feeding 5+ speech segments sequentially and verifying each produces correct, independent results.

**Acceptance Scenarios**:

1. **Given** a loaded speech recognition model, **When** 5 different speech segments are transcribed one after another, **Then** each returns accurate text independent of the others.
2. **Given** a loaded speech recognition model, **When** the same segment is transcribed twice, **Then** both calls return identical text.

---

### User Story 3 - Force-Segmented Long Speech Stitching (Priority: P3)

As a user, I speak continuously for longer than the force-segment threshold (30 seconds). The VAD splits this into overlapping chunks. The ASR engine transcribes each chunk and the system stitches results together, deduplicating the overlap region so the final text reads naturally without repeated words.

**Why this priority**: Long continuous speech is common in dictation. Without stitching, force-segmented audio would produce duplicated text at boundaries.

**Independent Test**: Can be tested by feeding two audio segments with a known 1-second overlap, transcribing both, and verifying the stitched result removes duplicate text from the overlap region.

**Acceptance Scenarios**:

1. **Given** two consecutive force-segmented audio chunks with 1-second overlap, **When** both are transcribed and stitched, **Then** the combined text contains no duplicate words at the boundary.
2. **Given** a continuous 45-second utterance split into three force-segmented chunks, **When** all are transcribed and stitched, **Then** the final text reads as a coherent continuous passage.

---

### Edge Cases

- What happens when the speech recognition model file is missing or corrupted? The system MUST return a clear error, not crash.
- What happens when audio contains non-speech sounds (typing, coughing, background noise)? The ASR should return empty text or ignore non-speech tokens.
- What happens when the audio is extremely short (less than 100ms)? The system MUST handle gracefully without errors.
- What happens when the GPU is unavailable or out of memory? The system MUST report the error clearly at model load time.
- What happens when two transcription requests arrive simultaneously? The system MUST serialize access to the model safely.

## Requirements

### Functional Requirements

- **FR-001**: System MUST load a speech recognition model from a file on disk with GPU acceleration enabled.
- **FR-002**: System MUST accept speech audio as 16 kHz mono PCM float samples and return transcribed text.
- **FR-003**: System MUST return an empty string (not an error) when given silent or empty audio input.
- **FR-004**: System MUST create fresh internal state (no reuse of internal buffers) for each transcription call to prevent cross-utterance contamination.
- **FR-005**: System MUST support safe sharing of the model across threads via cloning, so transcription can run on a background thread.
- **FR-006**: System MUST use greedy decoding with English language for all transcriptions in v1.0.
- **FR-007**: System MUST suppress non-speech tokens (music markers, laughter markers, etc.) from output.
- **FR-008**: System MUST treat each submitted segment as independent — the engine MUST NOT use prior transcription output to influence the current result.
- **FR-009**: System MUST support a stitching mechanism for force-segmented long speech, deduplicating the 1-second overlap region between consecutive segments.
- **FR-010**: System MUST report model loading failures with a descriptive error message.

### Key Entities

- **AsrEngine**: The speech recognition engine that holds a loaded model and performs transcription. Cheaply cloneable for use across threads. Accepts PCM audio, returns text.
- **Transcription Result**: The text output from a single transcription call. A plain string. Empty when no speech is detected.
- **Stitched Result**: The combined text from multiple force-segmented chunks after overlap deduplication.

## Success Criteria

### Measurable Outcomes

- **SC-001**: A 5-second spoken utterance is transcribed in under 50ms on the primary development machine and under 150ms on the mobile development machine.
- **SC-002**: A 10-second spoken utterance is transcribed in under 100ms on the primary development machine and under 300ms on the mobile development machine.
- **SC-003**: The speech recognition model loads from disk in under 5 seconds on both machines.
- **SC-004**: The model consumes less than 1.8 GB of GPU memory when loaded.
- **SC-005**: Silent audio segments produce empty text output 100% of the time.
- **SC-006**: Known speech audio (e.g., "hello world" test fixture) is transcribed correctly on every run.
- **SC-007**: 5 sequential transcriptions all produce correct results with no state leakage between calls.
- **SC-008**: Zero compiler warnings across the entire ASR module.

## Assumptions

- The Whisper Large V3 Turbo model (Q5_0 quantization, ~900 MB) is the target model. It provides ~8% word error rate, which is acceptable for dictation with LLM post-processing downstream.
- English-only transcription for v1.0. Multi-language support is out of scope.
- The VAD upstream provides properly segmented audio at 16 kHz mono. The ASR does not need to resample.
- GPU acceleration (CUDA on Windows, Metal on macOS) is mandatory. The system does not start without it per constitution principle 3.
- Flash attention is disabled — it is off by default and we do not enable it.
- The model file is downloaded by the model management system (a separate feature). This feature assumes the model file exists on disk.
