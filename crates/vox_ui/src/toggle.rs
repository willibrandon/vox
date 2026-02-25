//! Toggle switch component for boolean settings.
//!
//! Provides a pill-shaped track with a circular thumb that slides between
//! on/off positions. Click anywhere on the track or label to toggle state.

use gpui::{
    div, hsla, prelude::*, px, App, IntoElement, Pixels, SharedString, Window,
};

use crate::layout::spacing;
use crate::theme::VoxTheme;

/// Width of the toggle track in pixels.
const TRACK_WIDTH: Pixels = px(36.0);
/// Height of the toggle track in pixels.
const TRACK_HEIGHT: Pixels = px(20.0);
/// Diameter of the circular thumb in pixels.
const THUMB_SIZE: Pixels = px(16.0);
/// Inset from track edge to thumb center (2px padding).
const THUMB_INSET: Pixels = px(2.0);

/// A toggle switch component for binary on/off settings.
///
/// Renders as a horizontal row with label text and a pill-shaped track
/// containing a circular thumb. Thumb position and track color indicate
/// the current state.
#[derive(IntoElement)]
pub struct Toggle {
    enabled: bool,
    label: SharedString,
    on_change: Box<dyn Fn(bool, &mut Window, &mut App) + 'static>,
}

impl Toggle {
    /// Create a new toggle switch.
    ///
    /// - `enabled`: current on/off state
    /// - `label`: text displayed next to the toggle
    /// - `on_change`: called with the new state when toggled
    pub fn new(
        enabled: bool,
        label: impl Into<SharedString>,
        on_change: impl Fn(bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            enabled,
            label: label.into(),
            on_change: Box::new(on_change),
        }
    }
}

impl RenderOnce for Toggle {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let enabled = self.enabled;

        let track_bg = if enabled {
            theme.colors.accent
        } else {
            theme.colors.border
        };

        let thumb_bg = if enabled {
            hsla(0.0, 0.0, 1.0, 1.0)
        } else {
            theme.colors.text_muted
        };

        // Thumb offset: left (off) or right (on)
        let thumb_left = if enabled {
            TRACK_WIDTH - THUMB_SIZE - THUMB_INSET
        } else {
            THUMB_INSET
        };

        div()
            .id(SharedString::from(format!("toggle-{}", self.label)))
            .flex()
            .items_center()
            .gap(spacing::SM)
            .cursor_pointer()
            .on_click(move |_event, window, cx| {
                (self.on_change)(!enabled, window, cx);
            })
            .child(
                // Track
                div()
                    .w(TRACK_WIDTH)
                    .h(TRACK_HEIGHT)
                    .rounded(px(10.0))
                    .bg(track_bg)
                    .relative()
                    .child(
                        // Thumb
                        div()
                            .absolute()
                            .top(THUMB_INSET)
                            .left(thumb_left)
                            .w(THUMB_SIZE)
                            .h(THUMB_SIZE)
                            .rounded(px(8.0))
                            .bg(thumb_bg),
                    ),
            )
            .child(
                div()
                    .text_color(theme.colors.text)
                    .text_size(px(13.0))
                    .child(self.label.clone()),
            )
    }
}
