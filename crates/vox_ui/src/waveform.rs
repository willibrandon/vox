//! Real-time audio waveform visualization element.
//!
//! Provides [`WaveformVisualizer`], a custom GPUI [`Element`] that renders
//! vertical bars proportional to audio RMS amplitude values. Uses
//! [`paint_quad`](gpui::Window::paint_quad) with [`fill`](gpui::fill) for
//! GPU-accelerated bar rendering at 30fps during the Listening state.

use std::panic;

use gpui::{
    fill, point, px, App, Bounds, Element, ElementId, GlobalElementId, Hsla, InspectorElementId,
    IntoElement, LayoutId, Pixels, Size, Style, Window,
};

use crate::layout::size;

/// Renders audio amplitude as vertical bars for real-time waveform display.
///
/// Each sample value in [0.0, 1.0] maps to a bar height within the fixed
/// rendering bounds. Bars are centered vertically with a 2px minimum
/// baseline height. Empty sample lists render nothing (no division by zero).
///
/// Implements the low-level [`Element`] trait directly — one `paint_quad`
/// call per bar, no layout engine involvement beyond the initial size request.
pub struct WaveformVisualizer {
    /// RMS amplitude values to render as bars.
    samples: Vec<f32>,
    /// Color for bars above baseline height (active voice).
    bar_color: Hsla,
    /// Color for minimum-height baseline bars (silence).
    baseline_color: Hsla,
}

impl WaveformVisualizer {
    /// Create a new waveform visualizer with sample data and colors.
    ///
    /// Samples outside [0.0, 1.0] are clamped during rendering.
    /// An empty `samples` vec renders nothing.
    pub fn new(samples: Vec<f32>, bar_color: Hsla, baseline_color: Hsla) -> Self {
        Self {
            samples,
            bar_color,
            baseline_color,
        }
    }
}

impl IntoElement for WaveformVisualizer {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for WaveformVisualizer {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
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
        let style = Style {
            size: Size {
                width: size::WAVEFORM_WIDTH.into(),
                height: size::WAVEFORM_HEIGHT.into(),
            },
            ..Style::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        _cx: &mut App,
    ) {
        if self.samples.is_empty() {
            return;
        }

        let count = self.samples.len();
        let slot_width = bounds.size.width / count as f32;
        let bar_width = slot_width * 0.8;
        let min_height = px(2.0);

        for (i, &sample) in self.samples.iter().enumerate() {
            let clamped = sample.clamp(0.0, 1.0);
            let bar_height = (bounds.size.height * clamped).max(min_height);
            let x = bounds.origin.x + slot_width * i as f32 + (slot_width - bar_width) / 2.0;
            let y = bounds.origin.y + (bounds.size.height - bar_height) / 2.0;

            let color = if bar_height > min_height {
                self.bar_color
            } else {
                self.baseline_color
            };

            window.paint_quad(fill(
                Bounds::new(
                    point(x, y),
                    Size {
                        width: bar_width,
                        height: bar_height,
                    },
                ),
                color,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::hsla;

    #[test]
    fn test_waveform_empty() {
        let vis = WaveformVisualizer::new(
            vec![],
            hsla(0.35, 0.9, 0.55, 1.0),
            hsla(0.0, 0.0, 0.3, 1.0),
        );
        assert!(
            vis.samples.is_empty(),
            "empty samples should be preserved"
        );
    }

    #[test]
    fn test_waveform_full() {
        let samples: Vec<f32> = (0..50).map(|i| i as f32 / 49.0).collect();
        let vis = WaveformVisualizer::new(
            samples,
            hsla(0.35, 0.9, 0.55, 1.0),
            hsla(0.0, 0.0, 0.3, 1.0),
        );
        assert_eq!(vis.samples.len(), 50, "should hold 50 samples");
        for sample in &vis.samples {
            assert!(
                (0.0..=1.0).contains(sample),
                "all samples should be in [0.0, 1.0], got {sample}"
            );
        }
    }

    #[test]
    fn test_waveform_clamp_values() {
        let vis = WaveformVisualizer::new(
            vec![-0.5, 0.0, 0.5, 1.0, 1.5],
            hsla(0.35, 0.9, 0.55, 1.0),
            hsla(0.0, 0.0, 0.3, 1.0),
        );
        assert_eq!(vis.samples.len(), 5);
        // Values outside [0.0, 1.0] are clamped during paint, not at construction
        assert_eq!(vis.samples[0], -0.5);
        assert_eq!(vis.samples[4], 1.5);
    }
}
