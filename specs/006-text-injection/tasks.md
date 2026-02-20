# Tasks: Text Injection

**Input**: Design documents from `/specs/006-text-injection/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/injector-api.md

**Tests**: Explicitly requested in the feature specification. Unit tests (cross-platform pure logic) and platform-specific tests (require OS API access) are included per the plan's testing strategy.

**Organization**: Tasks grouped by user story. The 5 user stories share 4 implementation files heavily — US1/US3/US4 all exercise `inject_text_impl`, US2 exercises `execute_command`. The foundational phase builds complete platform backends; per-story phases focus on tests that validate specific behaviors. US3 (Unicode) and US5 (macOS Chunking) share a phase because their tests validate different aspects of the same UTF-16 encoding path.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2)
- All file paths relative to repository root

---

## Phase 1: Setup

**Purpose**: Add missing Cargo dependencies and populate the module root with public types and API

- [ ] T001 Add `Win32_System_Threading` and `Win32_Security` features to `windows` dependency in crates/vox_core/Cargo.toml (needed for OpenProcess, OpenProcessToken, GetTokenInformation, TOKEN_ELEVATION)
- [ ] T002 [P] Populate crates/vox_core/src/injector.rs with `InjectionResult` enum (Success, Blocked { reason: InjectionError, text: String }), `InjectionError` enum (ElevatedTarget, NoFocusedWindow, PlatformError(String)), `InjectionBuffer` struct (text: String, error: InjectionError, timestamp: Instant), submodule declarations (`mod windows`/`mod macos`/`mod commands` with `#[cfg]` gates), public `inject_text(text: &str) -> InjectionResult` (empty-string check → Success, then cfg-dispatch to platform impl) and `execute_command(command: &VoiceCommand) -> Result<()>` (cfg-dispatch to commands::execute_command), `//!` module doc and `///` doc comments on all pub items

---

## Phase 2: Foundational — Platform Backends

**Purpose**: Implement ALL platform-specific injection code across 3 files. These form the backbone that all user stories share.

**CRITICAL**: No user story tests can pass until this phase is complete.

- [ ] T003 [P] Implement Windows text injection and UIPI detection in crates/vox_core/src/injector/windows.rs — `is_foreground_elevated() -> Result<bool>` using GetForegroundWindow → GetWindowThreadProcessId → OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION) → OpenProcessToken(TOKEN_QUERY) → GetTokenInformation(TokenElevation) → CloseHandle cleanup, access-denied on OpenProcess → return true (assume elevated), null HWND → no-focus signal. `inject_text_impl(text: &str) -> InjectionResult` with null byte stripping (filter U+0000), GetForegroundWindow no-focus check → Blocked(NoFocusedWindow), is_foreground_elevated UIPI pre-check → Blocked(ElevatedTarget), UTF-16 encoding via encode_utf16(), INPUT array construction (KEYEVENTF_UNICODE key-down + KEYEVENTF_UNICODE|KEYEVENTF_KEYUP per code unit, wVk=VIRTUAL_KEY(0), wScan=code_unit), single SendInput call, return value validation (0 → Blocked(PlatformError))
- [ ] T004 Implement Windows keyboard helpers in crates/vox_core/src/injector/windows.rs — `send_shortcut(modifier: VIRTUAL_KEY, key: VIRTUAL_KEY) -> Result<()>` sending 4-event atomic sequence (modifier-down, key-down, key-up, modifier-up with KEYEVENTF_KEYUP) via single SendInput call for atomicity, `send_key(key: VIRTUAL_KEY) -> Result<()>` sending 2-event key-down/key-up, both verifying SendInput return count
- [ ] T005 [P] Implement macOS UTF-16 chunking and text injection in crates/vox_core/src/injector/macos.rs — `pub(crate) fn chunk_utf16(text: &str) -> Vec<Vec<u16>>` encoding to Vec<u16> via encode_utf16(), walking in steps of 20 code units, checking for high surrogates (0xD800..=0xDBFF) at chunk boundaries and shortening chunk by 1 to keep surrogate pairs together. `inject_chunk(utf16: &[u16]) -> Result<()>` using CGEventSource::new(HIDSystemState), CGEvent::new_keyboard_event(source, 0, true), CGEvent::keyboard_set_unicode_string(event, len, ptr) (unsafe raw pointer API), CGEvent::post(HIDEventTap), key-up event. `inject_text_impl(text: &str) -> InjectionResult` with AXIsProcessTrusted Accessibility check → Blocked(PlatformError("Accessibility permission not granted")), no-focus check, null byte stripping, chunk_utf16 + inject_chunk loop → Success or Blocked on any chunk failure
- [ ] T006 Implement macOS keyboard helpers and key code constants in crates/vox_core/src/injector/macos.rs — key code constants: RETURN=0x24, TAB=0x30, BACKSPACE=0x33, KEY_A=0x00, KEY_C=0x08, KEY_V=0x09, KEY_Z=0x06. `send_shortcut(flags: CGEventFlags, keycode: CGKeyCode) -> Result<()>` creating CGEvent key-down, setting flags via CGEvent::set_flags, posting, then key-up event. `send_key(keycode: CGKeyCode) -> Result<()>` posting key-down + key-up without modifier flags
- [ ] T007 Implement cross-platform voice command dispatch in crates/vox_core/src/injector/commands.rs — `pub fn execute_command(command: &VoiceCommand) -> Result<()>` matching on command.cmd.as_str() for all 8 commands: delete_last → Ctrl+Backspace (Windows) / Option+Backspace (macOS), undo → Ctrl+Z / Cmd+Z, select_all → Ctrl+A / Cmd+A, newline → Enter / Return, paragraph → Enter×2 / Return×2, copy → Ctrl+C / Cmd+C, paste → Ctrl+V / Cmd+V, tab → Tab / Tab. `#[cfg(target_os)]` dispatch to platform send_shortcut/send_key. Unknown command → `Err(anyhow!("Unknown command: {}", command.cmd))`. Doc comments on pub fn.

**Checkpoint**: All injection and command code implemented. Platform backends ready for all user stories.

---

## Phase 3: US1 — Dictated Text Appears in Active Application (Priority: P1) MVP

**Goal**: Basic text injection works — ASCII text appears in the focused application via simulated keyboard input.

**Independent Test**: Call `inject_text("Hello, world.")` with Notepad focused, verify text appears at cursor.

### Tests for User Story 1

- [ ] T008 [P] [US1] Unit tests in crates/vox_core/src/injector.rs — `test_empty_text_noop` (inject_text("") returns InjectionResult::Success immediately without calling platform impl), `test_whitespace_text_valid` (inject_text("  \t\n") is accepted as valid input, not treated as empty — proceeds to platform impl; assert result is NOT Success, e.g. Blocked(NoFocusedWindow), proving the empty-string short-circuit was bypassed)
- [ ] T009 [US1] Windows platform test in crates/vox_core/src/injector/windows.rs — `test_inject_text_basic` (inject short ASCII text "Hello", verify SendInput is called and returns expected event count matching 2× UTF-16 code unit count)

**Checkpoint**: Text injection works for basic ASCII on Windows. This is the MVP.

---

## Phase 4: US2 — Voice Commands Execute as Keyboard Shortcuts (Priority: P1)

**Goal**: All 8 voice commands map to correct keyboard shortcuts and execute without error.

**Independent Test**: Call `execute_command` for "delete_last", "undo", "newline" — verify no error and correct keystrokes sent.

### Tests for User Story 2

- [ ] T010 [US2] Command mapping tests in crates/vox_core/src/injector/commands.rs — `test_command_mapping_all_known` (execute_command for all 8 commands: delete_last, undo, select_all, newline, paragraph, copy, paste, tab — each returns Ok(())), `test_command_mapping_unknown` (execute_command with cmd="foobar" returns Err containing "Unknown command: foobar")

**Checkpoint**: All 8 voice commands execute correctly. Combined with US1, the core pipeline is complete.

---

## Phase 5: US3 — Unicode and Special Characters (Priority: P2) + US5 — macOS Text Chunking (Priority: P2)

**Goal**: Full Unicode support verified — accented, CJK, emoji, typographic characters inject correctly. macOS chunking logic handles surrogate pairs at boundaries without corruption.

**Independent Test**: Inject "café — naïve 'quotes' 🚀 你好" into text editor, verify all characters appear. Verify chunk_utf16 doesn't split emoji surrogate pairs.

### Tests for User Story 3 + User Story 5

- [ ] T011 [P] [US3] Windows Unicode test in crates/vox_core/src/injector/windows.rs — `test_inject_text_unicode` (inject emoji 🚀 and CJK 你好, verify UTF-16 encoding produces correct surrogate pairs for emoji and BMP code units for CJK, SendInput returns expected event count)
- [ ] T012 [P] [US5] Pure logic chunking tests (platform-independent, NO #[cfg] gate — chunk_utf16 is pure logic) in crates/vox_core/src/injector/macos.rs — `test_utf16_chunking` (30-char ASCII text → 2 chunks, first has 20 code units, second has 10), `test_utf16_chunking_surrogate` (string with emoji 🚀 at UTF-16 position 19 → emoji's surrogate pair not split, chunk shortened to 19 and pair goes to next chunk), `test_utf16_chunking_exact_20` (exactly 20-char ASCII text → single chunk of 20), `test_utf16_chunking_empty` (empty text → empty Vec)

**Checkpoint**: Unicode injection verified on Windows. Chunking logic verified on all platforms.

---

## Phase 6: US4 — Injection Failure Recovery (Priority: P2)

**Goal**: When injection fails (elevated target, no focus, permission denied), text is preserved in the Blocked result for caller-side buffering.

**Implementation Note**: UIPI detection (`is_foreground_elevated` in T003), no-focused-window detection (null HWND check in T003), and macOS Accessibility check (`AXIsProcessTrusted` in T005) are integral to the platform injection flow — already implemented in Phase 2. `InjectionResult::Blocked` preserves the original text byte-for-byte per SC-005. The `InjectionBuffer` struct is defined in T002.

Per SC-007: detection logic is unit-testable by isolating it from the OS keyboard API. Full failure→buffer→overlay scenarios are validated via manual testing with actual elevated processes (Windows) and revoked permissions (macOS).

**Independent Test**: Run an elevated Command Prompt on Windows, call `inject_text("test")`, verify `InjectionResult::Blocked { reason: ElevatedTarget, text: "test" }` returned with original text preserved.

(No additional automated test tasks — detection logic is embedded in T003/T005. Manual testing covers full scenarios per spec US4 acceptance scenarios 1-6.)

**Checkpoint**: Failure recovery verified via manual testing per the testing matrix in spec.md.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Build verification, test validation, quickstart verification

- [ ] T013 Verify `cargo build -p vox_core --features cuda` produces zero compiler warnings
- [ ] T014 Run `cargo test -p vox_core --features cuda` and verify all tests pass (unit + platform-specific)
- [ ] T015 Run quickstart.md validation — verify build and test commands from specs/006-text-injection/quickstart.md work as documented

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup (T001 for Cargo features, T002 for module root types and dispatch)
- **US1 (Phase 3)**: Depends on Foundational (T003 provides Windows inject_text_impl)
- **US2 (Phase 4)**: Depends on Foundational (T007 provides execute_command, T004 provides send_shortcut/send_key)
- **US3+US5 (Phase 5)**: Depends on Foundational (T003 for Windows inject, T005 for chunk_utf16)
- **US4 (Phase 6)**: Depends on Foundational (T003/T005 provide detection logic) — manual testing only
- **Polish (Phase 7)**: Depends on all implementation and test phases complete

### User Story Dependencies

- **US1 (P1)**: Can start after Foundational — no dependencies on other stories
- **US2 (P1)**: Can start after Foundational — no dependencies on other stories
- **US3+US5 (P2)**: Can start after Foundational — independent of US1/US2
- **US4 (P2)**: Implementation complete in Foundational — manual testing anytime after Phase 2

### Within Each Phase

- Windows tasks (T003 → T004) are sequential (same file: windows.rs)
- macOS tasks (T005 → T006) are sequential (same file: macos.rs)
- Commands (T007) depends on T004 and T006 being available (calls platform helpers)
- Test phases: tasks marked [P] can run in parallel

### Parallel Opportunities

- **Phase 1**: T001 || T002 (different files: Cargo.toml vs injector.rs)
- **Phase 2**: (T003 → T004) || (T005 → T006), then T007 after both streams complete
- **Phase 3-5**: US1, US2, US3+US5 can all proceed in parallel after Phase 2 — no cross-story dependencies
- **Phase 5**: T011 || T012 (different test files: windows.rs vs macos.rs)

---

## Parallel Example: Phase 2

```bash
# Stream A (Windows) and Stream B (macOS) run in parallel:
# Stream A:
Task: "Implement Windows text injection + UIPI detection in injector/windows.rs" (T003)
Task: "Implement Windows keyboard helpers in injector/windows.rs" (T004)

# Stream B (parallel with Stream A):
Task: "Implement macOS chunking + text injection in injector/macos.rs" (T005)
Task: "Implement macOS keyboard helpers in injector/macos.rs" (T006)

# After both streams complete:
Task: "Implement cross-platform command dispatch in injector/commands.rs" (T007)
```

---

## Implementation Strategy

### MVP First (US1 Only)

1. Complete Phase 1: Setup (T001-T002)
2. Complete Phase 2: Foundational — minimum T003-T004 for Windows backend
3. Complete Phase 3: US1 tests (T008-T009)
4. **STOP and VALIDATE**: Run `cargo test -p vox_core --features cuda` — basic injection works

### Incremental Delivery

1. Setup + Foundational → Platform backends ready
2. Add US1 → Basic text injection works → **MVP!**
3. Add US2 → Voice commands work → Core pipeline complete (P1 stories done)
4. Add US3+US5 → Unicode and chunking verified → Full encoding coverage
5. US4 → Manual failure recovery verification → Graceful degradation confirmed
6. Polish → Zero warnings, all tests pass

---

## Notes

- All source files are in `crates/vox_core/src/injector/` (windows.rs, macos.rs, commands.rs) + module root `crates/vox_core/src/injector.rs`
- All tests are in the same files as implementation (`#[cfg(test)] mod tests`)
- macOS `chunk_utf16` tests are platform-independent (pure logic, no OS calls) despite being in macos.rs — no `#[cfg]` gate needed
- US4 (Failure Recovery) has no automated test tasks — detection logic is tested implicitly by the injection flow; full scenarios per SC-007 require manual testing with actual elevated processes and permission states
- macOS code is written but cannot be tested on Windows CI — macOS CI runner required per SC-006
- ~500 LOC estimated across 4 files per plan.md
- NFR-003 (structured logging at DEBUG/WARN) is a SHOULD requirement — logging instrumentation is deferred to pipeline integration when the tracing subscriber is configured. Implementation tasks include the log points as part of natural error handling but no dedicated logging task is needed.
