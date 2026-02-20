# Quickstart: Text Injection

**Feature Branch**: `006-text-injection`
**Date**: 2026-02-20

## What This Feature Does

Implements OS-level text injection — the final stage of the Vox pipeline. Takes polished text from the LLM post-processor and simulates keyboard input to type it into whatever application has focus. Also executes voice commands by mapping them to keyboard shortcuts.

## Files to Create/Modify

### New Files

| File | Purpose |
|---|---|
| `crates/vox_core/src/injector/windows.rs` | Windows SendInput implementation + UIPI detection |
| `crates/vox_core/src/injector/macos.rs` | macOS CGEvent implementation + text chunking |
| `crates/vox_core/src/injector/commands.rs` | Cross-platform voice command → keystroke mapping |

### Modified Files

| File | Change |
|---|---|
| `crates/vox_core/src/injector.rs` | Populate module root with public API, re-exports, submodule declarations |
| `crates/vox_core/Cargo.toml` | Add `Win32_System_Threading` and `Win32_Security` features to `windows` dep |

## Build & Test

```bash
# Build (Windows)
cargo build -p vox_core --features cuda

# Run tests (Windows)
cargo test -p vox_core --features cuda

# Run specific injector tests
cargo test -p vox_core test_command_mapping --features cuda -- --nocapture
cargo test -p vox_core test_utf16_chunking --features cuda -- --nocapture
```

## Key Dependencies

| Crate | Version | What For |
|---|---|---|
| `windows` | 0.62 | SendInput, UIPI detection (Windows only) |
| `objc2` | 0.6 | Objective-C runtime (macOS only) |
| `objc2-core-graphics` | 0.3 | CGEvent keyboard simulation (macOS only) |
| `anyhow` | 1 | Error handling |
| `serde_json` | 1 | VoiceCommand deserialization (already in use) |

## Architecture Notes

- Module follows existing pattern: `injector.rs` as root + `injector/` directory for submodules
- Platform dispatch via `#[cfg(target_os = "...")]` at compile time (not trait objects)
- `VoiceCommand` type is already defined in `llm::processor` — consumed here, not redefined
- `CGEvent`/`CGEventSource` are NOT Send/Sync — macOS injection must happen on a single thread
- Windows `SendInput` is thread-safe but should be called from the same thread for atomicity
