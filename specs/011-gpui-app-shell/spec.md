# Feature Specification: GPUI Application Shell

**Feature Branch**: `011-gpui-app-shell`
**Created**: 2026-02-21
**Status**: Draft
**Dependencies**: 009-application-state-settings, 008-model-management

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Instant Application Launch (Priority: P1)

The user double-clicks the Vox application. The application opens a visible window within 100 milliseconds. The window displays an overlay HUD indicating the application state (initially "starting up"). No setup wizards, no dialogs, no configuration screens. The user sees the app immediately, even before any ML models are loaded.

**Why this priority**: The application must be perceived as responsive. If the window doesn't appear instantly, users will assume it's broken or slow. This is the foundational interaction — everything else depends on the window existing.

**Independent Test**: Launch the application and verify a themed window appears within 100ms. The window should be visible and styled, showing a loading/status indicator.

**Acceptance Scenarios**:

1. **Given** the application is not running, **When** the user launches it, **Then** a themed overlay window appears within 100 milliseconds
2. **Given** the application has just launched, **When** models are not yet loaded, **Then** the window displays a status indicator showing current initialization progress (downloading, loading, ready)
3. **Given** the application has launched, **When** the user's previously saved settings and dictionary exist, **Then** they are loaded into memory during startup without blocking the window from appearing

---

### User Story 2 - Background Pipeline Initialization (Priority: P1)

After the window appears, the application automatically checks for required ML models on disk. If any are missing, it downloads them without user intervention. Once all models are present, it loads them onto the GPU. The UI updates to reflect each stage: downloading (with progress), loading models, and ready. The user never needs to click anything to get the application ready.

**Why this priority**: Zero-click first launch is a core principle. The user should not have to take any manual action to get models downloaded or loaded. This is equally critical to the window appearing, because a visible window without a working pipeline is useless.

**Independent Test**: On a fresh install (no models on disk), launch the app and verify it automatically downloads models, loads them, and transitions to "Ready" state — all without user interaction.

**Acceptance Scenarios**:

1. **Given** the application has launched with all models present, **When** initialization begins, **Then** models load onto the GPU and the UI transitions to "Ready"
2. **Given** the application has launched with missing models, **When** initialization begins, **Then** downloads start automatically, progress is displayed in the UI, and after completion models load and the UI transitions to "Ready"
3. **Given** the application is downloading models, **When** the user interacts with the window, **Then** the UI remains responsive (no freezing or blocking)

---

### User Story 3 - Actions and Keyboard Shortcuts (Priority: P2)

The user can control the application via keyboard shortcuts. Key actions include toggling recording on/off, showing/hiding the overlay, opening settings, and quitting the application. These shortcuts work globally (from any application) for recording control, and within the Vox window for navigation actions.

**Why this priority**: Keyboard shortcuts are the primary interaction model for a dictation app — the user's hands are on the keyboard, not the mouse. Without shortcuts, the app is impractical to use during real work.

**Independent Test**: Launch the app, press the recording toggle shortcut, verify the app responds. Press the quit shortcut, verify the app exits cleanly.

**Acceptance Scenarios**:

1. **Given** the application is running, **When** the user presses the overlay toggle shortcut, **Then** the overlay window shows or hides
2. **Given** the application is running, **When** the user presses the settings shortcut, **Then** a settings panel opens or receives focus
3. **Given** the application is running, **When** the user presses the quit shortcut, **Then** the application exits cleanly without data loss

---

### User Story 4 - System Tray Integration (Priority: P2)

The application runs in the system tray, providing quick access to common actions. The tray icon reflects the application's current state (idle, listening, processing, error). Right-clicking the tray icon shows a context menu with actions like toggle recording, open settings, and quit.

**Why this priority**: The system tray is the standard location for always-running background apps. Users expect to find Vox there when the overlay is hidden, and to control it from the tray menu.

**Independent Test**: Launch the app, verify a tray icon appears. Right-click the tray icon, verify a context menu with basic actions appears. Select quit from the menu, verify the app exits.

**Acceptance Scenarios**:

1. **Given** the application is running, **When** the user looks at the system tray, **Then** a Vox icon is visible
2. **Given** the application is running, **When** the user right-clicks the tray icon, **Then** a context menu with common actions appears
3. **Given** the application is in different states (idle, listening, processing), **When** the state changes, **Then** the tray icon updates to reflect the current state

---

### User Story 5 - Themed Visual Appearance (Priority: P3)

The application uses a consistent dark theme with clearly defined color semantics for backgrounds, text, borders, accents, status indicators, waveform visualization, buttons, and input fields. All UI components draw from this shared theme, ensuring visual consistency. Layout spacing and sizing follow a defined scale.

**Why this priority**: A polished, consistent appearance builds user trust and reduces cognitive load. However, the app is functional without a perfect theme, so this is lower priority than core interactions.

**Independent Test**: Launch the app, verify the window uses the dark theme colors. Check that text is readable, status colors are distinguishable, and spacing is consistent.

**Acceptance Scenarios**:

1. **Given** the application has launched, **When** the overlay window renders, **Then** it uses the dark theme with correct background, text, and accent colors
2. **Given** the dark theme is active, **When** different UI states are shown (idle, listening, error), **Then** each state has a visually distinct, semantically appropriate color
3. **Given** the theme system is initialized, **When** any UI component accesses theme colors, **Then** it retrieves the correct shared color values without errors

---

### User Story 6 - Application Logging (Priority: P3)

The application writes structured logs to a platform-specific log directory. Logs rotate daily and are retained for 7 days. Log verbosity is configurable via environment variable. This enables post-hoc debugging without requiring the user to reproduce issues in real time.

**Why this priority**: Logging is essential for diagnosing issues but is invisible to the user during normal operation. It's a support and development tool, not a user-facing feature.

**Independent Test**: Launch the app, verify a log file is created in the correct platform directory. Verify the log contains startup messages.

**Acceptance Scenarios**:

1. **Given** the application launches, **When** initialization begins, **Then** a log file is created in the platform-specific log directory
2. **Given** the application is running, **When** events occur (startup, state changes, errors), **Then** they are written to the log with timestamps and severity levels
3. **Given** log files exist from previous days, **When** 7 days have passed, **Then** logs older than 7 days are automatically cleaned up

---

### Edge Cases

- What happens when the log directory does not exist at startup? (It should be created automatically)
- What happens when the application is launched while another instance is already running? (Assumption: single instance enforcement is out of scope for this feature; handled separately)
- What happens when the GPU is unavailable during pipeline initialization? (The application should display an error state in the UI, not crash)
- What happens when the window close button is clicked? (The application quits cleanly, flushing logs and saving state)
- What happens when model download fails partway through? (The UI should display an error state; re-download is attempted on next launch)
- What happens when the system tray is not available (e.g., headless Linux)? (Out of scope — Vox targets Windows and macOS only)

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST display a visible, themed window within 100 milliseconds of launch
- **FR-002**: System MUST initialize application state (settings, dictionary, transcript history) before opening the window, without blocking window appearance
- **FR-003**: System MUST register all application actions (toggle recording, stop recording, toggle overlay, open settings, quit, copy last transcript, clear history) at startup
- **FR-004**: System MUST bind keyboard shortcuts to registered actions at startup
- **FR-005**: System MUST provide a dark theme with semantically named colors for backgrounds, text, borders, accents, status indicators, waveform, buttons, and input fields
- **FR-006**: System MUST define layout constants (spacing scale, border radius scale, standard component sizes) that all UI components reference
- **FR-007**: System MUST write structured logs to a platform-specific directory with daily rotation and 7-day retention
- **FR-008**: System MUST automatically check for missing models and download them without user interaction after the window is visible
- **FR-009**: System MUST update the UI to reflect pipeline initialization stages (downloading, loading, ready)
- **FR-010**: System MUST display a system tray icon that reflects the current application state
- **FR-011**: System MUST provide a system tray context menu with common actions
- **FR-012**: System MUST register a global hotkey for toggling recording that works from any application
- **FR-013**: System MUST handle window close gracefully, preventing race conditions on Windows
- **FR-014**: System MUST quit cleanly when requested, flushing logs and persisting any unsaved state
- **FR-015**: System MUST set up log verbosity from environment variables, defaulting to informational level

### Key Entities

- **Application Theme**: Shared visual appearance definition containing all color and styling values. Accessible from any UI component. Supports dark theme (default). Contains color categories: backgrounds (overlay, surface, elevated surface, panel), text (primary, muted, accent), borders (standard, variant), accent (default, hover), status (idle, listening, processing, success, error, downloading), waveform (active, inactive), buttons (primary bg/text, secondary bg/text), and inputs (bg, border, focus border).
- **Layout Constants**: Defined spacing scale (xs through xl), border radius scale (sm through pill), and standard component sizes (overlay dimensions, settings panel dimensions).
- **Application Actions**: Discrete operations the user can trigger via keyboard shortcuts or UI interactions. Each action has a unique name and can be dispatched from any context. Actions include: toggle recording, stop recording, toggle overlay, open settings, quit, copy last transcript, clear history.
- **Key Bindings**: Mapping of keyboard combinations to application actions. Registered at startup. Includes both in-window shortcuts and global hotkeys.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users see the application window within 100 milliseconds of launch
- **SC-002**: State initialization (settings, dictionary, database) completes within 50 milliseconds
- **SC-003**: Theme initialization completes within 1 millisecond
- **SC-004**: Application transitions from launch to "Ready" state without any user interaction (zero clicks)
- **SC-005**: All keyboard shortcuts respond correctly when pressed, with no unregistered or conflicting bindings
- **SC-006**: Log files are created in the correct platform directory on every launch
- **SC-007**: Application quits without errors, data corruption, or orphaned processes
- **SC-008**: System tray icon is visible and context menu functions correctly on both supported platforms
- **SC-009**: All theme colors produce readable, distinguishable text and status indicators in the dark theme
- **SC-010**: Zero compiler warnings in the final build

## Assumptions

- The application targets Windows (CUDA GPU) and macOS (Metal GPU) only. Linux is out of scope.
- Single-instance enforcement is handled by a separate feature, not this one.
- The overlay HUD window is a minimal placeholder for this feature — full UI component wiring happens in subsequent features.
- Global hotkey registration may fail on some systems due to OS-level restrictions; the application should log a warning but not crash.
- System tray behavior follows OS conventions (Windows notification area, macOS menu bar).
- The pipeline initialization function and model download logic already exist from Features 007 and 008; this feature wires them into the startup sequence.
- Settings, dictionary, and transcript history persistence already exist from Features 009 and 010; this feature loads them at startup.
