# Quickstart & Test Scenarios: Settings Window & Panels

**Feature Branch**: `013-settings-window`
**Date**: 2026-02-23

## Build & Run

```bash
# Windows (CUDA)
cargo run -p vox --features vox_core/cuda

# macOS (Metal)
cargo run -p vox --features vox_core/metal
```

## Unit Tests

```bash
# All vox_ui tests (includes new panel/component tests)
cargo test -p vox_ui

# All vox_core tests (includes log_sink, config, benchmark tests)
cargo test -p vox_core --features cuda    # Windows
cargo test -p vox_core --features metal   # macOS

# Specific test
cargo test -p vox_ui test_panel_switching -- --nocapture
```

## Manual Test Scenarios

### TS-001: Open Settings Window from Tray

1. Launch Vox (system tray icon appears)
2. Right-click tray icon → click "Settings"
3. **Verify**: Settings window opens, sidebar shows 5 items, Settings panel active by default, status bar visible at bottom

### TS-002: Open Settings Window from Keyboard

1. With Vox running, press Ctrl+Comma (Windows) or Cmd+Comma (macOS)
2. **Verify**: Settings window opens identically to TS-001

### TS-003: Singleton Window

1. Open settings window via tray
2. Press Ctrl+Comma again
3. **Verify**: No second window opens; existing window is focused/brought to front

### TS-004: Panel Switching

1. Open settings window (Settings panel active)
2. Click each sidebar item in order: History, Dictionary, Models, Logs, Settings
3. **Verify**: Content area switches to each panel, sidebar highlights the active item, switch is instantaneous (<16ms)

### TS-005: Window Size/Position Persistence

1. Open settings window
2. Resize it to a custom size, drag to a non-default position
3. Close the window
4. Reopen the window
5. **Verify**: Window reopens at the saved size and position

### TS-006: Off-Screen Recovery

1. Open settings window on a secondary monitor
2. Close the window
3. Disconnect the secondary monitor
4. Reopen the window
5. **Verify**: Window appears centered on the primary display

### TS-007: Settings Panel — Audio Section

1. Navigate to Settings panel
2. Click the audio device dropdown
3. **Verify**: All system audio input devices are listed
4. Select a different device
5. **Verify**: Selection persists after closing and reopening settings

### TS-008: Settings Panel — Scrolling

1. Navigate to Settings panel (has 6 sections that may overflow)
2. Scroll down using mouse wheel
3. **Verify**: Always-visible scrollbar tracks position
4. Click the scrollbar track above the thumb
5. **Verify**: Content jumps to that proportional position
6. Drag the scrollbar thumb
7. **Verify**: Content scrolls smoothly following the drag

### TS-009: Settings Panel — Theme Change

1. Navigate to Settings panel → Appearance section
2. Change theme from Dark to Light
3. **Verify**: Entire application (settings window + overlay) immediately updates to Light theme

### TS-010: Settings Panel — Overlay Opacity

1. Navigate to Settings panel → Appearance section
2. Adjust the overlay opacity slider
3. **Verify**: Overlay HUD transparency changes in real time as the slider moves

### TS-011: History Panel — Search

1. Create several transcripts by dictating different phrases
2. Navigate to History panel
3. Type a search query that matches some transcripts
4. **Verify**: List filters to show only matching entries (searches both raw and polished text)
5. Clear the search field
6. **Verify**: All entries reappear

### TS-012: History Panel — Copy

1. Navigate to History panel with entries
2. Click the copy button on an entry
3. Paste into a text editor
4. **Verify**: Polished text from that entry is pasted

### TS-013: History Panel — Delete

1. Navigate to History panel with entries
2. Click delete on an entry
3. **Verify**: Inline confirmation appears ("Confirm? [Yes] [No]")
4. Wait 5 seconds without clicking
5. **Verify**: Confirmation reverts to delete button
6. Click delete again, then click "Yes"
7. **Verify**: Entry is permanently removed

### TS-014: History Panel — Clear All

1. Navigate to History panel with entries
2. Click "Clear All"
3. **Verify**: Modal confirmation dialog appears
4. Confirm deletion
5. **Verify**: All entries removed, panel shows empty state

### TS-015: History Panel — Large Dataset Scrolling

1. Populate 10,000+ transcript entries (via test helper or repeated dictation)
2. Navigate to History panel
3. Scroll rapidly through the list
4. **Verify**: Smooth 60fps scrolling with no jank (virtualized rendering)

### TS-016: Dictionary Panel — Add Entry

1. Navigate to Dictionary panel
2. Fill in: spoken="teh", written="the", category="typo"
3. Click Add
4. **Verify**: New entry appears in the list immediately

### TS-017: Dictionary Panel — Edit Entry

1. Click edit on an existing dictionary entry
2. Change the written form
3. Confirm the edit
4. **Verify**: Entry updates immediately, change persists after panel switch

### TS-018: Dictionary Panel — Delete Entry

1. Click delete on a dictionary entry
2. Confirm via inline confirmation
3. **Verify**: Entry permanently removed

### TS-019: Dictionary Panel — Import/Export Round-Trip

1. Add several dictionary entries
2. Click Export, choose a save location
3. **Verify**: JSON file saved at chosen location
4. Delete all entries
5. Click Import, select the exported JSON file
6. **Verify**: All entries restored; import result shows added/skipped counts

### TS-020: Dictionary Panel — Search and Sort

1. Add entries in multiple categories
2. Type a search query
3. **Verify**: List filters by spoken form, written form, or category
4. Click the "Category" sort header
5. **Verify**: Entries sorted alphabetically by category

### TS-021: Model Panel — Status Display

1. Navigate to Model panel
2. **Verify**: All 3 models shown with current status (Loaded if pipeline is ready)
3. **Verify**: Loaded models show file size, VRAM usage, and benchmark metric

### TS-022: Model Panel — Open Folder

1. Navigate to Model panel
2. Click "Open Model Folder"
3. **Verify**: OS file manager opens to the model storage directory

### TS-023: Model Panel — Swap Model

1. Navigate to Model panel
2. Click "Swap Model" for the Whisper model
3. Select a valid .ggml file from the file dialog
4. **Verify**: Model reloads, status updates, benchmark recomputed
5. Click "Swap Model" again
6. Select an invalid file (e.g., a .txt file)
7. **Verify**: Error message shown, previous model remains active

### TS-024: Log Panel — Real-Time Display

1. Navigate to Log panel
2. Perform actions that generate log events (start/stop dictation)
3. **Verify**: New log entries appear in real time without manual refresh

### TS-025: Log Panel — Level Filter

1. Navigate to Log panel with entries at multiple levels
2. Set filter to "Error"
3. **Verify**: Only Error-level entries displayed
4. Set filter to "Info"
5. **Verify**: Error + Warn + Info entries displayed

### TS-026: Log Panel — Auto-Scroll

1. Navigate to Log panel with auto-scroll enabled (default)
2. Generate new log entries
3. **Verify**: View auto-scrolls to latest entry
4. Disable auto-scroll toggle
5. Generate more entries
6. **Verify**: View stays at current scroll position

### TS-027: Log Panel — Copy and Clear

1. Click copy on a log entry
2. Paste into a text editor
3. **Verify**: Full entry text (timestamp, level, source, message) pasted
4. Click "Clear"
5. **Verify**: All displayed entries removed, new entries continue appearing

### TS-028: Status Bar — Runtime Info

1. With settings window open, start a dictation
2. **Verify**: Status bar shows "Recording" during dictation
3. Complete the dictation
4. **Verify**: Status bar shows last latency value and remains showing current pipeline state

### TS-029: Theme Consistency

1. Switch through all three themes (System, Light, Dark)
2. Navigate to each panel
3. **Verify**: All panels use consistent colors matching the selected theme

### TS-030: Zero Warnings

```bash
cargo build -p vox --features vox_core/cuda 2>&1 | grep warning
# Verify: No output (zero warnings)
```
