//! Layout constants for consistent spacing, border radius, and component sizing.
//!
//! All values use GPUI's [`Pixels`](gpui::Pixels) type via the [`px`](gpui::px)
//! function. Import sub-modules to use: `use vox_ui::layout::{spacing, radius, size};`

/// Spacing scale for consistent padding and margins across all UI components.
pub mod spacing {
    use gpui::{px, Pixels};

    /// Extra small spacing (4px).
    pub const XS: Pixels = px(4.0);
    /// Small spacing (8px).
    pub const SM: Pixels = px(8.0);
    /// Medium spacing (12px).
    pub const MD: Pixels = px(12.0);
    /// Large spacing (16px).
    pub const LG: Pixels = px(16.0);
    /// Extra large spacing (24px).
    pub const XL: Pixels = px(24.0);
}

/// Border radius scale for consistent rounded corners.
pub mod radius {
    use gpui::{px, Pixels};

    /// Small border radius (4px).
    pub const SM: Pixels = px(4.0);
    /// Medium border radius (8px).
    pub const MD: Pixels = px(8.0);
    /// Large border radius (12px).
    pub const LG: Pixels = px(12.0);
    /// Pill/fully rounded border radius (999px).
    pub const PILL: Pixels = px(999.0);
}

/// Standard component dimensions.
pub mod size {
    use gpui::{px, Pixels};

    /// Overlay HUD window width (360px).
    pub const OVERLAY_WIDTH: Pixels = px(360.0);
    /// Overlay HUD window height (80px).
    pub const OVERLAY_HEIGHT: Pixels = px(80.0);
    /// Settings panel width (800px).
    pub const SETTINGS_WIDTH: Pixels = px(800.0);
    /// Settings panel height (600px).
    pub const SETTINGS_HEIGHT: Pixels = px(600.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spacing_scale() {
        assert!(spacing::XS < spacing::SM);
        assert!(spacing::SM < spacing::MD);
        assert!(spacing::MD < spacing::LG);
        assert!(spacing::LG < spacing::XL);
    }

    #[test]
    fn test_radius_scale() {
        assert!(radius::SM < radius::MD);
        assert!(radius::MD < radius::LG);
        assert!(radius::LG < radius::PILL);
    }

    #[test]
    fn test_overlay_dimensions() {
        use gpui::px;
        assert_eq!(size::OVERLAY_WIDTH, px(360.0));
        assert_eq!(size::OVERLAY_HEIGHT, px(80.0));
        assert_eq!(size::SETTINGS_WIDTH, px(800.0));
        assert_eq!(size::SETTINGS_HEIGHT, px(600.0));
    }
}
