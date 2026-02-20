//! LLM post-processing engine for raw speech-to-text transcripts.
//!
//! Takes raw ASR output and produces polished text or structured voice commands
//! using Qwen 2.5 3B Instruct via llama.cpp. Handles filler removal, punctuation,
//! course correction, number/date/email formatting, tone adaptation, and command
//! detection.

mod processor;
mod prompts;

pub use processor::{PostProcessor, ProcessorOutput, VoiceCommand};
