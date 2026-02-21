//! Cross-platform voice command dispatch.
//!
//! Maps voice command names to platform-specific keyboard shortcuts and executes
//! them via the OS keyboard simulation API. The 8 standard commands cover text
//! editing (delete, undo, select all), navigation (newline, paragraph, tab),
//! and clipboard (copy, paste).

use anyhow::Result;

use crate::llm::VoiceCommand;

/// Execute a voice command by simulating the mapped keyboard shortcut.
///
/// Matches the command name against the 8 known commands and dispatches to
/// platform-specific `send_shortcut` or `send_key` helpers. Returns an error
/// for unrecognized command names.
pub fn execute_command(command: &VoiceCommand) -> Result<()> {
    match command.cmd.as_str() {
        "delete_last" => cmd_delete_last(),
        "undo" => cmd_undo(),
        "select_all" => cmd_select_all(),
        "newline" => cmd_newline(),
        "paragraph" => cmd_paragraph(),
        "copy" => cmd_copy(),
        "paste" => cmd_paste(),
        "tab" => cmd_tab(),
        _ => anyhow::bail!("Unknown command: {}", command.cmd),
    }
}

// --- Windows command implementations ---

#[cfg(target_os = "windows")]
fn cmd_delete_last() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_BACK, VK_CONTROL};
    super::windows::send_shortcut(VK_CONTROL, VK_BACK)
}

#[cfg(target_os = "windows")]
fn cmd_undo() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_Z};
    super::windows::send_shortcut(VK_CONTROL, VK_Z)
}

#[cfg(target_os = "windows")]
fn cmd_select_all() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_A, VK_CONTROL};
    super::windows::send_shortcut(VK_CONTROL, VK_A)
}

#[cfg(target_os = "windows")]
fn cmd_newline() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
    super::windows::send_key(VK_RETURN)
}

#[cfg(target_os = "windows")]
fn cmd_paragraph() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN;
    super::windows::send_key(VK_RETURN)?;
    super::windows::send_key(VK_RETURN)
}

#[cfg(target_os = "windows")]
fn cmd_copy() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_C, VK_CONTROL};
    super::windows::send_shortcut(VK_CONTROL, VK_C)
}

#[cfg(target_os = "windows")]
fn cmd_paste() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_CONTROL, VK_V};
    super::windows::send_shortcut(VK_CONTROL, VK_V)
}

#[cfg(target_os = "windows")]
fn cmd_tab() -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB;
    super::windows::send_key(VK_TAB)
}

// --- macOS command implementations ---

#[cfg(target_os = "macos")]
fn cmd_delete_last() -> Result<()> {
    use objc2_core_graphics::CGEventFlags;
    // Option + Backspace (backward word delete) — matches Ctrl+Backspace on Windows
    super::macos::send_shortcut(CGEventFlags::MaskAlternate, super::macos::KEY_BACKSPACE)
}

#[cfg(target_os = "macos")]
fn cmd_undo() -> Result<()> {
    use objc2_core_graphics::CGEventFlags;
    super::macos::send_shortcut(CGEventFlags::MaskCommand, super::macos::KEY_Z)
}

#[cfg(target_os = "macos")]
fn cmd_select_all() -> Result<()> {
    use objc2_core_graphics::CGEventFlags;
    super::macos::send_shortcut(CGEventFlags::MaskCommand, super::macos::KEY_A)
}

#[cfg(target_os = "macos")]
fn cmd_newline() -> Result<()> {
    super::macos::send_key(super::macos::KEY_RETURN)
}

#[cfg(target_os = "macos")]
fn cmd_paragraph() -> Result<()> {
    super::macos::send_key(super::macos::KEY_RETURN)?;
    super::macos::send_key(super::macos::KEY_RETURN)
}

#[cfg(target_os = "macos")]
fn cmd_copy() -> Result<()> {
    use objc2_core_graphics::CGEventFlags;
    super::macos::send_shortcut(CGEventFlags::MaskCommand, super::macos::KEY_C)
}

#[cfg(target_os = "macos")]
fn cmd_paste() -> Result<()> {
    use objc2_core_graphics::CGEventFlags;
    super::macos::send_shortcut(CGEventFlags::MaskCommand, super::macos::KEY_V)
}

#[cfg(target_os = "macos")]
fn cmd_tab() -> Result<()> {
    super::macos::send_key(super::macos::KEY_TAB)
}

// --- Unsupported platform fallbacks ---

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cmd_delete_last() -> Result<()> { anyhow::bail!("Unsupported platform") }
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cmd_undo() -> Result<()> { anyhow::bail!("Unsupported platform") }
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cmd_select_all() -> Result<()> { anyhow::bail!("Unsupported platform") }
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cmd_newline() -> Result<()> { anyhow::bail!("Unsupported platform") }
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cmd_paragraph() -> Result<()> { anyhow::bail!("Unsupported platform") }
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cmd_copy() -> Result<()> { anyhow::bail!("Unsupported platform") }
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cmd_paste() -> Result<()> { anyhow::bail!("Unsupported platform") }
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cmd_tab() -> Result<()> { anyhow::bail!("Unsupported platform") }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_mapping_all_known() {
        let commands = [
            "delete_last",
            "undo",
            "select_all",
            "newline",
            "paragraph",
            "copy",
            "paste",
            "tab",
        ];
        for cmd_name in &commands {
            let command = VoiceCommand {
                cmd: cmd_name.to_string(),
                args: None,
            };
            // On Windows CI without a desktop session, SendInput may fail but
            // the command should be recognized (no "Unknown command" error).
            let result = execute_command(&command);
            match &result {
                Ok(()) => {}
                Err(e) => {
                    let msg = format!("{e}");
                    assert!(
                        !msg.contains("Unknown command"),
                        "command '{cmd_name}' should be recognized, got: {msg}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_command_mapping_unknown() {
        let command = VoiceCommand {
            cmd: "foobar".to_string(),
            args: None,
        };
        let result = execute_command(&command);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Unknown command: foobar"),
            "expected 'Unknown command: foobar', got: {err}"
        );
    }
}
