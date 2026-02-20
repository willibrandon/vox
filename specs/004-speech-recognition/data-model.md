# Data Model: Speech Recognition (ASR)

**Feature**: 004-speech-recognition
**Date**: 2026-02-19

## Entities

### AsrEngine

The speech recognition engine. Holds a loaded Whisper model and performs transcription. Cheaply cloneable via `Arc` for use across threads.

| Field | Type | Description |
|-------|------|-------------|
| `ctx` | `Arc<Mutex<WhisperContext>>` | Thread-safe handle to the loaded Whisper model context. Mutex serializes access to state creation and inference. |

**Lifecycle**: Created once at startup via `AsrEngine::new()`. Lives for the application lifetime. Cloned into background tasks for transcription.

**Relationships**: Receives `Vec<f32>` speech segments from `SpeechChunker` (VAD subsystem) via `tokio::sync::mpsc` channel. Outputs `String` text to the LLM post-processing stage.

### TranscriptionParams (internal)

Configuration for a single transcription call. Not exposed publicly — built internally in `transcribe()`.

| Parameter | Value | Purpose |
|-----------|-------|---------|
| `sampling_strategy` | `Greedy { best_of: 1 }` | Fastest decoding for clean VAD-segmented audio |
| `language` | `"en"` | English only for v1.0 |
| `no_speech_thold` | `0.6` | Threshold below which segments are treated as silence |
| `suppress_nst` | `true` | Suppress `[music]`, `[laughter]`, etc. tokens |
| `single_segment` | `true` | Treat input as one segment (VAD already segmented) |
| `no_context` | `true` | No cross-utterance context (each segment independent) |
| `n_threads` | `4` | CPU threads for non-GPU compute |
| `print_progress` | `false` | Suppress stdout output |

### Stitching (for force-segmented speech)

Force-segmented speech is stitched via `stitch_segments(previous: &str, next: &str) -> String`. This is a pure function — no struct needed. It takes two transcription strings, deduplicates the overlap region using word-level LCS, and returns the combined text.

**State transitions**: N/A — the ASR engine is stateless between calls. Each transcription creates a fresh `WhisperState` and discards it after use. The stitcher operates purely on text output.

## Data Flow

```text
VAD SpeechChunker ─── Vec<f32> (16 kHz mono PCM) ──→ AsrEngine.transcribe()
                                                          │
                                                          ├─ Empty/silent audio → Ok("")
                                                          ├─ Normal speech → Ok("transcribed text")
                                                          └─ Error → Err(anyhow::Error)
                                                          │
                                              ┌───────────┘
                                              ▼
                                    (for force-segmented speech)
                                    Stitcher.stitch(prev_text, next_text)
                                              │
                                              ▼
                                    Deduplicated combined text
                                              │
                                              ▼
                                    LLM post-processing (Feature 005)
```
