# Data Model: Voice Activity Detection

**Feature**: 003-voice-activity-detection
**Date**: 2026-02-19

## Entities

### VadConfig

Configuration parameters for the VAD subsystem. All fields have sensible defaults. Immutable after construction — passed by reference to components that use it.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `threshold` | `f32` | `0.5` | Speech probability threshold. Values at or above trigger Speaking state. |
| `min_speech_ms` | `u32` | `250` | Minimum speech duration (ms) to accept. Shorter bursts are discarded as noise. |
| `min_silence_ms` | `u32` | `500` | Consecutive silence duration (ms) to trigger SpeechEnd. |
| `max_speech_ms` | `u32` | `30_000` | Maximum speech duration (ms) before force-segmenting. |
| `speech_pad_ms` | `u32` | `100` | Audio padding (ms) before and after detected speech boundaries. |
| `window_size_samples` | `u32` | `512` | Silero VAD input window size. Fixed at 512 for 16 kHz (32ms). |

**Traits**: `Clone`, `Debug`, `Default`

### SileroVad

ONNX model wrapper for Silero VAD v5 inference. Owns the `ort::Session` and carries hidden state between calls.

| Field | Type | Description |
|-------|------|-------------|
| `session` | `ort::session::Session` | Loaded ONNX model session. Single-threaded inference. |
| `hidden_state` | `Vec<f32>` | Hidden state tensor (2 × 1 × 128 = 256 elements). Carried across calls, reset between sessions. |
| `sample_rate` | `i64` | Always `16000`. Stored to avoid magic numbers in inference calls. |

**Not Send/Sync**: `Session` thread safety depends on ort version. Wrap in the processing thread — no sharing needed.

### VadState

Enum representing the current state of the streaming state machine.

| Variant | Fields | Description |
|---------|--------|-------------|
| `Silent` | (none) | Waiting for speech. Default initial state. |
| `Speaking` | `start_sample: usize`, `speech_duration_ms: u32` | Accumulating speech audio. Tracks when speech began and elapsed duration. |

**Traits**: `Debug`, `Clone`, `PartialEq`

### VadEvent

Events emitted by the state machine on transitions.

| Variant | Fields | Description |
|---------|--------|-------------|
| `SpeechStart` | (none) | Speech detected. Begin accumulating audio. |
| `SpeechEnd` | `duration_ms: u32` | Silence exceeded `min_silence_ms`. Segment ready. |
| `ForceSegment` | `duration_ms: u32` | Speech exceeded `max_speech_ms`. Force-emitting to cap memory. |

**Traits**: `Debug`, `Clone`, `PartialEq`

### VadStateMachine

Streaming state machine that converts speech probabilities into discrete events.

| Field | Type | Description |
|-------|------|-------------|
| `config` | `VadConfig` | Reference configuration for thresholds and durations. |
| `state` | `VadState` | Current state (Silent or Speaking). |
| `silence_duration_ms` | `u32` | Accumulated silence duration while in Speaking state. Resets on speech. |
| `total_samples_processed` | `usize` | Running count of 512-sample windows processed. Used for sample-position tracking. |

### SpeechChunker

Audio accumulator that buffers samples during speech segments and emits complete padded segments.

| Field | Type | Description |
|-------|------|-------------|
| `config` | `VadConfig` | Configuration for padding and overlap. |
| `speech_buffer` | `Vec<f32>` | Accumulated speech samples for current segment. |
| `pre_buffer` | `Vec<f32>` | Circular buffer holding last `speech_pad_ms` of audio for pre-padding. |
| `pre_buffer_pos` | `usize` | Write position in the circular pre-buffer. |
| `is_accumulating` | `bool` | Whether currently in a speech segment (between SpeechStart and SpeechEnd). |
| `post_pad_remaining` | `u32` | Samples still needed for post-padding after SpeechEnd. |

## Relationships

```
VadConfig ──────────────────────────┐
    │                               │
    ▼                               ▼
VadStateMachine                 SpeechChunker
    │                               │
    │ produces                      │ consumes
    ▼                               ▼
VadEvent ──────────────────────▶ SpeechChunker.feed()
                                    │
                                    │ produces
                                    ▼
                              Vec<f32> segment
                                    │
                                    ▼
                           mpsc::Sender<Vec<f32>>
                                    │
                                    ▼
                              ASR Engine (Feature 004)
```

```
SileroVad
    │
    │ process(audio) → f32
    │
    ▼
VadStateMachine.update(speech_prob) → Option<VadEvent>
```

## State Transitions

### VadStateMachine

```
                  speech_prob >= threshold
  ┌─────────┐  ────────────────────────────▶  ┌───────────┐
  │  Silent  │                                 │ Speaking   │
  │          │  ◀────────────────────────────  │            │
  └─────────┘   silence >= min_silence_ms      └───────────┘
                  (emit SpeechEnd)                   │
                                                     │ duration >= max_speech_ms
                                                     │ (emit ForceSegment,
                                                     │  stay in Speaking)
                                                     ▼
                                               ┌───────────┐
                                               │ Speaking   │
                                               │ (reset     │
                                               │  duration) │
                                               └───────────┘
```

**Transition rules:**
1. Silent → Speaking: `speech_prob >= threshold` (emit `SpeechStart`)
2. Speaking → Silent: `silence_duration_ms >= min_silence_ms` AND `speech_duration_ms >= min_speech_ms` (emit `SpeechEnd`)
3. Speaking → Silent (discard): `silence_duration_ms >= min_silence_ms` AND `speech_duration_ms < min_speech_ms` (no event, discard as noise)
4. Speaking → Speaking (force): `speech_duration_ms >= max_speech_ms` (emit `ForceSegment`, reset duration counter, stay Speaking)

### SpeechChunker

```
  ┌──────────┐  SpeechStart   ┌──────────────┐
  │  Idle    │ ──────────────▶│ Accumulating  │
  │ (pre-buf │                │ (speech_buf)  │
  │  filling)│                │               │
  └──────────┘                └──────┬────────┘
       ▲                             │
       │     SpeechEnd /             │
       │     ForceSegment            │
       │     (emit segment)          │
       └─────────────────────────────┘
```

## Data Volume

| Buffer | Size | Calculation |
|--------|------|-------------|
| Hidden state | 1 KB | 256 × f32 = 1,024 bytes |
| Pre-buffer (padding) | 6.4 KB | 1,600 samples × 4 bytes (100ms at 16 kHz) |
| Speech buffer (30s max) | 1.92 MB | 480,000 samples × 4 bytes (worst case at force-segment) |
| Overlap copy (1s) | 64 KB | 16,000 samples × 4 bytes |
| Model file (on disk) | ~1.1 MB | Silero VAD v5 ONNX |
| Model in memory | ~2 MB | ONNX Runtime session overhead |
| **Total peak** | **~4 MB** | Well under 5 MB budget |
