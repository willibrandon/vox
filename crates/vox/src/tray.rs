//! System tray icon management with dynamic state-reactive updates.
//!
//! Provides types and helpers for mapping [`AppReadiness`] and [`PipelineState`]
//! to tray icon visuals (color + tooltip). Five pre-decoded icon variants
//! (idle/gray, listening/green, processing/blue, downloading/orange, error/red)
//! are embedded as PNGs and decoded once at startup.

use tray_icon::menu::{Menu, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::Icon;

use vox_core::pipeline::PipelineState;
use vox_core::state::AppReadiness;

/// Visual state of the system tray icon, derived from the combination of
/// [`AppReadiness`] and [`PipelineState`].
#[derive(Clone, Debug)]
pub enum TrayIconState {
    /// Gray icon. Pipeline ready, no active recording.
    Idle,
    /// Green icon. Microphone active, VAD processing audio.
    Listening,
    /// Blue icon. ASR/LLM inference in progress.
    Processing,
    /// Orange icon. Models downloading or loading onto GPU.
    Downloading {
        /// Human-readable detail for the tooltip (e.g., progress percentage).
        tooltip_detail: String,
    },
    /// Red icon. An error occurred.
    Error {
        /// Human-readable error description for the tooltip.
        message: String,
    },
}

/// Command sent to the tray polling task to update the icon and tooltip.
pub enum TrayUpdate {
    /// Update icon, tooltip, and menu text to reflect the new state.
    ///
    /// `is_recording` indicates whether a recording session is currently
    /// active. This is separate from `TrayIconState` because the session
    /// can be active during Processing/Injecting states (not just Listening).
    /// The tray polling loop uses this to set the Toggle Recording menu
    /// label correctly.
    SetState {
        /// Visual state for icon and tooltip.
        state: TrayIconState,
        /// Whether a recording session is active (for menu label).
        is_recording: bool,
    },
}

/// Pre-decoded tray icon variants, ready for `tray_icon.set_icon()`.
///
/// Decoded once at startup from embedded PNG bytes. Avoids repeated
/// PNG decompression on every state transition.
pub struct TrayIcons {
    /// Gray microphone — pipeline idle.
    pub idle: Icon,
    /// Green microphone — recording/listening.
    pub listening: Icon,
    /// Blue microphone — processing (ASR/LLM).
    pub processing: Icon,
    /// Orange microphone — downloading or loading models.
    pub downloading: Icon,
    /// Red microphone — error state.
    pub error: Icon,
}

/// Menu item IDs used to match incoming [`tray_icon::menu::MenuEvent`]s
/// to application actions.
pub struct TrayMenuIds {
    /// "Toggle Recording" / "Stop Recording" menu item.
    pub toggle_recording: MenuId,
    /// "Settings" menu item.
    pub settings: MenuId,
    /// "Show/Hide Overlay" menu item.
    pub toggle_overlay: MenuId,
    /// "About Vox" menu item.
    pub about: MenuId,
    /// "Quit Vox" menu item.
    pub quit: MenuId,
}

/// Reference to the Toggle Recording menu item for dynamic text updates.
///
/// Stored alongside `TrayMenuIds` so the polling loop can call
/// `set_text()` when recording state changes.
pub struct TrayMenuItems {
    /// The Toggle Recording menu item (text changes between
    /// "Start Recording" and "Stop Recording").
    pub toggle_recording: MenuItem,
}

/// Decode all five tray icon variants from embedded PNG bytes.
///
/// Called once at startup. Each PNG is a 32×32 RGBA image of a
/// microphone in a different color.
pub fn decode_all_tray_icons() -> TrayIcons {
    TrayIcons {
        idle: decode_png_icon(include_bytes!("../../../assets/icons/tray-idle.png")),
        listening: decode_png_icon(include_bytes!("../../../assets/icons/tray-listening.png")),
        processing: decode_png_icon(include_bytes!("../../../assets/icons/tray-processing.png")),
        downloading: decode_png_icon(include_bytes!("../../../assets/icons/tray-downloading.png")),
        error: decode_png_icon(include_bytes!("../../../assets/icons/tray-error.png")),
    }
}

/// Derive the [`TrayIconState`] from the current application readiness
/// and pipeline state.
///
/// `AppReadiness` takes priority: if models are downloading, loading,
/// or in error, the tray shows that regardless of pipeline state.
/// `PipelineState` is only consulted when the app is `Ready`.
pub fn derive_tray_state(
    readiness: &AppReadiness,
    pipeline_state: &PipelineState,
) -> TrayIconState {
    match readiness {
        AppReadiness::Downloading { .. } => TrayIconState::Downloading {
            tooltip_detail: "Downloading models...".into(),
        },
        AppReadiness::Loading { stage } => TrayIconState::Downloading {
            tooltip_detail: stage.clone(),
        },
        AppReadiness::Error { message } => TrayIconState::Error {
            message: message.clone(),
        },
        AppReadiness::Ready => match pipeline_state {
            PipelineState::Idle => TrayIconState::Idle,
            PipelineState::Listening => TrayIconState::Listening,
            PipelineState::Processing { .. } | PipelineState::Injecting { .. } => {
                TrayIconState::Processing
            }
            PipelineState::Error { message } => TrayIconState::Error {
                message: message.clone(),
            },
            PipelineState::InjectionFailed { error, .. } => TrayIconState::Error {
                message: error.clone(),
            },
        },
    }
}

/// Create the expanded 6-item tray context menu.
///
/// Returns the [`Menu`], a [`TrayMenuIds`] struct for event matching,
/// and a [`TrayMenuItems`] struct for dynamic text updates.
pub fn create_tray_menu() -> (Menu, TrayMenuIds, TrayMenuItems) {
    let menu = Menu::new();

    let toggle_item = MenuItem::new("Start Recording", true, None);
    let settings_item = MenuItem::new("Settings", true, None);
    let overlay_item = MenuItem::new("Show Overlay", true, None);
    let about_item = MenuItem::new("About Vox", true, None);
    let quit_item = MenuItem::new("Quit Vox", true, None);

    let ids = TrayMenuIds {
        toggle_recording: toggle_item.id().clone(),
        settings: settings_item.id().clone(),
        toggle_overlay: overlay_item.id().clone(),
        about: about_item.id().clone(),
        quit: quit_item.id().clone(),
    };

    let items = TrayMenuItems {
        toggle_recording: toggle_item.clone(),
    };

    // Append in display order with separator before About/Quit
    if let Err(err) = menu.append(&toggle_item) {
        tracing::warn!(?err, "failed to append Toggle Recording");
    }
    if let Err(err) = menu.append(&settings_item) {
        tracing::warn!(?err, "failed to append Settings");
    }
    if let Err(err) = menu.append(&overlay_item) {
        tracing::warn!(?err, "failed to append Show Overlay");
    }
    if let Err(err) = menu.append(&PredefinedMenuItem::separator()) {
        tracing::warn!(?err, "failed to append separator");
    }
    if let Err(err) = menu.append(&about_item) {
        tracing::warn!(?err, "failed to append About Vox");
    }
    if let Err(err) = menu.append(&quit_item) {
        tracing::warn!(?err, "failed to append Quit Vox");
    }

    (menu, ids, items)
}

/// Get the tooltip string for a [`TrayIconState`].
pub fn tooltip_for_state(state: &TrayIconState) -> String {
    match state {
        TrayIconState::Idle => "Vox \u{2014} Idle".into(),
        TrayIconState::Listening => "Vox \u{2014} Listening...".into(),
        TrayIconState::Processing => "Vox \u{2014} Processing...".into(),
        TrayIconState::Downloading { tooltip_detail } => {
            format!("Vox \u{2014} {tooltip_detail}")
        }
        TrayIconState::Error { message } => {
            format!("Vox \u{2014} Error: {message}")
        }
    }
}

/// Select the matching pre-decoded [`Icon`] for a [`TrayIconState`].
pub fn icon_for_state<'a>(state: &TrayIconState, icons: &'a TrayIcons) -> &'a Icon {
    match state {
        TrayIconState::Idle => &icons.idle,
        TrayIconState::Listening => &icons.listening,
        TrayIconState::Processing => &icons.processing,
        TrayIconState::Downloading { .. } => &icons.downloading,
        TrayIconState::Error { .. } => &icons.error,
    }
}

/// Decode an embedded PNG file into a tray [`Icon`].
///
/// Uses the `png` crate to inflate the RGBA pixel data that
/// [`Icon::from_rgba`] requires.
fn decode_png_icon(png_bytes: &[u8]) -> Icon {
    let decoder = png::Decoder::new(png_bytes);
    let mut reader = decoder.read_info().expect("embedded PNG has valid header");
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("embedded PNG has valid frame");
    Icon::from_rgba(buf[..info.buffer_size()].to_vec(), info.width, info.height)
        .expect("embedded PNG dimensions match pixel data")
}
