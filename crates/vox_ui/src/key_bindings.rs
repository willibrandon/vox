//! Application actions and keyboard shortcut registration.
//!
//! Defines all user-dispatchable actions via GPUI's [`actions!`](gpui::actions)
//! macro and provides [`register_actions`] and [`register_key_bindings`] for
//! wiring them into the application context during startup.

use gpui::{actions, App, KeyBinding};

use crate::overlay_hud::OverlayWindowHandle;
use crate::workspace::open_settings_window;

actions!(
    vox,
    [
        ToggleRecording,
        StopRecording,
        ToggleOverlay,
        OpenSettings,
        Quit,
        CopyLastTranscript,
        ClearHistory,
        CopyInjectedText,
        RetryDownload,
        OpenModelFolder,
        DismissOverlay,
        CancelInjectionRetry,
    ]
);

/// Register action handlers on the GPUI App context.
///
/// Must be called once during app initialization. Handlers dispatch to
/// VoxState methods for state transitions. Non-implemented handlers log
/// the dispatch for development visibility.
pub fn register_actions(cx: &mut App) {
    // ToggleRecording and StopRecording handlers are registered in main.rs
    // where they have access to the full pipeline lifecycle (AudioCapture,
    // Pipeline orchestrator, tokio runtime). Registering no-op handlers here
    // ensures the actions are known to GPUI's action system.
    cx.on_action(|_: &ToggleRecording, _cx| {});
    cx.on_action(|_: &StopRecording, _cx| {});
    cx.on_action(|_: &ToggleOverlay, cx| {
        let handle = cx
            .try_global::<OverlayWindowHandle>()
            .and_then(|h| h.0);
        if let Some(handle) = handle {
            let _ = handle.update(cx, |hud, window, cx| {
                hud.toggle_visibility(window, cx);
            });
        }
    });
    cx.on_action(|_: &OpenSettings, cx| {
        open_settings_window(cx);
    });
    cx.on_action(|_: &Quit, cx| {
        cx.quit();
    });
    cx.on_action(|_: &CopyLastTranscript, _cx| {
        tracing::info!("CopyLastTranscript dispatched");
    });
    cx.on_action(|_: &ClearHistory, _cx| {
        tracing::info!("ClearHistory dispatched");
    });
    cx.on_action(|_: &CopyInjectedText, _cx| {
        tracing::info!("CopyInjectedText dispatched");
    });
    cx.on_action(|_: &RetryDownload, _cx| {
        tracing::info!("RetryDownload dispatched");
    });
    cx.on_action(|_: &OpenModelFolder, _cx| {
        tracing::info!("OpenModelFolder dispatched");
    });
    cx.on_action(|_: &DismissOverlay, _cx| {
        tracing::info!("DismissOverlay dispatched");
    });
    cx.on_action(|_: &CancelInjectionRetry, _cx| {});
}

/// Register keyboard shortcuts mapped to actions.
///
/// Platform-conditional: uses Cmd on macOS, Ctrl on Windows/Linux.
/// Must be called once during app initialization.
pub fn register_key_bindings(cx: &mut App) {
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-shift-v", ToggleOverlay, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-v", ToggleOverlay, None),

        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-,", OpenSettings, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-,", OpenSettings, None),

        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-q", Quit, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-q", Quit, None),
    ]);
}
