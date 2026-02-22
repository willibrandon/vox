# Research: Overlay HUD

**Feature**: 012-overlay-hud
**Date**: 2026-02-22

## R-001: GPUI State Reactivity — Bridge Global Pattern

**Decision**: Use a lightweight `OverlayDisplayState` GPUI Global as a bridge between `VoxState` (interior-mutable, set once) and the overlay's reactive rendering.

**Rationale**: GPUI's `observe_global::<T>()` fires when `cx.set_global::<T>()` is called. `VoxState` uses `RwLock` interior mutability — calling `state.set_readiness()` writes to the lock but does NOT trigger `observe_global`. A separate, cheap-to-clone global that IS replaced via `set_global()` on each state change provides the reactivity bridge.

**Alternatives considered**:
- **Polling timer**: Spawn a 100ms timer that reads VoxState and calls `cx.notify()` on change. Simpler but adds latency (up to 100ms vs. immediate) and burns CPU polling. Rejected because FR-015 requires <16ms state-to-render latency.
- **WindowHandle forwarding**: Pass `WindowHandle<OverlayHud>` to the async pipeline init task and call `update()` on state changes. Complex async boundary management; `WindowHandle::update` across `AsyncApp` contexts is fragile. Rejected for complexity.
- **Entity-based wrapper**: Make VoxState an `Entity<T>` instead of a Global. Would require major refactoring of the existing state architecture (Feature 009). Rejected — too invasive for a UI feature.

**Implementation**:
```rust
// In vox_ui::overlay_hud
#[derive(Clone)]
pub struct OverlayDisplayState {
    pub readiness: AppReadiness,
    pub pipeline_state: PipelineState,
}
impl gpui::Global for OverlayDisplayState {}

// In main.rs, after each state change:
cx.update(|cx| {
    cx.set_global(OverlayDisplayState {
        readiness: new_readiness,
        pipeline_state: current_pipeline_state,
    });
}).ok();

// In OverlayHud::new():
cx.observe_global::<OverlayDisplayState>(|this, cx| {
    let display = cx.global::<OverlayDisplayState>();
    this.readiness = display.readiness.clone();
    this.pipeline_state = display.pipeline_state.clone();
    cx.notify();
}).detach();
```

---

## R-002: Waveform Rendering — paint_quad Bars

**Decision**: Use `window.paint_quad(gpui::fill(...))` to draw individual waveform bars in a custom `Element` trait implementation. Not `PathBuilder` for connected line graphs.

**Rationale**: The spec calls for "vertical bars that animate with audio amplitude" — discrete bars, not a continuous waveform line. `paint_quad` with `fill()` draws solid rectangles, which is the correct primitive for bar visualization. `PathBuilder` (line/curve drawing) is for connected paths and would require significantly more code for the same visual result.

**Alternatives considered**:
- **PathBuilder stroke**: Draw connected line segments between sample points. Produces a continuous line graph, not the "bars" specified. Rejected — wrong visual.
- **Canvas/SVG rendering**: GPUI has no canvas API. SVG rendering (`paint_svg`) requires pre-built SVG paths. Rejected — not suitable for dynamic data.
- **Div-based bars**: Create `div()` elements for each bar with dynamic height. Works but creates 50+ elements per frame (one per sample), adding layout overhead. Rejected — custom `Element` with direct paint is more efficient.

**Implementation**: `WaveformVisualizer` implements `Element` trait directly. In `paint()`, iterate samples and call `paint_quad(fill(bounds, color))` for each bar. Bar width = total_width / sample_count. Bar height = sample_value * max_height. Bars are centered vertically with a 2px minimum height baseline.

**GPUI paint_quad API** (from Zed `crates/gpui/src/window.rs:2830`):
```rust
// fill() creates a PaintQuad with solid background, no border, no corner radius
pub fn fill(bounds: impl Into<Bounds<Pixels>>, background: impl Into<Background>) -> PaintQuad;

// paint_quad() inserts the quad into the scene graph for GPU rendering
pub fn paint_quad(&mut self, quad: PaintQuad);
```

---

## R-003: Waveform Data Flow — RMS via VoxState

**Decision**: Add `latest_rms: RwLock<f32>` to `VoxState`. OverlayHud maintains its own ring buffer of the last 50 values, populated from a 30fps animation timer during Listening state.

**Rationale**: The audio processing thread computes RMS every ~32ms (512-sample window at 16kHz). The overlay needs the last ~50 values for visualization. Storing only the latest value in VoxState keeps the shared state minimal. The overlay accumulates its own history because visualization buffer management is a UI concern.

**Alternatives considered**:
- **Full buffer in VoxState**: Store `Vec<f32>` of 50 samples in VoxState. Forces VoxState to manage visualization-specific buffer sizes. Rejected — mixes UI concerns into core state.
- **Dedicated broadcast channel**: Audio thread → broadcast → overlay subscriber. Requires new channel infrastructure and subscription management. Rejected — overkill for a single f32 value at 30Hz.
- **Include in OverlayDisplayState**: Waveform samples as part of the bridge global. Would trigger `set_global()` 30 times/second. While performant, it couples high-frequency waveform updates with low-frequency state changes. Rejected — mixing update frequencies.

**Implementation**: Audio processing thread writes `state.set_latest_rms(rms)` (RwLock write, <1μs). OverlayHud runs a 33ms animation timer (only during Listening) that reads `state.latest_rms()`, pushes to a `VecDeque<f32>` ring buffer (capacity 50), and calls `cx.notify()`.

---

## R-004: GPUI Element Trait — Exact Signatures

**Decision**: WaveformVisualizer implements `Element` with `RequestLayoutState = ()` and `PrepaintState = ()`.

**Rationale**: The waveform has a fixed layout size (configured at construction) and no prepaint requirements (no hitboxes, no interactivity). Both state types are unit.

**Exact trait definition** (from Zed `crates/gpui/src/element.rs:47`):
```rust
pub trait Element: 'static + IntoElement {
    type RequestLayoutState: 'static;
    type PrepaintState: 'static;

    fn id(&self) -> Option<ElementId>;
    fn source_location(&self) -> Option<&'static panic::Location<'static>>;

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState);

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState;

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    );
}
```

**Reference implementations**: `Surface` element in Zed uses `() / ()` state types (simplest pattern). `Svg` uses `() / Option<Hitbox>` for click detection. `SharedString` uses `TextLayout / ()` for text measurement.

---

## R-005: Window Position Persistence

**Decision**: Use `cx.observe_window_bounds()` to detect position changes, save to `Settings.overlay_position` as `Custom { x, y }`, and restore via `WindowOptions::window_bounds` on next launch.

**Rationale**: GPUI provides `observe_window_bounds()` (from `crates/gpui/src/app/context.rs:429`) which fires a callback whenever the window is moved or resized. This is the GPUI-idiomatic way to track window position.

**API** (from Zed codebase):
```rust
pub fn observe_window_bounds(
    &self,
    window: &mut Window,
    mut callback: impl FnMut(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> Subscription;
```

**Position clamping**: After reading saved position, clamp to current screen bounds before applying. If the saved monitor is no longer connected, fall back to primary monitor center (edge case from spec).

**Implementation**: On position change callback → read `window.bounds()` → convert to `OverlayPosition::Custom { x, y }` → call `state.update_settings(|s| s.overlay_position = pos)` which persists to `settings.json`.

---

## R-006: Clipboard API

**Decision**: Use `cx.write_to_clipboard(ClipboardItem::new_string(text))` for the injection failure Copy button.

**Rationale**: GPUI provides direct clipboard access via `App::write_to_clipboard()` and `App::read_from_clipboard()`. The Windows implementation uses Win32 clipboard APIs. No external crate needed.

**API** (from Zed `crates/gpui/src/app.rs:1044`):
```rust
pub fn write_to_clipboard(&self, item: ClipboardItem);
pub fn read_from_clipboard(&self) -> Option<ClipboardItem>;
```

`ClipboardItem` supports `ClipboardEntry::String` and `ClipboardEntry::Image`. For text copy, use `ClipboardItem::new_string(text)`.

---

## R-007: Animation — Pulsing Dot and Fade Effects

**Decision**: Use GPUI's `AnimationExt::with_animation()` for the pulsing Listening indicator and async timer for the 2-second injection text fade.

**Rationale**: GPUI has built-in animation support (from `crates/gpui/src/elements/animation.rs`). `with_animation()` takes a duration, easing function, and animator closure that receives a delta [0.0, 1.0]. For repeating animations (pulsing dot), use `.repeat()`. For one-shot effects (fade), use the default one-shot mode or `cx.spawn()` with a timer.

**API**:
```rust
pub trait AnimationExt {
    fn with_animation(
        self,
        id: impl Into<ElementId>,
        animation: Animation,
        animator: impl Fn(Self, f32) -> Self + 'static,
    ) -> AnimationElement<Self>;
}

impl Animation {
    pub fn new(duration: Duration) -> Self;
    pub fn repeat(mut self) -> Self;
    pub fn with_easing(mut self, easing: impl Fn(f32) -> f32 + 'static) -> Self;
}
```

**Pulsing dot**: `div().with_animation("pulse", Animation::new(Duration::from_secs(1)).repeat(), |div, delta| div.opacity(0.4 + delta * 0.6))`

**Fade after injection**: `cx.spawn(async move |this, cx| { timer(2s).await; this.update(cx, |this, cx| { this.show_fade = false; cx.notify(); }); })` — sets a flag that changes content area rendering.

---

## R-008: Context Menu / Quick Settings Dropdown

**Decision**: Implement a simple dropdown component for quick settings, referencing Tusk's `ContextMenu` pattern but simplified for the overlay's needs (2 items: dictation toggle + language selector).

**Rationale**: Tusk's `ContextMenu` (`crates/tusk_ui/src/context_menu.rs`) provides a production-grade implementation with keyboard navigation, submenus, and overflow handling. For the overlay's quick settings (only 2 items), a simplified version is appropriate. The key patterns to reuse: `ContextMenuLayer` global for managing the active menu, viewport overflow clamping, and click-outside-to-dismiss.

**Implementation**: A `QuickSettingsDropdown` struct rendered as a positioned div when open. Contains a dictation toggle (dispatches `ToggleRecording` action) and a language selector (updates `Settings.language`). Opens on ▾ button click, closes on click-outside or Escape.

---

## R-009: PipelineState — InjectionFailed Variant

**Decision**: Add `InjectionFailed { polished_text: String, error: String }` variant to `PipelineState` enum.

**Rationale**: The spec requires "Injection Failed" as a distinct overlay state with the buffered polished text visible and a Copy button (FR-013). The current `Error { message }` variant loses the polished text — it only stores the error message. A dedicated variant preserves both the text (for Copy) and the error (for display).

**Alternatives considered**:
- **Sub-case of Error**: Add polished_text field to Error variant. Breaks existing Error consumers that don't expect polished text. Rejected — semantic mismatch.
- **Overlay-only state**: Track injection failure only in OverlayDisplayState, not in PipelineState. The pipeline orchestrator is the component that detects injection failure — it should emit the correct state. Rejected — state should originate at the source.

**Impact**: `PipelineState` enum in `crates/vox_core/src/pipeline/state.rs` gains one variant. All `match` arms on `PipelineState` must handle the new variant. The pipeline orchestrator emits this state when text injection fails.

---

## R-010: Overlay Window Configuration

**Decision**: Reuse existing `WindowKind::PopUp` + `focus: false` + `WindowDecorations::Client` + `WindowBackgroundAppearance::Transparent` configuration from the current 011 implementation.

**Rationale**: The 011-gpui-app-shell already established the correct window configuration. Research confirmed: `WindowKind::PopUp` uses `_NET_WM_WINDOW_TYPE_NOTIFICATION` on X11 and equivalent always-on-top semantics on Windows/macOS. `focus: false` prevents focus stealing. `WindowDecorations::Client` removes OS chrome. `is_resizable: false` prevents user resizing (overlay has a fixed width).

**Existing configuration** (from `crates/vox/src/main.rs:67-98`):
```rust
WindowOptions {
    window_bounds: Some(WindowBounds::Windowed(bounds)),
    titlebar: Some(TitlebarOptions { appears_transparent: true, .. }),
    focus: false,
    show: true,
    kind: WindowKind::PopUp,
    is_movable: true,
    is_resizable: false,
    window_background: WindowBackgroundAppearance::Transparent,
    ..Default::default()
}
```

**Change for position persistence**: Replace `Bounds::centered(None, window_size, cx)` with saved position from settings (if `OverlayPosition::Custom`), falling back to centered.

---

## R-011: Theme Colors — Missing States

**Decision**: Add `status_loading` (blue, same as `status_processing`) and `status_injection_failed` (yellow) to `ThemeColors`.

**Rationale**: The existing theme has 6 status colors but is missing specific colors for Loading (blue spinner) and Injection Failed (yellow warning). The spec defines: blue for Loading (FR-005), yellow for Injection Failed (FR-005). Loading can reuse the `status_processing` blue, but a named alias improves code clarity. Injection Failed needs a distinct yellow.

**New colors**:
- `status_loading: Hsla` — `hsla(0.6, 0.8, 0.6, 1.0)` (same as `status_processing`)
- `status_injection_failed: Hsla` — `hsla(0.15, 0.9, 0.6, 1.0)` (amber/yellow)
