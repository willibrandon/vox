//! System prompt and prompt construction helpers for the LLM post-processor.
//!
//! Contains the instruction set that defines post-processing behavior and helpers
//! to build the per-call user message with active application context, dictionary
//! hints, and raw transcript text.

/// The system prompt that defines all post-processing rules for the Qwen LLM.
///
/// Encodes 8 rules: filler removal, punctuation, course correction, formatting,
/// tone adaptation, command detection, voice preservation, and output-only constraint.
pub const SYSTEM_PROMPT: &str = "\
You are a dictation post-processor. Clean up speech-to-text output and return ONLY the cleaned text.

Rules:
1. Remove filler words: um, uh, like, you know, basically, literally, so, I mean.
2. Fix punctuation and capitalization.
3. Course correction: keep only the final version when the speaker corrects themselves.
4. Format numbers ($25), dates (January 3, 2026), emails (john@outlook.com), URLs naturally.
5. Adapt tone to the active application (formal for email, casual for chat, technical for code editors).
6. Preserve the speaker's voice. Do NOT rephrase or summarize.
7. Output ONLY the cleaned text. No explanations, no commentary.
8. Exception: if the ENTIRE transcript is exactly one of these voice commands, return JSON instead:
   delete that → {\"cmd\":\"delete_last\"}
   undo that → {\"cmd\":\"undo\"}
   new line → {\"cmd\":\"newline\"}
   new paragraph → {\"cmd\":\"paragraph\"}
   select all → {\"cmd\":\"select_all\"}
   copy that → {\"cmd\":\"copy\"}
   paste → {\"cmd\":\"paste\"}
   tab → {\"cmd\":\"tab\"}

Examples:
Input: \"um uh let's meet tomorrow\"
Output: Let's meet tomorrow.

Input: \"twenty five dollars\"
Output: $25

Input: \"john at outlook dot com\"
Output: john@outlook.com

Input: \"tuesday no wait wednesday\"
Output: Wednesday

Input: \"delete that\"
Output: {\"cmd\":\"delete_last\"}";

/// The wake word prefix that triggers command-emphasis routing.
const WAKE_WORD: &str = "hey vox";

/// Build the user message block for a standard post-processing call.
///
/// Formats the active application name, dictionary hints, and raw transcript
/// into the structured user message that the LLM expects after the system prompt.
pub fn build_user_message(active_app: &str, dictionary_hints: &str, raw_text: &str) -> String {
    format!(
        "Active application: {active_app}\n\
         Dictionary: {dictionary_hints}\n\
         Raw transcript: \"{raw_text}\""
    )
}

/// Build a user message with command-emphasis when the wake word was detected.
///
/// Adds an instruction that the input is likely a voice command, biasing the LLM
/// toward structured JSON command output.
pub fn build_user_message_with_command_emphasis(
    active_app: &str,
    dictionary_hints: &str,
    raw_text: &str,
) -> String {
    format!(
        "Active application: {active_app}\n\
         Dictionary: {dictionary_hints}\n\
         Raw transcript: \"{raw_text}\"\n\
         Note: The user used the wake word. This is likely a voice command. \
         Return a JSON command if possible."
    )
}

/// Detect the "hey vox" wake word at the start of the transcript.
///
/// Returns `Some(remaining_text)` with the prefix stripped if the wake word is
/// found at the start. Returns `None` if the wake word is absent or not at the start.
pub fn detect_wake_word(text: &str) -> Option<&str> {
    let trimmed = text.trim_start();
    let lower = trimmed.to_lowercase();
    if lower.starts_with(WAKE_WORD) {
        let after = &trimmed[WAKE_WORD.len()..];
        // Require a word boundary after the wake word — the next character must be
        // whitespace, punctuation, or end-of-string. Without this, "hey voxel"
        // would falsely match and produce a corrupted remainder.
        match after.chars().next() {
            None => return Some(""),
            Some(c) if c.is_ascii_alphanumeric() => return None,
            _ => {}
        }
        let remaining = after.trim_start_matches(|c: char| c == ',' || c == ' ');
        Some(remaining)
    } else {
        None
    }
}
