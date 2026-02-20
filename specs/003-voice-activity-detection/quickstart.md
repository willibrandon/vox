# Quickstart: Voice Activity Detection (Feature 003)

**Branch**: `003-voice-activity-detection`
**Prerequisite**: Feature 002 (audio capture) merged into main

## Build

```bash
# Windows (CUDA) — VAD is CPU-only but builds within the cuda feature gate
cargo build -p vox_core --features cuda

# macOS (Metal)
cargo build -p vox_core --features metal
```

Zero warnings required.

## Dependencies

The following dependency is already in `crates/vox_core/Cargo.toml`:

| Crate | Version | Feature | Purpose |
|-------|---------|---------|---------|
| `ort` | `2.0.0-rc.11` | `load-dynamic` | ONNX Runtime for Silero VAD inference |
| `hound` | `3.5` | (dev-dependency) | WAV file reading in tests |

No new runtime dependencies need to be added. All other dependencies (`ringbuf`, `tokio`, `anyhow`, `tracing`) are already present from Feature 002.

## ONNX Runtime Shared Library

The `ort` crate with `load-dynamic` loads the ONNX Runtime shared library at runtime. The DLL is **not** auto-downloaded when `load-dynamic` is enabled (the build script exits early).

Setup:

1. Download ONNX Runtime v1.23.0+ from [Microsoft releases](https://github.com/microsoft/onnxruntime/releases)
2. Place the shared library in `vendor/onnxruntime/`:
   - **Windows**: `vendor/onnxruntime/onnxruntime.dll`
   - **macOS**: `vendor/onnxruntime/libonnxruntime.dylib`
3. `.cargo/config.toml` sets `ORT_DYLIB_PATH` to this location automatically

## Test Fixtures

Silero VAD v5 ONNX model (~2.3 MB) and speech sample WAV:

```bash
mkdir -p crates/vox_core/tests/fixtures

# Silero VAD v5 model
curl -L -o crates/vox_core/tests/fixtures/silero_vad_v5.onnx \
  https://raw.githubusercontent.com/snakers4/silero-vad/master/src/silero_vad/data/silero_vad.onnx

# Speech sample WAV (16 kHz mono PCM)
curl -L -o crates/vox_core/tests/fixtures/speech_sample.wav \
  https://raw.githubusercontent.com/snakers4/silero-vad/master/examples/c++/aepyx.wav
```

## Test

```bash
# All VAD tests (requires model file + WAV + onnxruntime DLL)
cargo test -p vox_core --features cuda -- vad

# All vox_core tests
cargo test -p vox_core --features cuda

# Single test with output
cargo test -p vox_core --features cuda test_state_machine_silent_to_speaking -- --nocapture
```

### Unit Tests

| Test | What it validates |
|------|-------------------|
| `test_state_machine_silent_to_speaking` | Threshold crossing triggers SpeechStart |
| `test_state_machine_speaking_to_silent` | Silence duration triggers SpeechEnd |
| `test_state_machine_force_segment` | 30s+ speech triggers ForceSegment |
| `test_state_machine_min_speech` | Speech < 250ms is discarded |
| `test_state_machine_brief_pause` | Brief silence < min_silence_ms stays in Speaking |
| `test_chunker_accumulates` | Samples accumulate during speech |
| `test_chunker_emits_on_end` | Complete segment returned on SpeechEnd |
| `test_chunker_padding` | Output includes pad_ms before/after speech |
| `test_chunker_flush` | Remaining samples returned on flush |
| `test_chunker_force_segment_overlap` | 1-second overlap on force-segmented speech |

### Integration Tests (require model file + WAV)

| Test | What it validates |
|------|-------------------|
| `test_vad_model_loads` | Silero VAD loads from ONNX file |
| `test_vad_silent_audio` | Silence produces speech_prob < 0.1 |
| `test_vad_speech_audio` | Speech WAV produces speech_prob > 0.5 in majority of windows |
| `test_vad_hidden_state_persistence` | Hidden state carries across calls |
| `test_vad_reset` | Hidden state resets to zeros |
| `test_vad_end_to_end` | Full pipeline segments speech from ring buffer |
| `test_vad_multiple_utterances` | Three utterances produce three segments |

## Key Files

| File | Purpose |
|------|---------|
| `crates/vox_core/src/vad.rs` | VadConfig, VadState, VadEvent, VadStateMachine, run_vad_loop |
| `crates/vox_core/src/vad/silero.rs` | SileroVad ONNX model wrapper |
| `crates/vox_core/src/vad/chunker.rs` | SpeechChunker segment accumulator |
| `.cargo/config.toml` | Sets ORT_DYLIB_PATH for ONNX Runtime discovery |
| `vendor/onnxruntime/` | Platform-specific ONNX Runtime shared library |

## Verification

After implementation, verify:

1. `cargo build -p vox_core --features cuda` — zero warnings
2. `cargo test -p vox_core --features cuda` — all 26 tests pass, 0 ignored
