# Feature Specification: System Tray & Global Hotkeys

**Feature Branch**: `014-tray-hotkeys`
**Created**: 2026-02-24
**Status**: Draft
**Input**: User description: "Implement system tray icon and global hotkey system with three activation modes (hold-to-talk, toggle, hands-free), dynamic tray icon states, expanded context menu, and universal hotkey response in every app state."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Hold-to-Talk Dictation (Priority: P1)

A user is typing an email and wants to dictate a sentence. They press and hold the hotkey (Ctrl+Shift+Space by default). The overlay shows "Listening..." and their speech is captured. When they release the key, the speech is processed and injected as text at their cursor position.

**Why this priority**: Hold-to-talk is the most intuitive activation mode for short dictation bursts. It gives users precise control over exactly when recording starts and stops, matching the mental model of a walkie-talkie. This is the default mode and the primary interaction pattern.

**Independent Test**: Can be fully tested by holding the hotkey in any text input, speaking, and releasing. Delivers the core dictation value of the entire application.

**Acceptance Scenarios**:

1. **Given** the pipeline is ready and idle, **When** the user presses and holds the hotkey, **Then** recording begins immediately and the overlay shows "Listening..."
2. **Given** the user is holding the hotkey and speaking, **When** they release the hotkey, **Then** recording stops, audio is processed, and the resulting text is injected at the cursor
3. **Given** the pipeline is ready and idle, **When** the user taps the hotkey briefly (< 100ms hold), **Then** the system still captures the press-release cycle and processes any captured audio (even if minimal)

---

### User Story 2 - Toggle Dictation (Priority: P2)

A user wants to dictate a long paragraph without holding a key the entire time. They press the hotkey once to start recording, speak freely, and press the hotkey again to stop. This mode is better for extended dictation sessions.

**Why this priority**: Toggle mode supports longer dictation sessions where holding a key becomes uncomfortable. It's the second most common activation pattern and serves users who prefer a different interaction style.

**Independent Test**: Can be tested by pressing the hotkey once, speaking a paragraph, pressing again, and verifying the full text appears.

**Acceptance Scenarios**:

1. **Given** the pipeline is ready and idle in toggle mode, **When** the user presses the hotkey once, **Then** recording starts and the overlay shows "Listening..."
2. **Given** recording is active in toggle mode, **When** the user presses the hotkey again, **Then** recording stops and audio is processed
3. **Given** recording is active in toggle mode, **When** the user switches to a different application, **Then** recording continues uninterrupted (the hotkey is global, not app-scoped)

---

### User Story 3 - Hands-Free Continuous Dictation (Priority: P3)

A user wants to dictate continuously while performing other tasks. They double-press the hotkey (two presses within 300ms). The system enters hands-free mode where VAD automatically segments speech into individual utterances, each processed and injected independently. A single press of the hotkey exits hands-free mode.

**Why this priority**: Hands-free mode enables the most advanced use case — continuous dictation without any ongoing key interaction. It depends on VAD segmentation working correctly and is the most complex activation pattern.

**Independent Test**: Can be tested by double-pressing the hotkey, speaking multiple sentences with natural pauses, and verifying each sentence is processed and injected independently.

**Acceptance Scenarios**:

1. **Given** the pipeline is ready and idle in hands-free mode, **When** the user presses the hotkey twice within 300ms, **Then** continuous recording begins with VAD auto-segmentation active
2. **Given** hands-free mode is active, **When** the user pauses between sentences, **Then** each completed utterance is independently processed and injected
3. **Given** hands-free mode is active, **When** the user presses the hotkey once, **Then** continuous recording stops after processing any remaining audio

---

### User Story 4 - Dynamic Tray Status Awareness (Priority: P2)

A user glances at their system tray to check on Vox's status. The tray icon color and tooltip reflect the current state — gray when idle, green when listening, blue when processing, orange when downloading models, red when an error occurred. The user can right-click the tray to toggle recording, open settings, show/hide the overlay, view version info, or quit.

**Why this priority**: The system tray is the persistent visual anchor for Vox. Without state-reflecting icons, users have no way to know Vox's status at a glance without opening the overlay. The expanded context menu provides essential controls.

**Independent Test**: Can be tested by observing the tray icon change through each pipeline state transition, and by exercising each context menu item.

**Acceptance Scenarios**:

1. **Given** the application is running, **When** the pipeline transitions between states (idle, listening, processing, downloading, error), **Then** the tray icon and tooltip update within 10ms to reflect the current state
2. **Given** the tray icon is visible, **When** the user right-clicks, **Then** a context menu appears with: Toggle Recording, Settings, Show Overlay, separator, About Vox, Quit
3. **Given** the user clicks "Toggle Recording" in the tray menu, **Then** recording starts or stops as a simple toggle (regardless of activation mode — no double-press required even in hands-free mode)

---

### User Story 5 - Hotkey Feedback in Non-Ready States (Priority: P1)

A user presses the hotkey while models are still downloading on first launch. Instead of silence, the overlay appears and shows "Models downloading... 43%" with a progress indication. If models are loading, it shows "Loading models..." If an error occurred, it shows the error with guidance. The hotkey always produces visible feedback.

**Why this priority**: Users will press the hotkey the moment they want to dictate, regardless of app readiness. Silent failure is the worst possible UX — users assume the app is broken. Immediate feedback in every state is a constitutional requirement (Principle V).

**Independent Test**: Can be tested by pressing the hotkey at each app readiness stage (downloading, loading, error) and verifying the overlay shows appropriate status.

**Acceptance Scenarios**:

1. **Given** models are downloading, **When** the user presses the hotkey, **Then** the overlay displays download progress (e.g., "Models downloading... 43%")
2. **Given** models are loading onto the GPU, **When** the user presses the hotkey, **Then** the overlay displays "Loading models..."
3. **Given** an error has occurred, **When** the user presses the hotkey, **Then** the overlay displays the error message with actionable guidance
4. **Given** the pipeline is ready, **When** the user presses the hotkey, **Then** recording starts per the active activation mode

---

### User Story 6 - Hotkey Remapping (Priority: P3)

A user finds the default Ctrl+Shift+Space hotkey conflicts with their workflow. They open Settings, navigate to the Hotkey section, click the recorder field, and press their preferred key combination (e.g., Ctrl+Shift+D or F13). The new hotkey takes effect immediately without restarting the application.

**Why this priority**: While most users will use the default hotkey, power users and users with accessibility needs or keyboard layout conflicts need the ability to remap. This builds on the existing hotkey recorder UI in the settings panel.

**Independent Test**: Can be tested by changing the hotkey in settings and verifying the new combination activates recording in another application.

**Acceptance Scenarios**:

1. **Given** the user is in the Settings hotkey section, **When** they click the hotkey recorder and press a new key combination, **Then** the new hotkey is displayed and saved
2. **Given** a new hotkey has been saved, **When** the user presses the new combination in any application, **Then** recording activates per the current mode
3. **Given** a new hotkey has been saved, **When** the user presses the old hotkey, **Then** nothing happens (the old binding is fully deregistered)

---

### Edge Cases

- What happens when the user presses the hotkey while a previous recording is still being processed? A new recording starts immediately; the previous processing continues in the background. The overlay transitions from "Processing..." to "Listening..." for the new session. (See Clarification Q1.)
- What happens when the user rapidly presses the hotkey many times? The system debounces: in hold-to-talk, only the latest press/release cycle matters. In toggle, rapid presses toggle state each time. In hands-free, only double-presses within the 300ms window trigger continuous mode.
- What happens when the hotkey is remapped to a combination already used by another application? The system registers it globally; the other application loses that binding while Vox is running. The user is not warned (this is standard OS behavior for global hotkeys).
- What happens on macOS when Input Monitoring permission is denied? Hotkey registration fails. The overlay shows an error with guidance to enable the permission in System Settings.
- What happens when the user changes activation mode while recording is active? The current recording completes under the old mode's rules. The new mode takes effect from the next activation.
- What happens in hands-free mode when a single press occurs but the user intended a double-press (they pressed too slowly)? After 300ms elapses without a second press and no recording is active, the single press is discarded. The system does not start recording from a lone first press — this prevents accidental activation.

## Requirements *(mandatory)*

### Functional Requirements

#### Activation Modes

- **FR-001**: System MUST support three mutually exclusive activation modes: hold-to-talk, toggle, and hands-free
- **FR-002**: Hold-to-talk mode MUST start recording on key press and stop recording on key release
- **FR-003**: Toggle mode MUST start recording on first key press and stop recording on second key press
- **FR-004**: Hands-free mode MUST start continuous VAD-segmented recording on double-press (two presses within 300ms) and stop on single press while active
- **FR-005**: In hands-free mode, a lone single press when not recording MUST be discarded after the 300ms detection window expires (no accidental activation)
- **FR-006**: Hold-to-talk MUST be the default activation mode
- **FR-007**: Users MUST be able to switch activation modes in the Settings panel, and the change MUST take effect immediately

#### Global Hotkey

- **FR-008**: Ctrl+Shift+Space MUST be the default hotkey
- **FR-009**: System MUST register the hotkey globally so it fires from any application, regardless of which window has focus
- **FR-010**: Hotkey MUST produce visible feedback in every application state (downloading, loading, ready, recording, processing, error) — silent failure is forbidden
- **FR-011**: Users MUST be able to remap the hotkey to any supported key or key combination (modifier+key, function keys F13-F24, standalone keys)
- **FR-012**: Hotkey remapping MUST take effect immediately without application restart (old binding deregistered, new binding registered)
- **FR-013**: On macOS, if the required Input Monitoring permission is not granted, the system MUST display an error with clear guidance to enable it

#### System Tray

- **FR-014**: System tray icon MUST be visible on both Windows and macOS while the application is running
- **FR-015**: Tray icon MUST visually change to reflect the current pipeline state: idle (gray), listening (green), processing (blue), downloading (orange), error (red)
- **FR-016**: Tray tooltip MUST update to describe the current state (e.g., "Vox — Idle", "Vox — Listening...", "Vox — Error: [message]")
- **FR-017**: Right-click context menu MUST include: Toggle Recording, Settings, Show/Hide Overlay, separator, About Vox, Quit
- **FR-018**: "Toggle Recording" menu item MUST act as a simple start/stop toggle regardless of the active activation mode (bypasses mode-specific keyboard mechanics like hands-free double-press)
- **FR-019**: "Settings" menu item MUST open the settings window
- **FR-020**: "Show/Hide Overlay" menu item MUST toggle the overlay HUD visibility
- **FR-021**: "About Vox" menu item MUST display version information
- **FR-022**: "Quit" menu item MUST gracefully shut down the application (stop recording if active, release resources)

#### Platform Behavior

- **FR-023**: On Windows, if CapsLock is chosen as the hotkey via remapping, it MUST suppress its normal toggle behavior (no caps lock state change when used as dictation trigger)
- **FR-024**: On macOS, the system MUST handle the Input Monitoring permission prompt that fires on first hotkey registration

### Key Entities

- **Activation Mode**: The user's chosen recording trigger behavior (hold-to-talk, toggle, or hands-free). Persisted in settings. Determines how hotkey press/release events map to start/stop recording actions.
- **Hotkey Binding**: The user's chosen key or key combination for activating dictation. Persisted in settings. Registered globally with the OS. Can be remapped at runtime.
- **Tray State**: The visual representation of the application's pipeline state in the system tray. Maps pipeline states to icon variants and tooltip text. Updated reactively on state transitions.
- **Hotkey Action**: The outcome of a hotkey event after being interpreted through the active activation mode. One of: start recording, stop recording, start hands-free continuous recording, or no-op (e.g., waiting for double-press detection in hands-free mode).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can activate dictation from any application within 5ms of pressing the hotkey (hotkey event detection to action dispatch)
- **SC-002**: Tray icon updates to reflect the current pipeline state within 10ms of a state transition
- **SC-003**: Hands-free double-press detection correctly distinguishes double-press from two separate single-presses using a 300ms detection window
- **SC-004**: Hotkey press during non-ready states (downloading, loading, error) always produces visible overlay feedback — zero silent failures
- **SC-005**: Users can remap the hotkey and have the new binding work in under 5 seconds (open settings, record new key, close settings, use new key)
- **SC-006**: All three activation modes (hold-to-talk, toggle, hands-free) function correctly with zero mode-confusion bugs (e.g., toggle mode never enters hands-free behavior)
- **SC-007**: Application produces zero compiler warnings

## Clarifications

### Session 2026-02-24

- Q: When the user presses the hotkey while a previous recording is still being processed, should a new recording start immediately, block, or cancel the previous? → A: Start new recording immediately (previous processing continues in background).
- Q: Should the tray menu's "Toggle Recording" follow mode-specific mechanics (e.g., double-click for hands-free) or always act as a simple toggle? → A: Always simple start/stop toggle regardless of activation mode.
- Q: Should the default hotkey change from Ctrl+Shift+Space to CapsLock? → A: No. Keep Ctrl+Shift+Space as default. Users can remap to CapsLock via Settings if desired.

## Assumptions

- The existing system tray setup from 011-gpui-app-shell (menu creation, event loop, action dispatch) provides a working foundation that this feature extends with dynamic icons and expanded menu items.
- The existing global hotkey registration from 011-gpui-app-shell provides the event polling infrastructure that this feature extends with activation mode logic.
- The existing `HotkeyRecorder` UI component in the Settings panel can be reused for hotkey remapping.
- The existing `hold_to_talk` and `hands_free_double_press` boolean settings fields will be replaced by a single activation mode setting.
- Five tray icon variants (gray, green, blue, orange, red) will be created as PNG assets embedded in the binary.
- The current default hotkey remains Ctrl+Shift+Space. Users can remap to CapsLock or any other key via the Settings panel.
