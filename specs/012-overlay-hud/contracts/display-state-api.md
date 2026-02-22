# Contract: OverlayDisplayState Bridge API

**Defined in**: `vox_ui::overlay_hud`
**Used by**: `vox` (main.rs), `vox_ui` (overlay_hud.rs)

## Purpose

`OverlayDisplayState` bridges the gap between `VoxState` (which uses `RwLock` interior mutability and is set as a GPUI Global once) and GPUI's reactive `observe_global` system (which fires callbacks only when `cx.set_global()` replaces the global value).

## Type Definition

```rust
/// Reactive bridge between VoxState and the overlay HUD.
///
/// GPUI's `observe_global` triggers when a global is replaced via `set_global()`.
/// Since `VoxState` uses interior mutability (`RwLock`) and is set once at startup,
/// mutations to its fields do NOT trigger observers. This lightweight global is
/// replaced on every state change, providing the reactivity the overlay needs.
///
/// # Contract
///
/// Every mutation of `VoxState::readiness` or `VoxState::pipeline_state` MUST be
/// immediately followed by `cx.set_global(OverlayDisplayState { ... })` with the
/// updated values. Failure to do so causes the overlay to show stale state.
#[derive(Clone)]
pub struct OverlayDisplayState {
    /// Current app lifecycle state. Mirrors `VoxState::readiness()`.
    pub readiness: AppReadiness,

    /// Current pipeline operational state. Mirrors `VoxState::pipeline_state()`.
    pub pipeline_state: PipelineState,
}

impl gpui::Global for OverlayDisplayState {}
```

## Usage: Producer (main.rs — primary; overlay_hud.rs — permitted for user actions)

```rust
// Initialization (alongside VoxState)
cx.set_global(OverlayDisplayState {
    readiness: AppReadiness::Downloading { /* initial progress */ },
    pipeline_state: PipelineState::Idle,
});

// On every state change (inside cx.spawn async task)
fn update_overlay_state(cx: &mut App) {
    let state = cx.global::<VoxState>();
    cx.set_global(OverlayDisplayState {
        readiness: state.readiness(),
        pipeline_state: state.pipeline_state(),
    });
}

// Pattern: always pair VoxState mutation with bridge update
cx.global::<VoxState>().set_readiness(AppReadiness::Loading { stage: "...".into() });
update_overlay_state(cx);
```

**Note**: The overlay (consumer) may also perform bridge updates for user-initiated state transitions that originate from overlay UI interactions (e.g., Copy button sets `PipelineState::Idle`). In these cases, the overlay mutates `VoxState` and immediately calls `cx.set_global(OverlayDisplayState { ... })` to keep the bridge in sync. The invariant remains: every `VoxState` mutation MUST be paired with a bridge update, regardless of call site.

## Usage: Consumer (overlay_hud.rs)

```rust
// In OverlayHud::new()
let _sub = cx.observe_global::<OverlayDisplayState>(|this, cx| {
    this.on_state_changed(cx);
});
self._subscriptions.push(_sub);

// Handler
fn on_state_changed(&mut self, cx: &mut Context<Self>) {
    let display = cx.global::<OverlayDisplayState>();
    let old_pipeline = self.pipeline_state.clone();
    self.readiness = display.readiness.clone();
    self.pipeline_state = display.pipeline_state.clone();

    // Start/stop waveform animation on Listening transitions
    match (&old_pipeline, &self.pipeline_state) {
        (_, PipelineState::Listening) if old_pipeline != PipelineState::Listening => {
            self.start_waveform_animation(cx);
        }
        (PipelineState::Listening, _) => {
            self.stop_waveform_animation();
        }
        _ => {}
    }

    // Cancel fade timer when leaving Injecting (edge case #4: new state during fade-out)
    if matches!(old_pipeline, PipelineState::Injecting { .. })
        && !matches!(self.pipeline_state, PipelineState::Injecting { .. })
    {
        self._fade_task = None;
        self.showing_injected_fade = false;
    }

    // Start fade timer on entering Injecting
    if matches!(self.pipeline_state, PipelineState::Injecting { .. })
        && !matches!(old_pipeline, PipelineState::Injecting { .. })
    {
        self.start_injection_fade(cx);
    }

    cx.notify(); // Trigger re-render
}
```

## Guarantees

1. `OverlayDisplayState` is always in sync with `VoxState` — they are updated atomically
2. `observe_global` fires synchronously on the foreground thread after `set_global()`
3. Multiple observers are supported (if other components need state reactivity)
4. Clone cost is minimal: `AppReadiness` and `PipelineState` are small enums with at most one `String` field
