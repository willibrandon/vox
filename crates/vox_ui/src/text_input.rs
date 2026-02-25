//! Text input component with focus management and keyboard handling.
//!
//! Provides [`TextInput`] as an entity with `Render` impl. Supports
//! placeholder text, on-change callbacks, Enter key submission, and
//! themed focus border styling.

use gpui::{
    div, prelude::*, px, App, Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render,
    SharedString, Window,
};

use crate::layout::{radius, spacing};
use crate::theme::VoxTheme;

/// A single-line text input field with focus and keyboard handling.
///
/// Created via `cx.new(|cx| TextInput::new(cx, ...))` and rendered as
/// an `Entity<TextInput>`. Fires `on_change` on every keystroke and
/// optional `on_submit` on Enter.
pub struct TextInput {
    content: String,
    placeholder: SharedString,
    on_change: Box<dyn Fn(&str, &mut Window, &mut App) + 'static>,
    on_submit: Option<Box<dyn Fn(&str, &mut Window, &mut App) + 'static>>,
    focus_handle: FocusHandle,
}

impl TextInput {
    /// Create a new text input.
    ///
    /// - `placeholder`: shown when content is empty
    /// - `on_change`: called with current text on every keystroke
    pub fn new(
        cx: &mut Context<Self>,
        placeholder: impl Into<SharedString>,
        on_change: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            content: String::new(),
            placeholder: placeholder.into(),
            on_change: Box::new(on_change),
            on_submit: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Set the Enter key submission callback.
    pub fn on_submit(mut self, handler: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Box::new(handler));
        self
    }

    /// Set the initial content value.
    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = content.into();
        self
    }

    /// Get the current text content.
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Set the text content programmatically.
    pub fn set_content(&mut self, content: String) {
        self.content = content;
    }
}

impl Render for TextInput {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let focused = self.focus_handle.is_focused(_window);

        let border_color = if focused {
            theme.colors.input_focus_border
        } else {
            theme.colors.input_border
        };

        let display_text: SharedString = if self.content.is_empty() {
            self.placeholder.clone()
        } else {
            SharedString::from(self.content.clone())
        };

        let text_color = if self.content.is_empty() {
            theme.colors.text_muted
        } else {
            theme.colors.text
        };

        div()
            .track_focus(&self.focus_handle)
            .px(spacing::MD)
            .py(spacing::SM)
            .rounded(radius::SM)
            .bg(theme.colors.input_bg)
            .border_1()
            .border_color(border_color)
            .text_color(text_color)
            .text_size(px(13.0))
            .min_w(px(120.0))
            .cursor_text()
            .on_key_down(cx.listener(
                |this, event: &KeyDownEvent, window, cx| {
                    let keystroke = &event.keystroke;
                    let key = keystroke.key.as_str();

                    match key {
                        "backspace" => {
                            this.content.pop();
                            (this.on_change)(&this.content, window, cx);
                            cx.notify();
                        }
                        "enter" => {
                            if let Some(ref on_submit) = this.on_submit {
                                on_submit(&this.content, window, cx);
                            }
                        }
                        "escape" => {
                            window.blur();
                        }
                        _ => {
                            if let Some(ch) = &keystroke.key_char {
                                this.content.push_str(ch);
                                (this.on_change)(&this.content, window, cx);
                                cx.notify();
                            } else if key.len() == 1
                                && !keystroke.modifiers.control
                                && !keystroke.modifiers.alt
                            {
                                this.content.push_str(key);
                                (this.on_change)(&this.content, window, cx);
                                cx.notify();
                            }
                        }
                    }
                },
            ))
            .child(display_text)
    }
}

/// Helper to create a TextInput entity.
pub fn new_text_input(
    _window: &mut Window,
    cx: &mut App,
    placeholder: impl Into<SharedString>,
    on_change: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> Entity<TextInput> {
    let placeholder = placeholder.into();
    cx.new(|cx| TextInput::new(cx, placeholder, on_change))
}
