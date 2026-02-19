# Feature 012: Overlay HUD

**Status:** Not Started
**Dependencies:** 011-gpui-application-shell
**Design Reference:** Sections 4.6.2 (Overlay HUD Window), 4.6.3 (UI States), 4.6.6 (Waveform Visualizer)
**Estimated Scope:** Floating borderless window, all UI states, waveform visualizer custom element

---

## Overview

Implement the overlay HUD — a compact floating pill that shows the current state of the dictation pipeline. It's always on top, borderless, semi-transparent, and updates in real-time. This is the primary user interface during dictation. Every possible app state has a visible, informative overlay — no state is invisible or silent.

---

## Requirements

### FR-001: Overlay Window Configuration

```rust
// crates/vox_ui/src/overlay_hud.rs

fn open_overlay_window(cx: &mut App) {
    let window_options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(
            Bounds::centered(None, size(px(360.0), px(80.0)), cx),
        )),
        titlebar: Some(TitlebarOptions {
            appears_transparent: true,
            ..Default::default()
        }),
        window_decorations: Some(WindowDecorations::Client), // No title bar
        window_min_size: Some(Size { width: px(200.0), height: px(60.0) }),
        focus: false,            // Don't steal focus from user's active app
        show: true,
        is_movable: true,
        kind: WindowKind::PopUp, // Always on top
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };

    cx.open_window(window_options, |window, cx| {
        cx.new(|cx| OverlayHud::new(window, cx))
    }).expect("Failed to open overlay window");
}
```

**Key window properties:**
- `WindowKind::PopUp` — makes the window always-on-top (confirmed from Zed codebase: PopUp windows use `_NET_WM_WINDOW_TYPE_NOTIFICATION` on X11 and equivalent on Windows/macOS)
- `focus: false` — does not steal focus from the user's current application
- `WindowDecorations::Client` — no OS title bar; the app draws its own chrome
- `WindowBackgroundAppearance::Transparent` — for rounded corners and semi-transparency
- `is_movable: true` — user can drag the overlay to reposition it

### FR-002: OverlayHud Component

```rust
pub struct OverlayHud {
    pipeline_state: PipelineState,
    app_readiness: AppReadiness,
    waveform_data: Vec<f32>,
    raw_transcript: Option<String>,
    polished_transcript: Option<String>,
    focus_handle: FocusHandle,
}

impl Render for OverlayHud {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.global::<VoxTheme>();

        div()
            .flex()
            .flex_col()
            .w(px(360.0))
            .min_h(px(60.0))
            .bg(theme.colors.overlay_bg)
            .rounded(radius::LG)
            .p(spacing::SM)
            .child(self.render_status_bar(cx))
            .child(self.render_content(cx))
    }
}
```

### FR-003: UI States

Every possible app state has a corresponding overlay display:

**Startup States:**

| State | Indicator | Content |
|---|---|---|
| Downloading | ↓ (arrow) + orange | "Whisper model: 43% (780 MB / 1.8 GB)" |
| Loading | ⟳ (spinner) + blue | "Loading Whisper model onto GPU..." |
| Download Failed | ⚠ (warning) + red | Model path + "Open Folder" / "Retry Download" buttons |

**Normal Operation:**

| State | Indicator | Content |
|---|---|---|
| Idle | Gray dot | "Press [Fn] to start dictating" |
| Listening | Green dot (pulsing) | Waveform animation |
| Processing | Blue spinner | Raw transcript text |
| Injected | Green check | Polished transcript text (fades after 2s) |

**Edge Cases:**

| State | Indicator | Content |
|---|---|---|
| Not Ready (hotkey pressed during download) | ↓ + orange | "Models downloading... 43%" |
| Error | Red ⚠ | Error message with guidance |
| Injection Failed | Yellow ⚠ | Buffered text with "Copy" button |

### FR-004: Status Bar

The top row of the overlay shows:
- State indicator (colored dot/icon)
- State label ("IDLE", "LISTENING", "PROCESSING", etc.)
- "Vox" title
- Dropdown arrow (▾) for quick settings
- Menu button (≡) for opening full settings

```rust
fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
    let theme = cx.global::<VoxTheme>();
    let (indicator_color, label) = match &self.pipeline_state {
        PipelineState::Idle => (theme.colors.status_idle, "IDLE"),
        PipelineState::Listening => (theme.colors.status_listening, "LISTENING"),
        PipelineState::Processing { .. } => (theme.colors.status_processing, "PROCESSING"),
        PipelineState::Injecting { .. } => (theme.colors.status_success, "INJECTED"),
        PipelineState::Error { .. } => (theme.colors.status_error, "ERROR"),
    };

    div()
        .flex()
        .items_center()
        .gap(spacing::SM)
        .child(
            div()
                .w(px(8.0))
                .h(px(8.0))
                .rounded(radius::PILL)
                .bg(indicator_color)
        )
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::BOLD)
                .text_color(theme.colors.text_muted)
                .child(label)
        )
        .child(div().flex_1()) // Spacer
        .child(
            div()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.colors.text)
                .child("Vox")
        )
}
```

### FR-005: Download Progress Display

During model downloading, show per-model progress:

```rust
fn render_download_progress(
    &self,
    vad: &DownloadProgress,
    whisper: &DownloadProgress,
    llm: &DownloadProgress,
    cx: &mut Context<Self>,
) -> impl IntoElement {
    let theme = cx.global::<VoxTheme>();
    // Show the currently downloading model's progress
    // With a progress bar and percentage
    // "Whisper model: 43% (387 MB / 900 MB)"
}
```

### FR-006: Waveform Visualizer

Custom GPUI element using the low-level `Element` trait for real-time waveform rendering:

```rust
// crates/vox_ui/src/waveform.rs

pub struct WaveformVisualizer {
    samples: Vec<f32>,  // Recent RMS values (last ~50 windows)
}

impl IntoElement for WaveformVisualizer {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}

impl Element for WaveformVisualizer {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> { None }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> { None }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, ()) {
        let layout_id = window.request_layout(
            gpui::Style {
                size: gpui::Size {
                    width: Length::Definite(AbsoluteLength::Pixels(px(320.0))),
                    height: Length::Definite(AbsoluteLength::Pixels(px(40.0))),
                },
                ..Default::default()
            },
            std::iter::empty(),
        );
        (layout_id, ())
    }

    fn prepaint(&mut self, ...) -> () { () }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        // Draw waveform bars using GPUI's paint_quad API
        let bar_count = self.samples.len();
        if bar_count == 0 { return; }

        let bar_width = bounds.size.width / bar_count as f32;
        let center_y = bounds.origin.y + bounds.size.height / 2.0;

        for (i, &sample) in self.samples.iter().enumerate() {
            let height = sample.clamp(0.0, 1.0) * bounds.size.height;
            let x = bounds.origin.x + (i as f32 * bar_width);

            window.paint_quad(gpui::fill(
                Bounds {
                    origin: point(x, center_y - height / 2.0),
                    size: gpui::size(bar_width * 0.8, height.max(px(2.0))),
                },
                // Color from theme based on active state
                gpui::hsla(0.35, 0.9, 0.55, 0.8),
            ));
        }
    }
}
```

The waveform receives RMS values from the audio pipeline every ~32ms and renders as vertical bars that dance with the user's voice.

### FR-007: Overlay Position Persistence

Save and restore the overlay position between sessions:
- User can drag the overlay to any screen position
- Position is saved to settings when the overlay is moved
- On next launch, the overlay appears at the saved position

### FR-008: Overlay Opacity

Overlay opacity is configurable (default 0.85):

```rust
div()
    .bg(theme.colors.overlay_bg) // Already includes alpha
    .opacity(self.settings.overlay_opacity)
```

### FR-009: State Update Subscription

The overlay subscribes to pipeline state changes and re-renders accordingly:

```rust
impl OverlayHud {
    fn subscribe_to_state(&self, cx: &mut Context<Self>) {
        // Listen to VoxState changes via GPUI observation
        cx.observe_global::<VoxState>(|this, cx| {
            let state = cx.global::<VoxState>();
            this.pipeline_state = state.pipeline_state().clone();
            this.app_readiness = state.readiness().clone();
            cx.notify(); // Trigger re-render
        }).detach();
    }
}
```

---

## Visual Mockups

```
DOWNLOADING:
┌─────────────────────────────────────────┐
│  ↓ DOWNLOADING  Vox  ▾  [≡]            │
│  Whisper model: 43% (387 MB / 900 MB)  │
│  ████████████░░░░░░░░░░░░░░░░░░░░      │
└─────────────────────────────────────────┘

IDLE:
┌─────────────────────────────────────────┐
│  ● IDLE           Vox  ▾  [≡]          │
│  Press [Fn] to start dictating          │
└─────────────────────────────────────────┘

LISTENING:
┌─────────────────────────────────────────┐
│  ● LISTENING      Vox  ▾  [≡]          │
│  ████████░░░░░░░░  (waveform animation) │
└─────────────────────────────────────────┘

PROCESSING:
┌─────────────────────────────────────────┐
│  ⟳ PROCESSING     Vox  ▾  [≡]          │
│  "let's meet wednesday at three pm"     │
└─────────────────────────────────────────┘

INJECTED:
┌─────────────────────────────────────────┐
│  ✓ INJECTED       Vox  ▾  [≡]          │
│  Let's meet Wednesday at 3 PM.          │
└─────────────────────────────────────────┘
```

---

## Acceptance Criteria

- [ ] Overlay window opens as always-on-top, borderless
- [ ] Overlay does not steal focus from the active application
- [ ] All UI states render correctly (downloading, loading, idle, listening, processing, injected, error)
- [ ] Waveform visualizer animates with audio input
- [ ] Download progress shows per-model status
- [ ] Status indicator colors match pipeline state
- [ ] Overlay position is draggable and persisted
- [ ] Overlay opacity is configurable
- [ ] State updates trigger immediate re-render
- [ ] "Copy" button appears when injection fails
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_overlay_state_idle` | Idle state renders correctly |
| `test_overlay_state_listening` | Listening state shows waveform |
| `test_overlay_state_processing` | Processing shows raw transcript |
| `test_overlay_state_download` | Download shows progress bar |
| `test_waveform_empty` | Empty samples renders without crash |
| `test_waveform_full` | Full samples renders bars |

---

## Performance Targets

| Metric | Target |
|---|---|
| Overlay render time | < 2 ms per frame |
| Waveform update rate | 30 fps |
| State update → render latency | < 16 ms |
