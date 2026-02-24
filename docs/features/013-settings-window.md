# Feature 013: Settings Window & Panels

**Status:** Not Started
**Dependencies:** 011-gpui-application-shell, 010-custom-dictionary
**Design Reference:** Sections 4.6.4 (Component Architecture), 4.6.5 (Settings Window Panels)
**Estimated Scope:** Workspace layout, five panels (Settings, History, Dictionary, Model, Log)

---

## Overview

Implement the full settings/management window accessible from the system tray or overlay menu. It uses a workspace layout with a sidebar navigation and five panels. This follows the Tusk workspace pattern — a dock-based layout where the left sidebar selects the active panel and the center area renders the selected panel's content.

Scrollable panels use a custom `Scrollbar` element (`crates/vox_ui/src/scrollbar.rs`) — a vertical-only, always-visible GPUI `Element` implementation modeled after Zed's `ScrollbarElement` (`crates/ui/src/components/scrollbar.rs`). Supports mouse wheel tracking, click-to-jump, and thumb dragging via `ScrollHandle` + `ScrollbarDragState` (`Rc<Cell<Option<Pixels>>>`). The scrollbar **must** be a sibling of the scroll container, never a child — GPUI's `with_element_offset(scroll_offset)` shifts all children of a scroll container during prepaint, including absolute-positioned elements (see FR-003 for the required pattern).

---

## Requirements

### FR-001: Settings Window

```rust
// crates/vox_ui/src/workspace.rs

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, IntoElement, Render, ScrollHandle, Window,
    WindowBounds, WindowOptions,
};
use crate::layout::spacing;
use crate::scrollbar::{new_drag_state, Scrollbar, ScrollbarDragState};
use crate::theme::VoxTheme;

pub struct SettingsWindow {
    focus_handle: gpui::FocusHandle,
    scroll_handle: ScrollHandle,
    scrollbar_drag: ScrollbarDragState,
}

impl SettingsWindow {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
            scrollbar_drag: new_drag_state(),
        }
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();

        div()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(theme.colors.surface)
            .text_color(theme.colors.text)
            .child(
                div()
                    .id("scroll-container")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .p(spacing::LG)
                    .flex()
                    .flex_col()
                    .gap(spacing::LG)
                    .children((0..50).map(|i| {
                        let theme = cx.global::<VoxTheme>();
                        div()
                            .px(spacing::LG)
                            .py(spacing::MD)
                            .rounded(px(6.0))
                            .bg(theme.colors.elevated_surface)
                            .border_1()
                            .border_color(theme.colors.border)
                            .child(format!("Section {i} — scroll me"))
                    })),
            )
            .child(Scrollbar::new(
                self.scroll_handle.clone(),
                cx.entity_id(),
                self.scrollbar_drag.clone(),
                theme.colors.text_muted,
                theme.colors.elevated_surface,
            ))
    }
}

pub fn open_settings_window(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(800.0), px(600.0)), cx);
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

    if let Err(err) = cx.open_window(options, |window, cx| {
        cx.new(|cx| SettingsWindow::new(window, cx))
    }) {
        tracing::error!(%err, "failed to open settings window");
    }
}
```

### FR-001a: Scrollbar Element

Custom GPUI `Element` implementation for vertical scrollbars. Renders a track and thumb overlay using `paint_quad`, reads scroll position from a shared `ScrollHandle`, supports mouse wheel tracking, click-to-jump, and thumb dragging. Must be a **sibling** of the scroll container div, never a child — GPUI's `window.with_element_offset(scroll_offset)` in `div.rs` line 1407 shifts ALL children of a scroll container during prepaint, including `Position::Absolute` elements.

```rust
// crates/vox_ui/src/scrollbar.rs

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
/// Stores the y-offset within the thumb where the drag started, so the thumb
/// doesn't jump to the cursor position on drag start. None means no drag
/// is in progress. Must be stored in the parent view (persists across frames)
/// and passed to Scrollbar::new.
pub type ScrollbarDragState = Rc<Cell<Option<Pixels>>>;

pub fn new_drag_state() -> ScrollbarDragState {
    Rc::new(Cell::new(None))
}

pub struct Scrollbar {
    scroll_handle: ScrollHandle,
    notify_entity: EntityId,
    drag_state: ScrollbarDragState,
    thumb_color: Hsla,
    track_color: Hsla,
}

pub struct ScrollbarPrepaint {
    track_bounds: Bounds<Pixels>,
    thumb_bounds: Bounds<Pixels>,
    _track_hitbox: Hitbox,
}

impl Scrollbar {
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

        // Thumb height proportional to visible fraction of content —
        // same formula as Zed's ScrollbarState::thumb_ranges
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

        // Track background (square corners)
        window.paint_quad(quad(
            prepaint.track_bounds,
            Corners::default(),
            self.track_color,
            Edges::default(),
            transparent,
            BorderStyle::default(),
        ));

        // Thumb with pill-shaped corners — Corners::all(Pixels::MAX) clamped
        // to the thumb size, identical to Zed's scrollbar thumb rendering.
        window.paint_quad(quad(
            prepaint.thumb_bounds,
            Corners::all(Pixels::MAX).clamp_radii_for_quad_size(prepaint.thumb_bounds.size),
            self.thumb_color,
            Edges::default(),
            transparent,
            BorderStyle::default(),
        ));

        // --- Event handlers ---

        // Scroll wheel: trigger re-render so thumb tracks scroll position.
        {
            let notify_entity = self.notify_entity;
            window.on_mouse_event(move |_: &ScrollWheelEvent, phase, _window, cx| {
                if phase == DispatchPhase::Bubble {
                    cx.notify(notify_entity);
                }
            });
        }

        // MouseDown: start drag on thumb, or click-to-jump on track.
        // Clicking the track also enters drag mode so the user can
        // immediately adjust after jumping.
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
                        // Drag from current thumb position — record offset within thumb
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

                        // Enter drag centered on the thumb
                        drag_state.set(Some(thumb_height / 2.0));
                    }
                },
            );
        }

        // MouseMove: handle thumb drag. Uses Capture phase so dragging
        // works even when the cursor leaves the scrollbar area (standard
        // scrollbar behavior, matching Zed's DispatchPhase::Capture for drag).
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
```

### FR-002: VoxWorkspace

```rust
pub struct VoxWorkspace {
    active_panel: Panel,
    settings_panel: Entity<SettingsPanel>,
    history_panel: Entity<HistoryPanel>,
    dictionary_panel: Entity<DictionaryPanel>,
    model_panel: Entity<ModelPanel>,
    log_panel: Entity<LogPanel>,
    focus_handle: FocusHandle,
}

#[derive(Clone, PartialEq)]
pub enum Panel {
    Settings,
    History,
    Dictionary,
    Model,
    Log,
}
```

Layout:

```
┌──────────────────────────────────────────────┐
│  Vox Settings                          [─ □ ×]│
├────────────┬─────────────────────────────────┤
│            │                                  │
│  Settings  │                                  │
│  History   │     Active Panel Content         │
│  Dictionary│                                  │
│  Models    │                                  │
│  Logs      │                                  │
│            │                                  │
│            │                                  │
├────────────┴─────────────────────────────────┤
│  Status: Ready | Latency: 165ms | VRAM: 5.2GB│
└──────────────────────────────────────────────┘
```

### FR-003: Settings Panel

| Section | Controls |
|---|---|
| **Audio** | Input device dropdown, noise gate slider |
| **VAD** | Threshold slider, min silence slider, min speech slider |
| **Hotkey** | Activation hotkey recorder, hold-to-talk toggle, hands-free toggle |
| **LLM** | Temperature slider, filler removal toggle, course correction toggle, punctuation toggle |
| **Appearance** | Theme dropdown (System/Light/Dark), overlay opacity slider, overlay position dropdown, show raw transcript toggle |
| **Advanced** | Max segment duration, overlap duration, command prefix input |

Each setting change saves to JSON immediately and takes effect without restart.

```rust
pub struct SettingsPanel {
    // Audio section
    input_devices: Vec<AudioDeviceInfo>,
    selected_device: Option<String>,

    // Sliders, toggles, etc. — all backed by Settings struct
    settings: Settings,

    // Scroll state — handle shared between scroll container and Scrollbar element,
    // drag state (Rc<Cell<Option<Pixels>>>) persists thumb-drag offset across frames
    scroll_handle: ScrollHandle,
    scrollbar_drag: ScrollbarDragState,
}

impl Render for SettingsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();
        // CRITICAL: Scrollbar (Position::Absolute element from scrollbar.rs) must be
        // a SIBLING of the scroll container, never a child. GPUI applies
        // with_element_offset(scroll_offset) to ALL children of a scroll container
        // during prepaint — including absolute-positioned elements — so a child
        // scrollbar shifts with the content. Sibling placement keeps the scrollbar
        // in the parent's unshifted coordinate space while reading from the shared
        // ScrollHandle. The scroll container MUST have .id() (StatefulInteractiveElement
        // required for overflow_y_scroll) and .track_scroll() to connect the handle.
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
                    .child(self.render_audio_section(cx))
                    .child(self.render_vad_section(cx))
                    .child(self.render_hotkey_section(cx))
                    .child(self.render_llm_section(cx))
                    .child(self.render_appearance_section(cx))
                    .child(self.render_advanced_section(cx)),
            )
            .child(Scrollbar::new(
                self.scroll_handle.clone(),
                cx.entity_id(),
                self.scrollbar_drag.clone(),
                theme.colors.text_muted,
                theme.colors.elevated_surface,
            ))
    }
}
```

### FR-004: History Panel

Displays past transcriptions with search and pagination:

```rust
pub struct HistoryPanel {
    transcripts: Vec<TranscriptEntry>,
    search_query: String,
    scroll_handle: UniformListScrollHandle,
}
```

Features:
- Search by raw or polished text
- Display: timestamp, raw text (optional), polished text, target app, latency
- Copy individual transcript to clipboard
- Delete individual transcript
- "Clear All" with confirmation dialog
- Infinite scroll using GPUI's `uniform_list`

### FR-005: Dictionary Panel

CRUD interface for the custom dictionary:

```rust
pub struct DictionaryPanel {
    entries: Vec<DictionaryEntry>,
    search_query: String,
    editing_entry: Option<DictionaryEntry>,
    new_spoken: String,
    new_written: String,
    new_category: String,
}
```

Features:
- List all entries with search/filter by category
- Add new entry (spoken, written, category)
- Edit existing entry inline
- Delete entry with confirmation
- Import/export buttons (JSON file)
- Toggle command phrase flag per entry
- Sort by name, category, use count

### FR-006: Model Panel

Model management interface:

```rust
pub struct ModelPanel {
    models: Vec<ModelStatus>,
    download_progress: HashMap<String, DownloadProgress>,
}

pub struct ModelStatus {
    pub info: ModelInfo,
    pub state: ModelState,
}

pub enum ModelState {
    Missing,
    Downloading { progress: DownloadProgress },
    Downloaded { file_size: u64 },
    Loaded { vram_usage: u64 },
    Error { message: String },
}
```

Features:
- Show status of each model (missing, downloading, loaded)
- Download progress for each model
- "Retry Download" for failed models
- "Open Model Folder" button
- Quick benchmark result (inference speed)
- Swap model button (select new GGUF/GGML file)

### FR-007: Log Panel

Live log viewer showing tracing output:

```rust
pub struct LogPanel {
    log_entries: Vec<LogEntry>,
    auto_scroll: bool,
    filter_level: LogLevel,
    scroll_handle: UniformListScrollHandle,
}

pub struct LogEntry {
    pub timestamp: String,
    pub level: LogLevel,
    pub target: String,
    pub message: String,
}

pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}
```

Features:
- Real-time log streaming from tracing subscriber
- Filter by log level
- Auto-scroll (toggle on/off)
- Copy log entries to clipboard
- Clear log display
- Color-coded by level (red=error, yellow=warn, white=info, gray=debug)

### FR-008: Sidebar Navigation

```rust
fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let theme = cx.global::<VoxTheme>();

    div()
        .flex()
        .flex_col()
        .w(px(160.0))
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

fn sidebar_item(
    &self,
    label: &str,
    panel: Panel,
    cx: &mut Context<Self>,
) -> impl IntoElement {
    let theme = cx.global::<VoxTheme>();
    let is_active = self.active_panel == panel;

    div()
        .px(spacing::MD)
        .py(spacing::SM)
        .rounded(radius::SM)
        .cursor_pointer()
        .when(is_active, |d| d.bg(theme.colors.accent).text_color(theme.colors.button_primary_text))
        .when(!is_active, |d| d.text_color(theme.colors.text_muted).hover(|d| d.bg(theme.colors.elevated_surface)))
        .child(label)
        .on_click(cx.listener(move |this, _, _, cx| {
            this.active_panel = panel.clone();
            cx.notify();
        }))
}
```

### FR-009: Status Bar

Bottom status bar showing runtime info:

```
Ready | Latency: 165ms | VRAM: 5.2 GB | Audio: MacBook Pro Microphone
```

---

## Acceptance Criteria

- [ ] Settings window opens from tray/overlay
- [ ] Sidebar navigation switches panels
- [ ] Settings panel: all controls work and persist immediately
- [ ] History panel: search, scroll, copy, delete work
- [ ] Dictionary panel: CRUD operations work
- [ ] Model panel: shows status of all models
- [ ] Log panel: displays logs in real-time
- [ ] Window remembers size/position between sessions
- [ ] All panels use consistent theme colors
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_panel_switching` | Active panel changes on sidebar click |
| `test_settings_persistence` | Changed setting persists to JSON |
| `test_history_search` | Search filters transcripts |
| `test_dictionary_add` | New entry appears in list |
| `test_dictionary_delete` | Deleted entry removed from list |

---

## Performance Targets

| Metric | Target |
|---|---|
| Panel switch time | < 16 ms |
| History list (10,000 entries) | 60 fps scroll |
| Log panel (live streaming) | No frame drops at 100 logs/sec |
| Settings save | < 10 ms |
