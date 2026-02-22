# Feature Specification: Overlay HUD

**Feature Branch**: `012-overlay-hud`
**Created**: 2026-02-22
**Status**: Draft
**Input**: User description: "Overlay HUD - floating borderless window with dictation pipeline state"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Seeing Dictation State at a Glance (Priority: P1)

A user launches Vox and immediately sees a compact floating overlay on their screen. The overlay shows the current state of the dictation pipeline — whether the app is idle, listening, processing speech, or injecting text. The state is communicated through a colored indicator dot and a text label. The overlay is always visible on top of other windows but never steals keyboard or mouse focus from the application the user is working in.

**Why this priority**: The overlay is the only visual interface during dictation. Without it, the user has no feedback about what the pipeline is doing. This is the foundational capability that all other stories depend on.

**Independent Test**: Can be fully tested by launching the app, observing the overlay in each pipeline state (idle, listening, processing, injecting, error), and verifying the correct indicator color and label appear for each state.

**Acceptance Scenarios**:

1. **Given** the app is running and pipeline is idle, **When** the user looks at the overlay, **Then** a gray dot and "IDLE" label are visible along with the hint "Press [hotkey] to start dictating"
2. **Given** the pipeline transitions to listening, **When** the state changes, **Then** the overlay immediately shows a green pulsing dot and "LISTENING" label
3. **Given** the pipeline is processing speech, **When** ASR completes, **Then** the overlay shows a blue spinner, "PROCESSING" label, and the raw transcript text
4. **Given** text has been injected, **When** injection completes, **Then** the overlay shows a green checkmark, "INJECTED" label, and the polished text which fades after 2 seconds
5. **Given** an error occurs, **When** the pipeline enters error state, **Then** the overlay shows a red warning icon, "ERROR" label, and a human-readable error message with guidance
6. **Given** the overlay is displayed, **When** the user clicks in another application, **Then** that application retains focus — the overlay never captures keyboard or mouse focus

---

### User Story 2 - Real-Time Audio Waveform During Listening (Priority: P2)

While the user is dictating, the overlay displays a real-time waveform visualization that responds to their voice. The waveform shows vertical bars that animate with the audio input amplitude, giving immediate visual confirmation that the microphone is capturing speech. This waveform appears in the content area below the status bar during the Listening state.

**Why this priority**: Visual audio feedback is critical for the user to know their microphone is working and speech is being captured. Without it, users cannot tell if their dictation is being heard, leading to repeated attempts and frustration.

**Independent Test**: Can be tested by activating dictation, speaking into the microphone, and verifying that waveform bars animate in real-time. Silent periods should show minimal bar height, and speech should produce visible bar movement.

**Acceptance Scenarios**:

1. **Given** the pipeline is in Listening state, **When** the user speaks, **Then** waveform bars animate proportionally to audio amplitude
2. **Given** the pipeline is in Listening state, **When** there is silence, **Then** waveform bars remain at minimum height (not zero — a subtle baseline is visible)
3. **Given** the waveform is displaying, **When** audio RMS values arrive from the pipeline (~every 32ms), **Then** the visualization updates at 30 frames per second
4. **Given** no audio samples have been received yet, **When** the waveform renders, **Then** it displays gracefully without visual artifacts or crashes

---

### User Story 3 - Model Download Progress on First Launch (Priority: P3)

On first launch (or whenever models need downloading), the overlay shows per-model download progress. The user sees which model is downloading, how many bytes have been transferred, the total size, and a visual progress bar. Each model (VAD, Whisper ASR, Qwen LLM) has its own progress tracked independently.

**Why this priority**: First launch is the user's first impression. Showing clear download progress sets expectations about wait time and prevents the user from thinking the app is frozen. Per-model detail gives transparency into what is happening.

**Independent Test**: Can be tested by clearing downloaded models (or on a fresh install), launching the app, and verifying that download progress appears for each model with accurate byte counts and a progress bar.

**Acceptance Scenarios**:

1. **Given** models need downloading, **When** the app starts, **Then** the overlay shows an orange download arrow indicator, "DOWNLOADING" label, and the currently downloading model's name and progress
2. **Given** a model is downloading, **When** bytes are received, **Then** the progress bar, percentage, and byte count update in real-time (e.g., "Whisper model: 43% (387 MB / 900 MB)")
3. **Given** all models are downloaded, **When** models are being loaded onto GPU, **Then** the overlay shows a blue spinner, "LOADING" label, and which component is being loaded (e.g., "Loading Whisper model onto GPU...")
4. **Given** a download fails, **When** the error is reported, **Then** the overlay shows a red warning icon, the model path, and "Open Folder" / "Retry Download" action buttons

---

### User Story 4 - Injection Failure Recovery (Priority: P4)

When text injection fails (e.g., the target application does not accept simulated keystrokes), the overlay notifies the user and provides a "Copy" button so the polished text is not lost. The user can click "Copy" to copy the text to the clipboard and paste it manually.

**Why this priority**: Lost dictation output is the worst user experience. Even if injection fails, the text has been transcribed and polished — the user must have a way to recover it.

**Independent Test**: Can be tested by simulating an injection failure (e.g., injecting into an elevated process) and verifying the overlay shows the buffered text with a Copy button, and clicking Copy places the text on the clipboard.

**Acceptance Scenarios**:

1. **Given** text injection fails, **When** the pipeline reports the failure, **Then** the overlay shows a yellow warning icon, "INJECTION FAILED" label, and the buffered polished text. This state persists until explicitly dismissed.
2. **Given** the injection failure overlay is visible, **When** the user clicks "Copy", **Then** the polished text is placed on the system clipboard and the overlay shows brief confirmation before returning to Idle
3. **Given** the injection failure overlay is visible, **When** the user presses the hotkey to start a new dictation, **Then** the failure state is dismissed and the overlay transitions to Listening (the uncopied text is lost)
4. **Given** the user has copied the text, **When** the copy completes, **Then** the overlay shows brief confirmation (e.g., indicator changes to checkmark) before returning to Idle

---

### User Story 5 - Overlay Position and Opacity Customization (Priority: P5)

The user can drag the overlay to any position on their screen, and the position persists between application restarts. The overlay opacity is configurable (default 85%) so the user can make it more or less transparent based on their preference.

**Why this priority**: Users have different screen layouts and workflows. A fixed overlay position would obstruct content for some users. Persistence avoids the annoyance of repositioning every launch. Opacity allows balance between visibility and unobtrusiveness.

**Independent Test**: Can be tested by dragging the overlay to a non-default position, closing and relaunching the app, and verifying the overlay appears at the saved position. Changing opacity in settings and verifying the visual change.

**Acceptance Scenarios**:

1. **Given** the overlay is visible, **When** the user drags it to a new position, **Then** the overlay moves to the new position and the position is saved to settings
2. **Given** the user previously repositioned the overlay, **When** the app is relaunched, **Then** the overlay appears at the saved position
3. **Given** no saved position exists (first launch), **When** the overlay opens, **Then** it appears centered on the screen
4. **Given** the user changes the overlay opacity setting, **When** the setting is applied, **Then** the overlay transparency changes accordingly (0% = invisible, 100% = fully opaque)

---

### User Story 6 - Quick Settings Access from Overlay (Priority: P6)

The overlay status bar includes controls to access settings without leaving the current workflow: a dropdown arrow for quick settings and a menu button for opening the full settings panel. This gives the user convenient access to configuration without hunting for a separate settings window.

**Why this priority**: The overlay is always visible, making it the natural place to access settings. Quick settings (e.g., toggle dictation, change language) should be within one click. Full settings for less common adjustments should also be reachable.

**Independent Test**: Can be tested by clicking the dropdown arrow and verifying a quick settings dropdown appears, and clicking the menu button and verifying the full settings panel opens.

**Acceptance Scenarios**:

1. **Given** the overlay is visible, **When** the user clicks the dropdown arrow (▾), **Then** a quick settings dropdown appears containing a dictation pause/resume toggle and a language selector
2. **Given** the quick settings dropdown is open, **When** the user toggles dictation, **Then** the pipeline pauses or resumes accordingly and the toggle reflects the new state
3. **Given** the quick settings dropdown is open, **When** the user selects a different language, **Then** the language setting is updated and the ASR pipeline uses the new language for subsequent transcriptions
4. **Given** the overlay is visible, **When** the user clicks the menu button (≡), **Then** the full settings panel opens
5. **Given** a dropdown is open, **When** the user clicks outside it, **Then** the dropdown closes

---

### Edge Cases

- What happens when the user presses the hotkey while models are still downloading? The overlay shows "Models downloading... 43%" with the orange download indicator — it does not ignore the hotkey or show an empty/broken state.
- What happens when the overlay is dragged to a screen edge or partially off-screen? The overlay remains fully visible within screen bounds; position is clamped to prevent the overlay from being lost off-screen.
- What happens when the user has multiple monitors and disconnects the monitor where the overlay was positioned? The overlay repositions to the primary monitor's center rather than appearing off-screen.
- What happens when the overlay receives a state update while already animating a fade-out (e.g., new dictation starts during the 2-second injected text fade)? The fade is cancelled immediately and the new state takes priority.
- What happens when the waveform visualizer receives extremely high amplitude values? Values are clamped to [0.0, 1.0] range so bars never exceed the visualizer bounds.
- What happens when the overlay renders with zero waveform samples? An empty waveform area is shown without visual artifacts — no division by zero or empty rendering.

## Clarifications

### Session 2026-02-22

- Q: What items should the quick settings dropdown contain? → A: Dictation toggle (pause/resume) + language selector
- Q: How long does the injection failure state persist? → A: Persists until user clicks Copy or starts new dictation (hotkey)

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST display the overlay as a floating, always-on-top, borderless window with semi-transparent background and rounded corners
- **FR-002**: Overlay MUST NOT steal keyboard or mouse focus from the user's active application
- **FR-003**: Overlay MUST display a status bar containing: a colored state indicator, a state label, the "Vox" title, a quick settings dropdown arrow (containing a dictation pause/resume toggle and a language selector), and a settings menu button that dispatches an action to open the full settings panel
- **FR-004**: Overlay MUST display a state-specific content area below the status bar that changes based on the current pipeline state
- **FR-005**: State indicator MUST use distinct colors for each state: gray for Idle, green (pulsing) for Listening, blue for Processing, green for Injected, red for Error, orange for Downloading, blue for Loading, yellow for Injection Failed
- **FR-006**: Overlay MUST display a real-time waveform visualization during the Listening state, rendered as vertical bars that animate with audio amplitude at 30 frames per second
- **FR-007**: Waveform visualizer MUST accept RMS amplitude values from the audio pipeline and clamp them to [0.0, 1.0] range for rendering
- **FR-008**: During Processing state, overlay MUST display the raw ASR transcript text
- **FR-009**: After successful injection, overlay MUST display the polished text with a green checkmark, then fade the text after 2 seconds
- **FR-010**: During Downloading state, overlay MUST show per-model download progress with model name, percentage, byte count (downloaded/total), and a visual progress bar
- **FR-011**: During Loading state, overlay MUST show which component is currently being loaded (e.g., "Loading Whisper model onto GPU...")
- **FR-012**: When a download fails, overlay MUST show the model path and provide "Open Folder" and "Retry Download" action buttons
- **FR-013**: When text injection fails, overlay MUST show the buffered polished text and a "Copy" button that copies it to the system clipboard. The injection failure state MUST persist until the user clicks Copy or starts a new dictation via hotkey
- **FR-014**: When the user presses the hotkey while models are downloading, overlay MUST show the current download progress (not ignore the hotkey or show an empty state)
- **FR-015**: Overlay MUST subscribe to pipeline state changes and re-render within 16ms of any state transition
- **FR-016**: Overlay position MUST be draggable by the user and persisted to settings between application restarts
- **FR-017**: Overlay position MUST default to screen center on first launch
- **FR-018**: Overlay position MUST be clamped to screen bounds to prevent the window from being lost off-screen
- **FR-019**: Overlay opacity MUST be configurable with a default of 85%
- **FR-020**: Overlay MUST enforce a minimum window size to prevent the content from being unreadable
- **FR-021**: Overlay MUST render at under 2ms per frame to avoid impacting system performance
- **FR-022**: Idle state MUST display the configured hotkey name in the hint text (e.g., "Press [CapsLock] to start dictating")

### Key Entities

- **Overlay HUD**: The always-on-top floating window that serves as the primary visual interface during dictation. Contains a status bar and a state-dependent content area.
- **Status Bar**: The top row of the overlay. Shows the state indicator (colored dot or icon), state label text, app title "Vox", quick settings dropdown arrow, and settings menu button.
- **Content Area**: The region below the status bar. Its contents change dynamically: waveform during listening, transcript text during processing/injection, progress bars during download, error messages during failure states.
- **Waveform Visualizer**: A custom-rendered visualization element that displays vertical bars representing audio amplitude. Receives RMS values from the audio pipeline at ~32ms intervals and renders them as animated bars.
- **Download Progress Display**: Per-model progress tracking showing model name, percentage, byte counts, and a visual progress bar. Covers three models: VAD (Silero), ASR (Whisper), and LLM (Qwen).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: User can identify the current pipeline state within 1 second of looking at the overlay, confirmed by distinct color-coded indicators and text labels for all 8+ states
- **SC-002**: Overlay renders each frame in under 2 milliseconds, ensuring zero impact on system responsiveness during dictation
- **SC-003**: Pipeline state changes are reflected in the overlay within 16 milliseconds (one frame at 60fps)
- **SC-004**: Waveform visualization updates at 30 frames per second during active listening, providing smooth real-time audio feedback
- **SC-005**: Download progress accuracy is within 1% of actual bytes downloaded, with per-model granularity
- **SC-006**: Overlay position persists across 100% of application restarts — the window appears at the last saved position every time
- **SC-007**: When injection fails, the user can recover their dictated text with a single click (Copy button), achieving 100% text recovery rate
- **SC-008**: Every possible app state (Downloading, Loading, Ready/Idle, Listening, Processing, Injecting, Error, Download Failed, Injection Failed, Not Ready) has a visually distinct and informative overlay display — no state is invisible or ambiguous
