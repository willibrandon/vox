# Feature 006: Text Injection

**Status:** Not Started
**Dependencies:** 005-llm-post-processing
**Design Reference:** Section 4.5 (Text Injection)
**Estimated Scope:** Windows SendInput, macOS CGEvent, voice command keystroke mapping

---

## Overview

Implement OS-level text injection that types polished text into whatever application has focus. This is the final stage of the pipeline — it takes the LLM's output and simulates keyboard input at the OS level, making Vox work in any text field: editors, browsers, terminals, chat apps, IDEs. Also implements voice command execution by mapping commands to keyboard shortcuts.

---

## Requirements

### FR-001: Text Injector Interface

```rust
// crates/vox_core/src/injector/mod.rs

pub struct TextInjector;

impl TextInjector {
    /// Inject polished text into the currently focused application.
    pub fn inject_text(text: &str) -> Result<()>;

    /// Execute a voice command (mapped to keyboard shortcuts).
    pub fn execute_command(command: &VoiceCommand) -> Result<()>;
}
```

### FR-002: Windows Implementation (windows 0.62)

```rust
// crates/vox_core/src/injector/windows.rs

#[cfg(target_os = "windows")]
mod injector {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    pub fn inject_text(text: &str) -> Result<()> {
        let chars: Vec<u16> = text.encode_utf16().collect();
        let mut inputs: Vec<INPUT> = Vec::with_capacity(chars.len() * 2);

        for ch in &chars {
            // Key down
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: *ch,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            // Key up
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: *ch,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
        Ok(())
    }
}
```

**Windows limitations:**
- `SendInput` (via `windows` 0.62) cannot inject into elevated (admin) processes due to UIPI (User Interface Privilege Isolation)
- This is an OS limitation, not a bug. Vox should detect this and show a message in the overlay.

### FR-003: macOS Implementation (objc2 0.6)

```rust
// crates/vox_core/src/injector/macos.rs

#[cfg(target_os = "macos")]
mod injector {
    use objc2_core_graphics::*;

    pub fn inject_text(text: &str) -> Result<()> {
        // CGEvent has an undocumented 20-character limit per call.
        // Must chunk text into 20-char segments.
        for chunk in text.as_bytes().chunks(20) {
            let chunk_str = std::str::from_utf8(chunk)?;
            inject_chunk(chunk_str)?;
        }
        Ok(())
    }

    fn inject_chunk(text: &str) -> Result<()> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)?;
        let event = CGEvent::new_keyboard_event(source.clone(), 0, true)?;
        let buf: Vec<u16> = text.encode_utf16().collect();
        event.set_string_from_utf16_unchecked(&buf);
        event.post(CGEventTapLocation::HID);

        let event_up = CGEvent::new_keyboard_event(source, 0, false)?;
        event_up.post(CGEventTapLocation::HID);
        Ok(())
    }
}
```

**macOS requirements:**
- Uses `objc2` 0.6 + `objc2-core-graphics` 0.3 (NOT the Servo `core-graphics` crate, which is heading toward deprecation)
- CGEvent has an **undocumented 20-character limit** per call — text MUST be chunked
- Requires **Accessibility** permission (System Settings → Privacy & Security → Accessibility)
- Chunking must respect UTF-16 character boundaries (don't split a surrogate pair)

### FR-004: Voice Command → Keystroke Mapping

```rust
// crates/vox_core/src/injector/commands.rs

pub fn execute_command(command: &VoiceCommand) -> Result<()> {
    match command.cmd.as_str() {
        "delete_last" => send_shortcut(modifier_key(), Key::Backspace),
        "undo" => send_shortcut(modifier_key(), Key::Z),
        "select_all" => send_shortcut(modifier_key(), Key::A),
        "newline" => send_key(Key::Enter),
        "paragraph" => {
            send_key(Key::Enter)?;
            send_key(Key::Enter)
        }
        "copy" => send_shortcut(modifier_key(), Key::C),
        "paste" => send_shortcut(modifier_key(), Key::V),
        "tab" => send_key(Key::Tab),
        _ => Err(anyhow!("Unknown command: {}", command.cmd)),
    }
}

/// Returns Ctrl on Windows, Cmd on macOS
fn modifier_key() -> Modifier {
    #[cfg(target_os = "windows")]
    { Modifier::Ctrl }
    #[cfg(target_os = "macos")]
    { Modifier::Cmd }
}
```

Command-to-keystroke mapping table:

| Command | Windows | macOS |
|---|---|---|
| `delete_last` | Ctrl+Backspace | Option+Delete |
| `undo` | Ctrl+Z | Cmd+Z |
| `select_all` | Ctrl+A | Cmd+A |
| `newline` | Enter | Enter |
| `paragraph` | Enter, Enter | Enter, Enter |
| `copy` | Ctrl+C | Cmd+C |
| `paste` | Ctrl+V | Cmd+V |
| `tab` | Tab | Tab |

### FR-005: Unicode Support

Text injection must handle:
- ASCII text (a-z, 0-9, punctuation)
- Unicode text (accented characters, CJK, emoji)
- Special characters (curly quotes, em-dash, ellipsis)

Windows `KEYEVENTF_UNICODE` handles all Unicode. macOS CGEvent handles Unicode via `set_string_from_utf16_unchecked`.

### FR-006: Injection Buffering

If text injection fails (focus lost, permission denied, elevated process), buffer the text and:
1. Show the buffered text in the overlay with a "Copy" button
2. Retry injection on the next focus change event
3. Clear the buffer once successfully injected

---

## Acceptance Criteria

- [ ] Text injects correctly into Notepad (Windows) / TextEdit (macOS)
- [ ] Text injects correctly into VS Code
- [ ] Text injects correctly into Chrome (Gmail compose)
- [ ] Text injects correctly into terminal
- [ ] Unicode characters inject correctly (accents, CJK)
- [ ] Voice commands map to correct keyboard shortcuts
- [ ] macOS text properly chunks at 20-character boundaries
- [ ] Failed injection buffers text with copy fallback
- [ ] Elevated process detection on Windows shows informative message
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_command_mapping` | All commands map to correct keystrokes |
| `test_modifier_key_platform` | Ctrl on Windows, Cmd on macOS |
| `test_macos_chunking` | Text > 20 chars properly chunked |
| `test_macos_chunking_unicode` | Chunking respects UTF-16 boundaries |
| `test_unicode_encoding` | Emoji and CJK encode correctly for injection |

### Manual Testing Matrix

| Scenario | Windows | macOS |
|---|---|---|
| Basic text into Notepad / TextEdit | ☐ | ☐ |
| Text into VS Code | ☐ | ☐ |
| Text into Chrome (Gmail compose) | ☐ | ☐ |
| Text into Slack desktop | ☐ | ☐ |
| Text into Terminal | ☐ | ☐ |
| Voice command: "delete that" | ☐ | ☐ |
| Voice command: "new line" | ☐ | ☐ |
| Voice command: "undo" | ☐ | ☐ |
| Unicode characters | ☐ | ☐ |
| Emoji injection | ☐ | ☐ |

---

## Performance Targets

| Metric | Target |
|---|---|
| Text injection latency | < 30 ms for typical utterance |
| Command execution latency | < 10 ms |
