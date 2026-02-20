//! PostProcessor engine for LLM-based transcript post-processing.
//!
//! Loads the Qwen 2.5 3B Instruct model via llama.cpp and processes raw ASR
//! transcripts into polished text or structured voice commands. Each inference
//! call creates a fresh context to prevent state leakage between transcriptions.

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use encoding_rs::UTF_8;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use parking_lot::Mutex;

use super::prompts::{
    build_user_message, build_user_message_with_command_emphasis, detect_wake_word, SYSTEM_PROMPT,
};

/// Maximum number of tokens the LLM can generate per call.
const MAX_OUTPUT_TOKENS: usize = 512;

/// Context window size for each inference call.
const CONTEXT_WINDOW: u32 = 2048;

/// Global llama.cpp backend singleton. `LlamaBackend::init()` can only succeed
/// once per process — this ensures it's initialized exactly once and shared.
/// Uses `Mutex<Option<...>>` because `OnceLock::get_or_try_init` is unstable.
static LLAMA_BACKEND: Mutex<Option<Arc<LlamaBackend>>> = Mutex::new(None);

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
    chat_template: Arc<LlamaChatTemplate>,
}

impl Clone for PostProcessor {
    /// Clone the PostProcessor by sharing the underlying model and backend via Arc.
    /// No model reload occurs — all clones share the same loaded model.
    fn clone(&self) -> Self {
        Self {
            backend: Arc::clone(&self.backend),
            model: Arc::clone(&self.model),
            system_prompt_tokens: Arc::clone(&self.system_prompt_tokens),
            chat_template: Arc::clone(&self.chat_template),
        }
    }
}

/// The result of LLM post-processing.
pub enum ProcessorOutput {
    /// Polished text ready for injection into the target application.
    Text(String),
    /// A structured voice command to execute instead of injecting text.
    Command(VoiceCommand),
}

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

impl PostProcessor {
    /// Load the LLM model from disk with GPU acceleration.
    ///
    /// Initializes the llama.cpp backend, loads the GGUF model file, extracts
    /// the chat template, and pre-tokenizes the system prompt for reuse across calls.
    ///
    /// # Errors
    /// Returns error if the backend fails to initialize, model file is missing/corrupt,
    /// or the chat template cannot be extracted from the model.
    pub fn new(model_path: &Path, use_gpu: bool) -> Result<Self> {
        let backend = {
            let mut guard = LLAMA_BACKEND.lock();
            if let Some(ref b) = *guard {
                b.clone()
            } else {
                let mut b = LlamaBackend::init()
                    .context("failed to initialize llama.cpp backend")?;
                b.void_logs();
                let arc = Arc::new(b);
                *guard = Some(arc.clone());
                arc
            }
        };

        let model_params = if use_gpu {
            LlamaModelParams::default().with_n_gpu_layers(u32::MAX)
        } else {
            LlamaModelParams::default()
        };

        if !model_path.exists() {
            anyhow::bail!("failed to load model from {}: file not found", model_path.display());
        }

        let model = LlamaModel::load_from_file(&backend, model_path, &model_params)
            .map_err(|e| anyhow::anyhow!("failed to load model from {}: {e}", model_path.display()))?;

        let chat_template = model
            .chat_template(None)
            .context("failed to extract chat template from model")?;

        // Pre-tokenize the system prompt by formatting it through the chat template
        // with just the system message, then tokenizing the result.
        let system_msg = LlamaChatMessage::new("system".into(), SYSTEM_PROMPT.into())
            .context("failed to create system chat message")?;
        let system_prompt_text = model
            .apply_chat_template(&chat_template, &[system_msg], false)
            .context("failed to apply chat template for system prompt")?;
        let system_prompt_tokens = model
            .str_to_token(&system_prompt_text, AddBos::Never)
            .context("failed to tokenize system prompt")?;

        Ok(Self {
            backend,
            model: Arc::new(model),
            system_prompt_tokens: Arc::new(system_prompt_tokens),
            chat_template: Arc::new(chat_template),
        })
    }

    /// Run inference on the given prompt tokens and return the generated text.
    ///
    /// Creates a fresh LlamaContext, encodes the prompt, and samples tokens
    /// auto-regressively until an EOG token, newline, or max token limit.
    fn run_inference(&self, prompt_tokens: &[LlamaToken]) -> Result<String> {
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(CONTEXT_WINDOW).expect("non-zero")));
        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| anyhow::anyhow!("failed to create inference context: {e}"))?;

        let mut batch = LlamaBatch::new(CONTEXT_WINDOW as usize, 1);

        // Add prompt tokens to batch — logits=true only for last token
        let last_idx = prompt_tokens.len().saturating_sub(1);
        for (i, &token) in prompt_tokens.iter().enumerate() {
            batch
                .add(token, i as i32, &[0], i == last_idx)
                .context("failed to add token to batch")?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| anyhow::anyhow!("failed to decode prompt: {e}"))?;

        let mut output = String::new();
        let mut decoder = UTF_8.new_decoder();
        let seed = 42u32;
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(0.1),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::dist(seed),
        ]);

        let mut n_cur = prompt_tokens.len() as i32;

        for _ in 0..MAX_OUTPUT_TOKENS {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if self.model.is_eog_token(token) {
                break;
            }

            let piece = self
                .model
                .token_to_piece(token, &mut decoder, true, None)
                .map_err(|e| anyhow::anyhow!("failed to decode token: {e}"))?;

            // Stop on newline to prevent multi-line output
            if piece.contains('\n') {
                break;
            }

            output.push_str(&piece);

            // Prepare next batch with this single token
            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .context("failed to add generated token to batch")?;

            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("failed to decode generated token: {e}"))?;

            n_cur += 1;
        }

        Ok(output)
    }

    /// Build prompt tokens for the given raw transcript, including wake word handling.
    fn build_prompt_tokens(
        &self,
        raw_text: &str,
        dictionary_hints: &str,
        active_app: &str,
    ) -> Result<Vec<LlamaToken>> {
        let user_msg_text = match detect_wake_word(raw_text) {
            Some(remaining) => {
                build_user_message_with_command_emphasis(active_app, dictionary_hints, remaining)
            }
            None => build_user_message(active_app, dictionary_hints, raw_text),
        };

        let system_msg = LlamaChatMessage::new("system".into(), SYSTEM_PROMPT.into())
            .context("failed to create system chat message")?;
        let user_msg = LlamaChatMessage::new("user".into(), user_msg_text)
            .context("failed to create user chat message")?;

        let prompt_text = self
            .model
            .apply_chat_template(&self.chat_template, &[system_msg, user_msg], true)
            .context("failed to apply chat template")?;

        self.model
            .str_to_token(&prompt_text, AddBos::Never)
            .context("failed to tokenize prompt")
    }

    /// Process a raw transcript and return polished text or a voice command.
    ///
    /// Builds a user message with the active application context and dictionary hints,
    /// formats a full prompt via the model's chat template, runs inference, and parses
    /// the output. Returns empty text immediately if the input is empty.
    pub fn process(
        &self,
        raw_text: &str,
        dictionary_hints: &str,
        active_app: &str,
    ) -> Result<ProcessorOutput> {
        if raw_text.is_empty() {
            return Ok(ProcessorOutput::Text(String::new()));
        }

        let prompt_tokens = self.build_prompt_tokens(raw_text, dictionary_hints, active_app)?;
        let output = self.run_inference(&prompt_tokens)?;
        Ok(parse_output(&output))
    }

    /// Process a raw transcript with streaming token output.
    ///
    /// Tokens are delivered via `on_token` as they are generated. For text output,
    /// tokens stream incrementally. For command output (JSON), the callback is NOT
    /// called — the full command is collected and returned.
    pub fn process_streaming(
        &self,
        raw_text: &str,
        dictionary_hints: &str,
        active_app: &str,
        mut on_token: impl FnMut(&str),
    ) -> Result<ProcessorOutput> {
        if raw_text.is_empty() {
            return Ok(ProcessorOutput::Text(String::new()));
        }

        let prompt_tokens = self.build_prompt_tokens(raw_text, dictionary_hints, active_app)?;

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(CONTEXT_WINDOW).expect("non-zero")));
        let mut ctx = self
            .model
            .new_context(&self.backend, ctx_params)
            .map_err(|e| anyhow::anyhow!("failed to create inference context: {e}"))?;

        let mut batch = LlamaBatch::new(CONTEXT_WINDOW as usize, 1);

        let last_idx = prompt_tokens.len().saturating_sub(1);
        for (i, &token) in prompt_tokens.iter().enumerate() {
            batch
                .add(token, i as i32, &[0], i == last_idx)
                .context("failed to add token to batch")?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| anyhow::anyhow!("failed to decode prompt: {e}"))?;

        let mut output = String::new();
        let mut decoder = UTF_8.new_decoder();
        let seed = 42u32;
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(0.1),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::dist(seed),
        ]);

        let mut n_cur = prompt_tokens.len() as i32;
        // Tracks whether output is a command (JSON) or text. `None` means we haven't
        // seen non-whitespace yet. Buffered pieces are held until the decision is made.
        let mut command_mode: Option<bool> = None;
        let mut buffered_pieces: Vec<String> = Vec::new();

        for _ in 0..MAX_OUTPUT_TOKENS {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if self.model.is_eog_token(token) {
                break;
            }

            let piece = self
                .model
                .token_to_piece(token, &mut decoder, true, None)
                .map_err(|e| anyhow::anyhow!("failed to decode token: {e}"))?;

            if piece.contains('\n') {
                break;
            }

            output.push_str(&piece);

            match command_mode {
                None => {
                    // Haven't decided yet — check if accumulated output has non-whitespace
                    buffered_pieces.push(piece);
                    let trimmed = output.trim_start();
                    if !trimmed.is_empty() {
                        let is_command = trimmed.starts_with('{');
                        command_mode = Some(is_command);
                        if !is_command {
                            // Flush all buffered pieces to the callback
                            for buffered in buffered_pieces.drain(..) {
                                on_token(&buffered);
                            }
                        }
                    }
                }
                Some(false) => {
                    on_token(&piece);
                }
                Some(true) => {
                    // Command mode — accumulate without streaming
                }
            }

            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .context("failed to add generated token to batch")?;

            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("failed to decode generated token: {e}"))?;

            n_cur += 1;
        }

        Ok(parse_output(&output))
    }
}

/// Parse the raw LLM output into a ProcessorOutput.
///
/// If the trimmed output starts with `{` and parses as valid JSON with a `cmd`
/// field, returns `Command`. Otherwise returns `Text` with the trimmed output.
fn parse_output(raw: &str) -> ProcessorOutput {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        match serde_json::from_str::<VoiceCommand>(trimmed) {
            Ok(command) => ProcessorOutput::Command(command),
            Err(_) => ProcessorOutput::Text(trimmed.to_string()),
        }
    } else {
        ProcessorOutput::Text(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::prompts::{build_user_message, detect_wake_word};
    use std::path::Path;

    // --- T007: Output parsing unit tests ---

    #[test]
    fn test_output_parsing_text() {
        let output = parse_output("Hello world.");
        match output {
            ProcessorOutput::Text(text) => assert_eq!(text, "Hello world."),
            ProcessorOutput::Command(_) => panic!("expected Text, got Command"),
        }
    }

    #[test]
    fn test_output_parsing_invalid_json() {
        let output = parse_output("{malformed json here");
        match output {
            ProcessorOutput::Text(text) => assert_eq!(text, "{malformed json here"),
            ProcessorOutput::Command(_) => panic!("expected Text for invalid JSON, got Command"),
        }
    }

    #[test]
    fn test_empty_input() {
        let output = parse_output("");
        match output {
            ProcessorOutput::Text(text) => assert_eq!(text, ""),
            ProcessorOutput::Command(_) => panic!("expected Text for empty input, got Command"),
        }
    }

    // --- T008: Prompt construction test ---

    #[test]
    fn test_prompt_construction() {
        let msg = build_user_message("Outlook", "Kubernetes\nPrometheus", "hello world");
        assert!(msg.contains("Active application: Outlook"));
        assert!(msg.contains("Dictionary: Kubernetes\nPrometheus"));
        assert!(msg.contains("Raw transcript: \"hello world\""));
    }

    // --- T010: Error test ---

    #[test]
    fn test_llm_model_load_error() {
        let result = PostProcessor::new(Path::new("/nonexistent/path/model.gguf"), false);
        match result {
            Ok(_) => panic!("expected error for nonexistent model path"),
            Err(e) => {
                let err_msg = e.to_string();
                assert!(
                    err_msg.contains("failed to load model")
                        || err_msg.contains("failed to initialize"),
                    "error should be descriptive, got: {err_msg}"
                );
            }
        }
    }

    // --- T012: Command parsing + wake word unit tests ---

    #[test]
    fn test_output_parsing_command() {
        let output = parse_output(r#"{"cmd":"delete_last"}"#);
        match output {
            ProcessorOutput::Command(cmd) => {
                assert_eq!(cmd.cmd, "delete_last");
                assert!(cmd.args.is_none());
            }
            ProcessorOutput::Text(t) => panic!("expected Command, got Text: {t}"),
        }
    }

    #[test]
    fn test_wake_word_detection() {
        let result = detect_wake_word("hey vox delete that");
        assert_eq!(result, Some("delete that"));
    }

    #[test]
    fn test_wake_word_case_insensitive() {
        let result = detect_wake_word("Hey Vox delete that");
        assert_eq!(result, Some("delete that"));
    }

    #[test]
    fn test_wake_word_not_in_middle() {
        let result = detect_wake_word("I said hey vox");
        assert!(result.is_none());
    }

    #[test]
    fn test_wake_word_boundary() {
        // "hey voxel" should NOT match — "voxel" is a longer word, not a boundary
        assert!(detect_wake_word("hey voxel something").is_none());
        // "hey vox," and "hey vox " should still match
        assert_eq!(detect_wake_word("hey vox, do something"), Some("do something"));
        assert_eq!(detect_wake_word("hey vox do something"), Some("do something"));
        // "hey vox" alone at end should match with empty remainder
        assert_eq!(detect_wake_word("hey vox"), Some(""));
    }

    // --- T011: Integration tests (require model file) ---

    const MODEL_PATH: &str = "tests/fixtures/qwen2.5-3b-instruct-q4_k_m.gguf";

    fn load_processor() -> PostProcessor {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(MODEL_PATH);
        PostProcessor::new(&path, true).expect("failed to load model")
    }

    #[test]
    fn test_llm_model_loads() {
        let _processor = load_processor();
    }

    #[test]
    fn test_llm_filler_removal() {
        let processor = load_processor();
        let result = processor
            .process("um uh let's meet", "", "General")
            .expect("process failed");
        match result {
            ProcessorOutput::Text(text) => {
                let lower = text.to_lowercase();
                assert!(!lower.contains("um"), "output still contains 'um': {text}");
                assert!(!lower.contains("uh"), "output still contains 'uh': {text}");
                assert!(lower.contains("meet"), "output should contain 'meet': {text}");
            }
            ProcessorOutput::Command(_) => panic!("expected Text, got Command"),
        }
    }

    #[test]
    fn test_llm_course_correction() {
        let processor = load_processor();
        let result = processor
            .process("tuesday no wait wednesday", "", "General")
            .expect("process failed");
        match result {
            ProcessorOutput::Text(text) => {
                let lower = text.to_lowercase();
                assert!(
                    lower.contains("wednesday"),
                    "output should contain 'wednesday': {text}"
                );
            }
            ProcessorOutput::Command(_) => panic!("expected Text, got Command"),
        }
    }

    #[test]
    fn test_llm_number_formatting() {
        let processor = load_processor();
        let result = processor
            .process("twenty five dollars", "", "General")
            .expect("process failed");
        match result {
            ProcessorOutput::Text(text) => {
                assert!(
                    text.contains("$25") || text.contains("25"),
                    "output should contain '$25' or '25': {text}"
                );
            }
            ProcessorOutput::Command(_) => panic!("expected Text, got Command"),
        }
    }

    #[test]
    fn test_llm_email_formatting() {
        let processor = load_processor();
        let result = processor
            .process("john at outlook dot com", "", "General")
            .expect("process failed");
        match result {
            ProcessorOutput::Text(text) => {
                assert!(
                    text.contains("john@outlook.com"),
                    "output should contain 'john@outlook.com': {text}"
                );
            }
            ProcessorOutput::Command(_) => panic!("expected Text, got Command"),
        }
    }

    #[test]
    fn test_llm_empty_input() {
        let processor = load_processor();
        let result = processor
            .process("", "", "General")
            .expect("process failed");
        match result {
            ProcessorOutput::Text(text) => {
                assert_eq!(text, "", "empty input should produce empty output");
            }
            ProcessorOutput::Command(_) => panic!("expected Text for empty input, got Command"),
        }
    }

    // --- T014: Command detection integration test ---

    #[test]
    fn test_llm_command_detection() {
        let processor = load_processor();
        let result = processor
            .process("delete that", "", "General")
            .expect("process failed");
        match result {
            ProcessorOutput::Command(cmd) => {
                assert_eq!(
                    cmd.cmd, "delete_last",
                    "expected 'delete_last' command, got: {}",
                    cmd.cmd
                );
            }
            ProcessorOutput::Text(text) => {
                panic!("expected Command for 'delete that', got Text: {text}");
            }
        }
    }

    // --- T015: Tone adaptation integration test ---

    #[test]
    fn test_llm_tone_adaptation() {
        let processor = load_processor();
        let transcript = "hey how are you doing";

        let formal = processor
            .process(transcript, "", "Outlook")
            .expect("process with Outlook failed");
        let casual = processor
            .process(transcript, "", "Slack")
            .expect("process with Slack failed");

        // Both should produce valid Text output
        match (&formal, &casual) {
            (ProcessorOutput::Text(f), ProcessorOutput::Text(c)) => {
                assert!(!f.is_empty(), "Outlook output should not be empty");
                assert!(!c.is_empty(), "Slack output should not be empty");
            }
            _ => panic!("expected Text output for both Outlook and Slack"),
        }
    }

    // --- T017: Streaming integration tests ---

    #[test]
    fn test_llm_streaming() {
        let processor = load_processor();
        let mut tokens = Vec::new();
        let result = processor
            .process_streaming("hello world how are you", "", "General", |token| {
                tokens.push(token.to_string());
            })
            .expect("streaming process failed");

        match result {
            ProcessorOutput::Text(text) => {
                assert!(!text.is_empty(), "streamed text should not be empty");
                assert!(
                    !tokens.is_empty(),
                    "callback should have been invoked with tokens"
                );
            }
            ProcessorOutput::Command(_) => {
                panic!("expected Text for 'hello world', got Command");
            }
        }
    }

    #[test]
    fn test_llm_command_not_streamed() {
        let processor = load_processor();
        let mut tokens = Vec::new();
        let result = processor
            .process_streaming("delete that", "", "General", |token| {
                tokens.push(token.to_string());
            })
            .expect("streaming process failed");

        match result {
            ProcessorOutput::Command(cmd) => {
                assert_eq!(cmd.cmd, "delete_last");
                assert!(
                    tokens.is_empty(),
                    "callback should NOT be invoked for commands, got {} tokens",
                    tokens.len()
                );
            }
            ProcessorOutput::Text(text) => {
                panic!("expected Command for 'delete that', got Text: {text}");
            }
        }
    }

    // --- T018: Dictionary hints integration test ---

    #[test]
    fn test_llm_dictionary_hints() {
        let processor = load_processor();
        let result = processor
            .process(
                "um we should use the react framework for this",
                "React\nTypeScript",
                "VS Code",
            )
            .expect("process with dictionary hints failed");

        match result {
            ProcessorOutput::Text(text) => {
                assert!(!text.is_empty(), "output with dictionary hints should not be empty");
                let lower = text.to_lowercase();
                assert!(
                    lower.contains("react"),
                    "output should preserve 'React' from dictionary hints: {text}"
                );
            }
            ProcessorOutput::Command(_) => {
                panic!("expected Text output with dictionary hints, got Command");
            }
        }
    }
}
