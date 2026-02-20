# Feature Specification: Voice Activity Detection

**Feature Branch**: `003-voice-activity-detection`
**Created**: 2026-02-19
**Status**: Draft
**Dependencies**: 002-audio-capture
**Design Reference**: Section 4.2 (Voice Activity Detection)
**Input**: User description: "Implement Voice Activity Detection (VAD) subsystem using Silero VAD v5 via ONNX Runtime, with streaming state machine and speech chunker"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Speech Detection from Microphone Stream (Priority: P1)

When the user speaks into their microphone, the system determines in real time which portions of the continuous audio stream contain speech and which are silence. Each analysis window (32ms of audio) receives a speech probability score so downstream components know exactly when the user is talking.

**Why this priority**: Without speech detection, the system cannot distinguish speech from background noise. This is the foundational capability that everything else depends on — no detection means no segmentation, no ASR dispatch, and no transcription.

**Independent Test**: Can be validated by feeding known audio files (silence-only and speech-containing) through the detector and verifying that speech probability scores are below 0.1 for silence and above 0.7 for speech.

**Acceptance Scenarios**:

1. **Given** the VAD model is loaded and the audio stream is active, **When** the user is silent, **Then** the speech probability for each 512-sample window is below 0.1.
2. **Given** the VAD model is loaded and the audio stream is active, **When** the user speaks clearly, **Then** the speech probability for each window during speech is above 0.7.
3. **Given** the VAD has processed multiple consecutive windows, **When** a new window arrives, **Then** the detector's internal state from prior windows is preserved and influences the current result (stateful inference).
4. **Given** a dictation session has ended and a new one begins, **When** the first window of the new session is processed, **Then** the internal state has been reset to initial values (clean slate between sessions).

---

### User Story 2 - Utterance Segmentation via State Machine (Priority: P1)

The system segments the continuous audio stream into discrete utterances by tracking transitions between silence and speech. When the user starts speaking, the system begins accumulating audio. When the user stops speaking for a sufficient pause, the accumulated audio is dispatched as a complete utterance for transcription.

**Why this priority**: Raw speech probabilities alone are not actionable — the ASR engine needs complete utterance segments, not frame-by-frame scores. The state machine turns a stream of probabilities into meaningful speech boundaries.

**Independent Test**: Can be validated by feeding a sequence of synthetic speech probabilities (simulating silence → speech → silence patterns) and verifying correct state transitions and event emissions without requiring the actual VAD model.

**Acceptance Scenarios**:

1. **Given** the system is in the Silent state, **When** speech probability meets or exceeds the threshold (0.5), **Then** a SpeechStart event is emitted and the system transitions to Speaking.
2. **Given** the system is in the Speaking state, **When** speech probability drops below threshold for at least 500ms of consecutive silence, **Then** a SpeechEnd event is emitted with the utterance duration.
3. **Given** the system is in the Speaking state, **When** the user speaks continuously for longer than 30 seconds, **Then** the system force-segments the audio and emits a ForceSegment event to prevent unbounded memory growth.
4. **Given** the system is in the Speaking state, **When** a brief silence shorter than 500ms occurs (e.g., a natural pause between words), **Then** the system remains in the Speaking state and continues accumulating.
5. **Given** the system is in the Silent state, **When** a very short burst of speech lasts less than 250ms (likely noise), **Then** no utterance is emitted and the system returns to Silent.

---

### User Story 3 - Speech Segment Delivery with Context Padding (Priority: P2)

When an utterance is complete, the system delivers the raw audio segment to the ASR engine with appropriate padding before and after the speech boundaries. This ensures the transcription engine receives enough acoustic context to accurately recognize the first and last words of each utterance.

**Why this priority**: Without padding, the ASR engine may cut off word beginnings/endings, producing inaccurate transcriptions. Without the chunker, there is no mechanism to deliver complete audio segments. This is essential for transcription quality but depends on the detection and segmentation layers.

**Independent Test**: Can be validated by feeding audio samples and simulated VAD events into the chunker and verifying that output segments include the correct amount of padding and that force-segmented long utterances include overlap for stitching continuity.

**Acceptance Scenarios**:

1. **Given** speech has been detected and is being accumulated, **When** a SpeechEnd event fires, **Then** the emitted audio segment includes 100ms of audio before the speech start and 100ms after the speech end.
2. **Given** speech is being accumulated, **When** a ForceSegment event fires for speech exceeding 30 seconds, **Then** the emitted segment includes a 1-second overlap region at the end so the next segment can stitch context.
3. **Given** the user presses the hotkey to stop recording mid-utterance, **When** a flush is requested, **Then** any buffered audio is immediately emitted as a final segment.
4. **Given** speech is being accumulated, **When** new audio samples arrive, **Then** the samples are appended to the buffer without blocking or allocating on the audio callback thread.

---

### User Story 4 - End-to-End VAD Processing Loop (Priority: P2)

The system continuously reads audio from the ring buffer, processes it through the VAD detector, feeds results to the state machine, accumulates audio in the chunker, and dispatches complete segments to the ASR engine — all on the processing thread, never blocking the audio capture.

**Why this priority**: This ties the individual components (detector, state machine, chunker) into a cohesive processing pipeline. It depends on all three sub-components being functional.

**Independent Test**: Can be validated end-to-end by feeding a WAV file containing multiple utterances separated by silence through the full pipeline and verifying the correct number of segments are emitted with expected durations.

**Acceptance Scenarios**:

1. **Given** audio is being captured into the ring buffer at any native sample rate, **When** the processing loop reads and (if needed) resamples to 16 kHz, **Then** 512-sample windows are correctly extracted and fed to the VAD.
2. **Given** a WAV file containing 3 distinct utterances separated by pauses, **When** processed through the full pipeline, **Then** exactly 3 speech segments are dispatched.
3. **Given** the VAD processing loop is running, **When** a complete speech segment is ready, **Then** it is sent to the ASR engine via a channel without blocking any other pipeline stage.

---

### Edge Cases

- What happens when the audio stream contains only background noise at varying levels (fan, keyboard, traffic)?
  → The VAD threshold filters these out; speech probability stays below threshold and no segments are emitted.

- What happens when the user speaks extremely quietly (near-threshold)?
  → Speech probabilities may fluctuate around the threshold. The min_speech_ms guard (250ms) prevents spurious short detections from being emitted.

- What happens when the user speaks without any pauses for over 30 seconds?
  → Force-segmentation at max_speech_ms (30s) prevents unbounded memory growth. Segments overlap by 1 second for ASR stitching continuity.

- What happens when the audio device disconnects mid-utterance?
  → The processing loop handles the stream ending gracefully. Any buffered audio is flushed as a final partial segment.

- What happens when the first audio windows arrive before the VAD model is fully loaded?
  → Per Constitution Principle 3, the pipeline does not start until all components are loaded. Audio capture only begins after the VAD model is ready.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST load the Silero VAD v5 model from an ONNX file and be ready to process audio windows.
- **FR-002**: System MUST process each 512-sample audio window (32ms at 16 kHz) and produce a speech probability in the range [0.0, 1.0].
- **FR-003**: System MUST preserve the VAD model's internal hidden state across consecutive window calls within a single dictation session.
- **FR-004**: System MUST reset the VAD model's hidden state to zeros when a new dictation session begins.
- **FR-005**: System MUST track a streaming state machine with Silent and Speaking states, transitioning based on configurable thresholds.
- **FR-006**: System MUST emit a SpeechStart event when speech probability meets or exceeds the threshold (default 0.5).
- **FR-007**: System MUST emit a SpeechEnd event when consecutive silence duration reaches min_silence_ms (default 500ms).
- **FR-008**: System MUST discard detected speech shorter than min_speech_ms (default 250ms) to filter noise bursts.
- **FR-009**: System MUST force-segment speech exceeding max_speech_ms (default 30,000ms) to prevent unbounded memory growth.
- **FR-010**: System MUST accumulate raw audio samples during speech and deliver complete segments when SpeechEnd or ForceSegment fires.
- **FR-011**: System MUST pad emitted speech segments with speech_pad_ms (default 100ms) of audio context before and after the detected speech boundaries.
- **FR-012**: System MUST include a 1-second overlap when force-segmenting long speech (> 10 seconds) to enable ASR stitching.
- **FR-013**: System MUST flush any buffered audio as a final segment when recording stops mid-utterance.
- **FR-014**: System MUST provide configurable parameters for all timing thresholds (speech threshold, min speech, min silence, max speech, padding, window size).
- **FR-015**: System MUST run all VAD processing on the processing thread, never on the real-time audio callback thread.
- **FR-016**: System MUST dispatch completed speech segments to the ASR engine via an asynchronous channel.

### Key Entities

- **VadConfig**: Configuration parameters for the VAD subsystem — speech probability threshold, timing durations for minimum speech, minimum silence, maximum speech, padding, and window size. All values have sensible defaults.
- **SileroVad**: The speech detection engine — loads the ONNX model, processes 512-sample audio windows, returns speech probabilities, and maintains hidden state across calls.
- **VadState**: The current state of the streaming state machine — either Silent (waiting for speech) or Speaking (with tracking of when speech started and how long it has lasted).
- **VadEvent**: Events emitted by state transitions — SpeechStart, SpeechEnd (with duration), and ForceSegment (with duration).
- **SpeechChunker**: Audio accumulator that buffers samples during speech segments, applies padding, handles overlap for force-segmented long speech, and emits complete audio chunks ready for transcription.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Each 32ms audio window is analyzed and a speech/silence decision is made in under 1ms of processing time.
- **SC-002**: The complete VAD subsystem (model + state) uses less than 5MB of memory.
- **SC-003**: Latency from audio arriving at the processing thread to a VAD decision being available is under 5ms.
- **SC-004**: Known speech audio (clear spoken words) is detected with probability above 0.7 in at least 95% of windows.
- **SC-005**: Known silence audio is correctly identified with probability below 0.1 in at least 99% of windows.
- **SC-006**: A test recording with 3 distinct utterances separated by 1-second pauses is correctly segmented into exactly 3 speech segments.
- **SC-007**: Continuous speech exceeding 30 seconds is force-segmented without any audio data loss (segments overlap for continuity).
- **SC-008**: The feature compiles with zero warnings.

### Assumptions

- The audio arriving at the VAD is already in 16 kHz mono f32 format (resampled by the audio capture pipeline from Feature 002 if the device native rate differs).
- The Silero VAD v5 ONNX model file (~1.1 MB) is available on disk. Model download is handled by a separate feature (model management).
- The VAD runs on CPU only — no GPU acceleration is needed or desired for this lightweight model.
- The downstream ASR engine (Feature 004) will consume speech segments via a channel. The VAD feature defines the sending side of this channel; the receiving side is out of scope.

## Clarifications

### Session 2026-02-19

- Q: What are the correct ONNX tensor names for the hidden state? → A: Official Silero VAD v5 uses `state` (input) and `stateN` (output), not `h`/`hn` as described in the original feature description. See research.md R-002.
- Q: Should the VAD processing loop be async or sync? → A: Synchronous dedicated thread, not an async Tokio task. The ring buffer consumer and ONNX inference are both synchronous operations; async adds overhead without benefit. See research.md R-003.
