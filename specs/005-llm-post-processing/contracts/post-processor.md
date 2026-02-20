# Contract: PostProcessor Engine

**Feature**: 005-llm-post-processing
**Date**: 2026-02-19
**Module**: `crates/vox_core/src/llm/processor.rs`

## Public API

### PostProcessor

```rust
/// The LLM post-processing engine. Takes raw speech-to-text output and produces
/// polished text or structured voice commands.
///
/// Holds a loaded Qwen 2.5 3B Instruct model and pre-tokenized system prompt.
/// Cheaply cloneable via `Arc` for use in `tokio::task::spawn_blocking`.
/// Each `process()` call creates a fresh inference context to prevent state leakage.
pub struct PostProcessor {
    backend: Arc<LlamaBackend>,
    model: Arc<LlamaModel>,
    system_prompt_tokens: Arc<Vec<LlamaToken>>,
    chat_template: Arc<String>,
}
```

### Constructor

```rust
impl PostProcessor {
    /// Load the LLM model from disk with GPU acceleration.
    ///
    /// # Arguments
    /// * `model_path` — Path to the GGUF model file (qwen2.5-3b-instruct-q4_k_m.gguf)
    /// * `use_gpu` — If true, offload all layers to GPU
    ///
    /// # Errors
    /// Returns error if the backend fails to initialize, model file is missing/corrupt,
    /// or the chat template cannot be extracted from the model.
    pub fn new(model_path: &Path, use_gpu: bool) -> Result<Self>
}
```

**Behavior**:
1. Initialize `LlamaBackend` (suppress logs)
2. Create `LlamaModelParams` with GPU layers if `use_gpu`
3. Load model from file
4. Extract chat template from model metadata
5. Tokenize the system prompt (using chat template) and cache
6. Return PostProcessor with all fields wrapped in `Arc`

### Process (batch mode)

```rust
impl PostProcessor {
    /// Process a raw transcript and return polished text or a voice command.
    ///
    /// # Arguments
    /// * `raw_text` — Raw transcript from the ASR engine
    /// * `dictionary_hints` — Domain-specific terms (newline-separated) for accuracy improvement
    /// * `active_app` — Name of the focused application for tone adaptation
    ///
    /// # Returns
    /// `ProcessorOutput::Text` with polished text, or `ProcessorOutput::Command` with
    /// a structured voice command.
    ///
    /// # Errors
    /// Returns error if context creation, tokenization, or inference fails.
    pub fn process(
        &self,
        raw_text: &str,
        dictionary_hints: &str,
        active_app: &str,
    ) -> Result<ProcessorOutput>
}
```

**Behavior**:
1. If `raw_text` is empty, return `ProcessorOutput::Text(String::new())`
2. Check for wake word prefix ("hey vox") — if present, strip prefix and set command emphasis
3. Build user message with active_app, dictionary_hints, and raw_text
4. Format prompt via `apply_chat_template()` (system + user messages)
5. Tokenize, create fresh context, encode prompt, run inference loop
6. Parse output: JSON with `cmd` field → Command, otherwise → Text
7. Return `ProcessorOutput`

### Process with Streaming

```rust
impl PostProcessor {
    /// Process a raw transcript with streaming token output.
    ///
    /// Tokens are delivered via `on_token` as they are generated. For text output,
    /// tokens stream incrementally. For command output (JSON), the callback is NOT
    /// called — the full command is collected and returned.
    ///
    /// # Arguments
    /// * `raw_text` — Raw transcript from the ASR engine
    /// * `dictionary_hints` — Domain-specific terms for accuracy improvement
    /// * `active_app` — Name of the focused application for tone adaptation
    /// * `on_token` — Callback invoked with each generated text token
    ///
    /// # Returns
    /// `ProcessorOutput::Text` or `ProcessorOutput::Command`.
    pub fn process_streaming(
        &self,
        raw_text: &str,
        dictionary_hints: &str,
        active_app: &str,
        on_token: impl FnMut(&str),
    ) -> Result<ProcessorOutput>
}
```

**Behavior**:
1. Same setup as `process()`
2. During token generation loop:
   - First token: if it starts with `{`, switch to "command accumulation" mode (no streaming)
   - Otherwise: call `on_token(&piece)` for each decoded token
3. If command mode: collect all tokens, parse JSON at end, return Command
4. If text mode: all tokens already streamed via callback, return Text with full accumulated string

### Clone

```rust
impl Clone for PostProcessor {
    /// Clone the PostProcessor by sharing the underlying model and backend via Arc.
    /// No model reload occurs — all clones share the same loaded model.
    fn clone(&self) -> Self
}
```

## Output Types

### ProcessorOutput

```rust
/// The result of LLM post-processing.
pub enum ProcessorOutput {
    /// Polished text ready for injection into the target application.
    Text(String),
    /// A structured voice command to execute instead of injecting text.
    Command(VoiceCommand),
}
```

### VoiceCommand

```rust
/// A voice command detected by the LLM from the raw transcript.
#[derive(serde::Deserialize)]
pub struct VoiceCommand {
    /// Command identifier (e.g., "delete_last", "undo", "newline").
    pub cmd: String,
    /// Optional arguments for the command. Currently unused by the standard
    /// catalog but reserved for extensible commands.
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}
```

## Module Structure

```text
crates/vox_core/src/llm/
├── processor.rs    # PostProcessor struct, new(), process(), process_streaming()
└── prompts.rs      # SYSTEM_PROMPT constant, prompt construction helpers
```

Re-exports from `crates/vox_core/src/llm.rs`:
```rust
pub use processor::{PostProcessor, ProcessorOutput, VoiceCommand};
```

## Test Contract

### Unit Tests (no model required)

| Test | Input | Expected Output |
|------|-------|-----------------|
| `test_output_parsing_text` | Simulate LLM returning "Hello world." | `ProcessorOutput::Text("Hello world.")` |
| `test_output_parsing_command` | Simulate LLM returning `{"cmd":"delete_last"}` | `ProcessorOutput::Command { cmd: "delete_last", args: None }` |
| `test_output_parsing_invalid_json` | Simulate LLM returning `{malformed` | `ProcessorOutput::Text("{malformed")` |
| `test_wake_word_detection` | Input "hey vox delete that" | Wake word detected, prefix stripped |
| `test_wake_word_case_insensitive` | Input "Hey Vox delete that" | Wake word detected |
| `test_wake_word_not_in_middle` | Input "I said hey vox" | Wake word NOT detected (not at start) |
| `test_empty_input` | Input "" | `ProcessorOutput::Text("")` |
| `test_prompt_construction` | Various active_app and dict_hints | Verify prompt contains expected sections |

### Integration Tests (require model file, `#[ignore]`)

| Test | Input | Expected Output Contains |
|------|-------|--------------------------|
| `test_llm_model_loads` | Load model from fixtures | Ok (no panic) |
| `test_llm_filler_removal` | "um uh let's meet" | "Let's meet" (no fillers) |
| `test_llm_course_correction` | "tuesday no wait wednesday" | "Wednesday" (correction only) |
| `test_llm_number_formatting` | "twenty five dollars" | "$25" |
| `test_llm_email_formatting` | "john at outlook dot com" | "john@outlook.com" |
| `test_llm_command_detection` | "delete that" | `ProcessorOutput::Command { cmd: "delete_last" }` |
| `test_llm_streaming` | "hello world" with callback | Callback invoked with tokens |
| `test_llm_command_not_streamed` | "delete that" with callback | Callback NOT invoked, full command returned |
| `test_llm_empty_input` | "" | `ProcessorOutput::Text("")` |
| `test_llm_model_load_error` | Nonexistent path | Err with descriptive message (NOT #[ignore]) |

## Inference Parameters

| Parameter | Value | Implementation |
|-----------|-------|----------------|
| Context window | 2048 | `LlamaContextParams::default().with_n_ctx(Some(NonZeroU32::new(2048).unwrap()))` |
| Temperature | 0.1 | `LlamaSampler::temp(0.1)` |
| Top-p | 0.9 | `LlamaSampler::top_p(0.9, 1)` |
| Max output tokens | 512 | Counter in generation loop |
| Stop condition | EOG tokens + `\n` | `model.is_eog_token(token)` + manual newline check on decoded text |

## Error Conditions

| Condition | Behavior |
|-----------|----------|
| Model file missing | `PostProcessor::new()` returns `Err` with path in message |
| Model file corrupt | `PostProcessor::new()` returns `Err` from llama.cpp |
| Backend already initialized | `PostProcessor::new()` returns `Err(BackendAlreadyInitialized)` |
| Empty raw_text | Returns `ProcessorOutput::Text("")` immediately (no inference) |
| Context creation fails | `process()` returns `Err` |
| Tokenization fails | `process()` returns `Err` |
| LLM output is invalid JSON starting with `{` | Returns `ProcessorOutput::Text(raw_output)` |
| LLM output exceeds 512 tokens | Generation stops at 512, returns accumulated output |
