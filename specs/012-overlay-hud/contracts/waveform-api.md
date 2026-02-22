# Contract: waveform Module API

**Module**: `vox_ui::waveform`
**File**: `crates/vox_ui/src/waveform.rs`

## Public Types

### WaveformVisualizer

```rust
/// A custom GPUI element that renders real-time audio amplitude as vertical bars.
///
/// Receives RMS amplitude values as a slice and renders them as bars within the
/// element's bounds using `paint_quad`. Each bar's height is proportional to its
/// sample value (clamped to [0.0, 1.0]). Bars are centered vertically with a
/// 2px minimum height to maintain a visible baseline during silence.
///
/// This is a stateless render-only element — it has no internal state or side effects.
/// OverlayHud manages the sample buffer and passes it on each render.
pub struct WaveformVisualizer {
    /// RMS amplitude values to render as bars. Each value should be in [0.0, 1.0].
    /// Values outside this range are clamped during rendering.
    samples: Vec<f32>,

    /// Color for waveform bars when amplitude is above baseline.
    bar_color: Hsla,

    /// Color for minimum-height baseline bars (during silence).
    baseline_color: Hsla,
}

impl WaveformVisualizer {
    /// Creates a new waveform visualizer with the given samples and colors.
    ///
    /// # Arguments
    /// * `samples` — RMS amplitude values. Empty is valid (renders nothing).
    /// * `bar_color` — Color for active bars (typically theme.colors.waveform_active).
    /// * `baseline_color` — Color for silence baseline (typically theme.colors.waveform_inactive).
    pub fn new(samples: Vec<f32>, bar_color: Hsla, baseline_color: Hsla) -> Self;
}

impl IntoElement for WaveformVisualizer {
    type Element = Self;
    fn into_element(self) -> Self::Element { self }
}

impl Element for WaveformVisualizer {
    type RequestLayoutState = ();
    type PrepaintState = ();

    // Layout: fixed size from layout::size constants (WAVEFORM_WIDTH × WAVEFORM_HEIGHT)
    // Prepaint: no-op
    // Paint: iterate samples, paint_quad for each bar
}
```

## Rendering Specification

### Bar Calculation

```
Given: bounds (from layout), N samples

bar_total_width = bounds.width / N
bar_drawn_width = bar_total_width * 0.8    (20% gap between bars)
bar_gap         = bar_total_width * 0.2

For each sample[i]:
    height = max(sample[i].clamp(0.0, 1.0) * bounds.height, px(2.0))
    x      = bounds.origin.x + i * bar_total_width
    y      = bounds.center_y - height / 2      (centered vertically)
    color  = if height > px(2.0) { bar_color } else { baseline_color }

    paint_quad(fill(Bounds { origin: (x, y), size: (bar_drawn_width, height) }, color))
```

### Edge Cases

- **Zero samples**: Return immediately from `paint()`. No quads painted, no division by zero.
- **Single sample**: One bar fills 80% of the width.
- **Values > 1.0**: Clamped to 1.0 before height calculation.
- **Values < 0.0**: Clamped to 0.0 — bar rendered at 2px minimum height.

## Layout Constants

```rust
// In vox_ui::layout::size
pub const WAVEFORM_WIDTH: Pixels = px(340.0);   // Slightly inset from overlay width
pub const WAVEFORM_HEIGHT: Pixels = px(40.0);   // Height of waveform area
```

## Performance Target

- Render 50 bars in < 0.5ms (50 paint_quad calls, each inserting a single GPU quad)
- No heap allocations during paint() beyond the samples Vec (passed by value)
