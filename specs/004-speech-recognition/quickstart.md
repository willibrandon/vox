# Quickstart: Speech Recognition (Feature 004)

**Branch**: `004-speech-recognition`
**Prerequisite**: Feature 003 (voice activity detection) merged into main

## Build

```bash
# Windows (CUDA) — ASR uses GPU via whisper.cpp CUDA backend
cargo build -p vox_core --features cuda

# macOS (Metal) — ASR uses GPU via whisper.cpp Metal backend
cargo build -p vox_core --features metal
```

Zero warnings required.

## Dependencies

The following dependency is already in `crates/vox_core/Cargo.toml`:

| Crate | Version | Feature | Purpose |
|-------|---------|---------|---------|
| `whisper-rs` | `0.15.1` | `cuda`, `metal` (feature-gated) | Whisper ASR via whisper.cpp FFI bindings |
| `hound` | `3.5` | (dev-dependency) | WAV file reading in tests |

No new runtime dependencies need to be added. `whisper-rs` is already declared. `whisper-rs-sys` (transitive) builds whisper.cpp from source via CMake during `cargo build`.

## Build Prerequisites

### Windows
- Visual Studio 2022 Build Tools (C++ workload)
- CUDA Toolkit 12.8+ with cuDNN 9.x
- `CMAKE_GENERATOR=Visual Studio 17 2022` (persistent env var — CUDA does not support VS Insiders)
- `CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8`

### macOS
- Xcode 26.x + Command Line Tools
- Metal Toolchain: `xcodebuild -downloadComponent MetalToolchain`

## Model File

The Whisper Large V3 Turbo model (Q5_0 quantization) must be downloaded separately:

```bash
# Create fixtures directory
mkdir -p crates/vox_core/tests/fixtures

# Download model (~900 MB)
# Option 1: Hugging Face
curl -L -o crates/vox_core/tests/fixtures/ggml-large-v3-turbo-q5_0.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin

# The speech_sample.wav from Feature 003 is reused for ASR testing
# It should already exist at: crates/vox_core/tests/fixtures/speech_sample.wav
```

The model file is gitignored (~900 MB). Tests requiring the model are marked `#[ignore]`.

## Test

```bash
# All ASR tests (requires model file + WAV)
cargo test -p vox_core --features cuda -- asr --ignored

# All vox_core tests (VAD + ASR)
cargo test -p vox_core --features cuda

# Single test with output
cargo test -p vox_core --features cuda test_asr_hello_world -- --nocapture --ignored
```

### Unit Tests (require model file, `#[ignore]`)

| Test | What it validates |
|------|-------------------|
| `test_asr_model_loads` | Whisper model loads from disk with GPU enabled |
| `test_asr_transcribe_speech` | Known speech WAV transcribes to expected text |
| `test_asr_empty_audio` | Empty audio returns empty string (not error) |
| `test_asr_silent_audio` | Silent audio returns empty string |
| `test_asr_short_segment` | Very short speech (< 1 second) handles gracefully |
| `test_asr_sequential` | 5 sequential transcriptions + 1 cloned engine transcription produce independent results |

### ASR Error Tests (no model required)

| Test | What it validates |
|------|-------------------|
| `test_asr_model_load_error` | Nonexistent model path returns descriptive error |

### Stitcher Tests (no model required)

| Test | What it validates |
|------|-------------------|
| `test_stitch_no_overlap` | Separate texts concatenated with space |
| `test_stitch_with_overlap` | Duplicate words at boundary removed |
| `test_stitch_empty_inputs` | Empty strings handled gracefully |
| `test_stitch_identical` | Identical texts produce single copy |

## Key Files

| File | Purpose |
|------|---------|
| `crates/vox_core/src/asr.rs` | AsrEngine: model loading, transcription, module root |
| `crates/vox_core/src/asr/stitcher.rs` | Force-segment overlap stitching |
| `crates/vox_core/Cargo.toml` | whisper-rs dependency (already present) |

## Verification

After implementation, verify:

1. `cargo build -p vox_core --features cuda` — zero warnings
2. `cargo test -p vox_core --features cuda` — all non-ignored tests pass
3. `cargo test -p vox_core --features cuda -- asr --ignored` — all ASR tests pass (requires model)
