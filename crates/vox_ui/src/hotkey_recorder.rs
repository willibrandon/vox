//! Hotkey recorder component for capturing keyboard shortcuts.
//!
//! Provides [`HotkeyRecorder`] as an entity with `Render` impl. Click to
//! enter recording mode, press a key combination to capture, or Escape to cancel.

use gpui::{
    div, prelude::*, px, App, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render,
    SharedString, Window,
};

use crate::layout::{radius, spacing};
use crate::theme::VoxTheme;

/// A hotkey recorder component that captures keyboard shortcuts.
///
/// Created via `cx.new(|cx| HotkeyRecorder::new(cx, ...))`. Shows current
/// binding as text; click to enter recording mode, then press desired key
/// combination.
pub struct HotkeyRecorder {
    current_binding: String,
    recording: bool,
    on_change: Box<dyn Fn(&str, &mut Window, &mut App) + 'static>,
    focus_handle: FocusHandle,
}

impl HotkeyRecorder {
    /// Create a new hotkey recorder.
    pub fn new(
        cx: &mut Context<Self>,
        current_binding: impl Into<String>,
        on_change: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            current_binding: current_binding.into(),
            recording: false,
            on_change: Box::new(on_change),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Get the current binding string.
    pub fn binding(&self) -> &str {
        &self.current_binding
    }

    /// Set the binding string programmatically.
    pub fn set_binding(&mut self, binding: String) {
        self.current_binding = binding;
    }
}

impl Render for HotkeyRecorder {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let recording = self.recording;

        let (display_text, border_color) = if recording {
            (
                SharedString::from("Press a key..."),
                theme.colors.accent,
            )
        } else {
            (
                SharedString::from(self.current_binding.clone()),
                theme.colors.input_border,
            )
        };

        div()
            .id("hotkey-recorder")
            .track_focus(&self.focus_handle)
            .px(spacing::MD)
            .py(spacing::SM)
            .rounded(radius::SM)
            .bg(theme.colors.input_bg)
            .border_1()
            .border_color(border_color)
            .text_color(if recording {
                theme.colors.accent
            } else {
                theme.colors.text
            })
            .text_size(px(13.0))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| {
                this.recording = true;
                this.focus_handle.focus(window, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(
                |this, event: &KeyDownEvent, window, cx| {
                    if !this.recording {
                        return;
                    }

                    let keystroke = &event.keystroke;
                    let key = keystroke.key.as_str();

                    if key == "escape" {
                        this.recording = false;
                        cx.notify();
                        return;
                    }

                    // Build binding string from modifiers + key
                    let mut parts = Vec::new();
                    if keystroke.modifiers.control {
                        parts.push("Ctrl");
                    }
                    if keystroke.modifiers.alt {
                        parts.push("Alt");
                    }
                    if keystroke.modifiers.shift {
                        parts.push("Shift");
                    }
                    #[cfg(target_os = "macos")]
                    if keystroke.modifiers.command {
                        parts.push("Cmd");
                    }

                    // Capitalize key name
                    let key_display = if key.len() == 1 {
                        key.to_uppercase()
                    } else {
                        // Keys like "space", "f1", etc.
                        let mut chars = key.chars();
                        match chars.next() {
                            Some(c) => {
                                let upper: String =
                                    c.to_uppercase().chain(chars).collect();
                                upper
                            }
                            None => key.to_string(),
                        }
                    };
                    parts.push(&key_display);

                    let binding = parts.join("+");
                    this.current_binding = binding;
                    this.recording = false;
                    (this.on_change)(&this.current_binding, window, cx);
                    cx.notify();
                },
            ))
            .child(display_text)
    }
}

/// Helper to create a HotkeyRecorder entity.
pub fn new_hotkey_recorder(
    _window: &mut Window,
    cx: &mut App,
    current_binding: impl Into<String>,
    on_change: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> Entity<HotkeyRecorder> {
    let current_binding = current_binding.into();
    cx.new(|cx| HotkeyRecorder::new(cx, current_binding, on_change))
}
