# Research: System Tray & Global Hotkeys

**Feature**: 014-tray-hotkeys
**Date**: 2026-02-24

## R-001: Default Hotkey — Keep Ctrl+Shift+Space

**Decision**: Keep the default hotkey as "Ctrl+Shift+Space". Do not change to CapsLock.

**Rationale**: CapsLock has undesirable side effects — it toggles caps lock state on every OS, which is confusing and requires platform-specific suppression logic. Ctrl+Shift+Space is already the established default from 011-gpui-app-shell, is unlikely to conflict with other applications, and works reliably with global-hotkey's `RegisterHotKey` / CGEvent tap mechanisms. Users who prefer CapsLock can remap via the Settings panel (FR-011).

**Alternatives considered**:
- CapsLock: Requires OS-level suppression of toggle behavior on Windows, confusing for users who actually use CapsLock
- F13–F24: Not present on standard keyboards
- Single modifier key (e.g., Right Alt): Varies by keyboard layout

**Migration**: None needed. The default remains `"Ctrl+Shift+Space"` — same as 011-gpui-app-shell. Existing users keep their current binding.

## R-002: Press/Release Events for Hold-to-Talk

**Decision**: Use `GlobalHotKeyEvent.state` field (`HotKeyState::Pressed` / `HotKeyState::Released`) to detect press vs. release.

**Rationale**: global-hotkey 0.6 exposes `HotKeyState` via the `state` field on `GlobalHotKeyEvent`. The current implementation (main.rs line 408) ignores this field, treating every event as a simple toggle. For hold-to-talk mode (press=start, release=stop), both event types are required.

**Platform considerations**:
- **macOS**: CGEvent tap natively supports both key-down and key-up events. Release detection is reliable.
- **Windows**: The underlying mechanism determines release event availability. If `RegisterHotKey` Win32 API is used internally, only press events fire. If a keyboard hook (`WH_KEYBOARD_LL`) is used, both fire. The `HotKeyState::Released` variant exists in the crate API.

**Verification approach**: Test hold-to-talk on Windows during implementation. If release events do not fire on Windows:
1. Install a supplementary `WH_KEYBOARD_LL` hook via the `windows` crate (already a dependency for text injection in `crates/vox_core/src/injector/windows.rs`)
2. The hook detects key-up for the registered hotkey and sends a `Released` event to the same crossbeam channel
3. The hook suppresses CapsLock's normal toggle behavior (by not calling `CallNextHookEx`)
4. This is platform-specific code using `#[cfg(target_os = "windows")]`, which is standard Rust platform detection — not a feature flag (compliant with Constitution Principle XI)

**Alternatives considered**:
- Replace global-hotkey entirely with custom implementation: Too risky, the crate handles cross-platform registration well
- Use `rdev` crate for key events: Adds another dependency; prefer using existing `windows` crate
- Only support toggle mode on Windows: Violates Constitution Principle VI (Scope Only Increases)

## R-003: Dynamic Tray Icon Updates

**Decision**: Use `TrayIcon::set_icon()` and `TrayIcon::set_tooltip()` to update the tray icon reactively on state changes. Pre-decode all five icon variants at startup.

**Rationale**: tray-icon 0.19 provides mutation methods on the `TrayIcon` instance:
- `set_icon(Some(icon))` — replaces the displayed icon
- `set_tooltip(Some("text"))` — updates the hover tooltip

All five icon variants are pre-decoded from `include_bytes!()` PNGs using the existing `decode_png_icon()` function (main.rs lines 635–642) and cached as `Icon` values. On state change, the appropriate cached icon is applied — no re-decoding per update.

**Icon assets**:
- Four icons already exist: `tray-idle.png` (gray), `tray-listening.png` (green), `tray-processing.png` (blue), `tray-error.png` (red)
- One new icon needed: `tray-downloading.png` (orange, 32×32 RGBA PNG)
- The new icon will be generated programmatically using the `png` crate at build time or created as a static asset matching the existing icon style

**Communication pattern**: The `TrayIcon` instance lives in the tray polling GPUI task (main.rs line 461). State changes are communicated via a `std::sync::mpsc::Sender<TrayUpdate>` channel from the state-forwarding code to the tray task. The tray task polls this channel alongside `MenuEvent` polling in the same loop.

**Alternatives considered**:
- Store TrayIcon as GPUI global: `TrayIcon` may not be `Send+Sync` on macOS (Cocoa main-thread constraint). Safer to keep in dedicated task and communicate via channel.
- Re-create TrayIcon on each state change: Causes icon flicker and is wasteful.
- Use GPUI Entity for TrayManager: Would require TrayIcon to be stored in entity state, which requires `Send`. Channel approach avoids this constraint entirely.

## R-004: Activation Mode Settings

**Decision**: Replace `hold_to_talk: bool` and `hands_free_double_press: bool` with `activation_mode: ActivationMode` (serialized as `"hold-to-talk"` | `"toggle"` | `"hands-free"`).

**Rationale**: Two booleans create four combinations but only three valid modes. A single enum eliminates the ambiguous state. Serialized as lowercase kebab-case via `#[serde(rename_all = "kebab-case")]`.

**Alternatives considered**:
- Numeric enum (0/1/2): Less readable in settings.json
- Keep booleans alongside new field: Redundancy

## R-005: Hands-Free Double-Press Detection

**Decision**: Pure timestamp comparison in `HotkeyInterpreter` — no timer or async callback needed.

**Rationale**: The 300ms detection window is implemented by comparing `Instant::now()` with the timestamp of the last press event. If elapsed < 300ms, it's a double-press → `StartHandsFree`. If elapsed >= 300ms, it's a new first press (the old one was implicitly discarded). This is simpler and more testable than timer-based approaches.

**State machine (hands-free mode)**:

| Current State | Event | Elapsed Since Last Press | Action |
|---------------|-------|--------------------------|--------|
| Not recording | Press | N/A (first press) | Record timestamp → None |
| Not recording | Press | < 300ms | Reset timestamp → StartHandsFree |
| Not recording | Press | >= 300ms | Record new timestamp → None |
| Recording | Press | Any | StopRecording |

**Edge case — rapid triple press**: After double-press triggers `StartHandsFree` (presses 1+2), `last_press_time` is reset to a stale value (> 300ms ago). Press 3 sees elapsed > 300ms, starts a new detection cycle. If recording is now active, press 3 stops recording instead. This is correct behavior.

**Alternatives considered**:
- Timer callback after 300ms: Requires async infrastructure, harder to unit test
- Polling-based `tick()` method: Adds complexity for no benefit since "discard" is implicit
- External debounce crate: Over-engineering for a single timing comparison

## R-006: Hotkey Remapping at Runtime

**Decision**: Unregister the old hotkey, parse the new key string, register the new hotkey. Communicate the new hotkey ID to the polling task via `Arc<AtomicU32>`.

**Rationale**: global-hotkey's `GlobalHotKeyManager` provides `unregister(hotkey)` to remove a registration and `register(hotkey)` to add a new one. The hotkey ID (`u32`) changes when the binding changes, so the polling task must be informed.

**Implementation flow**:
1. User records new hotkey in Settings → `HotkeyRecorder` captures key string
2. Settings update callback writes new `activation_hotkey` to settings.json
3. Callback also sends a re-registration request to the hotkey polling task via a channel
4. Polling task: unregisters old hotkey, parses new string, registers new hotkey
5. Polling task updates `Arc<AtomicU32>` with new hotkey ID for event matching
6. Old hotkey binding is immediately deregistered (FR-012)

**Why the polling task does the re-registration**: The `GlobalHotKeyManager` was created in that task and must be mutated from the same context. Sending the request via channel keeps the manager's lifetime simple.

**Alternatives considered**:
- Recreate the polling task: Disruptive, risk of dropped events during restart
- Store manager in a global: Send/Sync concerns on macOS
- Use crossbeam channel: std mpsc is sufficient and already available

## R-007: Tray Context Menu Expansion

**Decision**: Expand from 3 items (Toggle Recording, Settings, Quit) to 6 items with separator.

**New menu layout**:

| Position | Item | Action |
|----------|------|--------|
| 1 | Toggle Recording | Dispatches `ToggleRecording` action (simple toggle, bypasses mode mechanics) |
| 2 | Settings | Dispatches `OpenSettings` action |
| 3 | Show/Hide Overlay | Dispatches `ToggleOverlay` action (already defined in key_bindings.rs) |
| — | separator | `PredefinedMenuItem::separator()` |
| 4 | About Vox | Shows version dialog |
| 5 | Quit Vox | Calls `cx.quit()` |

**"Toggle Recording" behavior**: Per spec clarification (Q2), this item always acts as a simple start/stop toggle regardless of the active activation mode. It bypasses mode-specific mechanics (no double-press required even in hands-free mode).

**"About Vox" implementation**: Displays a small native-style dialog with version information. Implementation options (decided at task execution):
- GPUI window (consistent with app's UI framework)
- Platform-native message box via `windows::Win32` / `objc2` (simpler, less code)

**Menu item text updates**: "Toggle Recording" text can be updated dynamically via `MenuItem::set_text()` to show "Start Recording" or "Stop Recording" based on current state. This provides additional context beyond the tray icon.

**Alternatives considered**:
- Submenu for activation mode selection: Over-complicates the tray menu; mode selection belongs in Settings
- Include "Restart Pipeline" item: Not in spec, unnecessary scope

## R-008: Polling Interval Reduction

**Decision**: Reduce hotkey and tray polling interval from 50ms to 5ms.

**Rationale**: SC-001 requires "hotkey event detection to action dispatch within 5ms." The current 50ms polling adds up to 50ms latency before an event is processed. At 5ms, worst-case latency is 5ms, average is 2.5ms. The 200 iterations/second of `try_recv()` on a crossbeam channel is negligible CPU cost (no syscalls, no allocations, just atomic pointer checks).

**CPU impact**: At 5ms interval, the foreground executor wakes 200 times/second. Each wake is ~100ns of work (channel check + timer reschedule). Total CPU overhead: ~0.002% — well within the <2% idle CPU budget (Constitution Principle II).

**Alternatives considered**:
- 1ms polling: Marginal improvement (1ms avg vs 2.5ms), slightly more wakeups
- Blocking receive on background thread + dispatch to foreground: More complex architecture for same result
- Platform-specific event loop integration: Too much complexity for marginal latency gain

## R-009: Universal Hotkey Response in Non-Ready States

**Decision**: The hotkey action handler checks `AppReadiness` before `PipelineState`. In non-ready states, it shows the overlay with the current readiness status instead of starting/stopping recording.

**Rationale**: Per Constitution Principle V ("The hotkey MUST respond in every app state") and FR-010 ("silent failure is forbidden"), pressing the hotkey must always produce visible feedback. The overlay already renders download progress, loading stages, and errors — the hotkey just needs to ensure the overlay is visible.

**Implementation**:
```
on_hotkey_press:
  match readiness:
    Downloading → show overlay (renders download progress)
    Loading     → show overlay (renders loading stage)
    Error       → show overlay (renders error with guidance)
    Ready       → interpret via HotkeyInterpreter, dispatch action
```

The overlay's `OverlayDisplayState` global already maps `AppReadiness` to the correct rendering. The hotkey handler just ensures the overlay window is visible via `ToggleOverlay` (if hidden).

**Alternatives considered**:
- Show a separate notification/toast: Inconsistent with overlay-based feedback model
- Play a sound: Not in spec, adds audio dependency
- Queue the recording for when ready: Violates user expectation of immediate feedback
