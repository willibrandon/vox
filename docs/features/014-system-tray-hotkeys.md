# Feature 014: System Tray & Global Hotkeys

**Status:** Not Started
**Dependencies:** 011-gpui-application-shell, 007-pipeline-orchestration
**Design Reference:** Sections 3.3 (Platform Integration), 5.2 (Hands-Free Mode)
**Estimated Scope:** tray-icon integration, global-hotkey integration, activation modes

---

## Overview

Implement the system tray icon and global hotkey system. The system tray provides persistent access to Vox settings and status. Global hotkeys allow the user to activate dictation from any application without switching focus. Three activation modes are supported: hold-to-talk, toggle, and hands-free.

---

## Requirements

### FR-001: System Tray (tray-icon 0.19)

```rust
// crates/vox_core/src/hotkey.rs (or crates/vox/src/tray.rs)

use tray_icon::{TrayIconBuilder, TrayIconEvent, Icon, menu::*};

pub fn setup_system_tray(cx: &mut App) {
    let menu = Menu::new();

    let toggle_item = MenuItem::new("Toggle Recording", true, None);
    let settings_item = MenuItem::new("Settings...", true, None);
    let separator = PredefinedMenuItem::separator();
    let quit_item = MenuItem::new("Quit Vox", true, None);

    menu.append(&toggle_item).unwrap();
    menu.append(&settings_item).unwrap();
    menu.append(&separator).unwrap();
    menu.append(&quit_item).unwrap();

    let icon = load_tray_icon();

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("Vox — Voice Dictation")
        .build()
        .expect("Failed to create system tray icon");

    // Handle menu events
    // toggle_item → ToggleRecording action
    // settings_item → OpenSettings action
    // quit_item → Quit action
}
```

### FR-002: Tray Icon States

The tray icon visually reflects the pipeline state:

| State | Icon | Tooltip |
|---|---|---|
| Idle | Gray microphone | "Vox — Idle" |
| Listening | Green microphone (active) | "Vox — Listening..." |
| Processing | Blue microphone (processing) | "Vox — Processing..." |
| Downloading | Orange microphone (loading) | "Vox — Downloading models..." |
| Error | Red microphone (error) | "Vox — Error: [message]" |

### FR-003: Tray Context Menu

Right-click menu items:

| Item | Action |
|---|---|
| "Toggle Recording" | Start/stop recording |
| "Settings..." | Open settings window |
| "Show Overlay" | Show/hide the overlay HUD |
| separator | — |
| "About Vox" | Version info |
| "Quit Vox" | Exit application |

### FR-004: Global Hotkeys (global-hotkey 0.6)

```rust
use global_hotkey::{GlobalHotKeyManager, GlobalHotKeyEvent, hotkey::*};

pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    hotkey_id: u32,
    activation_mode: ActivationMode,
    last_press_time: Instant,
    is_held: bool,
}

impl HotkeyManager {
    pub fn new(hotkey_str: &str) -> Result<Self> {
        let manager = GlobalHotKeyManager::new()?;
        let hotkey = parse_hotkey(hotkey_str)?;
        manager.register(hotkey)?;

        Ok(Self {
            manager,
            hotkey_id: hotkey.id(),
            activation_mode: ActivationMode::HoldToTalk,
            last_press_time: Instant::now(),
            is_held: false,
        })
    }

    pub fn set_hotkey(&mut self, hotkey_str: &str) -> Result<()> {
        // Unregister old, register new
    }
}
```

**Default hotkey:** CapsLock (can be remapped in settings).

### FR-005: Activation Mode Logic

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum ActivationMode {
    HoldToTalk,  // Hold hotkey = recording. Release = stop.
    Toggle,      // Press once = start. Press again = stop.
    HandsFree,   // Double-press = continuous recording. Single press = stop.
}
```

**Hold-to-Talk (default):**
```
Key Down → Start recording
Key Up   → Stop recording, process remaining audio
```

**Toggle:**
```
Key Down (first) → Start recording
Key Down (second) → Stop recording, process remaining audio
```

**Hands-Free:**
```
Double-press (< 300ms gap) → Start continuous recording
  VAD auto-segments, each segment processed automatically
Single press → Stop continuous recording
```

```rust
impl HotkeyManager {
    pub fn on_hotkey_event(&mut self, event: GlobalHotKeyEvent) -> HotkeyAction {
        match self.activation_mode {
            ActivationMode::HoldToTalk => {
                match event.state {
                    HotKeyState::Pressed => HotkeyAction::StartRecording,
                    HotKeyState::Released => HotkeyAction::StopRecording,
                }
            }
            ActivationMode::Toggle => {
                if event.state == HotKeyState::Pressed {
                    if self.is_active {
                        self.is_active = false;
                        HotkeyAction::StopRecording
                    } else {
                        self.is_active = true;
                        HotkeyAction::StartRecording
                    }
                } else {
                    HotkeyAction::None
                }
            }
            ActivationMode::HandsFree => {
                if event.state == HotKeyState::Pressed {
                    let now = Instant::now();
                    if now.duration_since(self.last_press_time) < Duration::from_millis(300) {
                        self.last_press_time = now;
                        HotkeyAction::StartHandsFree
                    } else if self.is_active {
                        HotkeyAction::StopRecording
                    } else {
                        self.last_press_time = now;
                        HotkeyAction::None // Wait for potential double-press
                    }
                } else {
                    HotkeyAction::None
                }
            }
        }
    }
}

pub enum HotkeyAction {
    None,
    StartRecording,
    StopRecording,
    StartHandsFree,
}
```

### FR-006: Hotkey Responds in Every State

Constitution Principle V: The hotkey MUST respond in every app state. If the pipeline is not ready:

| App State | Hotkey Response |
|---|---|
| Downloading | Overlay shows "Models downloading... 43%" |
| Loading | Overlay shows "Loading models..." |
| Ready + Idle | Start recording |
| Ready + Listening | Depends on activation mode |
| Error | Overlay shows error with guidance |

**Never** silently do nothing when the hotkey is pressed.

### FR-007: Platform-Specific Hotkey Notes

**Windows:**
- `global-hotkey` uses `RegisterHotKey` Win32 API
- Works system-wide including in elevated console windows
- CapsLock as hotkey suppresses its normal toggle behavior

**macOS:**
- Requires **Input Monitoring** permission (System Settings → Privacy & Security → Input Monitoring)
- Runtime permission prompt fires on first hotkey registration
- `global-hotkey` uses CGEvent tap internally

### FR-008: Hotkey Configuration

Users can change the hotkey in the Settings panel:

```rust
pub fn parse_hotkey(hotkey_str: &str) -> Result<HotKey> {
    // Parse strings like "CapsLock", "Ctrl+Shift+D", "F13"
    // Map to global-hotkey types
}
```

Common hotkey options:
- CapsLock (default)
- F13–F24 (dedicated keys)
- Ctrl+Shift+<key>
- Cmd+Shift+<key> (macOS)

---

## Acceptance Criteria

- [ ] System tray icon appears on both platforms
- [ ] Tray icon changes with pipeline state
- [ ] Tray context menu items work
- [ ] Global hotkey registers and fires in any application
- [ ] Hold-to-talk mode works (press=start, release=stop)
- [ ] Toggle mode works (press=toggle)
- [ ] Hands-free mode works (double-press=continuous, single=stop)
- [ ] Hotkey pressed during download shows status (not silent)
- [ ] Hotkey can be remapped in settings
- [ ] macOS permission prompts handled gracefully
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_hold_to_talk_press` | Press → StartRecording |
| `test_hold_to_talk_release` | Release → StopRecording |
| `test_toggle_on_off` | First press = start, second = stop |
| `test_hands_free_double_press` | Two presses < 300ms = StartHandsFree |
| `test_hands_free_single_stop` | Single press while active = stop |
| `test_hotkey_parsing` | "CapsLock", "Ctrl+Shift+D" parse correctly |

### Manual Testing

| Scenario | Windows | macOS |
|---|---|---|
| Hotkey in text editor | ☐ | ☐ |
| Hotkey in browser | ☐ | ☐ |
| Hotkey in terminal | ☐ | ☐ |
| Tray icon visible | ☐ | ☐ |
| Tray menu works | ☐ | ☐ |

---

## Performance Targets

| Metric | Target |
|---|---|
| Hotkey event latency | < 5 ms |
| Tray icon update | < 10 ms |
| Mode detection (double-press) | 300 ms window |
