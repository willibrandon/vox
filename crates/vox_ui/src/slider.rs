//! Horizontal slider component for numeric range selection.
//!
//! Provides [`Slider`] as an entity with `Render` impl. Uses a GPUI `canvas`
//! element for the track area to get precise pixel bounds for click-to-set and
//! thumb dragging. State is shared between the entity and canvas paint closures
//! via `Rc<Cell<>>`, following the same pattern as the custom [`Scrollbar`](crate::scrollbar).

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    canvas, div, hsla, point, prelude::*, px, quad, App, BorderStyle, Bounds, Corners,
    DispatchPhase, Edges, Entity, EntityId, Hsla, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Render, SharedString, Size, Window,
};

use crate::layout::spacing;
use crate::theme::VoxTheme;

/// Height of the slider track in pixels.
const TRACK_HEIGHT: Pixels = px(4.0);
/// Diameter of the slider thumb in pixels.
const THUMB_SIZE: Pixels = px(16.0);
/// Total height of the interactive track area (includes thumb overhang).
const TRACK_AREA_HEIGHT: Pixels = px(20.0);

/// Snap a raw value to the nearest step within the slider range.
fn snap_value(min: f32, max: f32, step: f32, value: f32) -> f32 {
    if step <= 0.0 {
        return value.clamp(min, max);
    }
    let steps = ((value - min) / step).round();
    (min + steps * step).clamp(min, max)
}

/// Compute a slider value from a horizontal cursor position within track bounds.
fn value_from_x(min: f32, max: f32, step: f32, x: Pixels, bounds: Bounds<Pixels>) -> f32 {
    let ratio = ((x - bounds.origin.x) / bounds.size.width).clamp(0.0, 1.0);
    snap_value(min, max, step, min + ratio * (max - min))
}

/// A horizontal slider for selecting a value within a range.
///
/// Created via `cx.new(|cx| Slider::new(cx, ...))`. Renders as a label
/// with value display above a canvas-painted track with draggable thumb.
/// Mouse interaction (click-to-set, drag) is handled via global mouse event
/// handlers registered in the canvas paint callback.
pub struct Slider {
    min: f32,
    max: f32,
    step: f32,
    /// Current value, shared with canvas paint closures via Rc.
    value: Rc<Cell<f32>>,
    /// Whether a drag gesture is in progress.
    dragging: Rc<Cell<bool>>,
    label: SharedString,
    format_fn: Box<dyn Fn(f32) -> String>,
    /// Callback invoked on every value change (click or drag).
    on_change: Rc<dyn Fn(f32, &mut Window, &mut App) + 'static>,
}

impl Slider {
    /// Create a new slider with range, step size, and callbacks.
    pub fn new(
        _cx: &mut gpui::Context<Self>,
        min: f32,
        max: f32,
        step: f32,
        initial: f32,
        label: impl Into<SharedString>,
        on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            min,
            max,
            step,
            value: Rc::new(Cell::new(initial.clamp(min, max))),
            dragging: Rc::new(Cell::new(false)),
            label: label.into(),
            format_fn: Box::new(|v| format!("{v:.2}")),
            on_change: Rc::new(on_change),
        }
    }

    /// Set a custom value display formatter.
    pub fn with_format(mut self, f: impl Fn(f32) -> String + 'static) -> Self {
        self.format_fn = Box::new(f);
        self
    }

    /// Get the current value.
    pub fn value(&self) -> f32 {
        self.value.get()
    }

    /// Set the value programmatically.
    pub fn set_value(&mut self, v: f32) {
        self.value.set(v.clamp(self.min, self.max));
    }
}

impl Render for Slider {
    fn render(&mut self, _window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        let current_value = self.value.get();
        let ratio = if (self.max - self.min).abs() > f32::EPSILON {
            (current_value - self.min) / (self.max - self.min)
        } else {
            0.0
        };

        let display_value = (self.format_fn)(current_value);

        // Colors captured by value (Hsla is Copy) for the canvas closure
        let track_bg = theme.colors.border;
        let fill_color = theme.colors.accent;
        let thumb_color = theme.colors.accent;
        let transparent = hsla(0.0, 0.0, 0.0, 0.0);

        // Shared state cloned for canvas closures
        let value = self.value.clone();
        let dragging = self.dragging.clone();
        let on_change = self.on_change.clone();
        let min = self.min;
        let max = self.max;
        let step = self.step;
        let entity_id = cx.entity_id();

        div()
            .id(SharedString::from(format!("slider-{}", self.label)))
            .flex()
            .flex_col()
            .gap(spacing::XS)
            .child(
                // Label row
                div()
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.colors.text_muted)
                            .child(self.label.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(theme.colors.text)
                            .child(SharedString::from(display_value)),
                    ),
            )
            .child(
                // Track area — canvas provides bounds for mouse-to-value mapping
                div()
                    .h(TRACK_AREA_HEIGHT)
                    .w_full()
                    .cursor_pointer()
                    .child(
                        canvas(
                            |_bounds, _window, _cx| (),
                            move |bounds, (), window, _cx| {
                                paint_slider_track(
                                    bounds,
                                    ratio,
                                    track_bg,
                                    fill_color,
                                    thumb_color,
                                    transparent,
                                    window,
                                );
                                register_slider_mouse_handlers(
                                    bounds,
                                    min,
                                    max,
                                    step,
                                    entity_id,
                                    value,
                                    dragging,
                                    on_change,
                                    window,
                                );
                            },
                        )
                        .h_full()
                        .w_full(),
                    ),
            )
    }
}

/// Paint the slider track, fill bar, and thumb circle.
fn paint_slider_track(
    bounds: Bounds<Pixels>,
    ratio: f32,
    track_bg: Hsla,
    fill_color: Hsla,
    thumb_color: Hsla,
    transparent: Hsla,
    window: &mut Window,
) {
    let track_y = bounds.origin.y + (bounds.size.height - TRACK_HEIGHT) / 2.0;

    // Track background
    let track_bounds = Bounds::new(
        point(bounds.origin.x, track_y),
        Size {
            width: bounds.size.width,
            height: TRACK_HEIGHT,
        },
    );
    window.paint_quad(quad(
        track_bounds,
        Corners::all(px(2.0)),
        track_bg,
        Edges::default(),
        transparent,
        BorderStyle::default(),
    ));

    // Filled portion
    let fill_width = bounds.size.width * ratio;
    if fill_width > px(0.0) {
        let fill_bounds = Bounds::new(
            point(bounds.origin.x, track_y),
            Size {
                width: fill_width,
                height: TRACK_HEIGHT,
            },
        );
        window.paint_quad(quad(
            fill_bounds,
            Corners::all(px(2.0)),
            fill_color,
            Edges::default(),
            transparent,
            BorderStyle::default(),
        ));
    }

    // Thumb circle
    let usable_width = bounds.size.width - THUMB_SIZE;
    let thumb_x = bounds.origin.x + usable_width * ratio;
    let thumb_y = bounds.origin.y + (bounds.size.height - THUMB_SIZE) / 2.0;
    let thumb_bounds = Bounds::new(
        point(thumb_x, thumb_y),
        Size {
            width: THUMB_SIZE,
            height: THUMB_SIZE,
        },
    );
    window.paint_quad(quad(
        thumb_bounds,
        Corners::all(Pixels::MAX).clamp_radii_for_quad_size(thumb_bounds.size),
        thumb_color,
        Edges::default(),
        transparent,
        BorderStyle::default(),
    ));
}

/// Register global mouse event handlers for slider interaction.
///
/// MouseDown on the track: compute value from click x, start drag.
/// MouseMove in Capture phase: update value during drag (works outside bounds).
/// MouseUp: end drag.
fn register_slider_mouse_handlers(
    bounds: Bounds<Pixels>,
    min: f32,
    max: f32,
    step: f32,
    entity_id: EntityId,
    value: Rc<Cell<f32>>,
    dragging: Rc<Cell<bool>>,
    on_change: Rc<dyn Fn(f32, &mut Window, &mut App)>,
    window: &mut Window,
) {
    // MouseDown: click-to-set and start drag
    {
        let value = value.clone();
        let dragging = dragging.clone();
        let on_change = on_change.clone();
        window.on_mouse_event(
            move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                if !bounds.contains(&event.position) {
                    return;
                }
                let new_value = value_from_x(min, max, step, event.position.x, bounds);
                value.set(new_value);
                dragging.set(true);
                (on_change)(new_value, window, cx);
                cx.notify(entity_id);
            },
        );
    }

    // MouseMove (Capture phase): drag updates value even outside bounds
    {
        let value = value.clone();
        let dragging = dragging.clone();
        let on_change = on_change.clone();
        window.on_mouse_event(
            move |event: &MouseMoveEvent, phase, window, cx| {
                if !dragging.get() {
                    return;
                }
                if phase != DispatchPhase::Capture || !event.dragging() {
                    return;
                }
                let new_value = value_from_x(min, max, step, event.position.x, bounds);
                let old_value = value.get();
                if (new_value - old_value).abs() > f32::EPSILON {
                    value.set(new_value);
                    (on_change)(new_value, window, cx);
                    cx.notify(entity_id);
                }
            },
        );
    }

    // MouseUp: end drag
    {
        let dragging = dragging.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, _phase, _window, _cx| {
            dragging.set(false);
        });
    }
}

/// Helper to create a Slider entity.
pub fn new_slider(
    _window: &mut Window,
    cx: &mut App,
    min: f32,
    max: f32,
    step: f32,
    initial: f32,
    label: impl Into<SharedString>,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Entity<Slider> {
    let label = label.into();
    cx.new(|cx| Slider::new(cx, min, max, step, initial, label, on_change))
}
