# Feature Specification: Text Injection

**Feature Branch**: `006-text-injection`
**Created**: 2026-02-20
**Status**: Draft
**Input**: User description: "OS-level text injection — final pipeline stage. Takes LLM output and simulates keyboard input via SendInput (Windows) / CGEvent (macOS). Voice command execution via keystroke mapping."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Dictated Text Appears in Active Application (Priority: P1)

A user speaks a sentence while any text-capable application has focus (Notepad, VS Code, Chrome, terminal, Slack). After the pipeline processes the speech (VAD → ASR → LLM), the polished text appears at the cursor position in the active application, as if the user had typed it on the keyboard.

**Why this priority**: This is the core value proposition of Vox. Without text injection, the entire pipeline produces output that goes nowhere. Every other feature depends on this working correctly.

**Independent Test**: Dictate "let's meet tomorrow at three PM" with Notepad focused. Verify "Let's meet tomorrow at 3 PM." appears at the cursor. Repeat with VS Code, Chrome compose window, and terminal.

**Acceptance Scenarios**:

1. **Given** Notepad is focused with cursor in an empty document, **When** the pipeline delivers polished text "Hello, world.", **Then** "Hello, world." appears at the cursor position in Notepad.
2. **Given** VS Code is focused with cursor mid-file, **When** the pipeline delivers polished text, **Then** the text is inserted at the cursor position without disrupting existing content.
3. **Given** Chrome with Gmail compose open, **When** the pipeline delivers polished text, **Then** the text appears in the compose body at the cursor.
4. **Given** a terminal window is focused, **When** the pipeline delivers polished text, **Then** the text appears at the command prompt.

---

### User Story 2 - Voice Commands Execute as Keyboard Shortcuts (Priority: P1)

A user speaks a voice command like "delete that", "undo that", or "new line". The LLM post-processor returns a structured JSON command. The injector maps this command to the appropriate OS-level keyboard shortcut and executes it, producing the same result as if the user had pressed the keys.

**Why this priority**: Voice commands are essential for hands-free editing. Without them, users cannot correct mistakes or navigate without switching to keyboard, breaking the dictation flow.

**Independent Test**: Say "delete that" with Notepad focused after typing some text. Verify the last word is deleted (Ctrl+Backspace on Windows). Say "undo that" — verify the deletion is reversed (Ctrl+Z). Say "new line" — verify a line break is inserted (Enter).

**Acceptance Scenarios**:

1. **Given** text "Hello world" is in Notepad with cursor at end, **When** the command `{"cmd":"delete_last"}` is received, **Then** Ctrl+Backspace is simulated and "world" is deleted.
2. **Given** text was just deleted, **When** the command `{"cmd":"undo"}` is received, **Then** Ctrl+Z is simulated and the deletion is undone.
3. **Given** cursor is at end of a line, **When** the command `{"cmd":"newline"}` is received, **Then** Enter is simulated and a new line is created.
4. **Given** cursor is at end of a line, **When** the command `{"cmd":"paragraph"}` is received, **Then** Enter is simulated twice, creating a blank line between paragraphs.
5. **Given** any text field is focused, **When** the command `{"cmd":"select_all"}` is received, **Then** Ctrl+A (Windows) / Cmd+A (macOS) selects all text.
6. **Given** text is selected, **When** `{"cmd":"copy"}` is received, **Then** Ctrl+C / Cmd+C copies to clipboard.
7. **Given** clipboard has content, **When** `{"cmd":"paste"}` is received, **Then** Ctrl+V / Cmd+V pastes from clipboard.
8. **Given** a text field is focused, **When** `{"cmd":"tab"}` is received, **Then** Tab key is simulated.

---

### User Story 3 - Unicode and Special Characters (Priority: P2)

A user dictates text containing accented characters, CJK characters, emoji, or special punctuation (curly quotes, em-dashes, ellipses). The injector correctly encodes and injects all Unicode characters into the active application.

**Why this priority**: International users and common English formatting (smart quotes, em-dashes) require full Unicode support. Without it, the injector would be limited to ASCII, breaking the LLM's formatted output.

**Independent Test**: Inject the string "café — naïve 'quotes' 🚀" into Notepad. Verify all characters render correctly: accented e, em-dash, diaeresis, curly quotes, and rocket emoji.

**Acceptance Scenarios**:

1. **Given** any text field is focused, **When** text containing accented characters (e.g., "café", "naïve") is injected, **Then** the accented characters appear correctly.
2. **Given** any text field is focused, **When** text containing CJK characters is injected, **Then** the CJK characters appear correctly.
3. **Given** any text field is focused, **When** text containing emoji (e.g., 🚀, 👍) is injected, **Then** the emoji appear correctly.
4. **Given** any text field is focused, **When** text containing typographic characters (em-dash, curly quotes, ellipsis) is injected, **Then** these characters appear correctly.

---

### User Story 4 - Injection Failure Recovery (Priority: P2)

Text injection fails because the target application is elevated (admin process on Windows), focus was lost between pipeline completion and injection, or accessibility permissions are missing (macOS). The user sees the buffered text in the overlay with a "Copy" button, allowing them to paste it manually.

**Why this priority**: Graceful failure handling prevents data loss. Without it, dictated text that fails to inject is silently lost, frustrating users.

**Independent Test**: Run an elevated Command Prompt on Windows. Dictate text. Verify the overlay displays the buffered text with a "Copy" button. Click Copy. Manually paste into the elevated prompt. Verify the buffer clears. On macOS, revoke Accessibility permission for Vox, dictate text, and verify the overlay displays the buffered text with guidance to enable Accessibility permission.

**Acceptance Scenarios**:

1. **Given** an elevated process has focus on Windows, **When** text injection is attempted, **Then** the injection detects the UIPI restriction, buffers the text, and shows it in the overlay with a "Copy" button and a message explaining that the target application is running as administrator and keyboard simulation is blocked by Windows security policy.
2. **Given** focus changed between pipeline completion and injection, **When** text injection fails, **Then** the text is buffered and shown in the overlay.
3. **Given** text is buffered in the overlay, **When** the user clicks "Copy", **Then** the text is copied to the clipboard.
4. **Given** text is buffered and focus returns to a non-elevated application, **When** the next focus change event is detected, **Then** the buffered text is retried for injection.
5. **Given** buffered text is successfully injected on retry, **Then** the buffer is cleared and the overlay message is dismissed.
6. **Given** macOS Accessibility permission is not granted for Vox, **When** text injection is attempted, **Then** the injection detects the missing permission, buffers the text, and shows it in the overlay with a "Copy" button and guidance to enable Accessibility in System Settings → Privacy & Security → Accessibility.

---

### User Story 5 - macOS Text Chunking (Priority: P2)

On macOS, when injecting text longer than 20 UTF-16 code units, the injector automatically chunks the text into segments that respect the CGEvent 20 UTF-16 code unit limit. Text containing multi-byte Unicode characters is chunked at code unit boundaries, never splitting a UTF-16 surrogate pair.

**Why this priority**: Without chunking, macOS users would experience truncated or corrupted text for any utterance longer than 20 UTF-16 code units, which is most real-world dictation.

**Independent Test**: Inject a 50-character English string into TextEdit on macOS. Verify all 50 characters appear. Inject a string with emoji (which use UTF-16 surrogate pairs) near chunk boundaries. Verify no corruption.

**Acceptance Scenarios**:

1. **Given** macOS with TextEdit focused, **When** text longer than 20 UTF-16 code units is injected, **Then** the full text appears correctly without truncation.
2. **Given** text containing emoji at positions 19-20, **When** text is chunked, **Then** the emoji is not split across chunks (kept whole in one chunk).
3. **Given** a 100-character string, **When** injected, **Then** the string is split into chunks of at most 20 UTF-16 code units each, all injected sequentially with correct ordering.

---

### Edge Cases

- What happens when text injection is called with an empty string? No-op, return success.
- What happens when a voice command JSON contains an unrecognized command? Return error, surface to UI layer.
- What happens on macOS without Accessibility permission? Injection detects the missing permission (FR-017), returns a blocked result. The UI layer shows guidance to enable it in System Settings → Privacy & Security → Accessibility.
- What happens when the user rapidly dictates multiple utterances? Each injection is queued and executed sequentially per FR-016 to prevent interleaving.
- What happens when the target application does not accept keyboard input (e.g., a read-only field or locked screen)? Injection silently proceeds (OS handles it), text is buffered if `SendInput` reports failure.
- What happens when text contains only whitespace? Inject as-is — whitespace is valid input.
- What happens when the user switches focus mid-injection of a long text (e.g., 500 characters)? Remaining text is delivered to whichever window now has focus. The injector does not track focus changes during a single injection call — atomicity is best-effort at the OS level.
- What happens with rapid-fire voice commands (e.g., "undo that undo that undo that" producing three consecutive commands)? Each command is queued and executed sequentially per FR-016. The OS processes the resulting keystrokes in order.
- What happens when the pipeline delivers text while a previous injection is still in progress? The new injection is queued behind the in-progress one and executed after it completes, per FR-016.
- What happens when a voice command is received but no text-capable field has focus (e.g., user is on the desktop)? The injector detects no focused window (FR-018) and returns a blocked result with `NoFocusedWindow` reason. The command is not sent to the OS.
- What happens when text contains null bytes or control characters (ASCII 0-31)? Null bytes (U+0000) are stripped before injection to avoid truncation in C-based OS APIs. Other control characters are injected as-is via the OS keyboard API; the receiving application determines how to handle them.
- What happens with right-to-left (RTL) text (Arabic, Hebrew) or bidirectional text? RTL and bidirectional text is injected as-is via Unicode code points. The OS and receiving application handle directionality rendering. The injector does not modify or reorder text.
- What happens with emoji sequences (skin tone modifiers, ZWJ sequences like 👨‍👩‍👧) that span multiple Unicode code points? Emoji sequences are encoded as UTF-16 and injected in code-point order. On macOS, the chunking algorithm never splits a surrogate pair, but it may split a ZWJ sequence across chunks — the OS reassembles the rendered glyph from the code point sequence.
- What happens when text contains tab characters (`\t`) vs the `tab` voice command? Tab characters embedded in text are injected as literal tab keystrokes via Unicode injection (FR-001). The `tab` voice command simulates a Tab key press via virtual key code (FR-007). These are distinct code paths and do not conflict.
- What happens with very long text (e.g., 10,000+ characters)? There is no maximum text length enforced by the injector. Very long text is processed identically — on Windows via a single `SendInput` call with the full INPUT array, on macOS via sequential chunks. Latency scales linearly with text length.

## Clarifications

### Session 2026-02-20

- Q: Should `delete_last` on macOS use Option+Delete (forward word deletion) or Option+Backspace (backward word deletion)? → A: Option+Backspace (backward delete, macOS keycode 0x33) — matches the semantic intent of "delete that" (undo what was just dictated) and aligns with Windows Ctrl+Backspace behavior. Note: macOS keycode 0x33 is the key labeled "delete" on Mac keyboards, which functions as Backspace. Forward delete is keycode 0x75.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST inject arbitrary UTF-8 encoded text (Rust `&str`) into the currently focused application by simulating OS-level keyboard input.
- **FR-002**: System MUST use `SendInput` with `KEYEVENTF_UNICODE` on Windows to inject Unicode text code-unit-by-code-unit (key down + key up per UTF-16 code unit).
- **FR-003**: System MUST use `CGEvent` on macOS to inject text via keyboard event simulation with string-based injection.
- **FR-004**: On macOS, system MUST chunk text into segments of at most 20 UTF-16 code units each, respecting surrogate pair boundaries (never splitting a high-low surrogate pair). Windows has no per-call text length limit and handles the full text in a single API call.
- **FR-005**: System MUST map voice command JSON (from LLM post-processor) to platform-appropriate keyboard shortcuts and execute them.
- **FR-006**: System MUST use platform-appropriate modifier keys for command shortcuts:
  - **FR-006a**: Most commands (`undo`, `select_all`, `copy`, `paste`) use Ctrl as modifier on Windows and Cmd on macOS.
  - **FR-006b**: Exception: `delete_last` uses Ctrl+Backspace on Windows and Option+Backspace (macOS keycode 0x33) on macOS, performing backward word deletion to match the semantic intent of "delete what was just said".
  - **FR-006c**: Commands without modifiers (`newline`, `paragraph`, `tab`) use the same key on both platforms.
- **FR-007**: System MUST support the full command set: `delete_last`, `undo`, `select_all`, `newline`, `paragraph`, `copy`, `paste`, `tab`.
- **FR-008**: System MUST handle full Unicode: ASCII, accented characters, CJK, emoji (including multi-codepoint sequences such as ZWJ and skin tone modifiers), typographic symbols, and bidirectional text. The injector passes all code points through to the OS — it does not interpret, reorder, or filter text based on script or directionality.
- **FR-009**: On Windows, system MUST detect when the foreground process is elevated (UIPI restriction) before attempting injection, and return a blocked result with the original text preserved for buffering.
- **FR-010**: When injection fails, system MUST return the original text and failure reason to the caller via the function return value. The caller (pipeline/UI layer) is responsible for displaying the buffer and providing a copy-to-clipboard option. The clipboard copy operation itself is a UI-layer concern, not the injector's responsibility.
- **FR-011**: The caller SHOULD retry injection when focus changes to a suitable target. The injector itself is stateless — it does not manage buffers or subscribe to focus events. Focus-change detection is the responsibility of the pipeline/UI layer that calls the injector. If a retry also fails, the buffer is preserved — it is never discarded on failure. The buffer is only cleared on successful injection or explicit user dismissal.
- **FR-012**: System MUST return an error for unrecognized voice commands rather than silently ignoring them.
- **FR-013**: Text injection MUST complete within 30ms for a benchmark input of 50 ASCII characters, measured from function call entry to function return (wall-clock time). Latency scales linearly with text length.
- **FR-014**: Command execution MUST complete within 10ms per command, measured from function call entry to function return.
- **FR-015**: System MUST provide a platform-agnostic interface for text injection and command execution. The public API surface is identical on both platforms. The abstraction mechanism (compile-time conditional compilation vs. trait dispatch) is an implementation decision documented in the plan.
- **FR-016**: When multiple injection or command requests arrive concurrently, they MUST be serialized and executed sequentially. The injector processes one request at a time — no interleaving of concurrent injections. The caller is responsible for enforcing serialization (e.g., via a channel or mutex). Queue depth is bounded by the caller's concurrency mechanism, not the injector.
- **FR-017**: On macOS, system MUST detect when Accessibility permission is not granted and return a blocked result with the original text preserved, symmetrically with the Windows UIPI detection in FR-009.
- **FR-018**: System MUST detect when no window has focus and return a blocked result with a no-focused-window reason, on both platforms.

### Voice Command Mapping

All 8 voice commands with their platform-specific key sequences:

| Command | Windows | macOS |
|---|---|---|
| `delete_last` | Ctrl + Backspace | Option + Backspace (keycode 0x33) |
| `undo` | Ctrl + Z | Cmd + Z (keycode 0x06) |
| `select_all` | Ctrl + A | Cmd + A (keycode 0x00) |
| `newline` | Enter | Return (keycode 0x24) |
| `paragraph` | Enter × 2 | Return × 2 |
| `copy` | Ctrl + C | Cmd + C (keycode 0x08) |
| `paste` | Ctrl + V | Cmd + V (keycode 0x09) |
| `tab` | Tab | Tab (keycode 0x30) |

### Key Entities

- **injector module**: The platform-agnostic interface for injecting text and executing commands, exposed as module-level free functions (`inject_text`, `execute_command`). Delegates to OS-specific implementations at compile time via `#[cfg(target_os)]` conditional compilation. There is no `TextInjector` struct — the module itself is the interface.
- **VoiceCommand**: A structured command from the LLM post-processor (`vox_core::llm::processor::VoiceCommand`), containing a `cmd` field (`String`) that maps to a keyboard shortcut and an optional `args` field (`Option<serde_json::Value>`) reserved for future extensibility. Already defined in the LLM module, consumed (not redefined) by the injector.
- **InjectionBuffer**: Holds text that failed to inject, along with the failure reason and timestamp. Owned and managed by the caller (pipeline/UI layer), not the injector. The injector is stateless.

## Non-Functional Requirements

- **NFR-001**: On macOS, `CGEvent` and `CGEventSource` are not thread-safe. Injection functions that use these types MUST be called from a single designated thread. On Windows, `SendInput` is thread-safe but SHOULD be called from a consistent thread for atomicity of multi-keystroke injections.
- **NFR-002**: The `InjectionBuffer` persists until either (a) the buffered text is successfully injected on retry, or (b) the user explicitly dismisses it via the UI. There is no automatic expiry or eviction. Buffer memory is proportional to the text size (typically < 1 KB for a sentence).
- **NFR-003**: Injection events SHOULD be observable via structured logging: injection attempts (text length, platform), injection results (success/blocked with reason), command execution (command name, result), and latency measurements. Log level: injection attempts at DEBUG, failures at WARN.
- **NFR-004**: The injector receives text exclusively from the trusted pipeline (VAD → ASR → LLM). It does not accept input from external or untrusted sources. The pipeline's LLM post-processor sanitizes and structures output before it reaches the injector. Voice commands are a closed set of 8 known commands — unrecognized commands are rejected (FR-012). The text injection path simulates Unicode keystrokes, not arbitrary modifier+key combinations, so it cannot be used to execute keyboard shortcuts via text input.
- **NFR-005**: Performance sub-budgets: Windows UIPI elevation check < 5ms. macOS per-chunk CGEvent post < 2ms per 20 UTF-16 code unit chunk. These sub-budgets ensure the overall 30ms text injection budget (FR-013) is met for typical input.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Dictated text appears correctly in the target application within 30ms of the injector function being called (wall-clock time from function entry to return), for a benchmark input of 50 ASCII characters. Tested across application types: editors, browsers, terminals, chat apps.
- **SC-002**: All 8 voice commands produce the correct keyboard action on both Windows and macOS, verified against the command mapping table.
- **SC-003**: Unicode text injects correctly without corruption. Test corpus: ASCII (`"Hello, world."`), accented (`"café naïve"`), CJK (`"你好世界"`), emoji (`"🚀👍"`), typographic (`"curly 'quotes' — em-dash…"`), ZWJ sequence (`"👨‍👩‍👧"`), and mixed scripts in a single string.
- **SC-004**: On macOS, text of any length injects correctly without truncation, with no surrogate pair splitting at chunk boundaries.
- **SC-005**: When injection fails, the full original text is preserved in the blocked result — byte-for-byte identical to the input. The user can recover this text via the overlay copy-to-clipboard option. "Zero data loss" means every character of every failed injection is recoverable.
- **SC-006**: All unit tests pass on the build platform with zero compiler warnings. Platform-specific tests are gated by `#[cfg(target_os)]` at compile time, so each platform runs its own test set. CI runs Windows tests on Windows runners and macOS tests on macOS runners.
- **SC-007**: UIPI detection (Windows) and Accessibility permission detection (macOS) are unit-testable by isolating the detection logic from the OS keyboard API. The full failure→buffer→overlay scenarios are validated via manual testing with actual elevated processes (Windows) and revoked permissions (macOS).

## Dependencies & Assumptions

- **DEP-001**: The `VoiceCommand` struct is defined in `vox_core::llm::processor` and consumed by the injector. Any changes to `VoiceCommand` fields require coordinated updates to both modules.
- **ASM-001**: Windows `SendInput` accepts arbitrarily large INPUT arrays. No documented system limit exists on the number of INPUT structs per call. For extremely long text (>10,000 characters), the single-call approach is assumed viable. If a platform limit is discovered during implementation, the fallback is chunked `SendInput` calls.
- **ASM-002**: Focus-change events are available to the pipeline/UI layer (not the injector). The injector is stateless and does not subscribe to system events. The caller is responsible for detecting focus changes and triggering retry. This is a pipeline-layer concern, not an injector concern.
- **ASM-003**: macOS Accessibility permission status can be checked programmatically (e.g., via `AXIsProcessTrusted()` or its Rust equivalent). This check is performed before attempting CGEvent injection. If the API is unavailable, the injector falls back to detecting CGEvent creation failure as a proxy.
- **ASM-004**: The macOS CGEvent source state configuration (HIDSystemState, Private, or CombinedSessionState) is an implementation detail chosen during the plan phase. The spec requires only that text injection and command execution work correctly; the event source configuration is not a functional requirement.
