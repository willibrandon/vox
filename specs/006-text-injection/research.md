# Research: Text Injection

**Feature Branch**: `006-text-injection`
**Date**: 2026-02-20

## Decision Summary

| Topic | Decision | Rationale |
|---|---|---|
| Windows text injection | `SendInput` with `KEYEVENTF_UNICODE` | Handles all Unicode, character-by-character, standard Win32 API |
| macOS text injection | `CGEvent::keyboard_set_unicode_string` | Direct CoreGraphics API via objc2-core-graphics 0.3 |
| macOS chunking boundary | UTF-16 code units (not UTF-8 bytes) | CGEvent limit is 20 UniChar (u16), not 20 bytes |
| UIPI detection | Pre-check via TokenElevation | `SendInput` return value is unreliable for UIPI detection |
| macOS `delete_last` | Option + keycode 0x33 (backward delete) | Matches Ctrl+Backspace semantics on Windows |
| Module structure | `injector.rs` + `injector/` submodules | Follows existing vad/asr/llm pattern, no `mod.rs` per CLAUDE.md |

## Windows SendInput API (windows 0.62)

### Signature

```rust
pub unsafe fn SendInput(pinputs: &[INPUT], cbsize: i32) -> u32
```

- `pinputs`: Rust slice of `INPUT` structs
- `cbsize`: `std::mem::size_of::<INPUT>() as i32`
- Returns: number of events successfully inserted (0 = blocked)

### Unicode Text Injection

Two `INPUT` structs per character — key-down then key-up:

- `wVk`: `VIRTUAL_KEY(0)` (must be zero for Unicode)
- `wScan`: UTF-16 code unit (`u16`)
- `dwFlags`: `KEYEVENTF_UNICODE` (key-down) or `KEYEVENTF_UNICODE | KEYEVENTF_KEYUP` (key-up)
- System synthesizes `VK_PACKET` → `WM_CHAR` with the Unicode character

Surrogate pairs (characters above U+FFFF like emoji) require two separate INPUT events — one per surrogate. Rust's `encode_utf16()` produces these naturally.

### Keyboard Shortcuts

For shortcuts (e.g., Ctrl+Z), send virtual key events in order:

1. Modifier key down (`VK_CONTROL`, `dwFlags = KEYBD_EVENT_FLAGS(0)`)
2. Key down (`VK_Z`, `dwFlags = KEYBD_EVENT_FLAGS(0)`)
3. Key up (`VK_Z`, `dwFlags = KEYEVENTF_KEYUP`)
4. Modifier key up (`VK_CONTROL`, `dwFlags = KEYEVENTF_KEYUP`)

All four events should be passed in a single `SendInput` call for atomicity.

### Virtual Key Codes

| Key | Constant | Value |
|---|---|---|
| Backspace | `VK_BACK` | 0x08 |
| Tab | `VK_TAB` | 0x09 |
| Enter | `VK_RETURN` | 0x0D |
| Control | `VK_CONTROL` | 0x11 |
| A | `VK_A` | 0x41 |
| C | `VK_C` | 0x43 |
| V | `VK_V` | 0x56 |
| Z | `VK_Z` | 0x5A |

### Required Cargo Features

Currently enabled: `Win32_UI_Input_KeyboardAndMouse`, `Win32_UI_WindowsAndMessaging`, `Win32_Foundation`.

Must add for UIPI detection:
- `Win32_System_Threading` — `OpenProcess`, `OpenProcessToken`, `PROCESS_QUERY_LIMITED_INFORMATION`
- `Win32_Security` — `GetTokenInformation`, `TOKEN_ELEVATION`, `TokenElevation`, `TOKEN_QUERY`

## macOS CGEvent API (objc2-core-graphics 0.3)

### Key Type Mappings

| Apple Name | Rust Type | Repr |
|---|---|---|
| `CGEventRef` | `CFRetained<CGEvent>` | opaque CF type |
| `CGEventSourceRef` | `CFRetained<CGEventSource>` | opaque CF type |
| `CGKeyCode` | `CGKeyCode = u16` | u16 |
| `UniChar` | `UniChar = u16` | u16 |
| `UniCharCount` | `UniCharCount = c_ulong` | u64 on 64-bit |

### Creating Keyboard Events

```rust
CGEvent::new_keyboard_event(
    source: Option<&CGEventSource>,
    virtual_key: CGKeyCode,  // u16
    key_down: bool,
) -> Option<CFRetained<CGEvent>>
```

Required features: `CGEventTypes` + `CGRemoteOperation`.

### Unicode String Injection

```rust
CGEvent::keyboard_set_unicode_string(
    event: Option<&CGEvent>,
    string_length: UniCharCount,     // u64
    unicode_string: *const UniChar,  // *const u16
)
```

This is `unsafe` — raw pointer API. The caller must ensure buffer validity.

**Critical**: The design doc uses `event.set_string_from_utf16_unchecked(&buf)` which is the Servo `core-graphics` crate API. The `objc2-core-graphics` 0.3 API is the static function shown above.

### Posting Events

```rust
CGEvent::post(tap: CGEventTapLocation, event: Option<&CGEvent>)
```

Use `CGEventTapLocation::HIDEventTap` (value 0) for hardware-level injection.

### 20-Character Limit

`CGEventKeyboardSetUnicodeString` silently truncates to **20 UTF-16 code units**. No error, no return value indicating truncation.

**Design doc bug**: The design doc chunks on `text.as_bytes().chunks(20)` which operates on UTF-8 bytes, not UTF-16 code units. This will break on multi-byte UTF-8 characters. The correct approach:

1. Encode to `Vec<u16>` via `str::encode_utf16()`
2. Walk the slice in steps of 20
3. If the last u16 in a chunk is a high surrogate (0xD800..=0xDBFF), shorten chunk by 1 to keep the pair together

### Modifier Flags

| Modifier | Constant | Value |
|---|---|---|
| Command | `CGEventFlags::MaskCommand` | 0x0010_0000 |
| Option/Alt | `CGEventFlags::MaskAlternate` | 0x0008_0000 |
| Control | `CGEventFlags::MaskControl` | 0x0004_0000 |
| Shift | `CGEventFlags::MaskShift` | 0x0002_0000 |

Set via `CGEvent::set_flags(Some(&event), flags)` before posting.

### macOS Virtual Key Codes

`objc2-core-graphics` does NOT define key code constants. Must define our own:

| Key | Code | Decimal |
|---|---|---|
| Return | 0x24 | 36 |
| Tab | 0x30 | 48 |
| Delete/Backspace | 0x33 | 51 |
| Command (Left) | 0x37 | 55 |
| Option (Left) | 0x3A | 58 |
| Control (Left) | 0x3B | 59 |
| Forward Delete | 0x75 | 117 |
| A | 0x00 | 0 |
| C | 0x08 | 8 |
| V | 0x09 | 9 |
| Z | 0x06 | 6 |

### Event Source

```rust
CGEventSource::new(CGEventSourceStateID::HIDSystemState) -> Option<CFRetained<CGEventSource>>
```

`HIDSystemState` (value 1) makes injected events appear as hardware input.

### Thread Safety

`CGEvent` and `CGEventSource` are **NOT Send/Sync**. All event creation, modification, and posting must happen on a single thread.

### Required Cargo Features

```toml
objc2-core-graphics = { version = "0.3", features = [
    "CGEvent", "CGEventTypes", "CGEventSource", "CGRemoteOperation"
] }
```

Default features enable all of these, but if cherry-picking, all four are needed.

## UIPI Elevation Detection (Windows)

### Why Pre-Check Is Necessary

`SendInput` return value is **unreliable** for UIPI detection:
- Sometimes returns 0 with `ERROR_ACCESS_DENIED`
- Sometimes returns the full count while silently dropping events
- Microsoft docs say: "neither GetLastError nor the return value will indicate the failure was caused by UIPI blocking"

### Detection Chain

1. `GetForegroundWindow()` → `HWND`
2. `GetWindowThreadProcessId(hwnd, &mut pid)` → fills `pid: u32`
3. `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)` → `Result<HANDLE>`
4. `OpenProcessToken(process_handle, TOKEN_QUERY, &mut token_handle)` → `Result<()>`
5. `GetTokenInformation(token_handle, TokenElevation, ...)` → fills `TOKEN_ELEVATION`
6. `elevation.TokenIsElevated != 0` → elevated, UIPI will block

### Error Handling

- `OpenProcess` may fail with `ERROR_ACCESS_DENIED` on protected system processes → treat as elevated (safe default, triggers buffer)
- Null `HWND` from `GetForegroundWindow` → no window has focus, skip injection
- `HANDLE` does NOT implement `Drop` → must call `CloseHandle` manually

### TokenElevation vs TokenIntegrityLevel

`TokenElevation` (simpler) is sufficient for Vox. `TokenIntegrityLevel` is more technically accurate for UIPI enforcement but requires complex SID parsing for marginal benefit. The common case — "is foreground window a Run as Administrator process?" — is correctly answered by `TokenElevation`.

## Alternatives Considered

| Alternative | Rejected Because |
|---|---|
| Windows clipboard paste (Ctrl+V) | Overwrites user clipboard, breaks clipboard managers |
| macOS `NSEvent.keyEvent` | AppKit-level, doesn't work in non-AppKit apps |
| `enigo` crate | Adds dependency; wraps same OS APIs we'd call directly |
| Post-hoc UIPI detection via SendInput return | Return value unreliable per Microsoft docs |
| `TokenIntegrityLevel` for UIPI | Complex SID parsing for marginal benefit over `TokenElevation` |
| Servo `core-graphics` crate | Heading toward deprecation; objc2 ecosystem is the future |
