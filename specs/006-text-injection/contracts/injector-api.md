# API Contract: Text Injector

**Feature Branch**: `006-text-injection`
**Date**: 2026-02-20

## Module: `vox_core::injector`

### Public Interface

```rust
/// Inject text into the currently focused application via OS-level keyboard simulation.
///
/// On Windows, uses SendInput with KEYEVENTF_UNICODE. On macOS, uses CGEvent with
/// text chunking at 20 UTF-16 code unit boundaries. Returns `InjectionResult::Blocked`
/// if the target window is elevated (Windows UIPI) or no window has focus.
///
/// Empty text is a no-op that returns `InjectionResult::Success`.
pub fn inject_text(text: &str) -> InjectionResult;

/// Execute a voice command by simulating the mapped keyboard shortcut.
///
/// Maps the command name to platform-appropriate key sequences and sends them
/// via the OS keyboard API. Returns an error for unrecognized commands.
pub fn execute_command(command: &VoiceCommand) -> Result<()>;
```

### Types

```rust
/// Result of a text injection attempt.
pub enum InjectionResult {
    /// Text was successfully injected into the focused application.
    Success,
    /// Injection failed. The original text and failure reason are preserved
    /// so the UI layer can display a copy-to-clipboard fallback.
    Blocked {
        /// Why injection failed.
        reason: InjectionError,
        /// The text that was not injected (preserved for buffering).
        text: String,
    },
}

/// Reason an injection attempt failed.
pub enum InjectionError {
    /// The focused window belongs to an elevated process (Windows UIPI restriction).
    ElevatedTarget,
    /// No window currently has focus.
    NoFocusedWindow,
    /// An OS API call failed.
    PlatformError(String),
}

/// Buffer holding text that failed to inject, for UI display and retry.
pub struct InjectionBuffer {
    /// The text that was not injected.
    pub text: String,
    /// Why injection failed.
    pub error: InjectionError,
    /// When the failure occurred.
    pub timestamp: std::time::Instant,
}
```

### Internal Modules (not public API)

```rust
// injector/windows.rs — #[cfg(target_os = "windows")]
fn inject_text_impl(text: &str) -> InjectionResult;
fn send_shortcut(modifier: VIRTUAL_KEY, key: VIRTUAL_KEY) -> Result<()>;
fn send_key(key: VIRTUAL_KEY) -> Result<()>;
fn is_foreground_elevated() -> Result<bool>;

// injector/macos.rs — #[cfg(target_os = "macos")]
fn inject_text_impl(text: &str) -> InjectionResult;
fn inject_chunk(utf16: &[u16]) -> Result<()>;
fn send_shortcut(flags: CGEventFlags, keycode: CGKeyCode) -> Result<()>;
fn send_key(keycode: CGKeyCode) -> Result<()>;
pub(crate) fn chunk_utf16(text: &str) -> Vec<Vec<u16>>;

// injector/commands.rs — cross-platform
pub fn execute_command(command: &VoiceCommand) -> Result<()>;
```

## Performance Contracts

| Operation | Latency Budget |
|---|---|
| `inject_text` (typical sentence, ~50 chars) | < 30 ms |
| `execute_command` (single shortcut) | < 10 ms |
| UIPI elevation check (Windows) | < 5 ms |
| macOS chunk + post (per 20-char chunk) | < 2 ms |

## Error Contracts

| Condition | Behavior |
|---|---|
| Empty text | Return `InjectionResult::Success` (no-op) |
| Whitespace-only text | Inject as-is (whitespace is valid input) |
| Elevated foreground (Windows) | Return `InjectionResult::Blocked { reason: ElevatedTarget }` |
| No focused window | Return `InjectionResult::Blocked { reason: NoFocusedWindow }` |
| SendInput returns 0 | Return `InjectionResult::Blocked { reason: PlatformError }` |
| CGEvent creation fails | Return `InjectionResult::Blocked { reason: PlatformError }` |
| Unknown voice command | Return `Err(anyhow!("Unknown command: {cmd}"))` |
