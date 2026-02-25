# Quickstart: System Tray & Global Hotkeys

**Feature**: 014-tray-hotkeys
**Branch**: `014-tray-hotkeys`

## Prerequisites

- Rust 1.85+ (2024 edition)
- CMake 4.0+
- Windows: Visual Studio 2022 Build Tools, CUDA 12.8+
- macOS: Xcode 26.x + Metal Toolchain

## Build

```bash
# Windows
cargo run -p vox --features vox_core/cuda

# macOS
cargo run -p vox --features vox_core/metal
```

## Test

```bash
# Unit tests for hotkey interpreter (no GPU/hardware needed)
cargo test -p vox_core test_hotkey --features cuda -- --nocapture

# All vox_core tests
cargo test -p vox_core --features cuda
```

## Verify Features

### 1. Activation Modes

**Hold-to-Talk (default)**:
1. Launch app, wait for "Ready" state
2. Press and hold Ctrl+Shift+Space → overlay shows "Listening..."
3. Speak while holding
4. Release the hotkey → overlay shows "Processing..." then injected text

**Toggle**:
1. Open Settings → Hotkey section → set mode to "Toggle"
2. Press Ctrl+Shift+Space once → overlay shows "Listening..."
3. Speak freely
4. Press Ctrl+Shift+Space again → overlay shows "Processing..."

**Hands-Free**:
1. Open Settings → Hotkey section → set mode to "Hands-Free"
2. Double-press Ctrl+Shift+Space (two presses within 300ms) → continuous listening
3. Speak sentences with pauses → each segment processed independently
4. Single press Ctrl+Shift+Space → stops continuous recording

### 2. Dynamic Tray Icons

1. Observe tray icon changes through states:
   - Gray (idle) → Green (listening) → Blue (processing) → Gray (idle)
2. On first launch with no models: Orange (downloading)
3. Hover over tray icon to verify tooltip matches state
4. On error: Red with error message in tooltip

### 3. Tray Context Menu

Right-click the tray icon to verify 6 items:
- **Toggle Recording** → starts/stops recording (simple toggle regardless of mode)
- **Settings** → opens settings window
- **Show/Hide Overlay** → toggles overlay visibility
- *(separator)*
- **About Vox** → shows version info
- **Quit Vox** → graceful shutdown

### 4. Hotkey Remapping

1. Open Settings → Hotkey section
2. Click the hotkey recorder field
3. Press new key combination (e.g., F13 or Ctrl+Shift+D)
4. Verify new hotkey works in any application
5. Verify old hotkey (Ctrl+Shift+Space) no longer activates

### 5. Universal Hotkey Response

1. Delete model files from data directory to simulate first launch
2. Launch app → models begin downloading
3. Press Ctrl+Shift+Space → overlay shows "Models downloading... X%"
4. Wait for loading phase → press Ctrl+Shift+Space → shows "Loading models..."
5. After ready → press Ctrl+Shift+Space → recording starts normally

## Key Files

| File | Purpose |
|------|---------|
| `crates/vox_core/src/hotkey_interpreter.rs` | Activation mode state machine (ActivationMode, HotkeyInterpreter) |
| `crates/vox_core/src/config.rs` | Settings struct with activation_mode field |
| `crates/vox/src/main.rs` | Hotkey registration, event polling, action dispatch |
| `crates/vox/src/tray.rs` | TrayManager — icon lifecycle, state-reactive updates, menu |
| `crates/vox_ui/src/settings_panel.rs` | Activation mode dropdown selector |
| `assets/icons/tray-downloading.png` | Orange 32×32 icon for download/loading state |
| `crates/vox_core/tests/hotkey_interpreter_tests.rs` | Unit tests for all activation modes |

## Architecture Overview

```
┌─────────────────────┐     ┌──────────────────────┐
│   GlobalHotKeyEvent │     │     MenuEvent         │
│   (crossbeam chan)   │     │   (crossbeam chan)    │
└─────────┬───────────┘     └──────────┬───────────┘
          │ poll 5ms                    │ poll 5ms
          ▼                             ▼
┌─────────────────────┐     ┌──────────────────────┐
│  HotkeyInterpreter  │     │    Tray Polling Task  │
│  (mode → action)    │     │  (menu + TrayUpdate)  │
└─────────┬───────────┘     └──────────┬───────────┘
          │ HotkeyAction               │ TrayUpdate
          ▼                             ▲
┌─────────────────────┐                │
│  Action Dispatch    │                │
│  (ToggleRecording/  │     ┌──────────┴───────────┐
│   StopRecording/    │     │ State-Forwarding Task │
│   show overlay)     │     │ (pipeline broadcasts  │
└─────────────────────┘     │  → VoxState → tray)   │
                            └──────────────────────┘
```
