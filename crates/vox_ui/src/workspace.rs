//! Settings workspace window with sidebar navigation and panel switching.
//!
//! Provides [`VoxWorkspace`] as the root entity for the settings window,
//! [`open_settings_window`] for singleton window management with position
//! persistence, and [`StatusBar`] for runtime status display.

use std::sync::OnceLock;

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Entity, FocusHandle, IntoElement, Point,
    Render, SharedString, Size, Subscription, Window, WindowBounds, WindowHandle, WindowOptions,
};
use parking_lot::Mutex;

use vox_core::state::VoxState;

use crate::dictionary_panel::DictionaryPanel;
use crate::history_panel::HistoryPanel;
use crate::layout::{radius, size as sz, spacing};
use crate::log_panel::LogPanel;
use crate::model_panel::ModelPanel;
use crate::settings_panel::SettingsPanel;
use crate::theme::VoxTheme;

/// Global singleton handle for the settings window.
static SETTINGS_WINDOW: OnceLock<Mutex<Option<WindowHandle<VoxWorkspace>>>> = OnceLock::new();

fn window_handle_store() -> &'static Mutex<Option<WindowHandle<VoxWorkspace>>> {
    SETTINGS_WINDOW.get_or_init(|| Mutex::new(None))
}

/// Return the current settings window handle, if the window is open.
///
/// Used by the diagnostics screenshot handler to capture the settings window.
pub fn settings_window_handle() -> Option<WindowHandle<VoxWorkspace>> {
    *window_handle_store().lock()
}

/// Which panel is currently active in the workspace sidebar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Panel {
    /// General dictation settings.
    Settings,
    /// Transcript history browser.
    History,
    /// Custom dictionary editor.
    Dictionary,
    /// ML model status and management.
    Model,
    /// Live application log viewer.
    Log,
}

/// Root entity for the settings workspace window.
///
/// Holds all five panel entities, the active panel selector, and
/// manages sidebar navigation. Created by [`open_settings_window`].
pub struct VoxWorkspace {
    active_panel: Panel,
    settings_panel: Entity<SettingsPanel>,
    history_panel: Entity<HistoryPanel>,
    dictionary_panel: Entity<DictionaryPanel>,
    model_panel: Entity<ModelPanel>,
    log_panel: Entity<LogPanel>,
    focus_handle: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl VoxWorkspace {
    /// Create the workspace and all child panel entities.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings_panel = cx.new(|cx| SettingsPanel::new(window, cx));
        let history_panel = cx.new(|cx| HistoryPanel::new(window, cx));
        let dictionary_panel = cx.new(|cx| DictionaryPanel::new(window, cx));
        let model_panel = cx.new(|cx| ModelPanel::new(window, cx));
        let log_panel = cx.new(|cx| LogPanel::new(window, cx));

        let sub = cx.observe_global::<VoxState>(|_this, cx| {
            cx.notify();
        });

        Self {
            active_panel: Panel::Settings,
            settings_panel,
            history_panel,
            dictionary_panel,
            model_panel,
            log_panel,
            focus_handle: cx.focus_handle(),
            _subscriptions: vec![sub],
        }
    }

    /// Render the sidebar navigation column.
    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();

        div()
            .flex()
            .flex_col()
            .w(sz::SIDEBAR_WIDTH)
            .h_full()
            .bg(theme.colors.surface)
            .border_r_1()
            .border_color(theme.colors.border)
            .p(spacing::SM)
            .gap(spacing::XS)
            .child(self.sidebar_item("Settings", Panel::Settings, cx))
            .child(self.sidebar_item("History", Panel::History, cx))
            .child(self.sidebar_item("Dictionary", Panel::Dictionary, cx))
            .child(self.sidebar_item("Models", Panel::Model, cx))
            .child(self.sidebar_item("Logs", Panel::Log, cx))
    }

    /// Render a single sidebar navigation item.
    fn sidebar_item(
        &self,
        label: &'static str,
        panel: Panel,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let is_active = self.active_panel == panel;

        div()
            .id(SharedString::from(label))
            .px(spacing::MD)
            .py(spacing::SM)
            .rounded(radius::SM)
            .cursor_pointer()
            .when(is_active, |d| {
                d.bg(theme.colors.accent)
                    .text_color(theme.colors.button_primary_text)
            })
            .when(!is_active, |d| {
                d.text_color(theme.colors.text_muted)
                    .hover(|d| d.bg(theme.colors.elevated_surface))
            })
            .child(SharedString::from(label))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.active_panel = panel;
                cx.notify();
            }))
    }
}

impl Render for VoxWorkspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();

        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.colors.surface)
            .text_color(theme.colors.text)
            .flex()
            .flex_col()
            .child(
                // Main content area: sidebar + active panel.
                // overflow_hidden() at each flex level bounds children so
                // inner scroll containers can trigger overflow_y_scroll().
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .overflow_hidden()
                    .child(self.render_sidebar(cx))
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .child(match self.active_panel {
                                Panel::Settings => {
                                    self.settings_panel.clone().into_any_element()
                                }
                                Panel::History => {
                                    self.history_panel.clone().into_any_element()
                                }
                                Panel::Dictionary => {
                                    self.dictionary_panel.clone().into_any_element()
                                }
                                Panel::Model => self.model_panel.clone().into_any_element(),
                                Panel::Log => self.log_panel.clone().into_any_element(),
                            }),
                    ),
            )
            .child(StatusBar::new(cx))
    }
}

/// Bottom status bar showing pipeline state, latency, VRAM, and audio device.
#[derive(IntoElement)]
pub struct StatusBar {
    status_text: SharedString,
    latency_text: SharedString,
    vram_text: SharedString,
    audio_text: SharedString,
}

impl StatusBar {
    /// Build status bar content from current VoxState.
    fn new(cx: &mut Context<VoxWorkspace>) -> Self {
        let state = cx.global::<VoxState>();

        let status_text = match state.readiness() {
            vox_core::state::AppReadiness::Ready => SharedString::from("Ready"),
            vox_core::state::AppReadiness::Loading { stage } => {
                SharedString::from(format!("Loading: {stage}"))
            }
            vox_core::state::AppReadiness::Downloading { .. } => {
                SharedString::from("Downloading models...")
            }
            vox_core::state::AppReadiness::Error { message } => {
                SharedString::from(format!("Error: {message}"))
            }
        };

        let latency_ms = state.last_latency_ms().or_else(|| {
            // Fall back to the most recent transcript's latency for cross-session display
            state
                .get_transcripts(1, 0)
                .ok()
                .and_then(|t| t.first().map(|e| e.latency_ms))
        });
        let latency_text = match latency_ms {
            Some(ms) => SharedString::from(format!("Latency: {ms}ms")),
            None => SharedString::from("Latency: --"),
        };

        let model_runtime = state.all_model_runtime();
        let total_vram: u64 = model_runtime
            .values()
            .filter_map(|info| info.vram_bytes)
            .sum();
        let vram_text = if total_vram > 0 {
            let vram_gb = total_vram as f64 / (1024.0 * 1024.0 * 1024.0);
            SharedString::from(format!("VRAM: {vram_gb:.1} GB"))
        } else {
            SharedString::from("VRAM: --")
        };

        let settings = state.settings();
        let audio_text = SharedString::from(format!(
            "Audio: {}",
            settings
                .input_device
                .as_deref()
                .unwrap_or("Default Device")
        ));

        Self {
            status_text,
            latency_text,
            vram_text,
            audio_text,
        }
    }
}

impl RenderOnce for StatusBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();

        div()
            .h(sz::STATUS_BAR_HEIGHT)
            .w_full()
            .flex()
            .items_center()
            .px(spacing::MD)
            .gap(spacing::LG)
            .bg(theme.colors.surface)
            .border_t_1()
            .border_color(theme.colors.border)
            .text_size(px(12.0))
            .text_color(theme.colors.text_muted)
            .child(self.status_text)
            .child(SharedString::from("|"))
            .child(self.latency_text)
            .child(SharedString::from("|"))
            .child(self.vram_text)
            .child(SharedString::from("|"))
            .child(self.audio_text)
    }
}

/// Open the settings window, or focus it if already open.
///
/// Uses a singleton pattern: only one settings window exists at a time.
/// Window position and size are restored from persisted settings.
pub fn open_settings_window(cx: &mut App) {
    let store = window_handle_store();
    let mut handle_guard = store.lock();

    // If window already open, just focus it
    if let Some(ref handle) = *handle_guard {
        if handle
            .update(cx, |_, window, _cx| {
                window.activate_window();
            })
            .is_ok()
        {
            return;
        }
        // Window was closed/invalid, clear the handle
        *handle_guard = None;
    }

    // Read saved window position
    let state = cx.global::<VoxState>();
    let settings = state.settings();

    let default_size = size(px(800.0), px(600.0));
    let bounds = match (
        settings.window_x,
        settings.window_y,
        settings.window_width,
        settings.window_height,
    ) {
        (Some(x), Some(y), Some(w), Some(h)) => {
            let origin = gpui::point(px(x), px(y));

            // Clamp to a connected display so the window is never off-screen
            let target_display = cx
                .displays()
                .into_iter()
                .find(|display| display.bounds().contains(&origin));

            if let Some(display) = target_display {
                let db = display.bounds();
                let db_x: f32 = db.origin.x.into();
                let db_y: f32 = db.origin.y.into();
                let db_w: f32 = db.size.width.into();
                let db_h: f32 = db.size.height.into();
                // Clamp size to fit the display (floor at min window size, but
                // never exceed the display itself)
                let clamped_w = w.min(db_w).max(400.0_f32.min(db_w));
                let clamped_h = h.min(db_h).max(300.0_f32.min(db_h));
                let clamped_x = x.max(db_x).min(db_x + db_w - clamped_w);
                let clamped_y = y.max(db_y).min(db_y + db_h - clamped_h);
                Bounds {
                    origin: Point {
                        x: px(clamped_x),
                        y: px(clamped_y),
                    },
                    size: Size {
                        width: px(clamped_w),
                        height: px(clamped_h),
                    },
                }
            } else {
                tracing::warn!(
                    x,
                    y,
                    "saved settings window position is off-screen, centering on primary display"
                );
                // Clamp saved size to the primary display
                let fallback_size = if let Some(primary) = cx.primary_display() {
                    let pb = primary.bounds();
                    let pw: f32 = pb.size.width.into();
                    let ph: f32 = pb.size.height.into();
                    size(px(w.min(pw)), px(h.min(ph)))
                } else {
                    default_size
                };
                Bounds::centered(None, fallback_size, cx)
            }
        }
        _ => Bounds::centered(None, default_size, cx),
    };
    drop(settings);

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        window_min_size: Some(gpui::Size {
            width: px(400.0),
            height: px(300.0),
        }),
        focus: true,
        show: true,
        ..Default::default()
    };

    match cx.open_window(options, |window, cx| {
        cx.new(|cx| VoxWorkspace::new(window, cx))
    }) {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, cx| {
                // Clear singleton and persist bounds when window closes
                window.on_window_should_close(cx, |window, cx| {
                    let bounds = window.window_bounds();
                    if let WindowBounds::Windowed(bounds) = bounds {
                        let state = cx.global::<VoxState>();
                        let x: f32 = bounds.origin.x.into();
                        let y: f32 = bounds.origin.y.into();
                        let w: f32 = bounds.size.width.into();
                        let h: f32 = bounds.size.height.into();
                        if let Err(err) = state.update_settings(|s| {
                            s.window_x = Some(x);
                            s.window_y = Some(y);
                            s.window_width = Some(w);
                            s.window_height = Some(h);
                        }) {
                            tracing::error!(%err, "failed to save window bounds");
                        }
                    }
                    // Clear singleton handle
                    let store = window_handle_store();
                    let mut guard = store.lock();
                    *guard = None;
                    true
                });
            });

            *handle_guard = Some(handle);
        }
        Err(err) => {
            tracing::error!(%err, "failed to open settings window");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_switching() {
        // Verify Panel enum values are distinct and Copy works
        let panels = [
            Panel::Settings,
            Panel::History,
            Panel::Dictionary,
            Panel::Model,
            Panel::Log,
        ];

        for (i, a) in panels.iter().enumerate() {
            for (j, b) in panels.iter().enumerate() {
                if i == j {
                    assert_eq!(*a, *b);
                } else {
                    assert_ne!(*a, *b);
                }
            }
        }

        // Verify Copy semantics
        let p = Panel::History;
        let p2 = p;
        assert_eq!(p, p2);
    }
}
