# Quickstart Verification: GPUI Application Shell

**Input**: spec.md acceptance scenarios, contracts/public-api.md
**Date**: 2026-02-21

## Prerequisites

- Features 009 (Application State & Settings) and 010 (Custom Dictionary) complete and merged
- Rust 1.85+ with CUDA (Windows) or Metal (macOS) toolchain
- No models required on disk (the app should handle missing models gracefully)

## Verification Scenarios

### VS-001: Theme Colors Valid

1. Create `VoxTheme::dark()`
2. **Verify**: All 28 Hsla color values have h, s, l, a components in range 0.0..=1.0
3. **Verify**: `overlay_bg.a` < 1.0 (semi-transparent)
4. **Verify**: All other background colors have `a` == 1.0

### VS-002: Layout Constants Ordered

1. Access `spacing` constants
2. **Verify**: XS (4) < SM (8) < MD (12) < LG (16) < XL (24)
3. Access `radius` constants
4. **Verify**: SM (4) < MD (8) < LG (12) < PILL (999)
5. Access `size` constants
6. **Verify**: OVERLAY_WIDTH == px(360.0), OVERLAY_HEIGHT == px(80.0)
7. **Verify**: SETTINGS_WIDTH == px(800.0), SETTINGS_HEIGHT == px(600.0)

### VS-003: Log Directory Platform

1. Call `log_dir()`
2. **Verify** (Windows): Path contains `com.vox.app` and ends with `logs`
3. **Verify** (macOS): Path contains `com.vox.app` and ends with `com.vox.app`
4. **Verify**: Path is absolute

### VS-004: Log Retention Cleanup

1. Create a temporary directory with log files:
   - `vox.2026-02-14.log` (7 days ago)
   - `vox.2026-02-13.log` (8 days ago)
   - `vox.2026-02-10.log` (11 days ago)
   - `vox.2026-02-20.log` (1 day ago)
   - `vox.2026-02-21.log` (today)
2. Call `cleanup_old_logs(dir, 7)`
3. **Verify**: Files from 2026-02-13 and 2026-02-10 are deleted
4. **Verify**: Files from 2026-02-14, 2026-02-20, 2026-02-21 are retained
5. **Verify**: Non-log files in the directory are not touched

### VS-005: Application Launch (Manual)

1. Build the application: `cargo build -p vox --features vox_core/cuda`
2. Launch the built binary
3. **Verify**: A window appears within ~100ms of launch
4. **Verify**: The window has a dark background (semi-transparent)
5. **Verify**: The window displays a status indicator (e.g., "Downloading models..." or "Ready")
6. **Verify**: No setup wizard, dialog, or configuration screen appears
7. **Verify**: The application does not crash

### VS-006: System Tray (Manual)

1. Launch the application
2. **Verify**: A tray icon appears in the Windows notification area / macOS menu bar
3. Right-click the tray icon
4. **Verify**: A context menu appears with at least "Toggle Recording", "Settings", and "Quit"
5. Click "Quit" from the menu
6. **Verify**: The application exits cleanly

### VS-007: Keyboard Shortcuts (Manual)

1. Launch the application
2. Focus the Vox window
3. Press Ctrl+Shift+V (Windows) or Cmd+Shift+V (macOS)
4. **Verify**: The overlay window toggles visibility
5. Press Ctrl+Q (Windows) or Cmd+Q (macOS)
6. **Verify**: The application exits cleanly

### VS-008: Window Close (Manual)

1. Launch the application
2. Click the window close button (X)
3. **Verify**: The application exits cleanly without crash or error
4. **Verify**: No orphaned processes remain running

### VS-009: Logging Output (Manual)

1. Launch the application
2. Wait for startup to complete
3. Check the log directory:
   - Windows: `%LOCALAPPDATA%/com.vox.app/logs/`
   - macOS: `~/Library/Logs/com.vox.app/`
4. **Verify**: A log file exists with today's date
5. **Verify**: The log contains startup messages (e.g., state initialization, readiness transitions)

### VS-010: Zero Compiler Warnings

1. Build the application: `cargo build -p vox --features vox_core/cuda 2>&1`
2. **Verify**: Zero warnings in the build output
3. Build the UI crate: `cargo build -p vox_ui 2>&1`
4. **Verify**: Zero warnings in the build output

### VS-011: Pipeline Initialization (Manual)

1. Launch the application with all models present on disk
2. **Verify**: The overlay transitions from "Loading..." to "Ready" without any clicks
3. Delete one model file from the models directory
4. Relaunch the application
5. **Verify**: The overlay shows "Downloading..." with progress indication
6. **Verify**: After download completes, it transitions to "Loading..." then "Ready"
7. **Verify**: No user interaction was required
