# Research: LLM Post-Processing

**Feature**: 005-llm-post-processing
**Date**: 2026-02-19

## R-001: llama-cpp-2 Crate API (v0.1, utilityai)

**Decision**: Use `llama-cpp-2` 0.1 from utilityai (NOT `llama-cpp-rs` 0.4 — completely different crate).
**Rationale**: Already declared in `Cargo.toml` with `cuda`/`metal` feature gates. Provides safe Rust bindings to llama.cpp with full inference support.
**Alternatives Considered**: `llama-cpp-rs` 0.4 (different API, different maintainer), raw llama.cpp FFI (unsafe, no ergonomics).

### Key API Details

**Backend**:
- `LlamaBackend::init() -> Result<LlamaBackend>` — can only be called **once** (second call returns `Err(BackendAlreadyInitialized)`)
- `LlamaBackend` is `Send + Sync` (empty struct, proof of initialization)
- `Drop` calls `llama_backend_free()` — must share via `Arc`, not clone
- `backend.void_logs()` suppresses stderr logging from llama.cpp

**Model Loading**:
- `LlamaModel::load_from_file(&backend, path, &params) -> Result<Self, LlamaModelLoadError>`
- `LlamaModelParams::default()` then builder: `.with_n_gpu_layers(u32)` (default is all layers on GPU)
- `LlamaModel` is `Send + Sync` — share via `Arc`

**Context Creation**:
- `model.new_context(&backend, params) -> Result<LlamaContext<'a>, LlamaContextLoadError>`
- `LlamaContextParams::default().with_n_ctx(Some(NonZeroU32::new(2048).unwrap()))`
- `LlamaContext` is **NOT** `Send` or `Sync` — create fresh per inference call, never reuse across threads

**Tokenization**:
- `model.str_to_token(&str, AddBos::Always) -> Result<Vec<LlamaToken>>` — `AddBos` enum: `Always` or `Never`
- `model.token_to_piece(token, &mut decoder, special, lstrip) -> Result<String>` — needs `encoding_rs::UTF_8.new_decoder()`
- Special tokens: `model.token_bos()`, `model.token_eos()`, `model.token_nl()`, `model.is_eog_token(token)`

**Batch Processing**:
- `LlamaBatch::new(n_tokens, n_seq_max)` — NOT `Send`/`Sync`
- `batch.add(token, pos, &[seq_id], logits)` — `logits=true` only for last prompt token
- `batch.add_sequence(&tokens, seq_id, logits_all)` — convenience for adding multiple tokens
- `ctx.decode(&mut batch)` — processes the batch, populates KV cache and logits

**Sampling**:
- `LlamaSampler::chain_simple([LlamaSampler::temp(0.1), LlamaSampler::top_p(0.9, 1), LlamaSampler::dist(seed)])`
- `sampler.sample(&ctx, batch.n_tokens() - 1) -> LlamaToken`
- `sampler.accept(token)` — must call after each sampled token

**Chat Template** (built-in):
- `model.chat_template(None) -> Result<String>` — get model's embedded chat template
- `LlamaChatMessage::new(role, content) -> Result<Self>`
- `model.apply_chat_template(&template, &messages, add_ass) -> Result<String>` — `add_ass=true` appends assistant prefix

### Import Paths

```rust
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::AddBos;
use llama_cpp_2::model::LlamaChatMessage;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
```

---

## R-002: Qwen 2.5 Chat Template (ChatML)

**Decision**: Use the model's built-in ChatML template via `model.apply_chat_template()`.
**Rationale**: Qwen 2.5 Instruct is trained with ChatML format. Using the built-in template ensures correct formatting without manually constructing special token sequences.
**Alternatives Considered**: Manual prompt construction (error-prone, requires hardcoding special token IDs), raw completion mode (model won't follow instructions well).

### ChatML Format

```text
<|im_start|>system
{system message}<|im_end|>
<|im_start|>user
{user message}<|im_end|>
<|im_start|>assistant
{assistant generates here}
```

### Special Tokens

| Token | Token ID | Purpose |
|-------|----------|---------|
| `<\|im_start\|>` | 151644 | Turn start |
| `<\|im_end\|>` | 151645 | Turn end (primary EOS) |
| `<\|endoftext\|>` | 151643 | Document end (secondary EOS) |

### Stop Condition

Use `model.is_eog_token(token)` — catches both `<|im_end|>` (151645) and `<|endoftext|>` (151643). No manual stop sequence checking needed for turn boundary detection.

Additional stop on `"\n"` within the assistant response may be useful to prevent multi-line output for dictation — implement as manual check on decoded token text.

---

## R-003: System Prompt Token Caching Strategy

**Decision**: Pre-tokenize the system prompt and store as `Vec<LlamaToken>` in the PostProcessor. Each inference call creates a fresh context, encodes system tokens first, then appends user tokens.
**Rationale**: `LlamaContext` is not `Send`/`Sync`, so we can't share a context with a pre-filled KV cache across threads. Pre-tokenizing the system prompt avoids re-running the tokenizer on every call (saves ~1-2ms). The KV cache re-computation for ~300 system tokens takes ~10-20ms on RTX 4090, well within the 100ms budget.
**Alternatives Considered**:
- Session save/load (`save_session_file`/`load_session_file`): Adds file I/O overhead, temp file management complexity. Viable upgrade path if profiling shows need.
- Dedicated inference thread with long-lived context: More complex architecture, unnecessary since dictation is sequential.

### Upgrade Path

If per-call system prompt encoding proves too slow on mid-range GPUs:
1. Use `ctx.save_session_file()` after first call to persist the system prompt KV cache
2. On subsequent calls, `ctx.load_session_file()` to restore KV state instead of re-encoding
3. PostProcessor gains a `session_cache_path: Option<PathBuf>` field

---

## R-004: PostProcessor Thread Safety & Cloneability

**Decision**: `PostProcessor` stores `model: Arc<LlamaModel>` and `backend: Arc<LlamaBackend>`. Manual `Clone` impl via `Arc::clone` on both fields.
**Rationale**: `LlamaBackend::init()` can only succeed once — second call returns error. The backend must be shared, not duplicated. Both `LlamaModel` and `LlamaBackend` are `Send + Sync`, so `Arc` works correctly.
**Alternatives Considered**: Storing `LlamaBackend` directly and implementing Clone (would cause double-free on drop). Using `OnceCell<LlamaBackend>` (global state, harder to test).

### Architecture

```text
PostProcessor (Clone via Arc)
├── backend: Arc<LlamaBackend>      (Send+Sync, shared)
├── model: Arc<LlamaModel>          (Send+Sync, shared)
├── system_prompt_tokens: Arc<Vec<LlamaToken>>  (cached, shared)
└── chat_template: Arc<String>       (cached, shared)

process() call:
├── Create fresh LlamaContext<'a>    (NOT Send/Sync, per-call)
├── Create LlamaBatch                (NOT Send/Sync, per-call)
├── Create LlamaSampler              (per-call)
├── Encode system+user tokens
├── Generate tokens auto-regressively
└── Return ProcessorOutput
```

---

## R-005: Output Parsing Strategy

**Decision**: Use the LLM's ChatML response directly. If the trimmed output starts with `{`, attempt JSON parse as `VoiceCommand`. Otherwise treat as polished text.
**Rationale**: The system prompt instructs the LLM to output either clean text or a JSON command. Parsing by first-character detection is simple and reliable for this binary choice.
**Alternatives Considered**: Grammar-constrained output via `LlamaSampler::grammar()` (too restrictive for free-text output), separate command detection pass (doubles latency).

### Fallback

If JSON parsing fails on output starting with `{`, treat the entire output as text (edge case from spec: "LLM generates output that is neither clean text nor valid JSON → system treats it as text output").

---

## R-006: New Dependency — encoding_rs

**Decision**: Add `encoding_rs = "0.8"` to `vox_core` dependencies.
**Rationale**: Required by `LlamaModel::token_to_piece()` for incremental UTF-8 decoding of generated tokens. The `encoding_rs` crate is a well-maintained, widely-used encoding library (used by Firefox/Servo).
**Alternatives Considered**: `token_to_piece_bytes()` with manual UTF-8 handling (more error-prone, no incremental decoding support).

---

## R-007: Wake Word Detection

**Decision**: Simple case-insensitive string prefix check on the raw transcript before sending to LLM.
**Rationale**: Spec states wake word detection is string matching, not a separate ML model. Check `raw_text.to_lowercase().starts_with("hey vox")` before constructing the prompt.
**Alternatives Considered**: Regex-based detection (overkill for a fixed prefix), phonetic matching (complexity not justified for v1.0).

### Flow

```text
raw_text arrives
  ├── starts with "hey vox" → strip prefix, include command detection emphasis in prompt
  └── does not start with "hey vox" → normal post-processing prompt
```

---

## R-008: Prompt Construction

**Decision**: Use `model.apply_chat_template()` with `LlamaChatMessage` to construct prompts. System message contains the post-processing rules. User message contains the raw transcript with metadata (active app, dictionary hints).
**Rationale**: Built-in template handling ensures correct ChatML formatting for Qwen 2.5. Avoids manual special token construction.

### Message Structure

**System message** (constant, cached as tokens):
```
You are a dictation post-processor. Your ONLY job is to clean up speech-to-text output.
[... 8 rules from spec ...]
```

**User message** (variable, per-call):
```
Active application: {active_app}
Dictionary: {dictionary_hints}
Raw transcript: "{raw_text}"
```

**Tokenization**:
- System message: tokenized once at PostProcessor construction, stored as `Vec<LlamaToken>`
- Full prompt (system + user + assistant prefix): tokenized per-call via `apply_chat_template()` then `str_to_token()`
- `AddBos::Never` for the formatted chat template (template handles BOS internally)

---

## R-009: Stop Sequence Decision — Dropping `"`

**Decision**: Use EOG tokens (`model.is_eog_token()`) and manual `\n` check as stop conditions. Do NOT use `"` as a stop sequence.
**Rationale**: The original feature description included `"` as a stop sequence. This was dropped because dictation output legitimately contains quotation marks (e.g., "She said 'hello'"). Stopping on `"` would truncate output mid-sentence. The `\n` stop is sufficient for single-line dictation output, and EOG tokens handle the model's natural end-of-turn signal.
**Alternatives Considered**: Keeping `"` stop (too aggressive — truncates quoted speech). No `\n` stop (allows multi-line output that could include LLM explanations violating rule 8).
