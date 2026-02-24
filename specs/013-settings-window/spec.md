# Feature Specification: Settings Window & Panels

**Feature Branch**: `013-settings-window`
**Created**: 2026-02-23
**Status**: Draft
**Input**: User description: "Settings Window & Panels — workspace layout with sidebar navigation, five panels (Settings, History, Dictionary, Model, Log), custom scrollbar, status bar"

## User Scenarios & Testing *(mandatory)*

### User Story 1 — Open and Navigate the Settings Workspace (Priority: P1)

A user wants to access the settings/management window to configure Vox, review their history, manage their dictionary, check model status, or view logs. They open the settings window from the system tray icon or from the overlay menu. A sidebar on the left lists five panels. The user clicks a panel name to switch the main content area. The currently active panel is visually highlighted. A status bar at the bottom shows runtime information.

**Why this priority**: This is the foundation that enables access to all other panels. Without the workspace shell, no settings or management features are reachable.

**Independent Test**: Can be fully tested by opening the settings window, clicking each sidebar item, and verifying the content area switches. Delivers a navigable workspace shell.

**Acceptance Scenarios**:

1. **Given** the application is running, **When** the user clicks "Settings" in the system tray menu, **Then** the settings window opens with the Settings panel active by default.
2. **Given** the application is running, **When** the user clicks "Settings" in the overlay menu, **Then** the settings window opens with the Settings panel active by default.
3. **Given** the settings window is open showing the Settings panel, **When** the user clicks "History" in the sidebar, **Then** the main content area switches to the History panel and the sidebar highlights "History."
4. **Given** the settings window is already open, **When** the user triggers the open action again, **Then** the existing window is focused instead of opening a duplicate.
5. **Given** the settings window was previously closed at a specific size and position, **When** the user opens the settings window again, **Then** it restores to the previously saved size and position.
6. **Given** the settings window is open, **When** the user looks at the bottom status bar, **Then** it displays the current pipeline status, last transcription latency, GPU memory usage, and active audio input device.

---

### User Story 2 — Configure Dictation Settings (Priority: P1)

A user wants to customize how Vox captures audio, processes speech, and presents results. They open the Settings panel and adjust controls organized into sections: Audio, Voice Activity Detection, Hotkey, Language Model, Appearance, and Advanced. Every change takes effect immediately and persists across application restarts.

**Why this priority**: Settings configuration directly affects the core user experience. Users need to select their microphone, tune sensitivity, choose their hotkey, and adjust appearance before productive use.

**Independent Test**: Can be fully tested by changing each setting and verifying the change persists after closing and reopening the window. Delivers full configuration control.

**Acceptance Scenarios**:

1. **Given** the Settings panel is visible, **When** the user selects a different audio input device from the dropdown, **Then** the application immediately uses the new device for audio capture, and the choice persists after restart.
2. **Given** the Settings panel is visible, **When** the user adjusts the noise gate slider, **Then** the new threshold takes effect immediately for subsequent recordings.
3. **Given** the Settings panel is visible, **When** the user changes the theme to "Light," **Then** the application appearance updates immediately and the preference persists.
4. **Given** the Settings panel is visible, **When** the user adjusts the overlay opacity slider, **Then** the overlay HUD's transparency updates in real time.
5. **Given** the Settings panel has many sections that extend beyond the visible area, **When** the user scrolls, **Then** an always-visible scrollbar tracks the scroll position and supports mouse wheel, click-to-jump, and thumb dragging.

---

### User Story 3 — Review Transcript History (Priority: P2)

A user wants to review past dictation transcriptions. They open the History panel and see a scrollable list of entries showing timestamp, polished text, target application, and processing latency. They can search by text, copy individual entries, delete entries, or clear all history.

**Why this priority**: History review allows users to verify accuracy, recover past dictations, and track their usage patterns. It is valuable but not required for core dictation to function.

**Independent Test**: Can be fully tested by creating several transcripts, opening the History panel, searching, copying, deleting, and clearing. Delivers a complete transcript browsing experience.

**Acceptance Scenarios**:

1. **Given** the History panel is visible and contains 50 transcripts, **When** the user types a search query, **Then** the list filters to show only entries containing the query text in either the raw or polished transcript.
2. **Given** the History panel shows a transcript entry, **When** the user clicks the copy button on that entry, **Then** the polished text is copied to the system clipboard.
3. **Given** the History panel shows a transcript entry, **When** the user clicks the delete button and confirms, **Then** the entry is permanently removed from the list and the database.
4. **Given** the History panel is visible, **When** the user clicks "Clear All" and confirms in the confirmation dialog, **Then** all transcript history is permanently deleted.
5. **Given** the History panel contains 10,000 entries, **When** the user scrolls through the list, **Then** scrolling remains smooth at 60 frames per second using virtualized rendering.
6. **Given** the "show raw transcript" setting is enabled, **When** the user views a history entry, **Then** both the raw (unprocessed) text and the polished text are displayed.

---

### User Story 4 — Manage Custom Dictionary (Priority: P2)

A user wants to manage their custom vocabulary — words, phrases, and commands that the dictation system should recognize and substitute. They open the Dictionary panel, browse entries, add new ones, edit existing ones, delete entries, and import/export their dictionary as a file.

**Why this priority**: Custom dictionary management is essential for users with domain-specific vocabulary (medical, legal, technical). It improves accuracy for specialized use cases.

**Independent Test**: Can be fully tested by adding, editing, deleting, searching, sorting, importing, and exporting dictionary entries. Delivers a complete CRUD dictionary management interface.

**Acceptance Scenarios**:

1. **Given** the Dictionary panel is visible, **When** the user fills in the spoken form, written form, and category fields and clicks Add, **Then** a new entry appears in the list immediately.
2. **Given** the Dictionary panel shows an entry, **When** the user clicks edit on that entry and changes the written form, **Then** the entry updates immediately and the change persists.
3. **Given** the Dictionary panel shows an entry, **When** the user clicks delete and confirms, **Then** the entry is permanently removed.
4. **Given** the Dictionary panel is visible, **When** the user types a search query, **Then** the list filters by spoken form, written form, or category.
5. **Given** the Dictionary panel shows entries, **When** the user clicks the sort header for "category," **Then** entries are sorted alphabetically by category.
6. **Given** the Dictionary panel is visible, **When** the user clicks Export, **Then** all entries are saved to a JSON file at a user-chosen location.
7. **Given** the Dictionary panel is visible, **When** the user clicks Import and selects a JSON file, **Then** entries from the file are added to the dictionary (skipping duplicates).
8. **Given** a dictionary entry exists, **When** the user toggles the "command phrase" flag, **Then** the entry is marked as a voice command trigger rather than a text substitution.

---

### User Story 5 — Monitor and Manage Models (Priority: P3)

A user wants to see the status of the ML models that power the dictation pipeline, monitor downloads, and manage model files. They open the Model panel and see each model's status (missing, downloading, downloaded, loaded, error), with actions to retry failed downloads, open the model storage folder, view inference speed, and replace a model with a different file.

**Why this priority**: Model management is primarily needed during first-run setup or when troubleshooting. Most users interact with this panel infrequently after initial setup.

**Independent Test**: Can be fully tested by viewing model statuses, triggering a retry, opening the model folder, viewing benchmark results, and swapping a model file. Delivers complete model oversight.

**Acceptance Scenarios**:

1. **Given** the Model panel is visible, **When** all models are downloaded and loaded, **Then** each model shows a "Loaded" status with its file size and GPU memory usage.
2. **Given** a model download is in progress, **When** the user views the Model panel, **Then** a progress bar shows the download percentage and bytes downloaded.
3. **Given** a model download has failed, **When** the user clicks "Retry Download," **Then** the download restarts from the beginning and progress is shown.
4. **Given** the Model panel is visible, **When** the user clicks "Open Model Folder," **Then** the operating system's file manager opens to the model storage directory.
5. **Given** a model is loaded, **When** the user views its panel entry, **Then** a benchmark metric displays the model's inference speed (e.g., tokens per second or real-time processing factor).
6. **Given** the Model panel is visible, **When** the user clicks "Swap Model" for a model entry and selects a compatible file (GGUF or GGML format), **Then** the selected file replaces the current model and the system reloads it.
7. **Given** the user selects an incompatible file during model swap, **When** the system attempts to load it, **Then** an error message explains the incompatibility and the previous model remains active.

---

### User Story 6 — View Application Logs (Priority: P3)

A user wants to see real-time application logs to diagnose issues or understand system behavior. They open the Log panel and see a live-updating list of log entries, color-coded by severity. They can filter by log level, toggle auto-scrolling, copy entries, and clear the display.

**Why this priority**: Log viewing is a diagnostic tool primarily used during troubleshooting. It supports technical users and support workflows but is not part of the core dictation experience.

**Independent Test**: Can be fully tested by generating log events at various levels, filtering, toggling auto-scroll, copying, and clearing. Delivers a live log viewer.

**Acceptance Scenarios**:

1. **Given** the Log panel is visible, **When** the application generates log events, **Then** new entries appear in real time without manual refresh.
2. **Given** the Log panel shows entries at multiple severity levels, **When** the user sets the filter to "Error," **Then** only Error-level entries are displayed.
3. **Given** auto-scroll is enabled (default), **When** new log entries arrive, **Then** the view automatically scrolls to show the latest entry.
4. **Given** auto-scroll is enabled, **When** the user disables auto-scroll, **Then** the view stays at the current scroll position even as new entries arrive.
5. **Given** the Log panel shows log entries, **When** the user clicks copy on an entry, **Then** the entry's full text (timestamp, level, source, message) is copied to the clipboard.
6. **Given** the Log panel shows entries, **When** the user clicks "Clear," **Then** all displayed entries are removed from the view (log generation continues in the background).
7. **Given** the application is generating 100 log entries per second, **When** the user views the Log panel, **Then** the display remains responsive without dropped frames.

---

### Edge Cases

- **No audio devices available**: The audio input device dropdown in the Settings panel displays "No devices found" and the user cannot select a device until one becomes available.
- **Corrupted settings file**: If the persisted settings file cannot be read, the application falls back to default settings and logs the error. The next setting change overwrites the corrupted file with valid data.
- **History database unreachable**: If the transcript database cannot be opened, the History panel displays an error message explaining the issue. Other panels remain functional.
- **Empty dictionary**: The Dictionary panel displays a prompt encouraging the user to add their first entry, with guidance on what spoken/written forms mean.
- **Model download interrupted**: If a download is interrupted (network loss, app close), the partial file is cleaned up. On next launch or retry, the download starts fresh.
- **Model swap with active pipeline**: If the user swaps a model while the dictation pipeline is active, the system stops the pipeline, swaps the model, reloads, and restarts the pipeline. The overlay HUD shows loading state during the swap.
- **Window positioned off-screen**: If the saved window position is outside current display bounds (e.g., external monitor disconnected), the window resets to centered on the primary display.
- **Log buffer overflow**: The log display maintains a bounded buffer of the most recent entries. Entries beyond the buffer limit are discarded from the display but remain in the log file on disk.
- **Concurrent setting changes**: If settings are modified from multiple entry points simultaneously, the last write wins and the settings file remains consistent.
- **Dictionary import with duplicates**: When importing entries, entries with matching spoken forms that already exist are skipped. The import result reports how many entries were added and how many were skipped.

## Requirements *(mandatory)*

### Functional Requirements

**Window & Navigation**

- **FR-001**: System MUST provide a settings/management window accessible from the system tray menu, the overlay HUD menu, and the keyboard shortcut (Ctrl+Comma on Windows, Cmd+Comma on macOS).
- **FR-002**: The settings window MUST be a singleton — if already open, triggering the open action again MUST focus the existing window rather than opening a duplicate.
- **FR-003**: The settings window MUST remember its size and position between sessions and restore them on next open. If the saved position is outside current display bounds, it MUST reset to centered on the primary display.
- **FR-004**: The settings window MUST have a sidebar on the left with five navigation items: Settings, History, Dictionary, Models, and Logs. Clicking an item MUST switch the main content area to that panel.
- **FR-005**: The currently active panel MUST be visually highlighted in the sidebar to indicate which panel the user is viewing.
- **FR-006**: The settings window MUST display a status bar at the bottom showing: pipeline status (e.g., "Ready," "Recording," "Processing"), last transcription latency in milliseconds, GPU memory usage, and the name of the active audio input device.
- **FR-007**: Scrollable panels MUST display an always-visible vertical scrollbar that supports mouse wheel scrolling, click-to-jump (clicking the track scrolls to that proportional position), and thumb dragging.
- **FR-008**: The settings window MUST enforce a minimum window size to prevent controls from becoming unusable.
- **FR-009**: All panels MUST use consistent theme colors matching the selected application theme.

**Settings Panel**

- **FR-010**: The Settings panel MUST provide an Audio section with: a dropdown listing all available audio input devices (showing "No devices found" if none available), and a noise gate threshold slider.
- **FR-011**: The Settings panel MUST provide a Voice Activity Detection section with: a detection threshold slider, a minimum silence duration slider, and a minimum speech duration slider.
- **FR-012**: The Settings panel MUST provide a Hotkey section with: an activation hotkey recorder (user presses a key to set it), a hold-to-talk mode toggle, and a hands-free double-press mode toggle.
- **FR-013**: The Settings panel MUST provide a Language Model section with: a temperature slider, a filler word removal toggle, a course correction toggle, and a punctuation toggle.
- **FR-014**: The Settings panel MUST provide an Appearance section with: a theme dropdown (System, Light, Dark), an overlay opacity slider, an overlay position dropdown, and a show raw transcript toggle.
- **FR-015**: The Settings panel MUST provide an Advanced section with: a maximum segment duration input, an overlap duration input, and a command prefix text input.
- **FR-016**: Every setting change MUST persist to storage immediately upon modification. No "Save" button is required.
- **FR-017**: Every setting change MUST take effect immediately without requiring an application restart.

**History Panel**

- **FR-018**: The History panel MUST display a scrollable list of past transcription entries. Each entry MUST show: timestamp, polished text, target application name, and processing latency.
- **FR-019**: When the "show raw transcript" setting is enabled, each history entry MUST additionally display the raw (unprocessed) text.
- **FR-020**: The History panel MUST provide a search field that filters entries by matching text content in both raw and polished transcripts.
- **FR-021**: Each history entry MUST have a copy action that copies the polished text to the system clipboard.
- **FR-022**: Each history entry MUST have a delete action that permanently removes the entry after the user confirms via an inline confirmation mechanism (not a modal dialog).
- **FR-023**: The History panel MUST provide a "Clear All" action that permanently deletes all transcript history after the user confirms via a confirmation dialog.
- **FR-024**: The History panel MUST use virtualized rendering to maintain smooth 60fps scrolling with 10,000 or more entries.

**Dictionary Panel**

- **FR-025**: The Dictionary panel MUST display a list of all dictionary entries with the ability to search and filter by spoken form, written form, or category.
- **FR-026**: The Dictionary panel MUST allow adding a new entry by specifying: spoken form (what the user says), written form (what gets typed), and category (for organization).
- **FR-027**: The Dictionary panel MUST allow editing an existing entry inline (modifying spoken form, written form, or category in place).
- **FR-028**: The Dictionary panel MUST allow deleting an entry after the user confirms via an inline confirmation mechanism.
- **FR-029**: Each dictionary entry MUST have a toggleable "command phrase" flag that marks it as a voice command trigger rather than a text substitution.
- **FR-030**: The Dictionary panel MUST allow sorting entries by name (spoken form), category, or use count.
- **FR-031**: The Dictionary panel MUST provide an Export action that saves all entries to a JSON file at a user-selected location.
- **FR-032**: The Dictionary panel MUST provide an Import action that loads entries from a user-selected JSON file, adding new entries and skipping entries with duplicate spoken forms. The result (added count, skipped count) MUST be displayed to the user.

**Model Panel**

- **FR-033**: The Model panel MUST display each model used by the pipeline with its current status: Missing (not downloaded), Downloading (with progress), Downloaded (file present, not loaded), Loaded (active in GPU memory with memory usage shown), or Error (with error message).
- **FR-034**: For models in the Downloading state, the Model panel MUST display a progress bar showing bytes downloaded and total bytes.
- **FR-035**: For models in the Error state, the Model panel MUST provide a "Retry Download" action that restarts the download.
- **FR-036**: The Model panel MUST provide an "Open Model Folder" action that opens the model storage directory in the operating system's file manager.
- **FR-037**: For models in the Loaded state, the Model panel MUST display an inference speed benchmark result (e.g., tokens per second for the language model, real-time processing factor for the speech recognizer).
- **FR-038**: The Model panel MUST provide a "Swap Model" action for each model that allows the user to select a compatible replacement file (GGUF, GGML, or ONNX format matching the model type) from their file system. After selection, the system MUST validate the file, replace the current model, and reload it. If the file is incompatible, an error MUST be shown and the previous model MUST remain active.

**Log Panel**

- **FR-039**: The Log panel MUST display log entries from the application's logging system in real time, without requiring manual refresh.
- **FR-040**: Each log entry MUST display: timestamp, severity level (Error, Warn, Info, Debug, Trace), source component, and message text.
- **FR-041**: The Log panel MUST provide a severity level filter that shows only entries at or above the selected level.
- **FR-042**: The Log panel MUST provide an auto-scroll toggle (enabled by default) that automatically scrolls to show the latest entry when new entries arrive.
- **FR-043**: The Log panel MUST provide a copy action for individual log entries that copies the full entry text to the system clipboard.
- **FR-044**: The Log panel MUST provide a "Clear" action that removes all displayed entries from the view. Log generation continues in the background.
- **FR-045**: Log entries MUST be visually color-coded by severity level (distinct colors for Error, Warn, Info, Debug, and Trace).

### Key Entities

- **Setting**: A user-configurable preference that controls application behavior. Settings span audio capture, voice detection, hotkey behavior, language model processing, visual appearance, and advanced tuning. Each setting has a current value, a default value, and a valid range or set of options. Settings persist across sessions.
- **Transcript Entry**: A record of a single dictation session. Contains the original (raw) speech-to-text output, the polished (post-processed) output, the name of the application that received the text, timing metadata (duration, processing latency), and a creation timestamp. Transcript entries persist in a database.
- **Dictionary Entry**: A custom vocabulary mapping that the dictation system uses for substitution or command recognition. Contains a spoken form (what the user says), a written form (what gets typed), a category (for organization), a command-phrase flag (voice command vs. text substitution), and a use count. Dictionary entries persist in a database.
- **Model**: An ML model file used by the dictation pipeline. Each model has a name, expected filename, download URL, integrity checksum, file size, and a runtime status (missing, downloading, downloaded, loaded, error). The system uses three models: a voice activity detector, a speech recognizer, and a language model. Models reside on disk in a dedicated storage directory.
- **Log Entry**: An ephemeral application event record used for diagnostics. Contains a timestamp, severity level, source component identifier, and human-readable message. Log entries are displayed in real time and maintained in a bounded in-memory buffer (not persisted beyond the log file).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can open the settings window and navigate to any panel within 2 interactions (one to open, one to click the panel).
- **SC-002**: Panel switching feels instantaneous, completing in under 16 milliseconds.
- **SC-003**: Scrolling through a history list of 10,000 entries remains smooth at 60 frames per second.
- **SC-004**: The log panel displays 100 new entries per second without visible lag or dropped frames.
- **SC-005**: Any setting change persists and takes effect in under 10 milliseconds, with no restart required.
- **SC-006**: All five panels (Settings, History, Dictionary, Models, Logs) are fully functional with every feature specified in the requirements — no missing functionality.
- **SC-007**: The window correctly restores its previous size and position on subsequent opens, including handling off-screen recovery.
- **SC-008**: All UI elements use consistent theme colors across all panels, matching the user's selected theme.
- **SC-009**: Dictionary import/export round-trips losslessly — exporting then importing produces an identical dictionary.
- **SC-010**: The application compiles with zero warnings after implementing this feature.

## Assumptions

- The settings window is a separate, standalone window from the overlay HUD.
- Only one settings window instance exists at any time (singleton).
- The "show raw transcript" toggle in the Appearance settings controls whether raw text appears in History entries.
- Model benchmarks are computed once when a model is loaded and displayed as a static metric until the model is reloaded.
- The log display buffer holds the most recent 10,000 entries. Older entries scroll out of the buffer but remain in the on-disk log file.
- Inline confirmation (for delete actions) means the button transforms into a confirmation prompt within the same row for 5 seconds, reverting if not confirmed. This avoids disruptive modal dialogs for frequent actions.
- The "Clear All" history action uses a modal confirmation dialog because it is destructive and irreversible across all entries.
- Audio device enumeration happens when the Settings panel opens and does not auto-refresh. The user can close and reopen the panel to refresh the device list.
- Model swap stops the active dictation pipeline during the swap operation, showing a loading state to the user.
