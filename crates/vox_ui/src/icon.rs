//! Icon rendering utilities using Unicode text glyphs.
//!
//! Provides [`Icon`] enum with variants for common UI actions and
//! [`IconElement`] as a `RenderOnce` wrapper for embedding icons as children.

use gpui::{div, prelude::*, px, App, Hsla, IntoElement, SharedString, Window};

/// Available icon variants for UI actions.
///
/// Rendered as Unicode text glyphs with consistent sizing.
#[derive(Clone, Copy, PartialEq)]
pub enum Icon {
    /// Copy to clipboard.
    Copy,
    /// Delete / remove.
    Delete,
    /// Edit / modify.
    Edit,
    /// Settings gear.
    Settings,
    /// File folder.
    Folder,
    /// Download arrow.
    Download,
    /// Retry / refresh.
    Retry,
    /// Success checkmark.
    Check,
    /// Error indicator.
    Error,
    /// Warning indicator.
    Warning,
    /// Dropdown chevron.
    ChevronDown,
    /// Search magnifier.
    Search,
    /// Clear / close X.
    Clear,
    /// Play button.
    Play,
    /// Pause button.
    Pause,
}

impl Icon {
    /// Unicode glyph representing this icon.
    pub fn glyph(self) -> &'static str {
        match self {
            Icon::Copy => "\u{2398}",      // ⎘
            Icon::Delete => "\u{2715}",     // ✕
            Icon::Edit => "\u{270E}",       // ✎
            Icon::Settings => "\u{2699}",   // ⚙
            Icon::Folder => "\u{1F4C1}",    // 📁
            Icon::Download => "\u{2913}",   // ⤓
            Icon::Retry => "\u{21BB}",      // ↻
            Icon::Check => "\u{2713}",      // ✓
            Icon::Error => "\u{2718}",      // ✘
            Icon::Warning => "\u{26A0}",    // ⚠
            Icon::ChevronDown => "\u{25BE}", // ▾
            Icon::Search => "\u{1F50D}",    // 🔍
            Icon::Clear => "\u{2715}",      // ✕
            Icon::Play => "\u{25B6}",       // ▶
            Icon::Pause => "\u{23F8}",      // ⏸
        }
    }
}

/// Render wrapper for embedding an [`Icon`] as a child element.
#[derive(IntoElement)]
pub struct IconElement {
    icon: Icon,
    color: Hsla,
}

impl IconElement {
    /// Create an icon element with a specific color.
    pub fn new(icon: Icon, color: Hsla) -> Self {
        Self { icon, color }
    }
}

impl RenderOnce for IconElement {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        div()
            .text_color(self.color)
            .text_size(px(14.0))
            .child(SharedString::from(self.icon.glyph()))
    }
}
