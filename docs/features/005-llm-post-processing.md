# Feature 005: LLM Post-Processing

**Status:** Not Started
**Dependencies:** 004-speech-recognition
**Design Reference:** Section 4.4 (LLM Post-Processor)
**Estimated Scope:** llama.cpp via llama-cpp-2 0.1, system prompt, command detection, inference

---

## Overview

Implement the LLM post-processing engine using llama.cpp (via the `llama-cpp-2` 0.1 crate from utilityai). The LLM takes raw transcripts from Whisper and produces polished text: removing filler words, fixing punctuation, applying course corrections, formatting numbers/dates/emails, adapting tone based on the active application, and detecting voice commands. It runs on GPU alongside Whisper (combined VRAM ~5.2 GB). Token output is streamed to the text injector as tokens are generated, reducing perceived latency.

---

## Requirements

### FR-001: Post-Processor Engine

```rust
// crates/vox_core/src/llm/processor.rs

use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::LlamaBackend;
use llama_cpp_2::token::AddBos;
use std::sync::Arc;

pub struct PostProcessor {
    /// LlamaModel is Send+Sync — can be shared via Arc
    model: Arc<LlamaModel>,
    backend: LlamaBackend,
}
```

**Critical crate note:** This is `llama-cpp-2` from utilityai. This is **NOT** `llama-cpp-rs` 0.4 — they are completely different crates with incompatible APIs. Key differences:
- Types are nested: `model::LlamaModel`, `model::params::LlamaModelParams`
- `load_from_file` takes `&LlamaBackend` as first argument
- `str_to_token` takes the `AddBos` enum

### FR-002: Model Loading

```rust
impl PostProcessor {
    /// Load the LLM model.
    /// Model: Qwen 2.5 3B Instruct, Q4_K_M quantization
    /// File: qwen2.5-3b-instruct-q4_k_m.gguf (~1.6 GB on disk, ~2.2 GB VRAM)
    pub fn new(model_path: &Path, use_gpu: bool) -> Result<Self> {
        let backend = LlamaBackend::init()?;
        let mut model_params = LlamaModelParams::default();
        if use_gpu {
            model_params.set_n_gpu_layers(-1); // All layers on GPU
        }
        // load_from_file needs &LlamaBackend as first argument
        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)?;
        Ok(Self {
            model: Arc::new(model),
            backend,
        })
    }
}
```

**Thread safety:**
- `LlamaModel` is `Send + Sync` — share via `Arc`
- `LlamaContext` is **NOT** `Send` or `Sync` — create one per inference call, do not reuse or share

### FR-003: Text Processing

```rust
impl PostProcessor {
    /// Process raw transcript text.
    /// Returns either polished text or a voice command.
    /// `active_app` is the name of the focused application (for tone adaptation).
    pub fn process(
        &self,
        raw_text: &str,
        dictionary_hints: &str,
        active_app: &str,
    ) -> Result<ProcessorOutput> {
        // LlamaContext is NOT Send/Sync — create one per call
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZero::new(2048));
        let mut ctx = self.model.new_context(&self.backend, ctx_params)?;

        let prompt = format!(
            "{system_prompt}\n{dictionary_hints}\n\
             Active application: {active_app}\n\
             Raw transcript: \"{raw_text}\"\nCleaned output:",
        );

        // str_to_token takes AddBos enum
        let tokens = self.model.str_to_token(&prompt, AddBos::Always)?;
        // ... run inference, collect output tokens ...

        let output = String::new(); // collected from token generation loop
        if output.trim_start().starts_with('{') {
            Ok(ProcessorOutput::Command(serde_json::from_str(&output)?))
        } else {
            Ok(ProcessorOutput::Text(output.trim().to_string()))
        }
    }
}
```

### FR-004: Output Types

```rust
pub enum ProcessorOutput {
    /// Polished text ready for injection
    Text(String),
    /// Voice command to execute
    Command(VoiceCommand),
}

#[derive(serde::Deserialize)]
pub struct VoiceCommand {
    pub cmd: String,
    pub args: Option<serde_json::Value>,
}
```

### FR-005: System Prompt

The system prompt defines the LLM's behavior. It lives in `crates/vox_core/src/llm/prompts.rs`:

```
You are a dictation post-processor. Your ONLY job is to clean up speech-to-text output.

Rules:
1. Remove filler words (um, uh, like, you know, basically, literally, so, I mean).
2. Fix punctuation and capitalization.
3. Apply course correction: if the speaker corrects themselves, keep only the correction.
   Example input: "send it to john at gmail dot com wait no john at outlook dot com"
   Output: "Send it to john@outlook.com"
4. Format numbers, dates, emails, and URLs naturally.
   "twenty five dollars" → "$25"
   "january third twenty twenty six" → "January 3, 2026"
   "h t t p s colon slash slash github dot com" → "https://github.com"
5. Adapt tone and formality based on the active application:
   - Email apps (Outlook, Gmail): formal tone, complete sentences
   - Chat apps (Slack, Discord, iMessage): casual tone, shorter sentences
   - Code editors (VS Code, terminal): preserve technical terms exactly
   - Default: neutral professional tone
6. Detect and execute voice commands. Return them as JSON commands, not text:
   "delete that" → {"cmd": "delete_last"}
   "new line" → {"cmd": "newline"}
   "new paragraph" → {"cmd": "paragraph"}
   "select all" → {"cmd": "select_all"}
   "undo that" → {"cmd": "undo"}
7. Preserve the speaker's voice and intent. Do NOT rephrase or summarize.
8. Output ONLY the cleaned text or a JSON command. No explanations.
```

### FR-006: Inference Parameters

| Parameter | Value | Rationale |
|---|---|---|
| Context window | 2048 tokens | Sufficient for dictation, keeps latency low |
| Temperature | 0.1 | Near-deterministic output |
| Top-p | 0.9 | Standard for focused generation |
| Max output tokens | 512 | Dictation outputs are short |
| Stop sequences | `\n`, `"` | Stop after the cleaned output |

### FR-007: Persistent KV Cache

To keep latency down, maintain a persistent KV cache session:
- The system prompt tokens are processed once and cached
- Subsequent calls only process the variable part (raw transcript + dictionary hints)
- This saves ~100-200ms per call by not re-encoding the system prompt

### FR-008: Wake Word Detection

When the user says "hey vox" (configurable command prefix) followed by a command, route to command execution instead of text injection:

- Wake word detection is a simple keyword search on the raw transcript
- Not a separate ML model — just string matching
- Example: "hey vox, delete the last sentence" → detect "hey vox" prefix → route full text to LLM with command-detection prompt

### FR-009: Command Catalog

Standard voice commands the LLM should detect:

| Spoken | Command JSON | Action |
|---|---|---|
| "delete that" | `{"cmd": "delete_last"}` | Delete last injected text |
| "undo that" | `{"cmd": "undo"}` | Undo |
| "new line" | `{"cmd": "newline"}` | Enter |
| "new paragraph" | `{"cmd": "paragraph"}` | Enter, Enter |
| "select all" | `{"cmd": "select_all"}` | Ctrl+A / Cmd+A |
| "copy that" | `{"cmd": "copy"}` | Ctrl+C / Cmd+C |
| "paste" | `{"cmd": "paste"}` | Ctrl+V / Cmd+V |
| "tab" | `{"cmd": "tab"}` | Tab |

### FR-010: Token Streaming Output

Stream tokens to the text injector as they are generated, rather than waiting for the full response. This reduces perceived latency — the user sees text appearing character by character.

```rust
impl PostProcessor {
    /// Process with streaming callback.
    /// `on_token` is called for each generated token.
    pub fn process_streaming(
        &self,
        raw_text: &str,
        dictionary_hints: &str,
        active_app: &str,
        on_token: impl FnMut(&str),
    ) -> Result<ProcessorOutput> {
        // ... same setup as process() ...
        // In the token generation loop, call on_token for each decoded token
        // Accumulate full output for command detection
        // If output starts with '{', it's a command (don't stream commands)
    }
}
```

The pipeline orchestrator uses the streaming variant for text output. For commands (JSON output), the full response is collected before execution.

### FR-011: Tone Adaptation

The LLM adjusts formality based on the active application name passed to it. The text injector reports the focused window title/process name, which is forwarded through the pipeline.

| Application Pattern | Tone |
|---|---|
| Outlook, Gmail, Thunderbird | Formal: complete sentences, proper grammar |
| Slack, Discord, iMessage, Teams | Casual: shorter sentences, relaxed punctuation |
| VS Code, Terminal, iTerm | Technical: preserve exact terms, no reformatting |
| Default (unknown app) | Neutral professional tone |

### FR-012: Clone for Async

```rust
impl Clone for PostProcessor {
    fn clone(&self) -> Self {
        Self {
            model: Arc::clone(&self.model),
            backend: self.backend.clone(),
        }
    }
}
```

Used for `tokio::task::spawn_blocking` GPU-bound inference.

---

## Acceptance Criteria

- [ ] Qwen model loads from disk with GPU acceleration
- [ ] Filler words are removed ("um let's um meet" → "Let's meet")
- [ ] Punctuation and capitalization are fixed
- [ ] Course correction works ("tuesday wait no wednesday" → "Wednesday")
- [ ] Numbers format correctly ("twenty five dollars" → "$25")
- [ ] Dates format correctly ("january third" → "January 3")
- [ ] Emails format correctly ("john at gmail dot com" → "john@gmail.com")
- [ ] Voice commands return JSON, not text
- [ ] Dictionary hints are respected in output
- [ ] Wake word "hey vox" triggers command routing
- [ ] Output preserves speaker's voice (no rephrasing)
- [ ] Token streaming outputs tokens as generated (not batch)
- [ ] Tone adapts to active application (formal in email, casual in chat)
- [ ] Active application name is accepted and used in prompt
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_prompt_construction` | Verify prompt format with various inputs |
| `test_output_parsing_text` | Parse polished text output |
| `test_output_parsing_command` | Parse JSON command output |
| `test_wake_word_detection` | "hey vox delete that" detected as command |

### Integration Tests (require model file, `#[ignore]`)

| Test | Description |
|---|---|
| `test_llm_filler_removal` | "um uh let's meet" → "Let's meet." |
| `test_llm_course_correction` | "tuesday no wednesday" → keeps only Wednesday |
| `test_llm_number_formatting` | "twenty five dollars" → "$25" |
| `test_llm_command_detection` | "delete that" → `{"cmd": "delete_last"}` |
| `test_llm_email_formatting` | "john at outlook dot com" → "john@outlook.com" |

---

## Performance Targets

| Metric | RTX 4090 | M4 Pro |
|---|---|---|
| Post-processing (15 tokens out) | < 100 ms | < 275 ms |
| Post-processing (30 tokens out) | < 200 ms | < 550 ms |
| VRAM usage | ~2.2 GB | ~2.2 GB (unified) |
| Model load time | < 5 s | < 5 s |
| Combined VRAM (Whisper + Qwen) | ~4.0 GB | ~4.0 GB |
