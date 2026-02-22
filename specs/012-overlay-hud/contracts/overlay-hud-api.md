# Contract: overlay_hud Module API

**Module**: `vox_ui::overlay_hud`
**File**: `crates/vox_ui/src/overlay_hud.rs`

## Public Types

### OverlayHud

```rust
/// The primary overlay HUD view — a compact floating pill that displays
/// the current state of the dictation pipeline. Always-on-top, borderless,
/// semi-transparent, and updates in real-time.
///
/// Subscribes to `OverlayDisplayState` for reactive state updates and
/// runs animation timers for waveform visualization and fade effects.
pub struct OverlayHud { /* fields */ }

impl OverlayHud {
    /// Creates a new overlay HUD and subscribes to state changes.
    ///
    /// Registers `observe_global::<OverlayDisplayState>()` for state reactivity
    /// and `observe_window_bounds()` for position persistence.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self;
}

impl Render for OverlayHud {
    /// Renders the overlay as a vertical flex container with:
    /// - Status bar (indicator, label, title, quick settings, menu button)
    /// - Content area (state-dependent: waveform, text, progress, buttons)
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
}
```

### OverlayDisplayState

```rust
/// Lightweight GPUI global that bridges VoxState's interior-mutable updates
/// to the overlay's reactive rendering. Replaced via `cx.set_global()` on
/// every state change to trigger `observe_global` callbacks.
///
/// Every call to `VoxState::set_readiness()` or `VoxState::set_pipeline_state()`
/// MUST be followed by `cx.set_global(OverlayDisplayState { ... })`.
#[derive(Clone)]
pub struct OverlayDisplayState {
    /// Current app lifecycle state (Downloading, Loading, Ready, Error).
    pub readiness: AppReadiness,
    /// Current pipeline operational state (Idle, Listening, Processing, etc.).
    pub pipeline_state: PipelineState,
}

impl Global for OverlayDisplayState {}
```

## Public Functions

### open_overlay_window

```rust
/// Opens the overlay HUD as a floating, always-on-top, borderless window.
///
/// Window configuration: `WindowKind::PopUp`, `focus: false` (no focus stealing),
/// `WindowDecorations::Client` (no OS chrome), transparent background.
/// Position is restored from settings if available, otherwise centered.
///
/// Returns the window handle for state bridging in the pipeline init task.
pub fn open_overlay_window(cx: &mut App) -> anyhow::Result<WindowHandle<OverlayHud>>;
```

## Internal Methods (not pub, documented for plan reference)

```rust
impl OverlayHud {
    /// Renders the status bar: indicator dot/icon, state label, "Vox" title,
    /// quick settings dropdown trigger (▾), settings menu button (≡).
    fn render_status_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders the state-dependent content area below the status bar.
    /// Dispatches to specific render methods based on readiness + pipeline_state.
    fn render_content(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders download progress: model name, percentage, byte count, progress bar.
    fn render_download_progress(&self, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders loading stage text.
    fn render_loading(&self, stage: &str, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders idle hint text with configured hotkey name.
    fn render_idle_hint(&self, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders the waveform visualizer during Listening state.
    fn render_waveform(&self, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders raw transcript text during Processing state.
    fn render_processing(&self, raw_text: &Option<String>, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders polished text after injection (with fade animation).
    fn render_injected(&self, polished_text: &str, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders injection failure: polished text + Copy button.
    fn render_injection_failed(
        &self, polished_text: &str, error: &str, cx: &mut Context<Self>
    ) -> impl IntoElement;

    /// Renders error state: error message with guidance.
    fn render_error(&self, message: &str, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders download failure: model path + "Open Folder" / "Retry Download" buttons.
    fn render_download_failed(&self, cx: &mut Context<Self>) -> impl IntoElement;

    /// Renders the quick settings dropdown (dictation toggle + language selector).
    fn render_quick_settings(&self, cx: &mut Context<Self>) -> impl IntoElement;

    /// Starts the 30fps waveform animation timer. Called on transition to Listening.
    fn start_waveform_animation(&mut self, cx: &mut Context<Self>);

    /// Stops the waveform animation timer. Called on transition out of Listening.
    fn stop_waveform_animation(&mut self);

    /// Starts the 2-second fade timer for injected text. Called on transition to Injecting.
    fn start_injection_fade(&mut self, cx: &mut Context<Self>);

    /// Handles state change from OverlayDisplayState observer.
    fn on_state_changed(&mut self, cx: &mut Context<Self>);

    /// Handles window position change. Persists to settings.
    fn on_position_changed(&mut self, window: &mut Window, cx: &mut Context<Self>);

    /// Copies polished text to clipboard (injection failure recovery).
    fn copy_to_clipboard(&mut self, text: &str, cx: &mut Context<Self>);
}
```

## Event Flow

```
VoxState.set_readiness()  ──┐
                            ├──► main.rs bridge ──► cx.set_global(OverlayDisplayState)
VoxState.set_pipeline_state() ─┘                          │
                                                          ▼
                                              observe_global callback
                                                          │
                                                          ▼
                                              OverlayHud.on_state_changed()
                                                          │
                                                          ▼
                                                    cx.notify()
                                                          │
                                                          ▼
                                              OverlayHud.render()
```
