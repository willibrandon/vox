# Quickstart: LLM Post-Processing (Feature 005)

**Branch**: `005-llm-post-processing`
**Prerequisite**: Feature 004 (speech recognition) merged into main

## Build

```bash
# Windows (CUDA) — LLM uses GPU via llama.cpp CUDA backend
cargo build -p vox_core --features cuda

# macOS (Metal) — LLM uses GPU via llama.cpp Metal backend
cargo build -p vox_core --features metal
```

Zero warnings required.

## Dependencies

The following dependencies are already in `crates/vox_core/Cargo.toml`:

| Crate | Version | Feature | Purpose |
|-------|---------|---------|---------|
| `llama-cpp-2` | `0.1` | `cuda`, `metal` (feature-gated) | Qwen LLM inference via llama.cpp FFI bindings |
| `serde` | workspace | `derive` | Deserialize VoiceCommand JSON |
| `serde_json` | workspace | — | Parse LLM JSON command output |

New dependency to add:

| Crate | Version | Purpose |
|-------|---------|---------|
| `encoding_rs` | `0.8` | Incremental UTF-8 decoding for `LlamaModel::token_to_piece()` |

`llama-cpp-2` transitively builds `llama-cpp-sys-2` which compiles llama.cpp from source via CMake during `cargo build`.

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

The Qwen 2.5 3B Instruct model (Q4_K_M quantization) must be downloaded separately:

```bash
# Create fixtures directory (if not exists)
mkdir -p crates/vox_core/tests/fixtures

# Download model (~1.6 GB)
curl -L -o crates/vox_core/tests/fixtures/qwen2.5-3b-instruct-q4_k_m.gguf \
  https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf
```

The model file is gitignored (~1.6 GB, *.gguf pattern). Tests requiring the model are marked `#[ignore]`.

## Test

```bash
# All LLM tests (requires model file)
cargo test -p vox_core --features cuda -- llm --ignored

# All vox_core tests (VAD + ASR + LLM non-ignored)
cargo test -p vox_core --features cuda

# Single test with output
cargo test -p vox_core --features cuda test_llm_filler_removal -- --nocapture --ignored
```

### Unit Tests (no model required)

| Test | What it validates |
|------|-------------------|
| `test_output_parsing_text` | Polished text output parsed correctly |
| `test_output_parsing_command` | JSON command output parsed as VoiceCommand |
| `test_output_parsing_invalid_json` | Malformed JSON treated as text, not error |
| `test_wake_word_detection` | "hey vox" prefix detected at start of transcript |
| `test_wake_word_case_insensitive` | Wake word detection is case-insensitive |
| `test_wake_word_not_in_middle` | Wake word only triggers when at start |
| `test_empty_input` | Empty transcript returns empty text |
| `test_prompt_construction` | Prompt includes active_app, dict_hints, raw_text |

### Integration Tests (require model file, `#[ignore]`)

| Test | What it validates |
|------|-------------------|
| `test_llm_model_loads` | Qwen model loads from disk with GPU enabled |
| `test_llm_filler_removal` | "um uh let's meet" → cleaned text without fillers |
| `test_llm_course_correction` | "tuesday no wait wednesday" → keeps only correction |
| `test_llm_number_formatting` | "twenty five dollars" → "$25" |
| `test_llm_email_formatting` | "john at outlook dot com" → "john@outlook.com" |
| `test_llm_command_detection` | "delete that" → `ProcessorOutput::Command { cmd: "delete_last" }` |
| `test_llm_streaming` | Streaming callback invoked with tokens for text output |
| `test_llm_command_not_streamed` | Command output NOT streamed via callback |
| `test_llm_empty_input` | Empty transcript with model loaded returns empty text |

### Error Tests (no model required)

| Test | What it validates |
|------|-------------------|
| `test_llm_model_load_error` | Nonexistent model path returns descriptive error |

## Key Files

| File | Purpose |
|------|---------|
| `crates/vox_core/src/llm.rs` | Module root: re-exports PostProcessor, ProcessorOutput, VoiceCommand |
| `crates/vox_core/src/llm/processor.rs` | PostProcessor: model loading, inference, streaming |
| `crates/vox_core/src/llm/prompts.rs` | SYSTEM_PROMPT constant, prompt construction helpers |
| `crates/vox_core/Cargo.toml` | llama-cpp-2 + encoding_rs dependencies |

## Verification

After implementation, verify:

1. `cargo build -p vox_core --features cuda` — zero warnings
2. `cargo test -p vox_core --features cuda` — all non-ignored tests pass (unit + error)
3. `cargo test -p vox_core --features cuda -- llm --ignored` — all LLM integration tests pass (requires model)
4. Observe inference timing via `--nocapture` to confirm post-processing completes well under 100ms for short transcripts (manual proxy for SC-001 — formal benchmarks deferred to pipeline integration)
