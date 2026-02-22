# Data Model: Overlay HUD

**Feature**: 012-overlay-hud
**Date**: 2026-02-22

## Entities

### E-001: OverlayHud (View Component)

The root GPUI view entity for the overlay window. Implements `Render` to produce the overlay's element tree. Subscribes to `OverlayDisplayState` global for reactive updates and runs animation timers for waveform and fade effects.

```rust
pub struct OverlayHud {
    /// Current app lifecycle state (Downloading, Loading, Ready, Error).
    readiness: AppReadiness,

    /// Current pipeline operational state (Idle, Listening, Processing, etc.).
    pipeline_state: PipelineState,

    /// Ring buffer of recent RMS amplitude values for waveform visualization.
    /// Capacity: 50 samples. Populated at 30fps during Listening state.
    waveform_samples: VecDeque<f32>,

    /// Whether the quick settings dropdown is currently visible.
    quick_settings_open: bool,

    /// Whether the injected text is currently fading out (2-second timer).
    showing_injected_fade: bool,

    /// GPUI focus handle for keyboard event routing.
    focus_handle: FocusHandle,

    /// Active subscriptions (observe_global, observe_window_bounds).
    /// Stored to prevent deregistration on drop.
    _subscriptions: Vec<Subscription>,

    /// Background task for waveform animation timer (30fps during Listening).
    /// Dropped when leaving Listening state to stop the timer.
    _waveform_task: Option<Task<()>>,

    /// Background task for injected text fade timer (2 seconds).
    _fade_task: Option<Task<()>>,
}
```

**Lifecycle**:
1. Created by `cx.new(|cx| OverlayHud::new(window, cx))` inside `open_overlay_window()`
2. Subscribes to `OverlayDisplayState` global in constructor
3. Subscribes to window bounds changes for position persistence
4. Lives for the entire application lifetime (overlay window is never closed independently)

---

### E-002: OverlayDisplayState (GPUI Global — Reactivity Bridge)

A lightweight, cloneable global that bridges VoxState's interior-mutable updates to GPUI's `observe_global` reactivity system. Replaced via `cx.set_global()` on every state change to trigger overlay re-renders.

```rust
#[derive(Clone)]
pub struct OverlayDisplayState {
    /// Current app lifecycle state.
    pub readiness: AppReadiness,

    /// Current pipeline operational state.
    pub pipeline_state: PipelineState,
}

impl gpui::Global for OverlayDisplayState {}
```

**Update sources** (all in `main.rs`):
- Pipeline initialization task: updates readiness (Downloading → Loading → Ready)
- Pipeline orchestrator subscription: updates pipeline_state (Idle → Listening → Processing → Injecting)
- Error handlers: updates either readiness or pipeline_state with Error/InjectionFailed

**Invariant**: Every call to `VoxState::set_readiness()` or `VoxState::set_pipeline_state()` MUST be followed by a corresponding `cx.set_global(OverlayDisplayState { ... })` call to keep the overlay in sync.

---

### E-003: WaveformVisualizer (Custom Element)

A stateless GPUI `Element` that renders vertical audio amplitude bars using `paint_quad`. Receives sample data and colors as constructor parameters — no internal state or side effects.

```rust
pub struct WaveformVisualizer {
    /// RMS amplitude values to render as bars. Each value in [0.0, 1.0].
    samples: Vec<f32>,

    /// Color for active waveform bars.
    bar_color: Hsla,

    /// Color for minimum-height baseline bars (silence).
    baseline_color: Hsla,
}
```

**Rendering algorithm**:
1. If `samples` is empty, render nothing (no division by zero)
2. Bar width = `bounds.width / samples.len()` with 80% fill (20% gap between bars)
3. Bar height = `sample.clamp(0.0, 1.0) * bounds.height`, minimum 2px
4. Bars are centered vertically within bounds
5. Each bar is a `paint_quad(fill(bar_bounds, color))`

---

### E-004: PipelineState (Modified Enum)

Existing enum in `vox_core::pipeline::state`. Gains one new variant for injection failure recovery.

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum PipelineState {
    Idle,
    Listening,
    Processing { raw_text: Option<String> },
    Injecting { polished_text: String },
    Error { message: String },

    // NEW: Added for FR-013 (injection failure recovery)
    /// Text injection failed. The polished text is preserved for clipboard copy.
    /// This state persists until the user clicks Copy or starts a new dictation.
    InjectionFailed {
        /// The polished text that failed to inject (available for Copy).
        polished_text: String,
        /// Human-readable error description.
        error: String,
    },
}
```

**Transition rules**:
- `Injecting` → `InjectionFailed` (when text injection fails)
- `InjectionFailed` → `Idle` (when user clicks Copy)
- `InjectionFailed` → `Listening` (when user presses hotkey — uncopied text is lost)

---

## State Transitions

### AppReadiness State Machine

```
                    ┌──────────────┐
     App Start ───►│  Downloading  │
                    │ (per-model    │
                    │  progress)    │
                    └──────┬───────┘
                           │ all models complete
                           ▼
                    ┌──────────────┐
                    │   Loading    │
                    │ (per-stage   │
                    │  messages)   │
                    └──────┬───────┘
                           │ all components loaded
                           ▼
                    ┌──────────────┐
                    │    Ready     │◄─────── Pipeline operational
                    └──────────────┘

   Any state ────► ┌──────────────┐
                    │    Error     │ (with human-readable message)
                    └──────────────┘
```

### PipelineState State Machine (when Ready)

```
                    ┌──────────┐
     Ready ────────►│   Idle   │◄────────── Copy clicked
                    └────┬─────┘◄────────── Fade complete
                         │ hotkey
                         ▼
                    ┌──────────┐
                ┌──►│ Listening│◄──── hotkey (from InjectionFailed)
                │   └────┬─────┘
                │        │ speech detected (VAD)
                │        ▼
                │   ┌────────────┐
                │   │ Processing │ (raw_text: None → Some)
                │   └────┬───────┘
                │        │ LLM complete
                │        ▼
                │   ┌────────────┐      ┌───────────────────┐
                │   │ Injecting  │─────►│ InjectionFailed   │
                │   └────┬───────┘ fail │ (polished_text +  │
                │        │ success      │  Copy button)     │
                │        ▼              └───────────────────┘
                │   ┌────────────┐
                └───┤  (Idle)    │ ← shown as "INJECTED" with fade
                    └────────────┘

   Any state ──►┌──────────┐
                │  Error   │──► Listening or Idle (recovery)
                └──────────┘
```

### Overlay Display Mapping

| AppReadiness | PipelineState | Indicator | Label | Content |
|---|---|---|---|---|
| Downloading | — | ↓ orange | DOWNLOADING | Model name + progress bar + byte counts |
| Loading | — | ⟳ blue | LOADING | Stage description text |
| Error | — | ⚠ red | ERROR | Error message with guidance |
| Ready | Idle | ● gray | IDLE | "Press [hotkey] to start dictating" |
| Ready | Listening | ● green (pulse) | LISTENING | WaveformVisualizer |
| Ready | Processing | ⟳ blue | PROCESSING | Raw transcript text |
| Ready | Injecting | ✓ green | INJECTED | Polished text (fades after 2s) |
| Ready | InjectionFailed | ⚠ yellow | INJECTION FAILED | Polished text + Copy button |
| Ready | Error | ⚠ red | ERROR | Error message with guidance |
| Downloading (hotkey) | — | ↓ orange | DOWNLOADING | "Models downloading... N%" |

---

## VoxState Additions

### New Field: `latest_rms`

```rust
// In VoxState struct
latest_rms: RwLock<f32>,
```

- Written by: audio processing thread (after RMS computation on each 512-sample window)
- Read by: OverlayHud animation timer (every 33ms during Listening)
- Default: 0.0
- Range: [0.0, 1.0] (clamped at write site)

### New Methods

```rust
/// Returns the most recent RMS amplitude value from the audio pipeline.
pub fn latest_rms(&self) -> f32 {
    *self.latest_rms.read()
}

/// Updates the latest RMS amplitude value. Called by the audio processing thread.
pub fn set_latest_rms(&self, rms: f32) {
    *self.latest_rms.write() = rms.clamp(0.0, 1.0);
}
```

---

## Settings Additions

The `Settings` struct in `vox_core::config` already has `overlay_position: OverlayPosition` and `overlay_opacity: f32`. No new fields needed — the existing schema covers position persistence (FR-016) and opacity (FR-019).

Existing fields used by the overlay:
- `overlay_position: OverlayPosition` — `TopCenter` (default), `Custom { x, y }` for dragged position
- `overlay_opacity: f32` — `0.85` default
- `language: String` — displayed in quick settings dropdown
- `show_raw_transcript: bool` — controls whether raw text shows alongside polished

---

## Key Bindings Additions

New actions for overlay interaction:

```rust
actions!(vox, [
    // Existing:
    ToggleRecording,
    StopRecording,
    ToggleOverlay,
    OpenSettings,
    Quit,
    CopyLastTranscript,
    ClearHistory,

    // New for 012:
    CopyInjectedText,    // Copy buffered text from injection failure
    RetryDownload,       // Retry a failed model download
    OpenModelFolder,     // Open the model directory in file manager
    DismissOverlay,      // Dismiss injection failure or error state
]);
```
