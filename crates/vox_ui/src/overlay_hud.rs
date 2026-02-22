//! Overlay HUD window view displaying application status.
//!
//! Provides [`OverlayHud`], a minimal GPUI view that reads application state
//! from [`VoxState`](vox_core::state::VoxState) and displays the current
//! status with appropriate colors from [`VoxTheme`](crate::theme::VoxTheme).

use gpui::prelude::*;
use gpui::{div, Hsla, SharedString, WindowControlArea};

use crate::layout::{radius, size};
use crate::theme::{ThemeColors, VoxTheme};
use vox_core::models::DownloadProgress;
use vox_core::state::{AppReadiness, VoxState};

/// Minimal overlay window view showing application status.
///
/// Reads VoxState readiness and pipeline state from GPUI globals.
/// Displays status text and indicator colors from VoxTheme.
pub struct OverlayHud;

impl OverlayHud {
    /// Create a new overlay HUD instance.
    pub fn new(_cx: &mut gpui::Context<Self>) -> Self {
        Self
    }
}

impl gpui::Render for OverlayHud {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        let theme = cx.global::<VoxTheme>();
        let state = cx.global::<VoxState>();
        let readiness = state.readiness();

        let (status_text, status_color) = status_display(&readiness, &theme.colors);

        div()
            .w(size::OVERLAY_WIDTH)
            .h(size::OVERLAY_HEIGHT)
            .bg(theme.colors.overlay_bg)
            .rounded(radius::LG)
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(WindowControlArea::Drag)
            .child(div().text_color(status_color).child(status_text))
    }
}

/// Map application readiness state to display text and status color.
fn status_display(readiness: &AppReadiness, colors: &ThemeColors) -> (SharedString, Hsla) {
    fn progress_fraction(progress: &DownloadProgress) -> f32 {
        match progress {
            DownloadProgress::Pending => 0.0,
            DownloadProgress::InProgress {
                bytes_downloaded,
                bytes_total,
            } => {
                if *bytes_total == 0 {
                    0.0
                } else {
                    (*bytes_downloaded as f32 / *bytes_total as f32).clamp(0.0, 1.0)
                }
            }
            DownloadProgress::Complete => 1.0,
            DownloadProgress::Failed { .. } => 0.0,
        }
    }

    fn progress_label(progress: &DownloadProgress) -> String {
        match progress {
            DownloadProgress::Pending => "pending".into(),
            DownloadProgress::InProgress { .. } => {
                format!("{:.0}%", progress_fraction(progress) * 100.0)
            }
            DownloadProgress::Complete => "done".into(),
            DownloadProgress::Failed { .. } => "failed".into(),
        }
    }

    fn truncate_message(message: &str, max_chars: usize) -> String {
        let mut chars = message.chars();
        let truncated: String = chars.by_ref().take(max_chars).collect();
        if chars.next().is_some() {
            format!("{truncated}...")
        } else {
            truncated
        }
    }

    match readiness {
        AppReadiness::Downloading {
            vad_progress,
            whisper_progress,
            llm_progress,
        } => {
            let overall = ((progress_fraction(vad_progress)
                + progress_fraction(whisper_progress)
                + progress_fraction(llm_progress))
                / 3.0)
                * 100.0;

            let status = format!(
                "Downloading {:.0}% (VAD {}, ASR {}, LLM {})",
                overall,
                progress_label(vad_progress),
                progress_label(whisper_progress),
                progress_label(llm_progress)
            );
            (status.into(), colors.status_downloading)
        }
        AppReadiness::Loading { stage } => {
            if stage.trim().is_empty() {
                ("Loading...".into(), colors.status_processing)
            } else {
                (format!("Loading: {stage}").into(), colors.status_processing)
            }
        }
        AppReadiness::Ready => ("Ready".into(), colors.status_success),
        AppReadiness::Error { message } => {
            let msg = message.trim();
            if msg.is_empty() {
                ("Error: initialization failed".into(), colors.status_error)
            } else {
                (
                    format!("Error: {}", truncate_message(msg, 80)).into(),
                    colors.status_error,
                )
            }
        }
    }
}
