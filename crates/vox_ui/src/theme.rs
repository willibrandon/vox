//! Theme color palettes for the Vox UI.
//!
//! Provides [`VoxTheme`] as a GPUI Global containing all semantically-named
//! HSLA colors. Supports dark and light themes via [`VoxTheme::from_mode`].
//! Components access colors via `cx.global::<VoxTheme>().colors`.

use gpui::{hsla, Global, Hsla};

use vox_core::config::ThemeMode;

/// Shared visual theme accessible via `cx.global::<VoxTheme>()`.
///
/// Contains all color values for the Vox UI. Set as a GPUI Global during
/// app initialization.
pub struct VoxTheme {
    /// Complete color palette with semantically-named colors.
    pub colors: ThemeColors,
}

impl Global for VoxTheme {}

impl VoxTheme {
    /// Select theme based on the user's theme mode setting.
    ///
    /// `System` defaults to dark because GPUI does not expose an OS
    /// dark-mode query. Callers should update the global when the
    /// user changes theme mode via `cx.set_global(VoxTheme::from_mode(&mode))`.
    pub fn from_mode(mode: &ThemeMode) -> Self {
        match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark | ThemeMode::System => Self::dark(),
        }
    }

    /// Create the dark theme with all color values defined.
    pub fn dark() -> Self {
        Self {
            colors: ThemeColors {
                overlay_bg: hsla(0.0, 0.0, 0.1, 0.92),
                surface: hsla(0.0, 0.0, 0.12, 1.0),
                elevated_surface: hsla(0.0, 0.0, 0.16, 1.0),
                panel_bg: hsla(0.0, 0.0, 0.14, 1.0),

                text: hsla(0.0, 0.0, 0.93, 1.0),
                text_muted: hsla(0.0, 0.0, 0.55, 1.0),
                text_accent: hsla(0.58, 0.8, 0.65, 1.0),

                border: hsla(0.0, 0.0, 0.2, 1.0),
                border_variant: hsla(0.0, 0.0, 0.25, 1.0),

                accent: hsla(0.58, 0.8, 0.65, 1.0),
                accent_hover: hsla(0.58, 0.85, 0.7, 1.0),

                status_idle: hsla(0.0, 0.0, 0.55, 1.0),
                status_listening: hsla(0.35, 0.9, 0.55, 1.0),
                status_processing: hsla(0.58, 0.8, 0.65, 1.0),
                status_success: hsla(0.35, 0.9, 0.55, 1.0),
                status_error: hsla(0.0, 0.85, 0.6, 1.0),
                status_downloading: hsla(0.12, 0.9, 0.6, 1.0),
                status_loading: hsla(0.55, 0.7, 0.7, 1.0),
                status_injection_failed: hsla(0.15, 0.9, 0.6, 1.0),

                waveform_active: hsla(0.35, 0.9, 0.55, 1.0),
                waveform_inactive: hsla(0.0, 0.0, 0.3, 1.0),

                button_primary_bg: hsla(0.58, 0.8, 0.55, 1.0),
                button_primary_text: hsla(0.0, 0.0, 1.0, 1.0),
                button_secondary_bg: hsla(0.0, 0.0, 0.2, 1.0),
                button_secondary_text: hsla(0.0, 0.0, 0.8, 1.0),

                input_bg: hsla(0.0, 0.0, 0.08, 1.0),
                input_border: hsla(0.0, 0.0, 0.25, 1.0),
                input_focus_border: hsla(0.58, 0.8, 0.65, 1.0),

                log_error: hsla(0.0, 0.85, 0.6, 1.0),
                log_warn: hsla(0.1, 0.9, 0.6, 1.0),
                log_info: hsla(0.0, 0.0, 0.93, 1.0),
                log_debug: hsla(0.0, 0.0, 0.55, 1.0),
                log_trace: hsla(0.0, 0.0, 0.35, 1.0),
                scrollbar_thumb: hsla(0.0, 0.0, 0.45, 1.0),
                scrollbar_track: hsla(0.0, 0.0, 0.16, 0.5),
            },
        }
    }

    /// Create the light theme with all color values defined.
    pub fn light() -> Self {
        Self {
            colors: ThemeColors {
                overlay_bg: hsla(0.0, 0.0, 0.97, 0.92),
                surface: hsla(0.0, 0.0, 0.96, 1.0),
                elevated_surface: hsla(0.0, 0.0, 1.0, 1.0),
                panel_bg: hsla(0.0, 0.0, 0.98, 1.0),

                text: hsla(0.0, 0.0, 0.1, 1.0),
                text_muted: hsla(0.0, 0.0, 0.45, 1.0),
                text_accent: hsla(0.58, 0.8, 0.4, 1.0),

                border: hsla(0.0, 0.0, 0.82, 1.0),
                border_variant: hsla(0.0, 0.0, 0.88, 1.0),

                accent: hsla(0.58, 0.8, 0.45, 1.0),
                accent_hover: hsla(0.58, 0.85, 0.4, 1.0),

                status_idle: hsla(0.0, 0.0, 0.55, 1.0),
                status_listening: hsla(0.35, 0.8, 0.4, 1.0),
                status_processing: hsla(0.58, 0.7, 0.45, 1.0),
                status_success: hsla(0.35, 0.8, 0.4, 1.0),
                status_error: hsla(0.0, 0.8, 0.45, 1.0),
                status_downloading: hsla(0.12, 0.85, 0.45, 1.0),
                status_loading: hsla(0.55, 0.6, 0.5, 1.0),
                status_injection_failed: hsla(0.15, 0.85, 0.45, 1.0),

                waveform_active: hsla(0.35, 0.8, 0.4, 1.0),
                waveform_inactive: hsla(0.0, 0.0, 0.8, 1.0),

                button_primary_bg: hsla(0.58, 0.8, 0.45, 1.0),
                button_primary_text: hsla(0.0, 0.0, 1.0, 1.0),
                button_secondary_bg: hsla(0.0, 0.0, 0.9, 1.0),
                button_secondary_text: hsla(0.0, 0.0, 0.2, 1.0),

                input_bg: hsla(0.0, 0.0, 1.0, 1.0),
                input_border: hsla(0.0, 0.0, 0.78, 1.0),
                input_focus_border: hsla(0.58, 0.8, 0.45, 1.0),

                log_error: hsla(0.0, 0.8, 0.45, 1.0),
                log_warn: hsla(0.1, 0.85, 0.45, 1.0),
                log_info: hsla(0.0, 0.0, 0.1, 1.0),
                log_debug: hsla(0.0, 0.0, 0.55, 1.0),
                log_trace: hsla(0.0, 0.0, 0.7, 1.0),
                scrollbar_thumb: hsla(0.0, 0.0, 0.6, 1.0),
                scrollbar_track: hsla(0.0, 0.0, 0.88, 0.5),
            },
        }
    }
}

/// Complete color palette with semantically-named colors.
///
/// All colors use GPUI's `Hsla` type. Organized by category:
/// backgrounds (4), text (3), borders (2), accent (2), status (6),
/// waveform (2), buttons (4), inputs (3).
#[derive(Clone)]
pub struct ThemeColors {
    /// Semi-transparent overlay background.
    pub overlay_bg: Hsla,
    /// Standard surface background.
    pub surface: Hsla,
    /// Elevated surface background.
    pub elevated_surface: Hsla,
    /// Panel background.
    pub panel_bg: Hsla,

    /// Primary text color.
    pub text: Hsla,
    /// Secondary/muted text color.
    pub text_muted: Hsla,
    /// Accent-colored text.
    pub text_accent: Hsla,

    /// Standard border color.
    pub border: Hsla,
    /// Subtle border variant.
    pub border_variant: Hsla,

    /// Primary accent color.
    pub accent: Hsla,
    /// Accent hover state.
    pub accent_hover: Hsla,

    /// Gray — idle state indicator.
    pub status_idle: Hsla,
    /// Green — listening state indicator.
    pub status_listening: Hsla,
    /// Blue — processing state indicator.
    pub status_processing: Hsla,
    /// Green — success state indicator.
    pub status_success: Hsla,
    /// Red — error state indicator.
    pub status_error: Hsla,
    /// Orange — downloading state indicator.
    pub status_downloading: Hsla,
    /// Blue (lighter) — loading state indicator.
    pub status_loading: Hsla,
    /// Amber/yellow — injection failed state indicator.
    pub status_injection_failed: Hsla,

    /// Active waveform bar color.
    pub waveform_active: Hsla,
    /// Inactive waveform bar color.
    pub waveform_inactive: Hsla,

    /// Primary button background.
    pub button_primary_bg: Hsla,
    /// Primary button text color.
    pub button_primary_text: Hsla,
    /// Secondary button background.
    pub button_secondary_bg: Hsla,
    /// Secondary button text color.
    pub button_secondary_text: Hsla,

    /// Input field background.
    pub input_bg: Hsla,
    /// Input field border color.
    pub input_border: Hsla,
    /// Input field focus border color.
    pub input_focus_border: Hsla,

    /// Log panel error-level text color (red).
    pub log_error: Hsla,
    /// Log panel warn-level text color (amber).
    pub log_warn: Hsla,
    /// Log panel info-level text color (white/text).
    pub log_info: Hsla,
    /// Log panel debug-level text color (gray/muted).
    pub log_debug: Hsla,
    /// Log panel trace-level text color (dim gray).
    pub log_trace: Hsla,
    /// Scrollbar thumb color.
    pub scrollbar_thumb: Hsla,
    /// Scrollbar track background color.
    pub scrollbar_track: Hsla,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_all_colors_valid(label: &str, theme: &VoxTheme) {
        let c = &theme.colors;
        let all_colors = [
            c.overlay_bg, c.surface, c.elevated_surface, c.panel_bg,
            c.text, c.text_muted, c.text_accent,
            c.border, c.border_variant,
            c.accent, c.accent_hover,
            c.status_idle, c.status_listening, c.status_processing,
            c.status_success, c.status_error, c.status_downloading,
            c.status_loading, c.status_injection_failed,
            c.waveform_active, c.waveform_inactive,
            c.button_primary_bg, c.button_primary_text,
            c.button_secondary_bg, c.button_secondary_text,
            c.input_bg, c.input_border, c.input_focus_border,
            c.log_error, c.log_warn, c.log_info, c.log_debug, c.log_trace,
            c.scrollbar_thumb, c.scrollbar_track,
        ];

        for (i, color) in all_colors.iter().enumerate() {
            assert!(
                (0.0..=1.0).contains(&color.h),
                "{label} color {i}: hue {:.2} out of 0.0..=1.0", color.h
            );
            assert!(
                (0.0..=1.0).contains(&color.s),
                "{label} color {i}: saturation {:.2} out of 0.0..=1.0", color.s
            );
            assert!(
                (0.0..=1.0).contains(&color.l),
                "{label} color {i}: lightness {:.2} out of 0.0..=1.0", color.l
            );
            assert!(
                (0.0..=1.0).contains(&color.a),
                "{label} color {i}: alpha {:.2} out of 0.0..=1.0", color.a
            );
        }
    }

    #[test]
    fn test_theme_colors_valid() {
        assert_all_colors_valid("dark", &VoxTheme::dark());
        assert_all_colors_valid("light", &VoxTheme::light());
    }

    #[test]
    fn test_overlay_bg_transparent() {
        let dark = VoxTheme::dark();
        assert!(dark.colors.overlay_bg.a < 1.0, "dark overlay_bg should be semi-transparent");
        let light = VoxTheme::light();
        assert!(light.colors.overlay_bg.a < 1.0, "light overlay_bg should be semi-transparent");
    }

    #[test]
    fn test_from_mode() {
        use vox_core::config::ThemeMode;

        let dark = VoxTheme::from_mode(&ThemeMode::Dark);
        let light = VoxTheme::from_mode(&ThemeMode::Light);
        let system = VoxTheme::from_mode(&ThemeMode::System);

        // Light theme has bright surfaces, dark theme has dark surfaces
        assert!(light.colors.surface.l > 0.9, "light surface should be bright");
        assert!(dark.colors.surface.l < 0.2, "dark surface should be dark");
        // System defaults to dark
        assert_eq!(system.colors.surface.l, dark.colors.surface.l);
    }
}
