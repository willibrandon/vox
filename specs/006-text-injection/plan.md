# Implementation Plan: Text Injection

**Branch**: `006-text-injection` | **Date**: 2026-02-20 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/006-text-injection/spec.md`

## Summary

Implement the final pipeline stage: OS-level text injection that types LLM-polished text into the focused application via simulated keyboard input. On Windows, uses `SendInput` with `KEYEVENTF_UNICODE` for text and virtual key codes for shortcuts. On macOS, uses `CGEvent` with `keyboard_set_unicode_string` (chunked at 20 UTF-16 code units) for text and modifier flags for shortcuts. Includes UIPI elevation pre-detection on Windows and failure buffering with clipboard fallback for the UI layer. Voice commands are mapped to platform-specific keyboard shortcuts (8 commands: delete_last, undo, select_all, newline, paragraph, copy, paste, tab).

## Technical Context

**Language/Version**: Rust 2024 (1.85+)
**Primary Dependencies**: `windows` 0.62 (Win32 SendInput + UIPI detection), `objc2` 0.6 + `objc2-core-graphics` 0.3 (CGEvent), `anyhow` 1 (errors), `serde_json` 1 (VoiceCommand)
**Storage**: N/A
**Testing**: `cargo test -p vox_core --features cuda` (Windows), `cargo test -p vox_core --features metal` (macOS)
**Target Platform**: Windows 11 (CUDA) + macOS 26 Tahoe (Metal)
**Project Type**: Existing three-crate Rust workspace (`vox`, `vox_core`, `vox_ui`)
**Performance Goals**: Text injection < 30ms per sentence, command execution < 10ms
**Constraints**: < 300ms end-to-end (RTX 4090), < 750ms (M4 Pro). No blocking on audio callback thread. Text injection is the last pipeline stage — its latency is directly added to the end-to-end total.
**Scale/Scope**: 8 voice commands, 2 platform implementations, ~500 LOC estimated

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Notes |
|---|---|---|---|
| I | Local-Only Processing | PASS | SendInput and CGEvent are local OS API calls. No network. |
| II | Real-Time Latency Budget | PASS | 30ms text + 10ms command are well within 300ms/750ms budgets. |
| III | Full Pipeline — No Fallbacks | PASS | This IS the final pipeline component, completing VAD→ASR→LLM→Injection. |
| IV | Pure Rust / GPUI — No Web Tech | PASS | `windows` crate (Rust FFI to Win32) and `objc2` (Rust FFI to CoreGraphics). |
| V | Zero-Click First Launch | PASS | Windows: auto-works. macOS: Accessibility permission is OS-level, not app setup. |
| VI | Scope Only Increases | PASS | Full spec implemented: all 8 commands, both platforms, UIPI detection, failure buffering. |
| VII | Public API Documentation | PASS | All pub items will have `///` doc comments per Rust convention. |
| VIII | No Test Skipping | PASS | All tests run unconditionally. Platform-specific OS API tests use compile-time `#[cfg]` (not runtime skip). |
| IX | Explicit Commit Only | PASS | No commits without user instruction. |

**Post-Phase 1 Re-check**: All principles still hold. No storage, no network, no web tech introduced during design.

## Project Structure

### Documentation (this feature)

```text
specs/006-text-injection/
├── plan.md              # This file
├── spec.md              # Feature specification
├── research.md          # API research (Windows SendInput, macOS CGEvent, UIPI)
├── data-model.md        # Entities, types, command mapping table
├── quickstart.md        # Build/test guide
├── contracts/
│   └── injector-api.md  # Public API contract
├── checklists/
│   └── requirements.md  # Spec quality checklist
└── tasks.md             # Task list (created by /speckit.tasks)
```

### Source Code (repository root)

```text
crates/vox_core/
├── Cargo.toml                     # Add Win32_System_Threading, Win32_Security features
└── src/
    ├── injector.rs                # Module root: public API, InjectionResult, InjectionError,
    │                              # InjectionBuffer, re-exports, cfg-dispatched inject_text()
    └── injector/
        ├── windows.rs             # Win32 SendInput: inject_text_impl, send_shortcut, send_key,
        │                          # is_foreground_elevated (UIPI pre-check)
        ├── macos.rs               # CGEvent: inject_text_impl, inject_chunk, chunk_utf16,
        │                          # send_shortcut, send_key, macOS key code constants
        └── commands.rs            # Cross-platform: execute_command(), command-to-keystroke dispatch
```

**Structure Decision**: Follows the established module pattern in `vox_core` — top-level `injector.rs` as module root with submodules in `injector/` directory. Matches `vad.rs`+`vad/`, `asr.rs`+`asr/`, `llm.rs`+`llm/` structure. No `mod.rs` per CLAUDE.md coding guidelines.

## Design Decisions

### D1: Platform Dispatch via `#[cfg]` (Not Trait Objects)

The public `inject_text()` and `execute_command()` functions in `injector.rs` use compile-time `#[cfg(target_os)]` to dispatch to the platform implementation. This matches the existing codebase pattern and avoids runtime overhead from dynamic dispatch. The trade-off is that cross-platform tests can't test both implementations on a single platform — but that's correct because the implementations call OS-specific APIs.

### D2: InjectionResult Instead of Result<()>

`inject_text` returns `InjectionResult` (success or blocked-with-text) instead of `Result<()>` because a "blocked" injection is a normal operational outcome (not an unexpected error) and the caller needs the original text back for buffering. `execute_command` returns `Result<()>` because command failures are unexpected errors.

### D3: Pre-Check UIPI Before SendInput

Research confirmed that `SendInput`'s return value is unreliable for UIPI detection (sometimes returns success while silently dropping events). The implementation pre-checks elevation via `TokenElevation` before calling `SendInput`. If `OpenProcess` fails with access denied, conservatively assume elevated.

### D4: macOS Chunking on UTF-16 Code Units

The design doc's chunking approach (`text.as_bytes().chunks(20)`) operates on UTF-8 bytes, which is incorrect — the CGEvent limit is 20 UTF-16 code units. The implementation encodes to `Vec<u16>` first, then walks in steps of 20, checking for high surrogates at chunk boundaries.

### D5: VoiceCommand Consumed, Not Redefined

The `VoiceCommand` struct is already defined in `llm::processor`. The injector module imports and uses it directly — no re-export, no wrapper, no duplication. The `commands.rs` module takes `&VoiceCommand` as input.

### D6: InjectionBuffer Owned by Caller

`InjectionBuffer` is a simple data struct. The injector module defines it but does not manage buffer state — that responsibility belongs to the pipeline/UI layer that will call `inject_text()` and handle the `Blocked` variant. This keeps the injector stateless and testable.

## Testing Strategy

### Unit Tests (cross-platform, no OS API calls)

| Test | Module | What It Tests |
|---|---|---|
| `test_command_mapping_all_known` | `commands.rs` | All 8 commands are recognized and don't error |
| `test_command_mapping_unknown` | `commands.rs` | Unknown command returns error |
| `test_empty_text_noop` | `injector.rs` | Empty string returns Success immediately |
| `test_whitespace_text_valid` | `injector.rs` | Whitespace-only text is accepted (not treated as empty) |

### Platform-Specific Unit Tests (require OS API access)

| Test | Platform | What It Tests |
|---|---|---|
| `test_utf16_chunking` | All (pure logic) | Text >20 chars chunks correctly |
| `test_utf16_chunking_surrogate` | All (pure logic) | Emoji near chunk boundary not split |
| `test_utf16_chunking_exact_20` | All (pure logic) | Exactly 20-char text = single chunk |
| `test_utf16_chunking_empty` | All (pure logic) | Empty text = empty chunks |
| `test_inject_text_basic` | Windows (`#[cfg]`) | SendInput call with short text succeeds |
| `test_inject_text_unicode` | Windows (`#[cfg]`) | Emoji/CJK encode correctly to UTF-16 |

Note: The macOS `chunk_utf16` function is pure logic (no OS calls) and can be tested on any platform. We'll make it `pub(crate)` for testing and write platform-independent tests. The platform `#[cfg]` is only on the OS API call tests.

### Integration Tests (manual)

Per spec — see `specs/006-text-injection/spec.md` User Stories 1-5 for the full manual testing matrix across Notepad, VS Code, Chrome, terminal, Slack.

## Cargo.toml Changes

```toml
# In crates/vox_core/Cargo.toml — update windows features:
[target.'cfg(target_os = "windows")'.dependencies]
windows = { version = "0.62", features = [
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
    "Win32_System_Threading",     # NEW: OpenProcess, OpenProcessToken
    "Win32_Security",             # NEW: GetTokenInformation, TOKEN_ELEVATION
] }
```

No changes needed for macOS deps — `objc2 = "0.6"` and `objc2-core-graphics = "0.3"` default features include all needed CGEvent functionality.

## Complexity Tracking

> No Constitution violations. No complexity tracking entries needed.

## Artifacts Generated

| Artifact | Path | Content |
|---|---|---|
| Plan | `specs/006-text-injection/plan.md` | This file |
| Research | `specs/006-text-injection/research.md` | API research: SendInput, CGEvent, UIPI |
| Data Model | `specs/006-text-injection/data-model.md` | Entities, types, command mapping |
| API Contract | `specs/006-text-injection/contracts/injector-api.md` | Public interface specification |
| Quickstart | `specs/006-text-injection/quickstart.md` | Build/test guide |
