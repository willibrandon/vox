# Data Model: Text Injection

**Feature Branch**: `006-text-injection`
**Date**: 2026-02-20

## Entities

### InjectionResult

Outcome of a text injection attempt. Returned by the platform-specific injector.

| Field | Type | Description |
|---|---|---|
| (variant) | `Success` | Text was injected successfully |
| (variant) | `Blocked { reason: InjectionError, text: String }` | Injection failed; text preserved for buffering |

### InjectionError

Reason for injection failure.

| Variant | Description |
|---|---|
| `ElevatedTarget` | Windows UIPI: foreground window belongs to an elevated process |
| `NoFocusedWindow` | No window has focus at injection time |
| `PlatformError(String)` | OS API call failed (CGEvent creation, SendInput error, etc.) |

### VoiceCommand (existing)

Already defined in `crates/vox_core/src/llm/processor.rs`. Consumed by the injector.

| Field | Type | Description |
|---|---|---|
| `cmd` | `String` | Command identifier (e.g., "delete_last", "undo") |
| `args` | `Option<serde_json::Value>` | Reserved for future extensible commands |

### CommandMapping (internal)

Maps a voice command to platform-specific keyboard actions. Not exposed publicly — internal to the commands module.

| Concept | Type | Description |
|---|---|---|
| Command name | `&str` | Matches `VoiceCommand.cmd` |
| Key sequence | Platform-specific INPUT/CGEvent | One or more key events to simulate |
| Modifier | Platform modifier key | Ctrl (Windows) or Cmd/Option (macOS) per command |

### InjectionBuffer

Holds text that failed to inject. Provided to the UI layer for display.

| Field | Type | Description |
|---|---|---|
| `text` | `String` | The text that failed to inject |
| `error` | `InjectionError` | Why injection failed |
| `timestamp` | `std::time::Instant` | When the failure occurred |

## Command Mapping Table

| Command | Windows Keys | macOS Keys |
|---|---|---|
| `delete_last` | Ctrl (VK_CONTROL) + Backspace (VK_BACK) | Option (MaskAlternate) + Backspace (0x33) |
| `undo` | Ctrl + Z (VK_Z) | Cmd (MaskCommand) + Z (0x06) |
| `select_all` | Ctrl + A (VK_A) | Cmd + A (0x00) |
| `newline` | Enter (VK_RETURN) | Return (0x24) |
| `paragraph` | Enter × 2 | Return × 2 |
| `copy` | Ctrl + C (VK_C) | Cmd + C (0x08) |
| `paste` | Ctrl + V (VK_V) | Cmd + V (0x09) |
| `tab` | Tab (VK_TAB) | Tab (0x30) |

## State Transitions

### Injection Flow

```
inject_text(text) called
    │
    ├── text is empty → return Ok(Success) immediately
    │
    ├── [both] check for focused window
    │   └── no focused window → return Blocked(NoFocusedWindow, text)
    │
    ├── [Windows only] check foreground window elevation
    │   ├── elevated → return Blocked(ElevatedTarget, text)
    │   └── not elevated → continue
    │
    ├── [macOS only] check Accessibility permission
    │   ├── not granted → return Blocked(PlatformError("Accessibility permission not granted"), text)
    │   └── granted → continue
    │
    ├── [Windows] build INPUT array from UTF-16, call SendInput
    │   ├── SendInput returns expected count → Success
    │   └── SendInput returns 0 → Blocked(PlatformError, text)
    │
    └── [macOS] chunk text to ≤20 UTF-16 code units, post CGEvents
        ├── all chunks posted → Success
        └── CGEvent creation fails → Blocked(PlatformError, text)
```

### Command Execution Flow

```
execute_command(cmd) called
    │
    ├── cmd matches known command → build key sequence, send via SendInput/CGEvent
    │   ├── success → Ok(())
    │   └── failure → Err(platform error)
    │
    └── cmd is unknown → Err("Unknown command: {cmd}")
```

## Platform Differences

| Aspect | Windows | macOS |
|---|---|---|
| Text API | `SendInput` with `KEYEVENTF_UNICODE` | `CGEvent::keyboard_set_unicode_string` |
| Character limit | None (single call handles all) | 20 UTF-16 code units per CGEvent |
| Modifier for most commands | Ctrl (`VK_CONTROL`) | Command (`MaskCommand`) |
| Modifier for `delete_last` | Ctrl (`VK_CONTROL`) | Option (`MaskAlternate`) |
| Elevation detection | UIPI pre-check via `TokenElevation` | Accessibility permission check (AXIsProcessTrusted or equivalent) |
| Thread safety | `SendInput` is thread-safe | `CGEvent`/`CGEventSource` NOT Send/Sync |
| Permission required | None (auto-works for non-elevated targets) | Accessibility permission |
