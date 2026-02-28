//! Dropdown select component for choosing from a list of options.
//!
//! Provides [`Select`] as an entity with `Render` impl. Displays a trigger
//! showing the current selection, and an absolute-positioned dropdown list
//! when opened. Supports keyboard navigation and focus-based closing.

use gpui::{
    deferred, div, prelude::*, px, App, Context, Entity, FocusHandle, IntoElement, KeyDownEvent,
    Render, SharedString, Window,
};

use crate::icon::{Icon, IconElement};
use crate::layout::{radius, spacing};
use crate::theme::VoxTheme;

/// A single option in a select dropdown.
#[derive(Clone)]
pub struct SelectOption {
    /// Internal value used for identification.
    pub value: String,
    /// Display label shown to the user.
    pub label: SharedString,
}

impl SelectOption {
    /// Create a new select option.
    pub fn new(value: impl Into<String>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

/// A dropdown select component for choosing from a list.
///
/// Created via `cx.new(|cx| Select::new(cx, ...))`. Renders as a bordered
/// trigger div showing the selected option, with a dropdown that appears on click.
pub struct Select {
    options: Vec<SelectOption>,
    selected: String,
    label: SharedString,
    open: bool,
    on_change: Box<dyn Fn(&str, &mut Window, &mut App) + 'static>,
    focus_handle: FocusHandle,
}

impl Select {
    /// Create a new select dropdown.
    pub fn new(
        cx: &mut Context<Self>,
        options: Vec<SelectOption>,
        selected: impl Into<String>,
        label: impl Into<SharedString>,
        on_change: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            options,
            selected: selected.into(),
            label: label.into(),
            open: false,
            on_change: Box::new(on_change),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Get the currently selected value.
    pub fn selected(&self) -> &str {
        &self.selected
    }

    /// Set the selected value programmatically.
    pub fn set_selected(&mut self, value: String) {
        self.selected = value;
    }

    /// Get the display label for the currently selected option.
    fn selected_label(&self) -> SharedString {
        self.options
            .iter()
            .find(|o| o.value == self.selected)
            .map(|o| o.label.clone())
            .unwrap_or_else(|| SharedString::from(self.selected.clone()))
    }
}

impl Render for Select {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let open = self.open;
        let selected_label = self.selected_label();

        let border_color = if open {
            theme.colors.input_focus_border
        } else {
            theme.colors.input_border
        };

        let mut container = div()
            .track_focus(&self.focus_handle)
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key.as_str() == "escape" && this.open {
                    this.open = false;
                    cx.notify();
                }
            }));

        // Label
        container = container.child(
            div()
                .text_size(px(12.0))
                .text_color(theme.colors.text_muted)
                .child(self.label.clone()),
        );

        // Trigger
        let trigger_text_color = theme.colors.text;
        container = container.child(
            div()
                .id(SharedString::from(format!("select-{}", self.label)))
                .flex()
                .items_center()
                .justify_between()
                .px(spacing::MD)
                .py(spacing::SM)
                .rounded(radius::SM)
                .bg(theme.colors.input_bg)
                .border_1()
                .border_color(border_color)
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open = !this.open;
                    cx.notify();
                }))
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(trigger_text_color)
                        .child(selected_label),
                )
                .child(IconElement::new(Icon::ChevronDown, theme.colors.text_muted)),
        );

        // Dropdown options — wrapped in deferred() so it paints above sibling
        // elements (sliders, inputs) that would otherwise occlude it.
        if open {
            let mut dropdown = div()
                .absolute()
                .w_full()
                .mt(px(2.0))
                .bg(theme.colors.elevated_surface)
                .border_1()
                .border_color(theme.colors.border)
                .rounded(radius::SM)
                .occlude()
                .flex()
                .flex_col();

            for option in &self.options {
                let value = option.value.clone();
                let is_selected = option.value == self.selected;

                dropdown = dropdown.child(
                    div()
                        .id(SharedString::from(format!("opt-{}", option.value)))
                        .px(spacing::MD)
                        .py(spacing::SM)
                        .text_size(px(13.0))
                        .cursor_pointer()
                        .when(is_selected, |d| {
                            d.bg(theme.colors.accent)
                                .text_color(theme.colors.button_primary_text)
                        })
                        .when(!is_selected, |d| {
                            d.text_color(theme.colors.text)
                                .hover(|d| d.bg(theme.colors.surface))
                        })
                        .child(option.label.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.selected = value.clone();
                            this.open = false;
                            (this.on_change)(&this.selected, window, cx);
                            cx.notify();
                        })),
                );
            }

            container = container.child(deferred(dropdown).with_priority(1));
        }

        container
    }
}

/// Helper to create a Select entity.
pub fn new_select(
    _window: &mut Window,
    cx: &mut App,
    options: Vec<SelectOption>,
    selected: impl Into<String>,
    label: impl Into<SharedString>,
    on_change: impl Fn(&str, &mut Window, &mut App) + 'static,
) -> Entity<Select> {
    let selected = selected.into();
    let label = label.into();
    cx.new(|cx| Select::new(cx, options, selected, label, on_change))
}
