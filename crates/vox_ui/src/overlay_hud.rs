//! Overlay HUD window view displaying real-time dictation pipeline state.
//!
//! Provides [`OverlayHud`], a compact floating pill window that shows the
//! current state of the dictation pipeline. Uses [`OverlayDisplayState`] as a
//! reactive bridge — replaced via `cx.set_global()` on every state change to
//! trigger `observe_global` callbacks that drive re-rendering.
//!
//! Also provides [`open_overlay_window`] to create the always-on-top,
//! borderless, semi-transparent overlay window with position persistence.

use std::collections::VecDeque;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    div, point, px, relative, size, Animation, AnimationExt, App, ClipboardItem, Hsla,
    SharedString, Subscription, Task, TitlebarOptions, Window, WindowBackgroundAppearance,
    WindowBounds, WindowControlArea, WindowHandle, WindowKind, WindowOptions,
};

use crate::key_bindings::{OpenModelFolder, RetryDownload};
use crate::layout::{radius, size as layout_size, spacing};
use crate::theme::{ThemeColors, VoxTheme};
use crate::waveform::WaveformVisualizer;
use vox_core::config::OverlayPosition;
use vox_core::models::DownloadProgress;
use vox_core::pipeline::state::PipelineState;
use vox_core::state::{AppReadiness, VoxState};

/// Stores the overlay window handle for visibility toggling via [`ToggleOverlay`].
///
/// Set by [`open_overlay_window`] and read by the `ToggleOverlay` action handler
/// in `key_bindings.rs`. `None` if the overlay has not been opened yet.
pub struct OverlayWindowHandle(pub Option<WindowHandle<OverlayHud>>);

impl gpui::Global for OverlayWindowHandle {}

/// Reactive bridge between VoxState and the overlay HUD.
///
/// GPUI's `observe_global` triggers when a global is replaced via `set_global()`.
/// Since `VoxState` uses interior mutability (`RwLock`) and is set once at startup,
/// mutations to its fields do NOT trigger observers. This lightweight global is
/// replaced on every state change, providing the reactivity the overlay needs.
///
/// Every mutation of `VoxState::readiness` or `VoxState::pipeline_state` MUST be
/// immediately followed by `cx.set_global(OverlayDisplayState { ... })` with the
/// updated values. Failure to do so causes the overlay to show stale state.
#[derive(Clone)]
pub struct OverlayDisplayState {
    /// Current app lifecycle state. Mirrors `VoxState::readiness()`.
    pub readiness: AppReadiness,
    /// Current pipeline operational state. Mirrors `VoxState::pipeline_state()`.
    pub pipeline_state: PipelineState,
}

impl gpui::Global for OverlayDisplayState {}

/// Number of RMS amplitude samples held for waveform visualization.
const WAVEFORM_CAPACITY: usize = 50;

/// The primary overlay HUD view — a compact floating pill that displays
/// the current state of the dictation pipeline. Always-on-top, borderless,
/// semi-transparent, and updates in real-time.
///
/// Subscribes to `OverlayDisplayState` for reactive state updates and
/// runs animation timers for waveform visualization and fade effects.
pub struct OverlayHud {
    /// Current app lifecycle state (Downloading, Loading, Ready, Error).
    readiness: AppReadiness,
    /// Current pipeline operational state (Idle, Listening, Processing, etc.).
    pipeline_state: PipelineState,
    /// Ring buffer of recent RMS amplitude values for waveform visualization.
    waveform_samples: VecDeque<f32>,
    /// Whether the quick settings dropdown is currently visible.
    quick_settings_open: bool,
    /// Whether the injected text is currently showing (2-second fade timer).
    showing_injected_fade: bool,
    /// Whether the overlay is visible. Toggled by the `ToggleOverlay` action.
    /// When false, renders an empty transparent div and hides via OS API.
    visible: bool,
    /// Active subscriptions (observe_global, observe_window_bounds).
    _subscriptions: Vec<Subscription>,
    /// Background task for waveform animation timer (30fps during Listening).
    _waveform_task: Option<Task<()>>,
    /// Background task for injected text fade timer (2 seconds).
    _fade_task: Option<Task<()>>,
}

impl OverlayHud {
    /// Creates a new overlay HUD and subscribes to state changes.
    ///
    /// Registers `observe_global::<OverlayDisplayState>()` for state reactivity
    /// and `observe_window_bounds()` for position persistence.
    pub fn new(window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let display = cx.global::<OverlayDisplayState>();
        let readiness = display.readiness.clone();
        let pipeline_state = display.pipeline_state.clone();

        let mut subscriptions = Vec::new();

        subscriptions.push(cx.observe_global::<OverlayDisplayState>(|this, cx| {
            this.on_state_changed(cx);
        }));

        subscriptions.push(cx.observe_window_bounds(window, |this, window, cx| {
            this.on_position_changed(window, cx);
        }));

        Self {
            readiness,
            pipeline_state,
            waveform_samples: VecDeque::with_capacity(WAVEFORM_CAPACITY),
            quick_settings_open: false,
            showing_injected_fade: false,
            visible: true,
            _subscriptions: subscriptions,
            _waveform_task: None,
            _fade_task: None,
        }
    }

    /// Handles state change from OverlayDisplayState observer.
    fn on_state_changed(&mut self, cx: &mut gpui::Context<Self>) {
        let display = cx.global::<OverlayDisplayState>();
        let old_pipeline = self.pipeline_state.clone();
        self.readiness = display.readiness.clone();
        self.pipeline_state = display.pipeline_state.clone();

        // Start/stop waveform animation on Listening transitions
        match (&old_pipeline, &self.pipeline_state) {
            (_, PipelineState::Listening) if old_pipeline != PipelineState::Listening => {
                self.start_waveform_animation(cx);
            }
            (PipelineState::Listening, _) => {
                self.stop_waveform_animation();
            }
            _ => {}
        }

        // Cancel fade timer when leaving Injecting (edge case: new state during fade)
        if matches!(old_pipeline, PipelineState::Injecting { .. })
            && !matches!(self.pipeline_state, PipelineState::Injecting { .. })
        {
            self._fade_task = None;
            self.showing_injected_fade = false;
        }

        // Start fade timer on entering Injecting
        if matches!(self.pipeline_state, PipelineState::Injecting { .. })
            && !matches!(old_pipeline, PipelineState::Injecting { .. })
        {
            self.start_injection_fade(cx);
        }

        self.quick_settings_open = false;
        cx.notify();
    }

    /// Starts the 30fps waveform animation timer. Called on transition to Listening.
    fn start_waveform_animation(&mut self, cx: &mut gpui::Context<Self>) {
        self.waveform_samples.clear();
        let executor = cx.background_executor().clone();
        self._waveform_task = Some(cx.spawn(async move |this, cx| {
            loop {
                executor.timer(Duration::from_millis(33)).await;
                let result = this.update(cx, |this, cx| {
                    let rms = cx.global::<VoxState>().latest_rms();
                    if this.waveform_samples.len() >= WAVEFORM_CAPACITY {
                        this.waveform_samples.pop_front();
                    }
                    this.waveform_samples.push_back(rms);
                    cx.notify();
                });
                if result.is_err() {
                    break;
                }
            }
        }));
    }

    /// Stops the waveform animation timer. Called on transition out of Listening.
    fn stop_waveform_animation(&mut self) {
        self._waveform_task = None;
    }

    /// Starts the 2-second fade timer for injected text. Called on transition to Injecting.
    fn start_injection_fade(&mut self, cx: &mut gpui::Context<Self>) {
        self.showing_injected_fade = true;
        let executor = cx.background_executor().clone();
        self._fade_task = Some(cx.spawn(async move |this, cx| {
            executor.timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                this.showing_injected_fade = false;
                this.pipeline_state = PipelineState::Idle;
                cx.global::<VoxState>()
                    .set_pipeline_state(PipelineState::Idle);
                let readiness = cx.global::<VoxState>().readiness();
                let pipeline = cx.global::<VoxState>().pipeline_state();
                cx.set_global(OverlayDisplayState {
                    readiness,
                    pipeline_state: pipeline,
                });
                cx.notify();
            });
        }));
    }

    /// Toggle the overlay's visibility. Uses OS-level window hiding on
    /// Windows; on macOS the transparent empty render is sufficient.
    pub fn toggle_visibility(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.visible = !self.visible;
        #[cfg(target_os = "windows")]
        set_window_visible(_window, self.visible);
        cx.notify();
    }

    /// Handles window position change. Persists to settings.
    fn on_position_changed(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        let bounds = window.bounds();
        let position = OverlayPosition::Custom {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
        };
        if let Err(err) = cx.global::<VoxState>().update_settings(|s| {
            s.overlay_position = position;
        }) {
            tracing::warn!(%err, "failed to save overlay position");
        }
    }

    /// Copies polished text to clipboard and transitions to Idle (injection failure recovery).
    fn copy_to_clipboard(&mut self, text: &str, cx: &mut gpui::Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
        self.pipeline_state = PipelineState::Idle;
        cx.global::<VoxState>()
            .set_pipeline_state(PipelineState::Idle);
        let readiness = cx.global::<VoxState>().readiness();
        let pipeline = cx.global::<VoxState>().pipeline_state();
        cx.set_global(OverlayDisplayState {
            readiness,
            pipeline_state: pipeline,
        });
        cx.notify();
    }

    /// Renders the status bar: indicator dot, state label, "Vox" title, quick settings trigger.
    fn render_status_bar(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let colors = &theme.colors;

        let (label, indicator_color, pulsing) =
            status_indicator(&self.readiness, &self.pipeline_state, colors);

        let indicator = div()
            .size(px(8.0))
            .rounded_full()
            .bg(indicator_color);

        let indicator_element = if pulsing {
            indicator
                .with_animation(
                    "indicator-pulse",
                    Animation::new(Duration::from_secs(1)).repeat(),
                    |div, delta| {
                        let t = (delta * std::f32::consts::PI).sin();
                        div.opacity(0.4 + t * 0.6)
                    },
                )
                .into_any_element()
        } else {
            indicator.into_any_element()
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(spacing::SM)
            .px(spacing::MD)
            .h(px(28.0))
            .child(indicator_element)
            .child(
                div()
                    .text_xs()
                    .text_color(colors.text_muted)
                    .child(label),
            )
            .child(div().flex_grow())
            .child(div().text_sm().text_color(colors.text).child("Vox"))
            .child(div().flex_grow())
            .child(
                div()
                    .id("quick-settings-toggle")
                    .text_xs()
                    .text_color(colors.text_muted)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _event, _window, cx| {
                        this.quick_settings_open = !this.quick_settings_open;
                        cx.notify();
                    }))
                    .child("▾"),
            )
    }

    /// Renders the state-dependent content area below the status bar.
    fn render_content(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        if self.quick_settings_open {
            return self.render_quick_settings(cx).into_any_element();
        }

        match &self.readiness {
            AppReadiness::Downloading {
                vad_progress,
                whisper_progress,
                llm_progress,
            } => {
                // Route to render_download_failed if any model has failed
                let models = [
                    ("VAD", vad_progress),
                    ("ASR", whisper_progress),
                    ("LLM", llm_progress),
                ];
                if let Some((name, error, manual_url)) =
                    models.iter().find_map(|(name, progress)| {
                        if let DownloadProgress::Failed { error, manual_url } = progress {
                            Some((*name, error.as_str(), manual_url.as_str()))
                        } else {
                            None
                        }
                    })
                {
                    self.render_download_failed(name, error, manual_url, cx)
                        .into_any_element()
                } else {
                    self.render_download_progress(vad_progress, whisper_progress, llm_progress, cx)
                        .into_any_element()
                }
            }
            AppReadiness::Loading { stage } => {
                self.render_loading(stage, cx).into_any_element()
            }
            AppReadiness::Error { message } => {
                self.render_error(message, cx).into_any_element()
            }
            AppReadiness::Ready => match &self.pipeline_state {
                PipelineState::Idle => self.render_idle_hint(cx).into_any_element(),
                PipelineState::Listening => self.render_waveform(cx).into_any_element(),
                PipelineState::Processing { raw_text } => {
                    self.render_processing(raw_text, cx).into_any_element()
                }
                PipelineState::Injecting { polished_text } => {
                    self.render_injected(polished_text, cx).into_any_element()
                }
                PipelineState::InjectionFailed {
                    polished_text,
                    error,
                } => self
                    .render_injection_failed(polished_text, error, cx)
                    .into_any_element(),
                PipelineState::Error { message } => {
                    self.render_error(message, cx).into_any_element()
                }
            },
        }
    }

    /// Renders idle hint text with configured hotkey name.
    fn render_idle_hint(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let hotkey = cx.global::<VoxState>().settings().activation_hotkey.clone();
        div()
            .flex()
            .items_center()
            .justify_center()
            .flex_grow()
            .text_xs()
            .text_color(theme.colors.text_muted)
            .child(SharedString::from(format!(
                "Press {hotkey} to start dictating"
            )))
    }

    /// Renders the waveform visualizer during Listening state.
    fn render_waveform(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let samples: Vec<f32> = self.waveform_samples.iter().copied().collect();
        div()
            .flex()
            .items_center()
            .justify_center()
            .flex_grow()
            .child(WaveformVisualizer::new(
                samples,
                theme.colors.waveform_active,
                theme.colors.waveform_inactive,
            ))
    }

    /// Renders raw transcript text during Processing state.
    fn render_processing(
        &self,
        raw_text: &Option<String>,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let text = raw_text.as_deref().unwrap_or("Transcribing...");
        div()
            .flex()
            .items_center()
            .justify_center()
            .flex_grow()
            .px(spacing::MD)
            .overflow_hidden()
            .text_xs()
            .text_color(theme.colors.text)
            .child(SharedString::from(truncate_text(text, 60)))
    }

    /// Renders polished text after injection (with fade animation).
    fn render_injected(
        &self,
        polished_text: &str,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let opacity = if self.showing_injected_fade { 1.0 } else { 0.5 };
        div()
            .flex()
            .items_center()
            .justify_center()
            .flex_grow()
            .px(spacing::MD)
            .overflow_hidden()
            .text_xs()
            .text_color(theme.colors.status_success)
            .opacity(opacity)
            .child(SharedString::from(truncate_text(polished_text, 60)))
    }

    /// Renders injection failure: polished text + Copy button.
    fn render_injection_failed(
        &self,
        polished_text: &str,
        _error: &str,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let text_to_copy = polished_text.to_string();
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_grow()
            .px(spacing::MD)
            .gap(spacing::SM)
            .child(
                div()
                    .flex_grow()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(theme.colors.text)
                    .child(SharedString::from(truncate_text(polished_text, 40))),
            )
            .child(
                div()
                    .id("copy-injected-text")
                    .px(spacing::SM)
                    .py(spacing::XS)
                    .rounded(radius::SM)
                    .bg(theme.colors.button_primary_bg)
                    .text_xs()
                    .text_color(theme.colors.button_primary_text)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        this.copy_to_clipboard(&text_to_copy, cx);
                    }))
                    .child("Copy"),
            )
    }

    /// Renders error state: error message.
    fn render_error(&self, message: &str, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        div()
            .flex()
            .items_center()
            .justify_center()
            .flex_grow()
            .px(spacing::MD)
            .overflow_hidden()
            .text_xs()
            .text_color(theme.colors.status_error)
            .child(SharedString::from(truncate_text(message, 60)))
    }

    /// Renders download progress: model name, percentage, progress bar.
    fn render_download_progress(
        &self,
        vad: &DownloadProgress,
        whisper: &DownloadProgress,
        llm: &DownloadProgress,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let overall = (progress_fraction(vad) + progress_fraction(whisper)
            + progress_fraction(llm))
            / 3.0;
        let percent = overall * 100.0;

        let active_model = if !matches!(vad, DownloadProgress::Complete) {
            "VAD"
        } else if !matches!(whisper, DownloadProgress::Complete) {
            "ASR"
        } else {
            "LLM"
        };

        div()
            .flex()
            .flex_col()
            .flex_grow()
            .justify_center()
            .px(spacing::MD)
            .gap(spacing::XS)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.text)
                    .child(SharedString::from(format!(
                        "{active_model} model \u{2014} {percent:.0}%"
                    ))),
            )
            .child(
                div()
                    .w_full()
                    .h(layout_size::PROGRESS_BAR_HEIGHT)
                    .rounded(radius::SM)
                    .bg(theme.colors.surface)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .rounded(radius::SM)
                            .bg(theme.colors.status_downloading)
                            .w(relative(overall)),
                    ),
            )
    }

    /// Renders download failure: error message, Retry and Open Folder buttons.
    fn render_download_failed(
        &self,
        model_name: &str,
        error: &str,
        _manual_url: &str,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        div()
            .flex()
            .flex_col()
            .flex_grow()
            .justify_center()
            .px(spacing::MD)
            .gap(spacing::XS)
            .child(
                div()
                    .text_xs()
                    .text_color(theme.colors.status_error)
                    .overflow_hidden()
                    .child(SharedString::from(truncate_text(
                        &format!("{model_name}: {error}"),
                        50,
                    ))),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .id("retry-download")
                            .px(spacing::SM)
                            .py(spacing::XS)
                            .rounded(radius::SM)
                            .bg(theme.colors.button_primary_bg)
                            .text_xs()
                            .text_color(theme.colors.button_primary_text)
                            .cursor_pointer()
                            .on_click(cx.listener(|_this, _event, _window, cx| {
                                cx.dispatch_action(&RetryDownload);
                            }))
                            .child("Retry"),
                    )
                    .child(
                        div()
                            .id("open-model-folder")
                            .px(spacing::SM)
                            .py(spacing::XS)
                            .rounded(radius::SM)
                            .bg(theme.colors.button_secondary_bg)
                            .text_xs()
                            .text_color(theme.colors.button_secondary_text)
                            .cursor_pointer()
                            .on_click(cx.listener(|_this, _event, _window, cx| {
                                cx.dispatch_action(&OpenModelFolder);
                            }))
                            .child("Open Folder"),
                    ),
            )
    }

    /// Renders loading stage text.
    fn render_loading(&self, stage: &str, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let display = if stage.trim().is_empty() {
            "Loading..."
        } else {
            stage
        };
        div()
            .flex()
            .items_center()
            .justify_center()
            .flex_grow()
            .text_xs()
            .text_color(theme.colors.text_muted)
            .child(SharedString::from(display.to_string()))
    }

    /// Renders the quick settings dropdown (language and raw transcript toggle).
    fn render_quick_settings(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let settings = cx.global::<VoxState>().settings();
        let language = settings.language.clone();
        let show_raw = settings.show_raw_transcript;
        drop(settings);

        div()
            .flex()
            .flex_col()
            .flex_grow()
            .justify_center()
            .px(spacing::MD)
            .gap(spacing::XS)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.text_muted)
                            .child("Language"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.text)
                            .child(SharedString::from(language)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.colors.text_muted)
                            .child("Show raw text"),
                    )
                    .child(
                        div().text_xs().text_color(if show_raw {
                            theme.colors.status_success
                        } else {
                            theme.colors.text_muted
                        })
                        .child(if show_raw { "On" } else { "Off" }),
                    ),
            )
    }
}

impl gpui::Render for OverlayHud {
    /// Renders the overlay as a vertical flex container with status bar and content area.
    /// When hidden, renders an empty transparent div (OS-level hiding handles
    /// the actual invisibility on Windows; macOS compositing makes transparent
    /// windows click-through).
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        if !self.visible {
            return div().into_any_element();
        }

        let theme = cx.global::<VoxTheme>();
        let opacity = cx.global::<VoxState>().settings().overlay_opacity;

        div()
            .w(layout_size::OVERLAY_WIDTH)
            .h(layout_size::OVERLAY_HEIGHT)
            .bg(theme.colors.overlay_bg)
            .rounded(radius::LG)
            .border_1()
            .border_color(theme.colors.border)
            .flex()
            .flex_col()
            .opacity(opacity)
            .window_control_area(WindowControlArea::Drag)
            .child(self.render_status_bar(cx))
            .child(self.render_content(cx))
            .into_any_element()
    }
}

/// Opens the overlay HUD as a floating, always-on-top, borderless window.
///
/// Window configuration: `WindowKind::PopUp`, `focus: false` (no focus stealing),
/// `WindowDecorations::Client` (no OS chrome), transparent background.
/// Position is restored from settings if available, otherwise centered.
pub fn open_overlay_window(cx: &mut App) -> anyhow::Result<WindowHandle<OverlayHud>> {
    let window_size = size(layout_size::OVERLAY_WIDTH, layout_size::OVERLAY_HEIGHT);

    let settings = cx.global::<VoxState>().settings();
    let bounds = match &settings.overlay_position {
        OverlayPosition::Custom { x, y } => {
            let origin = point(px(*x), px(*y));
            let target_display = cx
                .displays()
                .into_iter()
                .find(|display| display.bounds().contains(&origin));
            if let Some(display) = target_display {
                // Clamp so the full window rect stays within the display
                let db = display.bounds();
                let db_x = f32::from(db.origin.x);
                let db_y = f32::from(db.origin.y);
                let db_w = f32::from(db.size.width);
                let db_h = f32::from(db.size.height);
                let win_w = f32::from(window_size.width);
                let win_h = f32::from(window_size.height);
                let clamped_x = (*x).max(db_x).min(db_x + db_w - win_w);
                let clamped_y = (*y).max(db_y).min(db_y + db_h - win_h);
                gpui::Bounds::new(point(px(clamped_x), px(clamped_y)), window_size)
            } else {
                tracing::warn!(
                    x, y,
                    "saved overlay position is off-screen, centering on primary display"
                );
                gpui::Bounds::centered(None, window_size, cx)
            }
        }
        _ => gpui::Bounds::centered(None, window_size, cx),
    };
    drop(settings);

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: None,
        }),
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };

    let handle = cx.open_window(options, |window, cx| {
        // GPUI's PopUp uses WS_EX_TOOLWINDOW on Windows, which does NOT set
        // WS_EX_TOPMOST. Manually call SetWindowPos to make the overlay
        // stay above all other windows (including other applications).
        #[cfg(target_os = "windows")]
        set_window_topmost(window);

        window.on_window_should_close(cx, |_window, cx| {
            cx.defer(|cx| cx.quit());
            false
        });
        cx.new(|cx| OverlayHud::new(window, cx))
    })?;

    cx.set_global(OverlayWindowHandle(Some(handle)));
    tracing::info!("overlay HUD window opened");
    Ok(handle)
}

/// Sets the window to always-on-top using Win32 `SetWindowPos` with `HWND_TOPMOST`.
///
/// GPUI's `WindowKind::PopUp` only applies `WS_EX_TOOLWINDOW` on Windows (hides
/// from taskbar, smaller title bar) but does NOT set `HWND_TOPMOST`. On macOS,
/// `PopUp` maps to `NSPopUpWindowLevel` which IS always-on-top. This function
/// bridges the platform gap.
#[cfg(target_os = "windows")]
fn set_window_topmost(window: &Window) {
    use raw_window_handle::HasWindowHandle;

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        tracing::warn!("failed to get native window handle for topmost");
        return;
    };

    let raw_window_handle::RawWindowHandle::Win32(win32) = handle.as_raw() else {
        tracing::warn!("unexpected non-Win32 window handle");
        return;
    };

    let hwnd = win32.hwnd.get() as isize;
    const HWND_TOPMOST: isize = -1;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOACTIVATE: u32 = 0x0010;

    let result = unsafe {
        win32_set_window_pos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    };

    if result == 0 {
        tracing::warn!("SetWindowPos(HWND_TOPMOST) failed");
    } else {
        tracing::info!("overlay window set to always-on-top");
    }
}

/// Shows or hides the overlay window using Win32 `ShowWindow`.
///
/// `SW_HIDE` makes the window invisible at the OS level (no click
/// interception, no taskbar entry). `SW_SHOWNOACTIVATE` restores it
/// without stealing focus from the user's current application.
#[cfg(target_os = "windows")]
fn set_window_visible(window: &Window, visible: bool) {
    use raw_window_handle::HasWindowHandle;

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        tracing::warn!("failed to get native window handle for visibility toggle");
        return;
    };

    let raw_window_handle::RawWindowHandle::Win32(win32) = handle.as_raw() else {
        tracing::warn!("unexpected non-Win32 window handle");
        return;
    };

    let hwnd = win32.hwnd.get() as isize;
    const SW_HIDE: i32 = 0;
    const SW_SHOWNOACTIVATE: i32 = 4;

    let cmd = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
    unsafe {
        win32_show_window(hwnd, cmd);
    }
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "SetWindowPos"]
    fn win32_set_window_pos(
        hwnd: isize,
        hwnd_insert_after: isize,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;

    #[link_name = "ShowWindow"]
    fn win32_show_window(hwnd: isize, cmd_show: i32) -> i32;
}

/// Map readiness and pipeline state to display label, indicator color, and pulse flag.
fn status_indicator(
    readiness: &AppReadiness,
    pipeline_state: &PipelineState,
    colors: &ThemeColors,
) -> (SharedString, Hsla, bool) {
    match readiness {
        AppReadiness::Downloading { .. } => {
            ("DOWNLOADING".into(), colors.status_downloading, false)
        }
        AppReadiness::Loading { .. } => ("LOADING".into(), colors.status_loading, false),
        AppReadiness::Error { .. } => ("ERROR".into(), colors.status_error, false),
        AppReadiness::Ready => match pipeline_state {
            PipelineState::Idle => ("IDLE".into(), colors.status_idle, false),
            PipelineState::Listening => ("LISTENING".into(), colors.status_listening, true),
            PipelineState::Processing { .. } => {
                ("PROCESSING".into(), colors.status_processing, false)
            }
            PipelineState::Injecting { .. } => {
                ("INJECTED".into(), colors.status_success, false)
            }
            PipelineState::InjectionFailed { .. } => {
                ("FAILED".into(), colors.status_injection_failed, false)
            }
            PipelineState::Error { .. } => ("ERROR".into(), colors.status_error, false),
        },
    }
}

/// Calculate download progress as a fraction (0.0–1.0).
fn progress_fraction(progress: &DownloadProgress) -> f32 {
    match progress {
        DownloadProgress::Pending => 0.0,
        DownloadProgress::InProgress {
            bytes_downloaded,
            bytes_total,
        } => {
            if *bytes_total == 0 {
                0.0
            } else {
                (*bytes_downloaded as f32 / *bytes_total as f32).clamp(0.0, 1.0)
            }
        }
        DownloadProgress::Complete => 1.0,
        DownloadProgress::Failed { .. } => 0.0,
    }
}

/// Truncate text to a maximum character count with ellipsis.
fn truncate_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;
    use vox_core::models::DownloadProgress;

    #[test]
    fn test_status_indicator_downloading() {
        let colors = test_colors();
        let (label, color, pulse) = status_indicator(
            &AppReadiness::Downloading {
                vad_progress: DownloadProgress::Pending,
                whisper_progress: DownloadProgress::Pending,
                llm_progress: DownloadProgress::Pending,
            },
            &PipelineState::Idle,
            &colors,
        );
        assert_eq!(label, "DOWNLOADING");
        assert_eq!(color, colors.status_downloading);
        assert!(!pulse);
    }

    #[test]
    fn test_status_indicator_listening_pulses() {
        let colors = test_colors();
        let (label, color, pulse) = status_indicator(
            &AppReadiness::Ready,
            &PipelineState::Listening,
            &colors,
        );
        assert_eq!(label, "LISTENING");
        assert_eq!(color, colors.status_listening);
        assert!(pulse, "listening indicator should pulse");
    }

    #[test]
    fn test_status_indicator_idle() {
        let colors = test_colors();
        let (label, _, pulse) = status_indicator(
            &AppReadiness::Ready,
            &PipelineState::Idle,
            &colors,
        );
        assert_eq!(label, "IDLE");
        assert!(!pulse);
    }

    #[test]
    fn test_status_indicator_injection_failed() {
        let colors = test_colors();
        let (label, color, pulse) = status_indicator(
            &AppReadiness::Ready,
            &PipelineState::InjectionFailed {
                polished_text: "test".into(),
                error: "failed".into(),
            },
            &colors,
        );
        assert_eq!(label, "FAILED");
        assert_eq!(color, colors.status_injection_failed);
        assert!(!pulse);
    }

    #[test]
    fn test_status_indicator_all_states() {
        let colors = test_colors();
        let states: Vec<(AppReadiness, PipelineState, &str)> = vec![
            (
                AppReadiness::Loading {
                    stage: "test".into(),
                },
                PipelineState::Idle,
                "LOADING",
            ),
            (
                AppReadiness::Error {
                    message: "err".into(),
                },
                PipelineState::Idle,
                "ERROR",
            ),
            (AppReadiness::Ready, PipelineState::Idle, "IDLE"),
            (AppReadiness::Ready, PipelineState::Listening, "LISTENING"),
            (
                AppReadiness::Ready,
                PipelineState::Processing { raw_text: None },
                "PROCESSING",
            ),
            (
                AppReadiness::Ready,
                PipelineState::Injecting {
                    polished_text: "t".into(),
                },
                "INJECTED",
            ),
            (
                AppReadiness::Ready,
                PipelineState::Error {
                    message: "e".into(),
                },
                "ERROR",
            ),
        ];

        for (readiness, pipeline, expected_label) in &states {
            let (label, _, _) = status_indicator(readiness, pipeline, &colors);
            assert_eq!(
                label, *expected_label,
                "readiness={readiness:?}, pipeline={pipeline:?}"
            );
        }
    }

    #[test]
    fn test_progress_fraction() {
        assert_eq!(progress_fraction(&DownloadProgress::Pending), 0.0);
        assert_eq!(progress_fraction(&DownloadProgress::Complete), 1.0);
        assert_eq!(
            progress_fraction(&DownloadProgress::Failed {
                error: "err".into(),
                manual_url: "https://example.com".into(),
            }),
            0.0
        );

        let half = DownloadProgress::InProgress {
            bytes_downloaded: 50,
            bytes_total: 100,
        };
        assert!((progress_fraction(&half) - 0.5).abs() < f32::EPSILON);

        let zero_total = DownloadProgress::InProgress {
            bytes_downloaded: 50,
            bytes_total: 0,
        };
        assert_eq!(progress_fraction(&zero_total), 0.0);
    }

    #[test]
    fn test_truncate_text() {
        assert_eq!(truncate_text("hello", 10), "hello");
        assert_eq!(truncate_text("hello world", 5), "hello...");
        assert_eq!(truncate_text("", 5), "");
        assert_eq!(truncate_text("exact", 5), "exact");
    }

    fn test_colors() -> ThemeColors {
        ThemeColors {
            overlay_bg: hsla(0.0, 0.0, 0.1, 0.92),
            surface: hsla(0.0, 0.0, 0.12, 1.0),
            elevated_surface: hsla(0.0, 0.0, 0.16, 1.0),
            panel_bg: hsla(0.0, 0.0, 0.14, 1.0),
            text: hsla(0.0, 0.0, 0.93, 1.0),
            text_muted: hsla(0.0, 0.0, 0.55, 1.0),
            text_accent: hsla(0.58, 0.8, 0.65, 1.0),
            border: hsla(0.0, 0.0, 0.2, 1.0),
            border_variant: hsla(0.0, 0.0, 0.25, 1.0),
            accent: hsla(0.58, 0.8, 0.65, 1.0),
            accent_hover: hsla(0.58, 0.85, 0.7, 1.0),
            status_idle: hsla(0.0, 0.0, 0.55, 1.0),
            status_listening: hsla(0.35, 0.9, 0.55, 1.0),
            status_processing: hsla(0.58, 0.8, 0.65, 1.0),
            status_success: hsla(0.35, 0.9, 0.55, 1.0),
            status_error: hsla(0.0, 0.85, 0.6, 1.0),
            status_downloading: hsla(0.12, 0.9, 0.6, 1.0),
            status_loading: hsla(0.55, 0.7, 0.7, 1.0),
            status_injection_failed: hsla(0.15, 0.9, 0.6, 1.0),
            waveform_active: hsla(0.35, 0.9, 0.55, 1.0),
            waveform_inactive: hsla(0.0, 0.0, 0.3, 1.0),
            button_primary_bg: hsla(0.58, 0.8, 0.55, 1.0),
            button_primary_text: hsla(0.0, 0.0, 1.0, 1.0),
            button_secondary_bg: hsla(0.0, 0.0, 0.2, 1.0),
            button_secondary_text: hsla(0.0, 0.0, 0.8, 1.0),
            input_bg: hsla(0.0, 0.0, 0.08, 1.0),
            input_border: hsla(0.0, 0.0, 0.25, 1.0),
            input_focus_border: hsla(0.58, 0.8, 0.65, 1.0),
            log_error: hsla(0.0, 0.85, 0.6, 1.0),
            log_warn: hsla(0.1, 0.9, 0.6, 1.0),
            log_info: hsla(0.0, 0.0, 0.93, 1.0),
            log_debug: hsla(0.0, 0.0, 0.55, 1.0),
            log_trace: hsla(0.0, 0.0, 0.35, 1.0),
            scrollbar_thumb: hsla(0.0, 0.0, 0.45, 1.0),
            scrollbar_track: hsla(0.0, 0.0, 0.16, 0.5),
        }
    }
}
