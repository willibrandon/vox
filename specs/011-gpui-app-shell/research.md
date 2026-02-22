# Research: GPUI Application Shell

**Input**: spec.md functional requirements, Zed/Tusk reference repos
**Date**: 2026-02-21

## R-001: GPUI Overlay Window Configuration

**Decision**: Use `WindowKind::PopUp` with `WindowBackgroundAppearance::Transparent` and hidden titlebar for the overlay HUD.

**Rationale**: GPUI provides `WindowKind::PopUp` which creates a window that floats above all other windows — exactly what a dictation overlay needs. Combined with `WindowBackgroundAppearance::Transparent`, the overlay can show semi-transparent content over the user's active application. `TitlebarOptions { appears_transparent: true }` hides the native titlebar, giving full control over the window appearance.

**API Details** (from `D:\SRC\zed\crates\gpui\src\platform.rs`):
- `WindowKind::PopUp` — Always-on-top window (line 1269)
- `WindowBackgroundAppearance::Transparent` — Alpha transparency (line 1323)
- `TitlebarOptions { title: None, appears_transparent: true, traffic_light_position: None }` — No visible titlebar (line 1250)
- `Bounds::centered(None, size, cx)` — Center on primary display

**Window configuration**:
```rust
let overlay_size = size(px(360.0), px(80.0));
let bounds = Bounds::centered(None, overlay_size, cx);
let options = WindowOptions {
    window_bounds: Some(WindowBounds::Windowed(bounds)),
    titlebar: Some(TitlebarOptions {
        title: None,
        appears_transparent: true,
        traffic_light_position: None,
    }),
    focus: false, // Don't steal focus from active app
    show: true,
    kind: WindowKind::PopUp,
    is_movable: true,
    is_resizable: false,
    window_background: WindowBackgroundAppearance::Transparent,
    app_id: Some("com.vox.app".into()),
    ..Default::default()
};
```

**Alternatives considered**:
- `WindowKind::Normal` — Would not float above other windows; rejected because the overlay must be visible while the user types in other applications.
- `WindowKind::Floating` — Floats on top of parent window only; rejected because Vox has no parent window concept.

## R-002: System Tray Integration (tray-icon 0.19)

**Decision**: Use `tray-icon` 0.19 crate (already in vox_core/Cargo.toml) with crossbeam channel event polling via GPUI `cx.spawn()`.

**Rationale**: tray-icon is the Tauri team's cross-platform system tray crate, supporting both Windows notification area and macOS menu bar. The crate is already declared as a dependency. Its event model uses `crossbeam_channel::Receiver<MenuEvent>` which must be bridged to GPUI's dispatch system.

**API Pattern**:
```rust
use tray_icon::{TrayIcon, TrayIconBuilder, Icon};
use tray_icon::menu::{Menu, MenuItem, MenuEvent};

// Build menu
let menu = Menu::new();
let toggle_item = MenuItem::new("Toggle Recording", true, None);
let settings_item = MenuItem::new("Settings...", true, None);
let quit_item = MenuItem::new("Quit Vox", true, None);
menu.append(&toggle_item);
menu.append(&settings_item);
menu.append(&quit_item);

// Build tray icon
let icon = Icon::from_rgba(icon_data, width, height)?;
let tray = TrayIconBuilder::new()
    .with_menu(Box::new(menu))
    .with_icon(icon)
    .with_tooltip("Vox — Voice Dictation")
    .build()?;

// Poll events via cx.spawn()
cx.spawn(|cx| async move {
    let receiver = MenuEvent::receiver();
    loop {
        if let Ok(event) = receiver.try_recv() {
            match event.id {
                id if id == toggle_item.id() => { /* dispatch ToggleRecording */ }
                id if id == settings_item.id() => { /* dispatch OpenSettings */ }
                id if id == quit_item.id() => { /* dispatch Quit */ }
                _ => {}
            }
        }
        cx.background_executor().timer(Duration::from_millis(50)).await;
    }
}).detach();
```

**Icon state updates**: Call `tray.set_icon(new_icon)` when PipelineState changes. Prepare 4 icon variants: idle (gray), listening (green), processing (blue), error (red).

**Alternatives considered**:
- Polling in a `background_spawn` thread with blocking `recv()` — Simpler but requires forwarding to GPUI thread, adding channel overhead. The `try_recv()` approach with 50ms timer is cleaner and latency-acceptable for menu clicks.

## R-003: Global Hotkey Registration (global-hotkey 0.6)

**Decision**: Use `global-hotkey` 0.6 crate (already in vox_core/Cargo.toml) with the same crossbeam channel bridging pattern as system tray.

**Rationale**: global-hotkey provides OS-level keyboard shortcut capture that works regardless of which application has focus. The default recording toggle hotkey is CapsLock (from `Settings::activation_hotkey`). The event model mirrors tray-icon's crossbeam pattern.

**API Pattern**:
```rust
use global_hotkey::{GlobalHotKeyManager, GlobalHotKeyEvent, hotkey::{HotKey, Code}};

let manager = GlobalHotKeyManager::new()?;
let hotkey = HotKey::new(None, Code::CapsLock);
manager.register(hotkey)?;

// Poll events (combined with tray polling in same spawn task)
let hotkey_receiver = GlobalHotKeyEvent::receiver();
if let Ok(event) = hotkey_receiver.try_recv() {
    if event.state == HotKeyState::Pressed {
        // dispatch ToggleRecording action
    }
}
```

**Edge cases**:
- Registration may fail if another application already claims the hotkey → log warning, continue without global hotkey (per spec assumptions).
- CapsLock as a hotkey requires platform-specific handling — on some systems CapsLock still toggles caps state. This is a known limitation documented in the settings.

**Alternatives considered**:
- Per-window key bindings via GPUI `bind_keys()` — Only works when the Vox window has focus; rejected because the recording toggle must work from any application.

## R-004: Structured Logging with tracing-appender

**Decision**: Add `tracing-appender` 0.2 to workspace dependencies. Implement logging in a new `vox_core::logging` module following the Tusk pattern. Implement manual 7-day log retention via directory scan at startup.

**Rationale**: tracing-appender provides daily rotating file appender compatible with tracing-subscriber 0.3 (already in workspace). The Tusk codebase (`D:\SRC\tusk\crates\tusk_core\src\logging.rs`) demonstrates the exact pattern: `RollingFileAppender` with `Rotation::DAILY`, non-blocking writes via `WorkerGuard`, env-filter configuration.

**Key detail**: tracing-appender does NOT provide automatic log retention/cleanup. Daily rotation creates new files (e.g., `vox.2026-02-21.log`) but never deletes old ones. The 7-day retention requirement (FR-007) requires manual cleanup: scan the log directory at startup, delete `.log` files with date stamps older than 7 days.

**Log directory paths** (matches existing `state::data_dir()` pattern):
- Windows: `%LOCALAPPDATA%/com.vox.app/logs/`
- macOS: `~/Library/Logs/com.vox.app/`

**Env filter priority**: `VOX_LOG` > `RUST_LOG` > default (`info,vox=info,vox_core=info,vox_ui=info`)

**Log file naming**: `vox.YYYY-MM-DD.log` (daily rotation by tracing-appender)

**Implementation structure** (new module `crates/vox_core/src/logging.rs`):
```rust
pub struct LoggingGuard { _guard: WorkerGuard }
pub fn init_logging() -> LoggingGuard { /* ... */ }
pub fn log_dir() -> PathBuf { /* platform-specific */ }
fn cleanup_old_logs(dir: &Path, retention_days: u32) { /* scan + delete */ }
```

**Alternatives considered**:
- `log4rs` — Supports retention natively but doesn't integrate with tracing ecosystem; rejected for consistency with existing tracing usage.
- Stdout-only logging — Insufficient for post-hoc debugging; rejected per FR-007.

## R-005: Pipeline Initialization Wiring

**Decision**: Use `cx.spawn()` to kick off async model checking, downloading, and pipeline loading after the window is visible. Bridge VoxState readiness updates to GPUI via `cx.update()`.

**Rationale**: The critical requirement is that the window appears BEFORE any ML model loading (FR-001, FR-008). The existing `ModelDownloader` provides `download_missing()` async method and broadcast event channel. The existing `VoxState` provides `set_readiness()` for state machine transitions. The wiring connects these via GPUI's async spawn.

**Flow**:
1. `main.rs` opens overlay window (immediate, <100ms)
2. `cx.spawn()` starts async pipeline initialization
3. Inside spawn: `check_missing_models()` → if missing → `ModelDownloader::download_missing()` → update `AppReadiness::Downloading`
4. After downloads: `set_readiness(AppReadiness::Loading)` → load VAD/ASR/LLM
5. After load: `set_readiness(AppReadiness::Ready)`
6. Each `set_readiness()` call triggers `cx.notify()` on the overlay entity to re-render

**Error handling**: If pipeline initialization fails (GPU unavailable, model corrupt), set readiness to a displayable error state. The overlay shows the error message. The app remains running but dictation is unavailable.

**Existing APIs used**:
- `vox_core::models::check_missing_models() -> Result<Vec<&ModelInfo>>`
- `vox_core::models::ModelDownloader::new() -> Self`
- `vox_core::models::ModelDownloader::download_missing(&self, missing) -> Result<()>`
- `vox_core::state::VoxState::set_readiness(&self, state: AppReadiness)`
- `vox_core::state::VoxState::readiness(&self) -> AppReadiness`

**Alternatives considered**:
- Blocking initialization before window open — Violates FR-001 (100ms window appearance); rejected.
- Separate thread without GPUI integration — Would require custom channel bridging; `cx.spawn()` is the idiomatic GPUI approach and already has access to `AsyncApp` for state updates.

## R-006: Window Close Handling

**Decision**: Follow the Tusk pattern: `on_window_should_close` returns `false` to prevent default close, then `cx.defer(|cx| cx.quit())` to schedule quit after current dispatch completes.

**Rationale**: On Windows, the WM_CLOSE handler can race with WM_ACTIVATE during window destruction. By deferring the quit, GPUI finishes processing the close event before initiating shutdown. This pattern is proven in Tusk (`D:\SRC\tusk\crates\tusk\src\main.rs:66-69`).

**Implementation**:
```rust
window.on_window_should_close(cx, |_window, cx| {
    cx.defer(|cx| cx.quit());
    false
});
```

**Alternatives considered**:
- Returning `true` (allow default close) — Causes race conditions on Windows; rejected.
- Platform-conditional close handling — Unnecessary complexity; the deferred quit pattern is safe on both platforms.

## R-007: Dependencies to Add

**Decision**: Add `tracing-appender` to workspace and `vox` binary crate. No other new dependencies needed.

**New workspace dependency**:
```toml
tracing-appender = "0.2"
```

**Add to `crates/vox/Cargo.toml`**:
```toml
tracing-appender.workspace = true
```

**Already available** (no changes needed):
- `tray-icon = "0.19"` — in vox_core (accessible via `vox_core` dep)
- `global-hotkey = "0.6"` — in vox_core
- `gpui` — workspace dep
- `tracing`, `tracing-subscriber` — workspace deps
- `dirs = "5"` — in vox_core
- `anyhow` — workspace dep

## R-008: Icon Assets

**Decision**: Use programmatically generated colored square icons for system tray states. Place icon PNGs in `assets/icons/`.

**Rationale**: The tray icon needs 4 state variants (idle, listening, processing, error). For the initial implementation, simple colored square icons (16x16 or 32x32) are sufficient. A proper designed icon set can replace these later without code changes.

**Icon set**:
- `tray-idle.png` — Gray square (matches `status_idle` theme color)
- `tray-listening.png` — Green square (matches `status_listening`)
- `tray-processing.png` — Blue square (matches `status_processing`)
- `tray-error.png` — Red square (matches `status_error`)

**Format**: 32x32 RGBA PNG files, loaded via `tray_icon::Icon::from_rgba()`.

**Alternatives considered**:
- SVG icons — tray-icon requires raster format; rejected.
- Single icon with no state indication — Violates FR-010; rejected.
- Embedded icons via `include_bytes!` — Better for distribution; will use this approach to avoid runtime file path issues.
