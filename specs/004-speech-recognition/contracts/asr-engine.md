# API Contract: AsrEngine

**Feature**: 004-speech-recognition
**Date**: 2026-02-19

## Public API

### `AsrEngine::new`

Load a Whisper model from disk with GPU acceleration.

```text
Input:  model_path: &Path    — Path to ggml model file on disk
        use_gpu: bool         — Enable GPU acceleration (CUDA/Metal)
Output: Result<AsrEngine>     — Loaded engine or descriptive error
```

**Errors**:
- Model file missing or unreadable → error with path in message
- Model file corrupted or invalid format → error from whisper-rs
- GPU unavailable or out of memory → error at context creation time

**Constraints**:
- Model load time < 5 seconds on both target machines
- GPU memory consumption < 1.8 GB for the model

### `AsrEngine::transcribe`

Transcribe a complete speech segment into text.

```text
Input:  audio_pcm: &[f32]    — 16 kHz mono PCM float samples
Output: Result<String>        — Transcribed text (empty for silence) or error
```

**Behavior**:
- Empty slice → returns `Ok(String::new())` (not an error, per FR-003)
- Silent audio → returns `Ok(String::new())` (via no-speech threshold)
- Normal speech → returns trimmed transcription text
- Creates fresh WhisperState per call (no cross-utterance state, per FR-004/FR-008)
- Non-speech tokens (`[music]`, `[laughter]`) suppressed (per FR-007)

**Constraints**:
- 5s audio < 50ms on RTX 4090, < 150ms on M4 Pro
- 10s audio < 100ms on RTX 4090, < 300ms on M4 Pro
- Thread-safe: can be called while holding the internal mutex lock

### `AsrEngine::clone`

Cheaply clone the engine for use on another thread.

```text
Input:  (none)
Output: AsrEngine             — Clone sharing the same underlying model
```

**Behavior**:
- Clones the `Arc<Mutex<WhisperContext>>` — increments reference count only
- Both original and clone share the same model memory
- Callers must serialize access (the Mutex handles this internally)

### `stitch_segments`

Combine two transcription results from force-segmented speech, deduplicating the overlap region.

```text
Input:  previous: &str        — Text from the preceding segment
        next: &str            — Text from the following segment (with overlap)
Output: String                — Combined text with overlap deduplicated
```

**Behavior**:
- Splits both texts into word tokens
- Finds longest common subsequence between tail of `previous` and head of `next`
- Removes duplicate words from `next` and concatenates
- If no overlap found, concatenates with a space separator
- Empty inputs handled gracefully (returns the non-empty input)
