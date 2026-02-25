//! Custom GPUI scrollbar element for vertical scrolling.
//!
//! Renders a track-and-thumb overlay using [`paint_quad`](gpui::Window::paint_quad).
//! Reads scroll position from a shared [`ScrollHandle`](gpui::ScrollHandle) and
//! supports mouse wheel tracking, click-to-jump, and thumb dragging.
//!
//! **Critical layout rule:** The scrollbar must be a **sibling** of the scroll
//! container, never a child. GPUI's `with_element_offset(scroll_offset)` shifts
//! ALL children of a scroll container during prepaint — including
//! `Position::Absolute` elements. Sibling placement keeps the scrollbar in the
//! parent's unshifted coordinate space while still reading from the shared
//! `ScrollHandle`.

use std::cell::Cell;
use std::panic;
use std::rc::Rc;

use gpui::{
    hsla, point, px, quad, relative, size, App, BorderStyle, Bounds, Corners, DispatchPhase,
    Edges, Element, ElementId, EntityId, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    InspectorElementId, IntoElement, IsZero, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Position, ScrollHandle, ScrollWheelEvent, Size, Style,
    Window,
};

/// Width of the scrollbar thumb and track in pixels.
const SCROLLBAR_WIDTH: Pixels = px(8.0);

/// Padding between the scrollbar track and the container edge.
const SCROLLBAR_PADDING: Pixels = px(4.0);

/// Minimum height for the scrollbar thumb to remain clickable.
const MIN_THUMB_HEIGHT: Pixels = px(25.0);

/// Shared drag state between the Scrollbar element and its mouse event handlers.
///
/// Stores the y-offset within the thumb where the drag started, preventing the
/// thumb from jumping to the cursor on drag start. `None` means no drag is in
/// progress. Must be stored in the parent view (persists across frames) and
/// passed to [`Scrollbar::new`].
pub type ScrollbarDragState = Rc<Cell<Option<Pixels>>>;

/// Create a fresh drag state for use with [`Scrollbar`].
pub fn new_drag_state() -> ScrollbarDragState {
    Rc::new(Cell::new(None))
}

/// A vertical scrollbar element that paints a track and draggable thumb.
///
/// Constructed per frame in the parent's `render` method and passed as a
/// `.child()` of a non-scrolling wrapper div — **never** as a child of the
/// scroll container itself.
pub struct Scrollbar {
    scroll_handle: ScrollHandle,
    notify_entity: EntityId,
    drag_state: ScrollbarDragState,
    thumb_color: Hsla,
    track_color: Hsla,
}

/// Prepaint output holding computed track and thumb geometry plus the hit-test
/// region. Consumed during [`Element::paint`].
pub struct ScrollbarPrepaint {
    track_bounds: Bounds<Pixels>,
    thumb_bounds: Bounds<Pixels>,
    _track_hitbox: Hitbox,
}

impl Scrollbar {
    /// Build a scrollbar element for the current frame.
    ///
    /// * `scroll_handle` — shared with the scroll container via `.track_scroll()`
    /// * `notify_entity` — the parent view's entity id, notified on scroll changes
    /// * `drag_state` — `Rc<Cell<Option<Pixels>>>` persisting drag offset across frames
    /// * `thumb_color` / `track_color` — theme colors for the two visual parts
    pub fn new(
        scroll_handle: ScrollHandle,
        notify_entity: EntityId,
        drag_state: ScrollbarDragState,
        thumb_color: Hsla,
        track_color: Hsla,
    ) -> Self {
        Self {
            scroll_handle,
            notify_entity,
            drag_state,
            thumb_color,
            track_color,
        }
    }
}

impl IntoElement for Scrollbar {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for Scrollbar {
    type RequestLayoutState = ();
    type PrepaintState = Option<ScrollbarPrepaint>;

    fn id(&self) -> Option<ElementId> {
        Some("vox-scrollbar".into())
    }

    fn source_location(&self) -> Option<&'static panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        // Absolute positioning covers the full parent bounds.
        // Parent must NOT be the scroll container — use a non-scrolling
        // wrapper div as the common parent of both the scroll container
        // and this Scrollbar element.
        let style = Style {
            position: Position::Absolute,
            inset: Edges::default(),
            size: size(relative(1.), relative(1.)).map(Into::into),
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) -> Option<ScrollbarPrepaint> {
        let max_offset = self.scroll_handle.max_offset();
        let viewport = self.scroll_handle.bounds();

        // No scrollbar when content fits within viewport
        if max_offset.height.is_zero() || viewport.size.height.is_zero() {
            return None;
        }

        // Thumb height proportional to visible fraction of content
        let content_height = viewport.size.height + max_offset.height;
        let visible_ratio = viewport.size.height / content_height;
        let thumb_height = (viewport.size.height * visible_ratio).max(MIN_THUMB_HEIGHT);

        // Track on the right edge with padding
        let track_x = bounds.origin.x + bounds.size.width - SCROLLBAR_WIDTH - SCROLLBAR_PADDING;
        let track_y = bounds.origin.y + SCROLLBAR_PADDING;
        let track_height = bounds.size.height - SCROLLBAR_PADDING - SCROLLBAR_PADDING;

        if track_height <= thumb_height {
            return None;
        }

        let track_bounds = Bounds::new(
            point(track_x, track_y),
            Size {
                width: SCROLLBAR_WIDTH,
                height: track_height,
            },
        );

        // Thumb position from current scroll offset
        let current_offset = self.scroll_handle.offset().y.abs();
        let scroll_ratio = current_offset / max_offset.height;
        let thumb_y = track_y + scroll_ratio * (track_height - thumb_height);

        let thumb_bounds = Bounds::new(
            point(track_x, thumb_y),
            Size {
                width: SCROLLBAR_WIDTH,
                height: thumb_height,
            },
        );

        let track_hitbox = window.insert_hitbox(track_bounds, HitboxBehavior::Normal);

        Some(ScrollbarPrepaint {
            track_bounds,
            thumb_bounds,
            _track_hitbox: track_hitbox,
        })
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        prepaint: &mut Option<ScrollbarPrepaint>,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let Some(prepaint) = prepaint.take() else {
            return;
        };

        let transparent = hsla(0.0, 0.0, 0.0, 0.0);

        // Track background
        window.paint_quad(quad(
            prepaint.track_bounds,
            Corners::default(),
            self.track_color,
            Edges::default(),
            transparent,
            BorderStyle::default(),
        ));

        // Thumb with pill-shaped corners
        window.paint_quad(quad(
            prepaint.thumb_bounds,
            Corners::all(Pixels::MAX).clamp_radii_for_quad_size(prepaint.thumb_bounds.size),
            self.thumb_color,
            Edges::default(),
            transparent,
            BorderStyle::default(),
        ));

        // --- Event handlers ---

        // Scroll wheel: trigger re-render so thumb tracks scroll position
        {
            let notify_entity = self.notify_entity;
            window.on_mouse_event(move |_: &ScrollWheelEvent, phase, _window, cx| {
                if phase == DispatchPhase::Bubble {
                    cx.notify(notify_entity);
                }
            });
        }

        // MouseDown: start drag on thumb, or click-to-jump on track
        {
            let drag_state = self.drag_state.clone();
            let scroll_handle = self.scroll_handle.clone();
            let notify_entity = self.notify_entity;
            let track_bounds = prepaint.track_bounds;
            let thumb_bounds = prepaint.thumb_bounds;

            window.on_mouse_event(
                move |event: &MouseDownEvent, phase, _window, cx| {
                    if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                        return;
                    }

                    if thumb_bounds.contains(&event.position) {
                        let offset = event.position.y - thumb_bounds.origin.y;
                        drag_state.set(Some(offset));
                    } else if track_bounds.contains(&event.position) {
                        // Click-to-jump: center thumb on click, then enter drag
                        let track_y = track_bounds.origin.y;
                        let track_height = track_bounds.size.height;
                        let thumb_height = thumb_bounds.size.height;
                        let max_offset = scroll_handle.max_offset();

                        let available = track_height - thumb_height;
                        let click_pos = event.position.y - track_y - thumb_height / 2.0;
                        let ratio = (click_pos / available).clamp(0.0, 1.0);
                        let new_y = -(max_offset.height * ratio);

                        let current = scroll_handle.offset();
                        scroll_handle.set_offset(point(current.x, new_y));
                        cx.notify(notify_entity);

                        drag_state.set(Some(thumb_height / 2.0));
                    }
                },
            );
        }

        // MouseMove: handle thumb drag (Capture phase for dragging outside scrollbar)
        {
            let drag_state = self.drag_state.clone();
            let scroll_handle = self.scroll_handle.clone();
            let notify_entity = self.notify_entity;
            let track_bounds = prepaint.track_bounds;
            let thumb_height = prepaint.thumb_bounds.size.height;

            window.on_mouse_event(
                move |event: &MouseMoveEvent, phase, _window, cx| {
                    let Some(drag_offset) = drag_state.get() else {
                        return;
                    };
                    if phase != DispatchPhase::Capture || !event.dragging() {
                        return;
                    }

                    let track_y = track_bounds.origin.y;
                    let track_height = track_bounds.size.height;
                    let max_offset = scroll_handle.max_offset();

                    let available = track_height - thumb_height;
                    let thumb_top = event.position.y - track_y - drag_offset;
                    let ratio = (thumb_top / available).clamp(0.0, 1.0);
                    let new_y = -(max_offset.height * ratio);

                    let current = scroll_handle.offset();
                    scroll_handle.set_offset(point(current.x, new_y));
                    cx.notify(notify_entity);
                },
            );
        }

        // MouseUp: end drag
        {
            let drag_state = self.drag_state.clone();
            window.on_mouse_event(move |_: &MouseUpEvent, _phase, _window, _cx| {
                drag_state.set(None);
            });
        }
    }
}
