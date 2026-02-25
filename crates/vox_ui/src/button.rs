//! Themed button component with variant styling.
//!
//! Provides [`Button`] as a `RenderOnce` element with four visual variants:
//! Primary, Secondary, Ghost, and Danger. Supports optional icons, disabled
//! state, and hover/active visual feedback.

use gpui::{div, prelude::*, px, App, IntoElement, SharedString, Window};

use crate::icon::{Icon, IconElement};
use crate::layout::{radius, spacing};
use crate::theme::VoxTheme;

/// Visual style variant for buttons.
#[derive(Clone, Copy, PartialEq)]
pub enum ButtonVariant {
    /// Filled accent background with high-contrast text.
    Primary,
    /// Subtle background with standard text.
    Secondary,
    /// Transparent background, text-only (minimal visual weight).
    Ghost,
    /// Red-tinted for destructive actions.
    Danger,
}

/// A themed button with label, optional icon, and click handler.
///
/// Implements `RenderOnce` for use as a direct child element.
/// Visual style is determined by [`ButtonVariant`].
#[derive(IntoElement)]
pub struct Button {
    label: SharedString,
    icon: Option<Icon>,
    variant: ButtonVariant,
    disabled: bool,
    on_click: Option<Box<dyn Fn(&mut Window, &mut App) + 'static>>,
}

impl Button {
    /// Create a new button with a label and variant.
    pub fn new(label: impl Into<SharedString>, variant: ButtonVariant) -> Self {
        Self {
            label: label.into(),
            icon: None,
            variant,
            disabled: false,
            on_click: None,
        }
    }

    /// Set the button icon (displayed before the label).
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Set the disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set the click handler.
    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let disabled = self.disabled;

        let (bg, text_color, hover_bg) = if disabled {
            (
                theme.colors.border,
                theme.colors.text_muted,
                theme.colors.border,
            )
        } else {
            match self.variant {
                ButtonVariant::Primary => (
                    theme.colors.button_primary_bg,
                    theme.colors.button_primary_text,
                    theme.colors.accent_hover,
                ),
                ButtonVariant::Secondary => (
                    theme.colors.button_secondary_bg,
                    theme.colors.button_secondary_text,
                    theme.colors.elevated_surface,
                ),
                ButtonVariant::Ghost => (
                    gpui::hsla(0.0, 0.0, 0.0, 0.0),
                    theme.colors.text,
                    theme.colors.elevated_surface,
                ),
                ButtonVariant::Danger => (
                    theme.colors.status_error,
                    theme.colors.button_primary_text,
                    gpui::hsla(0.0, 0.85, 0.7, 1.0),
                ),
            }
        };

        let mut el = div()
            .id(SharedString::from(format!("btn-{}", self.label)))
            .flex()
            .items_center()
            .gap(spacing::XS)
            .px(spacing::MD)
            .py(spacing::SM)
            .rounded(radius::SM)
            .bg(bg)
            .text_color(text_color)
            .text_size(px(13.0));

        if !disabled {
            el = el.cursor_pointer().hover(move |s| s.bg(hover_bg));
        }

        if let Some(icon) = self.icon {
            el = el.child(IconElement::new(icon, text_color));
        }

        el = el.child(self.label.clone());

        if let Some(on_click) = self.on_click {
            if !disabled {
                el = el.on_click(move |_event, window, cx| {
                    on_click(window, cx);
                });
            }
        }

        el
    }
}
