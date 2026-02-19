# Feature 011: GPUI Application Shell

**Status:** Not Started
**Dependencies:** 009-application-state-settings, 008-model-management
**Design Reference:** Sections 4.6.1 (Startup State Machine), 4.6.1a (Entry Point), 4.6.4 (Component Architecture)
**Estimated Scope:** Application entry point, GPUI setup, actions, keybindings, theme system

---

## Overview

Implement the GPUI application shell — the entry point that creates the GPUI `Application`, initializes global state, registers actions and keybindings, opens the overlay HUD window, sets up the system tray, and kicks off async model loading. By the end of this feature, the app launches and shows a themed window, even though the pipeline and full UI aren't wired yet.

---

## Requirements

### FR-001: Application Entry Point

```rust
// crates/vox/src/main.rs

use gpui::*;

fn main() {
    // 1. Initialize logging
    let _guard = init_logging();

    // 2. Run GPUI Application
    Application::new().run(|cx: &mut App| {
        // 3. Create state (SQLite, settings, dictionary — lightweight, no ML)
        let state = VoxState::new().expect("Failed to create app data directory");
        cx.set_global(state);
        cx.set_global(VoxTheme::dark());

        // 4. Register actions & key bindings
        register_actions(cx);
        register_key_bindings(cx);

        // 5. Open overlay HUD IMMEDIATELY (before model loading)
        open_overlay_window(cx);

        // 6. Set up system tray
        setup_system_tray(cx);

        // 7. Set up global hotkey
        setup_global_hotkey(cx);

        // 8. Kick off async model check + download + pipeline init
        //    This runs in the background. The UI is already visible.
        cx.spawn(|cx| async move {
            initialize_pipeline(cx).await;
        }).detach();

        cx.activate(true);
    });
}
```

**Critical sequence:** The overlay HUD opens IMMEDIATELY on launch — before models load, before GPU init, before anything. The user sees the app instantly. Model downloading happens in the background.

### FR-002: Async Pipeline Initialization

```rust
async fn initialize_pipeline(mut cx: AsyncApp) {
    // Check which models exist on disk
    let missing = cx.update(|cx| {
        check_models()
    }).unwrap();

    if !missing.is_empty() {
        // Update UI: AppReadiness::Downloading
        cx.update(|cx| {
            let state = cx.global_mut::<VoxState>();
            state.set_readiness(AppReadiness::Downloading { /* ... */ });
        }).ok();

        // Start downloads automatically — no user action needed
        download_missing_models(missing, &mut cx).await;
    }

    // Update UI: AppReadiness::Loading
    cx.update(|cx| {
        let state = cx.global_mut::<VoxState>();
        state.set_readiness(AppReadiness::Loading {
            stage: "Loading Whisper model onto GPU...".into(),
        });
    }).ok();

    // Load all models into GPU memory: VAD, Whisper, LLM
    load_pipeline(&mut cx).await.expect("Pipeline initialization failed");

    // Update UI: AppReadiness::Ready
    cx.update(|cx| {
        let state = cx.global_mut::<VoxState>();
        state.set_readiness(AppReadiness::Ready);
    }).ok();
}
```

### FR-003: Action Registration

Define actions using GPUI's `actions!` macro:

```rust
// crates/vox_ui/src/key_bindings.rs

use gpui::actions;

actions!(vox, [
    ToggleRecording,
    StopRecording,
    ToggleOverlay,
    OpenSettings,
    Quit,
    CopyLastTranscript,
    ClearHistory,
]);
```

Register action handlers:

```rust
pub fn register_actions(cx: &mut App) {
    cx.on_action(|action: &ToggleRecording, cx| {
        // Toggle pipeline recording state
    });
    cx.on_action(|action: &OpenSettings, cx| {
        // Open or focus settings window
    });
    cx.on_action(|action: &Quit, cx| {
        cx.quit();
    });
}
```

### FR-004: Key Bindings

```rust
pub fn register_key_bindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-v", ToggleOverlay, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
}
```

### FR-005: Theme System

```rust
// crates/vox_ui/src/theme.rs

use gpui::{Hsla, hsla, Global};

pub struct VoxTheme {
    pub colors: ThemeColors,
}

impl Global for VoxTheme {}

pub struct ThemeColors {
    // Backgrounds
    pub overlay_bg: Hsla,
    pub surface: Hsla,
    pub elevated_surface: Hsla,
    pub panel_bg: Hsla,

    // Text
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_accent: Hsla,

    // Borders
    pub border: Hsla,
    pub border_variant: Hsla,

    // Accent
    pub accent: Hsla,
    pub accent_hover: Hsla,

    // Status
    pub status_idle: Hsla,
    pub status_listening: Hsla,
    pub status_processing: Hsla,
    pub status_success: Hsla,
    pub status_error: Hsla,
    pub status_downloading: Hsla,

    // Waveform
    pub waveform_active: Hsla,
    pub waveform_inactive: Hsla,

    // Buttons
    pub button_primary_bg: Hsla,
    pub button_primary_text: Hsla,
    pub button_secondary_bg: Hsla,
    pub button_secondary_text: Hsla,

    // Input fields
    pub input_bg: Hsla,
    pub input_border: Hsla,
    pub input_focus_border: Hsla,
}

impl VoxTheme {
    pub fn dark() -> Self {
        Self {
            colors: ThemeColors {
                // Dark theme with warm accent colors
                overlay_bg: hsla(0.0, 0.0, 0.1, 0.92),
                surface: hsla(0.0, 0.0, 0.12, 1.0),
                elevated_surface: hsla(0.0, 0.0, 0.16, 1.0),
                panel_bg: hsla(0.0, 0.0, 0.14, 1.0),

                text: hsla(0.0, 0.0, 0.93, 1.0),
                text_muted: hsla(0.0, 0.0, 0.55, 1.0),
                text_accent: hsla(0.58, 0.8, 0.65, 1.0), // Blue accent

                border: hsla(0.0, 0.0, 0.2, 1.0),
                border_variant: hsla(0.0, 0.0, 0.25, 1.0),

                accent: hsla(0.58, 0.8, 0.65, 1.0),
                accent_hover: hsla(0.58, 0.85, 0.7, 1.0),

                status_idle: hsla(0.0, 0.0, 0.55, 1.0),       // Gray
                status_listening: hsla(0.35, 0.9, 0.55, 1.0),  // Green
                status_processing: hsla(0.58, 0.8, 0.65, 1.0), // Blue
                status_success: hsla(0.35, 0.9, 0.55, 1.0),    // Green
                status_error: hsla(0.0, 0.85, 0.6, 1.0),       // Red
                status_downloading: hsla(0.12, 0.9, 0.6, 1.0), // Orange

                waveform_active: hsla(0.35, 0.9, 0.55, 1.0),
                waveform_inactive: hsla(0.0, 0.0, 0.3, 1.0),

                button_primary_bg: hsla(0.58, 0.8, 0.55, 1.0),
                button_primary_text: hsla(0.0, 0.0, 1.0, 1.0),
                button_secondary_bg: hsla(0.0, 0.0, 0.2, 1.0),
                button_secondary_text: hsla(0.0, 0.0, 0.8, 1.0),

                input_bg: hsla(0.0, 0.0, 0.08, 1.0),
                input_border: hsla(0.0, 0.0, 0.25, 1.0),
                input_focus_border: hsla(0.58, 0.8, 0.65, 1.0),
            },
        }
    }
}
```

Access from any component:

```rust
let theme = cx.global::<VoxTheme>();
div().bg(theme.colors.surface).text_color(theme.colors.text)
```

### FR-006: Layout Constants

```rust
// crates/vox_ui/src/layout.rs

use gpui::px;

pub mod spacing {
    use gpui::{Pixels, px};
    pub const XS: Pixels = px(4.0);
    pub const SM: Pixels = px(8.0);
    pub const MD: Pixels = px(12.0);
    pub const LG: Pixels = px(16.0);
    pub const XL: Pixels = px(24.0);
}

pub mod radius {
    use gpui::{Pixels, px};
    pub const SM: Pixels = px(4.0);
    pub const MD: Pixels = px(8.0);
    pub const LG: Pixels = px(12.0);
    pub const PILL: Pixels = px(999.0);
}

pub mod size {
    use gpui::{Pixels, px};
    pub const OVERLAY_WIDTH: Pixels = px(360.0);
    pub const OVERLAY_HEIGHT: Pixels = px(80.0);
    pub const SETTINGS_WIDTH: Pixels = px(800.0);
    pub const SETTINGS_HEIGHT: Pixels = px(600.0);
}
```

### FR-007: Logging Initialization

```rust
fn init_logging() -> tracing_appender::non_blocking::WorkerGuard {
    let log_dir = log_dir();
    let file_appender = tracing_appender::rolling::daily(&log_dir, "vox.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("vox=info".parse().unwrap())
                .add_directive("vox_core=info".parse().unwrap())
                .add_directive("vox_ui=info".parse().unwrap()),
        )
        .with_writer(non_blocking)
        .init();

    guard
}

fn log_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    { dirs::data_local_dir().unwrap().join("com.vox.app").join("logs") }
    #[cfg(target_os = "macos")]
    { dirs::home_dir().unwrap().join("Library/Logs/com.vox.app") }
}
```

Log levels: `ERROR` (always), `WARN` (default), `INFO` (verbose), `DEBUG` (development), `TRACE` (pipeline timing). Rotated daily, 7-day retention.

### FR-008: Window Close Handling

Based on the Tusk pattern for preventing Windows WM_ACTIVATE race conditions:

```rust
window.on_window_should_close(cx, |_window, cx| {
    cx.defer(|cx| cx.quit());
    false // Prevent default close
});
```

---

## GPUI Patterns Used

Based on research of Zed and Tusk codebases:

| Pattern | GPUI API | Source |
|---|---|---|
| Global state | `cx.set_global(state)` / `cx.global::<T>()` | `Global` trait |
| Entity creation | `cx.new(\|cx\| MyView::new(cx))` | `Entity<T>` |
| Window opening | `cx.open_window(options, \|window, cx\| ...)` | `App::open_window` |
| Async tasks | `cx.spawn(\|cx\| async { ... }).detach()` | `App::spawn` |
| Action registration | `cx.on_action(\|action: &MyAction, cx\| ...)` | `App::on_action` |
| Key bindings | `cx.bind_keys([KeyBinding::new(...)])` | `App::bind_keys` |

---

## Acceptance Criteria

- [ ] Application launches and shows a GPUI window
- [ ] VoxState initializes as a GPUI Global
- [ ] VoxTheme initializes and is accessible from components
- [ ] Actions register without errors
- [ ] Key bindings register without errors
- [ ] Logging writes to platform-specific log directory
- [ ] Async pipeline initialization kicks off on launch
- [ ] Window appears within 100ms of launch
- [ ] Application quits cleanly
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_theme_colors_valid` | All Hsla values in valid range |
| `test_layout_constants` | Constants have expected values |
| `test_log_dir_platform` | Correct path on each platform |

---

## Performance Targets

| Metric | Target |
|---|---|
| Time to first window | < 100 ms |
| State initialization | < 50 ms |
| Theme initialization | < 1 ms |
