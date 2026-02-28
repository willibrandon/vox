//! Settings panel with scrollable sections for all dictation configuration.
//!
//! Provides [`SettingsPanel`] as an entity with `Render` impl. Contains six
//! sections (Audio, VAD, Hotkey, LLM, Appearance, Advanced) with interactive
//! Entity controls (sliders, selects, toggles, hotkey recorder, text input).
//! Every setting change persists to JSON immediately via [`VoxState::update_settings`].
//! Uses the scrollbar-as-sibling pattern required by GPUI.

use gpui::{
    div, prelude::*, px, App, Entity, EntityId, IntoElement, Render, ScrollHandle, SharedString,
    Window,
};

use vox_core::audio::capture::list_input_devices;
use vox_core::config::{DebugAudioLevel, OverlayPosition, Settings, ThemeMode};
use vox_core::hotkey_interpreter::ActivationMode;
use vox_core::state::VoxState;

use gpui::ClipboardItem;

use crate::hotkey_recorder::HotkeyRecorder;
use crate::layout::{radius, spacing};
use crate::scrollbar::{new_drag_state, Scrollbar, ScrollbarDragState};
use crate::select::{Select, SelectOption};
use crate::slider::Slider;
use crate::text_input::TextInput;
use crate::theme::VoxTheme;
use crate::toggle::Toggle;

/// Settings panel displaying all dictation configuration grouped into sections.
///
/// Uses a scroll container with a sibling `Scrollbar` element — the scrollbar
/// must NOT be a child of the scroll container because GPUI shifts all children
/// (including absolute-positioned elements) by the scroll offset.
pub struct SettingsPanel {
    /// Snapshot of current settings, refreshed each render.
    settings: Settings,
    /// Scroll handle shared between the scroll container and Scrollbar element.
    scroll_handle: ScrollHandle,
    /// Drag state for scrollbar thumb interaction, persists across frames.
    scrollbar_drag: ScrollbarDragState,

    // --- Audio controls ---
    /// Dropdown for selecting the audio input device.
    device_select: Entity<Select>,
    /// Slider for the noise gate threshold (0.0–1.0).
    noise_gate_slider: Entity<Slider>,

    // --- VAD controls ---
    /// Slider for the VAD confidence threshold (0.0–1.0).
    vad_threshold_slider: Entity<Slider>,
    /// Slider for minimum silence duration before ending a segment (ms).
    min_silence_slider: Entity<Slider>,
    /// Slider for minimum speech duration before starting a segment (ms).
    min_speech_slider: Entity<Slider>,

    // --- Hotkey controls ---
    /// Recorder for capturing the activation keyboard shortcut.
    hotkey_recorder: Entity<HotkeyRecorder>,
    /// Dropdown for selecting the activation mode (Hold-to-Talk / Toggle / Hands-Free).
    activation_mode_select: Entity<Select>,

    // --- LLM controls ---
    /// Slider for LLM sampling temperature (0.0–2.0).
    temperature_slider: Entity<Slider>,

    // --- Appearance controls ---
    /// Dropdown for theme selection (System/Light/Dark).
    theme_select: Entity<Select>,
    /// Slider for overlay background opacity (0.0–1.0).
    opacity_slider: Entity<Slider>,
    /// Dropdown for overlay screen position.
    position_select: Entity<Select>,

    // --- Advanced controls ---
    /// Dropdown for debug audio recording level (Off / Segments Only / Full).
    debug_audio_select: Entity<Select>,
    /// Slider for maximum audio segment duration (ms).
    max_segment_slider: Entity<Slider>,
    /// Slider for overlap between consecutive segments (ms).
    overlap_slider: Entity<Slider>,
    /// Text input for the voice command prefix phrase.
    command_prefix_input: Entity<TextInput>,
}

impl SettingsPanel {
    /// Create a new settings panel, loading current settings and audio devices.
    ///
    /// Creates all interactive Entity controls with persistence callbacks that
    /// write to VoxState on every change.
    pub fn new(_window: &mut Window, cx: &mut gpui::Context<Self>) -> Self {
        let state = cx.global::<VoxState>();
        let settings = state.settings().clone();
        let panel_id = cx.entity_id();

        // --- Audio entities ---
        // Start with just "Default" — device enumeration runs on a background
        // thread because cpal's Core Audio calls on macOS can block for hundreds
        // of milliseconds, which would freeze the main GPUI thread.
        let device_options = vec![SelectOption::new("", "Default")];
        let selected_device = settings.input_device.clone().unwrap_or_default();

        let device_select = cx.new(|cx| {
            Select::new(
                cx,
                device_options,
                selected_device,
                "Input Device",
                move |value, _window, cx| {
                    let device = if value.is_empty() {
                        None
                    } else {
                        Some(value.to_string())
                    };
                    if let Err(err) =
                        cx.global::<VoxState>().update_settings(|s| s.input_device = device)
                    {
                        tracing::warn!(%err, "failed to save input device");
                    }
                    cx.notify(panel_id);
                },
            )
        });

        let noise_gate_slider = cx.new(|cx| {
            Slider::new(
                cx,
                0.0,
                1.0,
                0.01,
                settings.noise_gate,
                "Noise Gate",
                move |value, _window, cx| {
                    if let Err(err) =
                        cx.global::<VoxState>().update_settings(|s| s.noise_gate = value)
                    {
                        tracing::warn!(%err, "failed to save noise gate");
                    }
                    cx.notify(panel_id);
                },
            )
        });

        // --- VAD entities ---
        let vad_threshold_slider = cx.new(|cx| {
            Slider::new(
                cx,
                0.0,
                1.0,
                0.01,
                settings.vad_threshold,
                "Threshold",
                move |value, _window, cx| {
                    if let Err(err) =
                        cx.global::<VoxState>().update_settings(|s| s.vad_threshold = value)
                    {
                        tracing::warn!(%err, "failed to save VAD threshold");
                    }
                    cx.notify(panel_id);
                },
            )
        });

        let min_silence_slider = cx.new(|cx| {
            Slider::new(
                cx,
                0.0,
                2000.0,
                50.0,
                settings.min_silence_ms as f32,
                "Min Silence",
                move |value, _window, cx| {
                    if let Err(err) =
                        cx.global::<VoxState>()
                            .update_settings(|s| s.min_silence_ms = value as u32)
                    {
                        tracing::warn!(%err, "failed to save min silence");
                    }
                    cx.notify(panel_id);
                },
            )
            .with_format(|v| format!("{} ms", v as u32))
        });

        let min_speech_slider = cx.new(|cx| {
            Slider::new(
                cx,
                0.0,
                2000.0,
                50.0,
                settings.min_speech_ms as f32,
                "Min Speech",
                move |value, _window, cx| {
                    if let Err(err) =
                        cx.global::<VoxState>()
                            .update_settings(|s| s.min_speech_ms = value as u32)
                    {
                        tracing::warn!(%err, "failed to save min speech");
                    }
                    cx.notify(panel_id);
                },
            )
            .with_format(|v| format!("{} ms", v as u32))
        });

        // --- Hotkey entities ---
        let hotkey_recorder = cx.new(|cx| {
            HotkeyRecorder::new(
                cx,
                settings.activation_hotkey.clone(),
                move |binding, _window, cx| {
                    let new_hotkey = binding.to_string();
                    let vox = cx.global::<VoxState>();
                    if let Err(err) =
                        vox.update_settings(|s| s.activation_hotkey = new_hotkey.clone())
                    {
                        tracing::warn!(%err, "failed to save hotkey");
                    }
                    vox.notify_hotkey_change(&new_hotkey);
                    cx.notify(panel_id);
                },
            )
        });

        let mode_value = match settings.activation_mode {
            ActivationMode::HoldToTalk => "HoldToTalk",
            ActivationMode::Toggle => "Toggle",
            ActivationMode::HandsFree => "HandsFree",
        };
        let activation_mode_select = cx.new(|cx| {
            Select::new(
                cx,
                vec![
                    SelectOption::new("HoldToTalk", "Hold to Talk"),
                    SelectOption::new("Toggle", "Toggle"),
                    SelectOption::new("HandsFree", "Hands-Free"),
                ],
                mode_value,
                "Activation Mode",
                move |value, _window, cx| {
                    let mode = match value {
                        "Toggle" => ActivationMode::Toggle,
                        "HandsFree" => ActivationMode::HandsFree,
                        _ => ActivationMode::HoldToTalk,
                    };
                    if let Err(err) =
                        cx.global::<VoxState>().update_settings(|s| s.activation_mode = mode)
                    {
                        tracing::warn!(%err, "failed to save activation mode");
                    }
                    cx.notify(panel_id);
                },
            )
        });

        // --- LLM entity ---
        let temperature_slider = cx.new(|cx| {
            Slider::new(
                cx,
                0.0,
                2.0,
                0.1,
                settings.temperature,
                "Temperature",
                move |value, _window, cx| {
                    if let Err(err) =
                        cx.global::<VoxState>().update_settings(|s| s.temperature = value)
                    {
                        tracing::warn!(%err, "failed to save temperature");
                    }
                    cx.notify(panel_id);
                },
            )
            .with_format(|v| format!("{:.1}", v))
        });

        // --- Appearance entities ---
        let theme_value = match settings.theme {
            ThemeMode::System => "System",
            ThemeMode::Light => "Light",
            ThemeMode::Dark => "Dark",
        };
        let theme_select = cx.new(|cx| {
            Select::new(
                cx,
                vec![
                    SelectOption::new("System", "System"),
                    SelectOption::new("Light", "Light"),
                    SelectOption::new("Dark", "Dark"),
                ],
                theme_value,
                "Theme",
                move |value, _window, cx| {
                    let mode = match value {
                        "Light" => ThemeMode::Light,
                        "Dark" => ThemeMode::Dark,
                        _ => ThemeMode::System,
                    };
                    cx.set_global(VoxTheme::from_mode(&mode));
                    if let Err(err) =
                        cx.global::<VoxState>().update_settings(|s| s.theme = mode)
                    {
                        tracing::warn!(%err, "failed to save theme");
                    }
                    cx.notify(panel_id);
                },
            )
        });

        let opacity_slider = cx.new(|cx| {
            Slider::new(
                cx,
                0.0,
                1.0,
                0.05,
                settings.overlay_opacity,
                "Overlay Opacity",
                move |value, _window, cx| {
                    if let Err(err) =
                        cx.global::<VoxState>().update_settings(|s| s.overlay_opacity = value)
                    {
                        tracing::warn!(%err, "failed to save overlay opacity");
                    }
                    cx.notify(panel_id);
                },
            )
            .with_format(|v| format!("{:.0}%", v * 100.0))
        });

        let position_value = match &settings.overlay_position {
            OverlayPosition::TopCenter => "TopCenter",
            OverlayPosition::TopRight => "TopRight",
            OverlayPosition::BottomCenter => "BottomCenter",
            OverlayPosition::BottomRight => "BottomRight",
            OverlayPosition::Custom { .. } => "TopCenter",
        };
        let position_select = cx.new(|cx| {
            Select::new(
                cx,
                vec![
                    SelectOption::new("TopCenter", "Top Center"),
                    SelectOption::new("TopRight", "Top Right"),
                    SelectOption::new("BottomCenter", "Bottom Center"),
                    SelectOption::new("BottomRight", "Bottom Right"),
                ],
                position_value,
                "Overlay Position",
                move |value, _window, cx| {
                    let position = match value {
                        "TopRight" => OverlayPosition::TopRight,
                        "BottomCenter" => OverlayPosition::BottomCenter,
                        "BottomRight" => OverlayPosition::BottomRight,
                        _ => OverlayPosition::TopCenter,
                    };
                    if let Err(err) =
                        cx.global::<VoxState>().update_settings(|s| s.overlay_position = position)
                    {
                        tracing::warn!(%err, "failed to save overlay position");
                    }
                    cx.notify(panel_id);
                },
            )
        });

        // --- Advanced entities ---
        let max_segment_slider = cx.new(|cx| {
            Slider::new(
                cx,
                1000.0,
                30000.0,
                500.0,
                settings.max_segment_ms as f32,
                "Max Segment Duration",
                move |value, _window, cx| {
                    if let Err(err) =
                        cx.global::<VoxState>()
                            .update_settings(|s| s.max_segment_ms = value as u32)
                    {
                        tracing::warn!(%err, "failed to save max segment");
                    }
                    cx.notify(panel_id);
                },
            )
            .with_format(|v| format!("{} ms", v as u32))
        });

        let overlap_slider = cx.new(|cx| {
            Slider::new(
                cx,
                0.0,
                5000.0,
                100.0,
                settings.overlap_ms as f32,
                "Overlap Duration",
                move |value, _window, cx| {
                    if let Err(err) =
                        cx.global::<VoxState>()
                            .update_settings(|s| s.overlap_ms = value as u32)
                    {
                        tracing::warn!(%err, "failed to save overlap");
                    }
                    cx.notify(panel_id);
                },
            )
            .with_format(|v| format!("{} ms", v as u32))
        });

        let command_prefix_input = cx.new(|cx| {
            TextInput::new(
                cx,
                "Enter command prefix...",
                move |value, _window, cx| {
                    if let Err(err) = cx
                        .global::<VoxState>()
                        .update_settings(|s| s.command_prefix = value.to_string())
                    {
                        tracing::warn!(%err, "failed to save command prefix");
                    }
                    cx.notify(panel_id);
                },
            )
            .with_content(settings.command_prefix.clone())
        });

        let debug_audio_value = match settings.debug_audio {
            DebugAudioLevel::Off => "Off",
            DebugAudioLevel::Segments => "Segments",
            DebugAudioLevel::Full => "Full",
        };
        let debug_audio_select = cx.new(|cx| {
            Select::new(
                cx,
                vec![
                    SelectOption::new("Off", "Off"),
                    SelectOption::new("Segments", "Segments Only"),
                    SelectOption::new("Full", "Full (includes raw capture)"),
                ],
                debug_audio_value,
                "Debug Audio Recording",
                move |value, _window, cx| {
                    let level = match value {
                        "Segments" => DebugAudioLevel::Segments,
                        "Full" => DebugAudioLevel::Full,
                        _ => DebugAudioLevel::Off,
                    };
                    let state = cx.global::<VoxState>();
                    if let Err(err) = state.update_settings(|s| s.debug_audio = level) {
                        tracing::warn!(%err, "failed to save debug audio level");
                    }
                    state.debug_tap().set_level(level);
                    cx.notify(panel_id);
                },
            )
        });

        // Enumerate audio devices on a background thread, then replace the
        // device_select entity with one containing the full device list.
        {
            let executor = cx.background_executor().clone();
            cx.spawn(async move |this, cx| {
                let devices = executor
                    .spawn(async { list_input_devices().unwrap_or_default() })
                    .await;
                if devices.is_empty() {
                    return;
                }
                let _ = this.update(cx, |panel, cx| {
                    let mut options = vec![SelectOption::new("", "Default")];
                    options.extend(devices.iter().map(|d| {
                        let label = if d.is_default {
                            format!("{} (default)", d.name)
                        } else {
                            d.name.clone()
                        };
                        SelectOption::new(d.name.clone(), label)
                    }));
                    let selected =
                        panel.settings.input_device.clone().unwrap_or_default();
                    let panel_id = cx.entity_id();
                    panel.device_select = cx.new(|cx| {
                        Select::new(
                            cx,
                            options,
                            selected,
                            "Input Device",
                            move |value, _window, cx| {
                                let device = if value.is_empty() {
                                    None
                                } else {
                                    Some(value.to_string())
                                };
                                if let Err(err) = cx
                                    .global::<VoxState>()
                                    .update_settings(|s| s.input_device = device)
                                {
                                    tracing::warn!(
                                        %err,
                                        "failed to save input device"
                                    );
                                }
                                cx.notify(panel_id);
                            },
                        )
                    });
                    cx.notify();
                });
            })
            .detach();
        }

        Self {
            settings,
            scroll_handle: ScrollHandle::new(),
            scrollbar_drag: new_drag_state(),
            device_select,
            noise_gate_slider,
            vad_threshold_slider,
            min_silence_slider,
            min_speech_slider,
            hotkey_recorder,
            activation_mode_select,
            temperature_slider,
            theme_select,
            opacity_slider,
            position_select,
            debug_audio_select,
            max_segment_slider,
            overlap_slider,
            command_prefix_input,
        }
    }

    /// Render a section header with title and description.
    fn render_section_header(
        title: &str,
        description: &str,
        theme: &VoxTheme,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .child(
                div()
                    .text_size(px(16.0))
                    .text_color(theme.colors.text)
                    .child(SharedString::from(title.to_string())),
            )
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(theme.colors.text_muted)
                    .child(SharedString::from(description.to_string())),
            )
    }

    /// Render the Audio settings section.
    fn render_audio_section(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();

        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::MD)
            .rounded(radius::SM)
            .bg(theme.colors.elevated_surface)
            .border_1()
            .border_color(theme.colors.border)
            .child(Self::render_section_header(
                "Audio",
                "Input device and noise gate configuration",
                theme,
            ))
            .child(self.device_select.clone())
            .child(self.noise_gate_slider.clone())
    }

    /// Render the VAD (Voice Activity Detection) settings section.
    fn render_vad_section(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();

        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::MD)
            .rounded(radius::SM)
            .bg(theme.colors.elevated_surface)
            .border_1()
            .border_color(theme.colors.border)
            .child(Self::render_section_header(
                "Voice Activity Detection",
                "Sensitivity and timing for speech detection",
                theme,
            ))
            .child(self.vad_threshold_slider.clone())
            .child(self.min_silence_slider.clone())
            .child(self.min_speech_slider.clone())
    }

    /// Render the Hotkey settings section.
    fn render_hotkey_section(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();

        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::MD)
            .rounded(radius::SM)
            .bg(theme.colors.elevated_surface)
            .border_1()
            .border_color(theme.colors.border)
            .child(Self::render_section_header(
                "Hotkey",
                "Activation shortcut and recording mode",
                theme,
            ))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::XS)
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.colors.text_muted)
                            .child("Activation Hotkey"),
                    )
                    .child(self.hotkey_recorder.clone()),
            )
            .child(self.activation_mode_select.clone())
    }

    /// Render the LLM post-processing settings section.
    fn render_llm_section(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let panel_id = cx.entity_id();

        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::MD)
            .rounded(radius::SM)
            .bg(theme.colors.elevated_surface)
            .border_1()
            .border_color(theme.colors.border)
            .child(Self::render_section_header(
                "LLM Post-Processing",
                "Text polishing and correction settings",
                theme,
            ))
            .child(self.temperature_slider.clone())
            .child(Toggle::new(
                self.settings.remove_fillers,
                "Remove Fillers",
                toggle_callback(panel_id, |s, enabled| s.remove_fillers = enabled),
            ))
            .child(Toggle::new(
                self.settings.course_correction,
                "Course Correction",
                toggle_callback(panel_id, |s, enabled| s.course_correction = enabled),
            ))
            .child(Toggle::new(
                self.settings.punctuation,
                "Punctuation",
                toggle_callback(panel_id, |s, enabled| s.punctuation = enabled),
            ))
    }

    /// Render the Appearance settings section.
    fn render_appearance_section(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let panel_id = cx.entity_id();

        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::MD)
            .rounded(radius::SM)
            .bg(theme.colors.elevated_surface)
            .border_1()
            .border_color(theme.colors.border)
            .child(Self::render_section_header(
                "Appearance",
                "Theme, overlay, and display preferences",
                theme,
            ))
            .child(self.theme_select.clone())
            .child(self.opacity_slider.clone())
            .child(self.position_select.clone())
            .child(Toggle::new(
                self.settings.show_raw_transcript,
                "Show Raw Transcript",
                toggle_callback(panel_id, |s, enabled| s.show_raw_transcript = enabled),
            ))
    }

    /// Render the Advanced settings section.
    fn render_advanced_section(&self, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let debug_audio_active = self.settings.debug_audio != DebugAudioLevel::Off;
        let debug_dir_path = if debug_audio_active {
            let state = cx.global::<VoxState>();
            Some(state.debug_tap().debug_audio_dir().to_string_lossy().to_string())
        } else {
            None
        };

        div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::MD)
            .rounded(radius::SM)
            .bg(theme.colors.elevated_surface)
            .border_1()
            .border_color(theme.colors.border)
            .child(Self::render_section_header(
                "Advanced",
                "Segment timing, command configuration, and debug audio",
                theme,
            ))
            .child(self.debug_audio_select.clone())
            .when_some(debug_dir_path, |this, path| {
                let path_for_copy = path.clone();
                let muted = theme.colors.text_muted;
                let accent = theme.colors.accent;
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(spacing::SM)
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(muted)
                                .flex_shrink()
                                .overflow_x_hidden()
                                .child(SharedString::from(path)),
                        )
                        .child(
                            div()
                                .id("copy-debug-dir")
                                .text_size(px(11.0))
                                .text_color(accent)
                                .cursor_pointer()
                                .flex_shrink_0()
                                .child("Copy")
                                .on_click(move |_, _window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        path_for_copy.clone(),
                                    ));
                                }),
                        ),
                )
            })
            .child(self.max_segment_slider.clone())
            .child(self.overlap_slider.clone())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(spacing::XS)
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.colors.text_muted)
                            .child("Command Prefix"),
                    )
                    .child(self.command_prefix_input.clone()),
            )
    }
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        // Refresh settings snapshot so toggles reflect current state
        self.settings = cx.global::<VoxState>().settings().clone();

        // Extract scrollbar colors before borrowing cx for section rendering
        let scrollbar_thumb = cx.global::<VoxTheme>().colors.scrollbar_thumb;
        let scrollbar_track = cx.global::<VoxTheme>().colors.scrollbar_track;

        // Build section elements — convert to AnyElement immediately so the
        // mutable borrow on cx is released before the next call.  Rust 2024's
        // `impl Trait` captures all in-scope lifetimes, so without this
        // conversion the borrows would overlap.
        let audio = self.render_audio_section(cx).into_any_element();
        let vad = self.render_vad_section(cx).into_any_element();
        let hotkey = self.render_hotkey_section(cx).into_any_element();
        let llm = self.render_llm_section(cx).into_any_element();
        let appearance = self.render_appearance_section(cx).into_any_element();
        let advanced = self.render_advanced_section(cx).into_any_element();

        // CRITICAL: Scrollbar must be a SIBLING of the scroll container.
        // GPUI's with_element_offset(scroll_offset) shifts ALL children of a
        // scroll container during prepaint, including absolute-positioned elements.
        // Placing Scrollbar as a sibling keeps it in the parent's unshifted
        // coordinate space while it reads from the shared ScrollHandle.
        div()
            .size_full()
            .child(
                div()
                    .id("settings-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .flex()
                    .flex_col()
                    .p(spacing::LG)
                    .gap(spacing::XL)
                    .child(audio)
                    .child(vad)
                    .child(hotkey)
                    .child(llm)
                    .child(appearance)
                    .child(advanced),
            )
            .child(Scrollbar::new(
                self.scroll_handle.clone(),
                cx.entity_id(),
                self.scrollbar_drag.clone(),
                scrollbar_thumb,
                scrollbar_track,
            ))
    }
}

/// Create a toggle on_change callback that persists a boolean setting.
///
/// Captures the panel entity ID for re-render notification and applies
/// the mutation via `VoxState::update_settings`.
fn toggle_callback(
    panel_id: EntityId,
    mutate: fn(&mut Settings, bool),
) -> impl Fn(bool, &mut Window, &mut App) + 'static {
    move |enabled, _window, cx| {
        if let Err(err) = cx
            .global::<VoxState>()
            .update_settings(|s| mutate(s, enabled))
        {
            tracing::warn!(%err, "failed to save toggle setting");
        }
        cx.notify(panel_id);
    }
}
