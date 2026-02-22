//! Application actions and keyboard shortcut registration.
//!
//! Defines all user-dispatchable actions via GPUI's [`actions!`](gpui::actions)
//! macro and provides [`register_actions`] and [`register_key_bindings`] for
//! wiring them into the application context during startup.

use gpui::{actions, App, KeyBinding};

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
    ]
);

/// Register action handlers on the GPUI App context.
///
/// Must be called once during app initialization. Handlers dispatch to
/// VoxState methods for state transitions. Non-implemented handlers log
/// the dispatch for development visibility.
pub fn register_actions(cx: &mut App) {
    cx.on_action(|_: &ToggleRecording, _cx| {
        tracing::info!("ToggleRecording dispatched");
    });
    cx.on_action(|_: &StopRecording, _cx| {
        tracing::info!("StopRecording dispatched");
    });
    cx.on_action(|_: &ToggleOverlay, _cx| {
        tracing::info!("ToggleOverlay dispatched");
    });
    cx.on_action(|_: &OpenSettings, _cx| {
        tracing::info!("OpenSettings dispatched");
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
