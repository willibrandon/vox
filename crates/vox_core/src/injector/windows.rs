//! Windows text injection via SendInput and UIPI elevation pre-detection.
//!
//! Uses `KEYEVENTF_UNICODE` for text injection (handles all Unicode including
//! emoji via surrogate pairs) and virtual key codes for keyboard shortcuts.
//! Pre-checks foreground window elevation via `TokenElevation` because
//! `SendInput`'s return value is unreliable for UIPI detection.

use std::mem;

use anyhow::{Context, Result};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_SHIFT, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

use super::{InjectionError, InjectionResult};

/// Check whether the foreground window belongs to an elevated (admin) process.
///
/// Uses the `TokenElevation` chain: GetForegroundWindow → GetWindowThreadProcessId →
/// OpenProcess → OpenProcessToken → GetTokenInformation. If `OpenProcess` fails
/// with access denied, conservatively assumes elevated (safe default that triggers
/// buffering rather than silent event drops).
///
/// Returns `Ok(true)` if elevated, `Ok(false)` if not, or `Err` on unexpected
/// API failures. A null foreground HWND is not handled here — the caller checks
/// for no-focus before calling this function.
fn is_foreground_elevated(hwnd: HWND) -> Result<bool> {
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return Ok(false);
        }

        let process_handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => handle,
            Err(_) => {
                // Access denied on protected system processes — assume elevated
                return Ok(true);
            }
        };

        let mut token_handle = HANDLE::default();
        let token_result =
            OpenProcessToken(process_handle, TOKEN_QUERY, &mut token_handle);

        // Close process handle immediately — we either have the token or failed
        let _ = CloseHandle(process_handle);

        token_result.context("OpenProcessToken failed")?;

        let mut elevation = TOKEN_ELEVATION {
            TokenIsElevated: 0,
        };
        let mut return_length: u32 = 0;

        let info_result = GetTokenInformation(
            token_handle,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut std::ffi::c_void),
            mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_length,
        );

        let _ = CloseHandle(token_handle);

        info_result.context("GetTokenInformation failed")?;

        Ok(elevation.TokenIsElevated != 0)
    }
}

/// Build key-up INPUT events for all modifier keys (Ctrl, Shift, Alt, Space).
///
/// When the user activates dictation with a modifier hotkey (e.g. Ctrl+Shift+Space),
/// those keys may still be physically held when injection starts. This causes the
/// target application to interpret injected characters as keyboard shortcuts
/// (Ctrl+Shift+B instead of 'B'), silently swallowing text. These key-up events
/// must be prepended to the text INPUT array and sent in a **single** `SendInput`
/// call — sending them as a separate call creates a window where physical key state
/// can re-assert between the modifier release and text injection.
fn modifier_release_inputs() -> Vec<INPUT> {
    const MODIFIERS: [VIRTUAL_KEY; 10] = [
        VK_CONTROL,
        VK_LCONTROL,
        VK_RCONTROL,
        VK_SHIFT,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_MENU,
        VK_LMENU,
        VK_RMENU,
        VK_SPACE,
    ];

    MODIFIERS
        .iter()
        .map(|&vk| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: KEYEVENTF_KEYUP,
                    ..Default::default()
                },
            },
        })
        .collect()
}

/// Inject text into the focused application via `SendInput` with `KEYEVENTF_UNICODE`.
///
/// Strips null bytes to avoid C API truncation, checks for focused window and
/// UIPI elevation, releases any held modifier keys, then builds an INPUT array
/// with key-down + key-up pairs for each UTF-16 code unit and sends them in a
/// single `SendInput` call.
pub(super) fn inject_text_impl(text: &str) -> InjectionResult {
    // Strip null bytes that could truncate C-level string processing
    let clean: String = text.chars().filter(|&c| c != '\0').collect();

    // If null stripping removed all characters, nothing left to inject
    if clean.is_empty() {
        return InjectionResult::Success;
    }

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == HWND::default() {
        return InjectionResult::Blocked {
            reason: InjectionError::NoFocusedWindow,
            text: text.to_string(),
        };
    }

    match is_foreground_elevated(hwnd) {
        Ok(true) => {
            return InjectionResult::Blocked {
                reason: InjectionError::ElevatedTarget,
                text: text.to_string(),
            };
        }
        Ok(false) => {}
        Err(e) => {
            return InjectionResult::Blocked {
                reason: InjectionError::PlatformError(format!(
                    "UIPI elevation check failed: {e}"
                )),
                text: text.to_string(),
            };
        }
    }

    // Release modifier keys first so held hotkey keys (Ctrl+Shift+Space)
    // don't cause the target app to interpret injected text as shortcuts.
    let modifier_inputs = modifier_release_inputs();
    let modifier_sent =
        unsafe { SendInput(&modifier_inputs, mem::size_of::<INPUT>() as i32) };

    // Brief pause for the modifier releases to take effect in the target
    // app's input processing before text events start arriving.
    std::thread::sleep(std::time::Duration::from_millis(5));

    // Inject text in small chunks. SendInput inserts events into the system
    // input stream atomically per call, but the target app processes them
    // asynchronously via its message loop. Large batches (200+ events) can
    // overwhelm the app's input processing — events get queued in the system
    // input stream but the app only processes a fraction before its message
    // loop yields to other work. Chunking with brief pauses lets each batch
    // fully reach the app before the next batch arrives.
    const CHUNK_CHARS: usize = 32;
    let utf16: Vec<u16> = clean.encode_utf16().collect();
    let mut total_sent: u32 = 0;
    let mut total_expected: u32 = 0;
    let chunks: Vec<&[u16]> = utf16.chunks(CHUNK_CHARS).collect();
    let chunk_count = chunks.len();

    for (i, chunk) in chunks.iter().enumerate() {
        let mut inputs = Vec::with_capacity(chunk.len() * 2);
        for &code_unit in *chunk {
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: code_unit,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
            inputs.push(INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: code_unit,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            });
        }

        let sent = unsafe { SendInput(&inputs, mem::size_of::<INPUT>() as i32) };
        total_sent += sent;
        total_expected += inputs.len() as u32;

        if sent == 0 {
            tracing::error!(
                chunk = i,
                chunk_count,
                total_sent,
                total_expected,
                "SendInput returned 0 mid-injection"
            );
            return InjectionResult::Blocked {
                reason: InjectionError::PlatformError(format!(
                    "SendInput returned 0 at chunk {i}/{chunk_count}"
                )),
                text: text.to_string(),
            };
        }

        // Pause between chunks so the target app can process each batch.
        // Skip delay after the final chunk.
        if i < chunk_count - 1 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    tracing::info!(
        total_sent,
        total_expected,
        modifier_sent,
        text_len = clean.len(),
        chunk_count,
        "SendInput chunked injection"
    );

    if total_sent < total_expected {
        return InjectionResult::Blocked {
            reason: InjectionError::PlatformError(format!(
                "SendInput partial: {total_sent}/{total_expected} events injected"
            )),
            text: text.to_string(),
        };
    }

    InjectionResult::Success
}

/// Send a keyboard shortcut (modifier + key) as an atomic 4-event sequence.
///
/// All four events (modifier-down, key-down, key-up, modifier-up) are sent in a
/// single `SendInput` call to prevent other input from interleaving.
pub(super) fn send_shortcut(modifier: VIRTUAL_KEY, key: VIRTUAL_KEY) -> Result<()> {
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: modifier,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: KEYEVENTF_KEYUP,
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: modifier,
                    dwFlags: KEYEVENTF_KEYUP,
                    ..Default::default()
                },
            },
        },
    ];

    let sent = unsafe { SendInput(&inputs, mem::size_of::<INPUT>() as i32) };
    if sent != 4 {
        anyhow::bail!("SendInput shortcut: expected 4 events, sent {sent}");
    }
    Ok(())
}

/// Send a single key press (key-down + key-up).
pub(super) fn send_key(key: VIRTUAL_KEY) -> Result<()> {
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    ..Default::default()
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: KEYEVENTF_KEYUP,
                    ..Default::default()
                },
            },
        },
    ];

    let sent = unsafe { SendInput(&inputs, mem::size_of::<INPUT>() as i32) };
    if sent != 2 {
        anyhow::bail!("SendInput key: expected 2 events, sent {sent}");
    }
    Ok(())
}

/// Get the name of the currently focused application on Windows.
///
/// Flow: GetForegroundWindow → GetWindowThreadProcessId → OpenProcess →
/// QueryFullProcessImageNameW → extract filename stem. Returns "Unknown"
/// on any failure.
pub(super) fn get_focused_app_name_impl() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == HWND::default() {
            return "Unknown".to_string();
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return "Unknown".to_string();
        }

        let process_handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => handle,
            Err(_) => return "Unknown".to_string(),
        };

        let mut buf = [0u16; 260]; // MAX_PATH
        let mut size = buf.len() as u32;

        let result = windows::Win32::System::Threading::QueryFullProcessImageNameW(
            process_handle,
            windows::Win32::System::Threading::PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );

        let _ = CloseHandle(process_handle);

        if result.is_err() {
            return "Unknown".to_string();
        }

        let path = String::from_utf16_lossy(&buf[..size as usize]);
        // Extract filename stem: "C:\...\notepad.exe" → "notepad"
        std::path::Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_text_basic() {
        // In CI there's no focused window, so this should return Blocked(NoFocusedWindow)
        // rather than panicking. On a desktop with Notepad focused, it would succeed.
        let result = inject_text_impl("Hello");
        match &result {
            InjectionResult::Success => {
                // Running on desktop with a focused window — verify it didn't error
            }
            InjectionResult::Blocked { reason, .. } => match reason {
                InjectionError::NoFocusedWindow => {
                    // Expected in CI — no desktop session
                }
                InjectionError::ElevatedTarget => {
                    // Running against an elevated window — acceptable
                }
                InjectionError::PlatformError(msg) => {
                    // Platform API issue — still a valid code path
                    assert!(!msg.is_empty(), "platform error should have a message");
                }
            },
        }
    }

    #[test]
    fn test_inject_null_only_text() {
        // Text that becomes empty after null byte stripping should return Success,
        // not a spurious PlatformError from SendInput receiving zero events.
        let result = inject_text_impl("\0\0");
        assert!(
            matches!(result, InjectionResult::Success),
            "null-only text should succeed after sanitization, got: {result:?}"
        );
    }

    #[test]
    fn test_inject_text_unicode() {
        // Verify emoji and CJK don't panic and produce correct code path.
        // Emoji 🚀 = U+1F680 → surrogate pair (2 UTF-16 code units → 4 INPUT events)
        // CJK 你好 = U+4F60 U+597D → 2 BMP code units → 4 INPUT events
        let result = inject_text_impl("🚀你好");
        match &result {
            InjectionResult::Success => {}
            InjectionResult::Blocked { reason, text } => {
                // Text should be preserved byte-for-byte
                assert_eq!(text, "🚀你好");
                match reason {
                    InjectionError::NoFocusedWindow | InjectionError::ElevatedTarget => {}
                    InjectionError::PlatformError(msg) => {
                        assert!(!msg.is_empty());
                    }
                }
            }
        }
    }
}
