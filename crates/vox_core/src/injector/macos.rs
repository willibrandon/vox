//! macOS text injection via CGEvent and Accessibility permission detection.
//!
//! Uses `CGEvent::keyboard_set_unicode_string` for text injection, chunking at
//! 20 UTF-16 code unit boundaries to respect the undocumented CGEvent limit.
//! Surrogate pairs are never split across chunks.

use anyhow::{Context, Result};

#[cfg(target_os = "macos")]
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGKeyCode,
};

use super::{InjectionError, InjectionResult};

/// Maximum UTF-16 code units per CGEvent keyboard string injection.
const MAX_CHUNK_UTF16: usize = 20;

// macOS virtual key codes (not provided by objc2-core-graphics)
#[cfg(target_os = "macos")]
const KEY_RETURN: CGKeyCode = 0x24;
#[cfg(target_os = "macos")]
const KEY_TAB: CGKeyCode = 0x30;
#[cfg(target_os = "macos")]
const KEY_BACKSPACE: CGKeyCode = 0x33;
#[cfg(target_os = "macos")]
const KEY_A: CGKeyCode = 0x00;
#[cfg(target_os = "macos")]
const KEY_C: CGKeyCode = 0x08;
#[cfg(target_os = "macos")]
const KEY_V: CGKeyCode = 0x09;
#[cfg(target_os = "macos")]
const KEY_Z: CGKeyCode = 0x06;

/// Split text into chunks of at most 20 UTF-16 code units, never splitting
/// a surrogate pair across chunk boundaries.
///
/// The CGEvent `keyboard_set_unicode_string` API silently truncates strings
/// longer than 20 UTF-16 code units. This function encodes the text to UTF-16
/// first, then walks in steps of `MAX_CHUNK_UTF16`, checking whether the last
/// code unit in each chunk is a high surrogate (0xD800..=0xDBFF). If so, the
/// chunk is shortened by one to keep the surrogate pair together.
pub(crate) fn chunk_utf16(text: &str) -> Vec<Vec<u16>> {
    let utf16: Vec<u16> = text.encode_utf16().collect();
    if utf16.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < utf16.len() {
        let mut end = (start + MAX_CHUNK_UTF16).min(utf16.len());

        // If the last code unit is a high surrogate, shorten by 1 to keep
        // the surrogate pair together in the next chunk.
        if end < utf16.len() && end > start {
            let last = utf16[end - 1];
            if (0xD800..=0xDBFF).contains(&last) {
                end -= 1;
            }
        }

        chunks.push(utf16[start..end].to_vec());
        start = end;
    }

    chunks
}

/// Check whether macOS Accessibility permission has been granted.
///
/// Used as a preflight by both text injection and voice command execution.
/// Without this permission, CGEvent keyboard events are silently dropped by
/// the OS, so callers must fail explicitly rather than report false success.
#[cfg(target_os = "macos")]
fn check_accessibility() -> Result<()> {
    let trusted: bool = unsafe {
        extern "C" {
            fn AXIsProcessTrusted() -> bool;
        }
        AXIsProcessTrusted()
    };
    if !trusted {
        anyhow::bail!("Accessibility permission not granted");
    }
    Ok(())
}

/// Check whether the frontmost application has a focused UI element that can
/// receive keyboard input.
///
/// Resolves the frontmost application via `NSWorkspace`, obtains its PID, then
/// queries the Accessibility API (`AXUIElementCopyAttributeValue` with
/// `kAXFocusedUIElementAttribute`) to confirm an actual UI element has
/// keyboard focus.
///
/// Returns `Ok(true)` when a focused element is confirmed or when the AX query
/// fails for non-definitive reasons (AX-incompatible or unresponsive apps —
/// proceed optimistically). Returns `Ok(false)` only when there is definitively
/// no focused target (no frontmost app, or `kAXErrorNoValue`). Returns `Err`
/// for genuine platform failures.
#[cfg(target_os = "macos")]
fn has_focused_window() -> Result<bool> {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    // AX error codes from HIServices/AXError.h
    const AX_ERROR_INVALID_UI_ELEMENT: i32 = -25202;
    const AX_ERROR_CANNOT_COMPLETE: i32 = -25204;
    const AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
    const AX_ERROR_NO_VALUE: i32 = -25212;

    unsafe {
        extern "C" {
            fn AXUIElementCreateApplication(pid: i32) -> *const std::ffi::c_void;
            fn AXUIElementCopyAttributeValue(
                element: *const std::ffi::c_void,
                attribute: *const std::ffi::c_void,
                value: *mut *const std::ffi::c_void,
            ) -> i32;
            static kAXFocusedUIElementAttribute: *const std::ffi::c_void;
            fn CFRelease(cf: *const std::ffi::c_void);
        }

        let cls = AnyClass::get(c"NSWorkspace")
            .context("NSWorkspace class not found")?;
        let workspace: *const AnyObject = msg_send![cls, sharedWorkspace];
        anyhow::ensure!(!workspace.is_null(), "NSWorkspace.sharedWorkspace returned null");
        let app: *const AnyObject = msg_send![&*workspace, frontmostApplication];
        if app.is_null() {
            return Ok(false);
        }

        let pid: i32 = msg_send![&*app, processIdentifier];
        if pid <= 0 {
            return Ok(false);
        }

        let ax_app = AXUIElementCreateApplication(pid);
        if ax_app.is_null() {
            anyhow::bail!("AXUIElementCreateApplication returned null for pid {pid}");
        }

        let mut focused_element: *const std::ffi::c_void = std::ptr::null();
        let ax_error = AXUIElementCopyAttributeValue(
            ax_app,
            kAXFocusedUIElementAttribute,
            &mut focused_element,
        );

        let has_focus = ax_error == 0 && !focused_element.is_null();

        // Release CF objects — focused_element if non-null regardless of error
        // code (defensive: AX may write a value even on failure)
        if !focused_element.is_null() {
            CFRelease(focused_element);
        }
        CFRelease(ax_app);

        match ax_error {
            0 => Ok(has_focus),
            AX_ERROR_NO_VALUE => Ok(false),
            // AX-incompatible or unresponsive apps — proceed optimistically
            // rather than incorrectly blocking injection
            AX_ERROR_ATTRIBUTE_UNSUPPORTED
            | AX_ERROR_CANNOT_COMPLETE
            | AX_ERROR_INVALID_UI_ELEMENT => Ok(true),
            code => anyhow::bail!("AXUIElementCopyAttributeValue failed with error {code}"),
        }
    }
}

/// Post a single chunk of UTF-16 code units as a CGEvent keyboard event.
#[cfg(target_os = "macos")]
fn inject_chunk(utf16: &[u16]) -> Result<()> {
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .context("CGEventSource::new failed")?;

    // Key-down event with Unicode string attached
    let event_down = CGEvent::new_keyboard_event(Some(&source), 0, true)
        .context("CGEvent::new_keyboard_event (down) failed")?;

    unsafe {
        CGEvent::keyboard_set_unicode_string(
            Some(&event_down),
            utf16.len() as _,
            utf16.as_ptr(),
        );
    }
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event_down));

    // Key-up event (no string needed)
    let event_up = CGEvent::new_keyboard_event(Some(&source), 0, false)
        .context("CGEvent::new_keyboard_event (up) failed")?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event_up));

    Ok(())
}

/// Inject text into the focused application via CGEvent with UTF-16 chunking.
///
/// Strips null bytes first (matching Windows ordering so that null-only input
/// is a no-op on both platforms), then checks Accessibility permission via
/// `AXIsProcessTrusted`, verifies a focused application exists, chunks text
/// to 20 UTF-16 code units, and posts CGEvents for each chunk.
#[cfg(target_os = "macos")]
pub(super) fn inject_text_impl(text: &str) -> InjectionResult {
    // Strip null bytes that could truncate C-level string processing
    let clean: String = text.chars().filter(|&c| c != '\0').collect();

    // If null stripping removed all characters, nothing left to inject
    if clean.is_empty() {
        return InjectionResult::Success;
    }

    if let Err(e) = check_accessibility() {
        return InjectionResult::Blocked {
            reason: InjectionError::PlatformError(e.to_string()),
            text: text.to_string(),
        };
    }

    match has_focused_window() {
        Ok(true) => {}
        Ok(false) => {
            return InjectionResult::Blocked {
                reason: InjectionError::NoFocusedWindow,
                text: text.to_string(),
            };
        }
        Err(e) => {
            return InjectionResult::Blocked {
                reason: InjectionError::PlatformError(format!(
                    "Focus detection failed: {e}"
                )),
                text: text.to_string(),
            };
        }
    }

    let chunks = chunk_utf16(&clean);
    for chunk in &chunks {
        if let Err(e) = inject_chunk(chunk) {
            return InjectionResult::Blocked {
                reason: InjectionError::PlatformError(format!("CGEvent injection failed: {e}")),
                text: text.to_string(),
            };
        }
    }

    InjectionResult::Success
}

/// Send a keyboard shortcut with modifier flags on macOS.
///
/// Creates a key-down CGEvent, sets the modifier flags, posts it, then sends
/// a key-up event.
#[cfg(target_os = "macos")]
pub(super) fn send_shortcut(flags: CGEventFlags, keycode: CGKeyCode) -> Result<()> {
    check_accessibility()?;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .context("CGEventSource::new failed")?;

    let event_down = CGEvent::new_keyboard_event(Some(&source), keycode, true)
        .context("CGEvent::new_keyboard_event (down) failed")?;
    CGEvent::set_flags(Some(&event_down), flags);
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event_down));

    let event_up = CGEvent::new_keyboard_event(Some(&source), keycode, false)
        .context("CGEvent::new_keyboard_event (up) failed")?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event_up));

    Ok(())
}

/// Send a single key press (key-down + key-up) without modifier flags.
#[cfg(target_os = "macos")]
pub(super) fn send_key(keycode: CGKeyCode) -> Result<()> {
    check_accessibility()?;

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .context("CGEventSource::new failed")?;

    let event_down = CGEvent::new_keyboard_event(Some(&source), keycode, true)
        .context("CGEvent::new_keyboard_event (down) failed")?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event_down));

    let event_up = CGEvent::new_keyboard_event(Some(&source), keycode, false)
        .context("CGEvent::new_keyboard_event (up) failed")?;
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event_up));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // chunk_utf16 is pure logic — these tests run on ALL platforms

    #[test]
    fn test_utf16_chunking() {
        // 30-char ASCII → 2 chunks: first 20, second 10
        let text = "abcdefghijklmnopqrstuvwxyz1234";
        let chunks = chunk_utf16(text);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 20);
        assert_eq!(chunks[1].len(), 10);
    }

    #[test]
    fn test_utf16_chunking_surrogate() {
        // Build a string where emoji 🚀 (surrogate pair) lands at UTF-16 position 19-20.
        // 19 ASCII chars + 🚀 = positions 0..18 (ASCII) + 19..20 (surrogate pair)
        // Chunk boundary at 20 would split the pair, so chunk should shorten to 19.
        let text = "abcdefghijklmnopqrs🚀";
        let utf16: Vec<u16> = text.encode_utf16().collect();
        // 19 ASCII (19 code units) + 1 emoji (2 code units) = 21 total
        assert_eq!(utf16.len(), 21);

        let chunks = chunk_utf16(text);
        assert_eq!(chunks.len(), 2);
        // First chunk shortened to 19 (before high surrogate)
        assert_eq!(chunks[0].len(), 19);
        // Second chunk has the surrogate pair (2 code units)
        assert_eq!(chunks[1].len(), 2);
    }

    #[test]
    fn test_utf16_chunking_exact_20() {
        // Exactly 20 ASCII chars → single chunk of 20
        let text = "abcdefghijklmnopqrst";
        let chunks = chunk_utf16(text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 20);
    }

    #[test]
    fn test_utf16_chunking_empty() {
        let chunks = chunk_utf16("");
        assert!(chunks.is_empty());
    }
}
