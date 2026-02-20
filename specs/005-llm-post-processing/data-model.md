# Data Model: LLM Post-Processing

**Feature**: 005-llm-post-processing
**Date**: 2026-02-19

## Entities

### PostProcessor

The LLM post-processing engine. Holds a loaded model and cached system prompt tokens. Cheaply cloneable via `Arc` for use in background tasks.

| Field | Type | Description |
|-------|------|-------------|
| `backend` | `Arc<LlamaBackend>` | Shared llama.cpp backend instance. Can only be initialized once — must share via Arc, not duplicate. |
| `model` | `Arc<LlamaModel>` | Thread-safe handle to the loaded Qwen 2.5 3B Instruct model. Send+Sync. |
| `system_prompt_tokens` | `Arc<Vec<LlamaToken>>` | Pre-tokenized system prompt for reuse across calls. Avoids re-tokenizing the ~300-token instruction set each call. |
| `chat_template` | `Arc<String>` | Model's embedded ChatML template, extracted once at load time. |

**Lifecycle**: Created once at startup via `PostProcessor::new()`. Lives for the application lifetime. Cloned (via Arc) into `tokio::task::spawn_blocking` for GPU-bound inference.

**Relationships**: Receives `String` raw transcripts from `AsrEngine` (Feature 004). Outputs `ProcessorOutput` to the text injection stage (Feature 006).

### ProcessorOutput

The result of post-processing a single raw transcript. Either polished text ready for injection or a structured voice command.

```text
ProcessorOutput
├── Text(String)           — Polished text ready for injection
└── Command(VoiceCommand)  — Structured command for execution
```

**Determination**: If the LLM's output (trimmed) starts with `{` and parses as valid JSON with a `cmd` field, it's a `Command`. Otherwise it's `Text`. Invalid JSON starting with `{` falls through to `Text`.

### VoiceCommand

A structured command parsed from the LLM's JSON output.

| Field | Type | Description |
|-------|------|-------------|
| `cmd` | `String` | Command identifier from the standard catalog (e.g., `delete_last`, `undo`, `newline`). |
| `args` | `Option<Value>` | Optional arguments for extensible commands. Currently unused by the standard catalog but reserved for wake-word-prefixed freeform commands. |

**Standard Command Catalog** (compile-time fixed for v1.0):

| Command | Spoken Phrase | Action |
|---------|---------------|--------|
| `delete_last` | "delete that" | Delete last injected text |
| `undo` | "undo that" | Undo |
| `newline` | "new line" | Line break |
| `paragraph` | "new paragraph" | Double line break |
| `select_all` | "select all" | Select all text |
| `copy` | "copy that" | Copy selection |
| `paste` | "paste" | Paste clipboard |
| `tab` | "tab" | Tab character |

### Inference Parameters (internal, not a public entity)

Configuration applied to every inference call. Not exposed publicly — built internally in `process()`.

| Parameter | Value | Purpose |
|-----------|-------|---------|
| `n_ctx` | 2048 | Context window size |
| `temperature` | 0.1 | Near-deterministic output |
| `top_p` | 0.9 | Nucleus sampling threshold |
| `max_tokens` | 512 | Maximum output length per call |
| `stop` | EOG tokens + `\n` | Stop at end-of-generation or newline |

### System Prompt (internal constant)

The instruction set defining post-processing behavior. Lives in `crates/vox_core/src/llm/prompts.rs` as a `&str` constant. Tokenized once during `PostProcessor::new()` and cached for reuse.

**Rules encoded**:
1. Filler word removal
2. Punctuation and capitalization
3. Course correction (keep only corrections)
4. Number/date/email/URL formatting
5. Tone adaptation by active application
6. Voice command detection (JSON output)
7. Voice and intent preservation
8. Output-only constraint (no explanations)

## Data Flow

```text
AsrEngine.transcribe() ─── String (raw transcript) ──→ PostProcessor.process()
                                                          │
                                                          ├─ Empty text → Ok(Text(""))
                                                          ├─ Wake word prefix → command-emphasis prompt
                                                          └─ Normal text → standard prompt
                                                          │
                                                    ┌─────┘
                                                    ▼
                                              LLM Inference
                                              (Qwen 2.5 3B)
                                                    │
                                                    ├─ Output starts with '{' + valid JSON
                                                    │   → ProcessorOutput::Command(VoiceCommand)
                                                    │
                                                    └─ Otherwise
                                                        → ProcessorOutput::Text(polished)
                                                        │
                                                        ▼
                                              Text Injection (Feature 006)
```

## State Transitions

N/A — the PostProcessor is stateless between calls. Each call creates a fresh `LlamaContext` with its own KV cache and discards it after inference completes. No state leaks between transcriptions (FR-014).
