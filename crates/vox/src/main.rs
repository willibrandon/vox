#![windows_subsystem = "windows"]

//! Vox application entry point.
//!
//! Creates the GPUI [`Application`], initializes structured logging, sets up
//! global state ([`VoxState`] and [`VoxTheme`]), registers actions and
//! keybindings, opens the overlay HUD window, configures the system tray
//! and global hotkey, and kicks off background pipeline initialization.
//! When models are loaded and Ready, the ToggleRecording action creates
//! an AudioCapture → VAD → ASR → LLM → TextInjection pipeline that
//! processes speech and injects polished text into the focused application.

mod tray;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::{App, AppContext as _, Application, AsyncApp};
use parking_lot::Mutex;
use tray_icon::menu::MenuEvent;
use tray_icon::TrayIconBuilder;

use vox_core::asr::AsrEngine;
use vox_core::audio::{AudioCapture, AudioConfig};
use vox_core::llm::PostProcessor;
use vox_core::logging::init_logging;
use vox_core::models::{self, check_missing_models, DownloadProgress, ModelDownloader};
use vox_core::hotkey_interpreter::{HotkeyAction, HotkeyInterpreter};
use vox_core::pipeline::{Pipeline, PipelineCommand, PipelineState};
use vox_core::state::{ensure_data_dirs, AppReadiness, VoxState};
use vox_core::vad::VadConfig;
use vox_ui::key_bindings::{
    register_actions, register_key_bindings, CancelInjectionRetry, StopRecording,
    ToggleOverlay, ToggleRecording,
};
use vox_ui::overlay_hud::{open_overlay_window, OverlayDisplayState};
use vox_ui::workspace::open_settings_window;
use vox_ui::theme::VoxTheme;

/// Holds the command channel sender for an active recording session.
///
/// When dropped, the channel closes, signaling the pipeline's run loop
/// to break and execute its shutdown sequence (stop VAD thread, drain
/// buffered segments, broadcast Idle).
struct ActiveRecording {
    // Held alive to keep the mpsc channel open. Dropping this sender closes
    // the channel, causing Pipeline::run()'s select! to break and shut down.
    #[allow(dead_code)]
    command_tx: tokio::sync::mpsc::Sender<PipelineCommand>,
}

/// Tracks whether a recording session is in progress.
///
/// Uses interior mutability (Mutex) because GPUI Globals are accessed
/// through shared references. The Mutex is uncontended in practice — all
/// access occurs on the single GPUI foreground thread.
///
/// The `generation` counter prevents stale state-forwarding tasks from
/// clearing a newer session's handle. Each `start_recording` increments
/// the counter; the task only clears `active` if its captured generation
/// still matches.
struct RecordingSession {
    active: Mutex<Option<ActiveRecording>>,
    generation: AtomicU64,
}

impl gpui::Global for RecordingSession {}

/// Wraps the tray update channel sender as a GPUI Global.
///
/// Set in [`setup_system_tray`]. The `observe_global` callback registered
/// in [`run_app`] reads this sender to push tray state updates whenever
/// [`OverlayDisplayState`] changes (from any source).
struct TraySender(std::sync::mpsc::Sender<tray::TrayUpdate>);

impl gpui::Global for TraySender {}

/// Holds subscriptions that must live for the app's entire lifetime.
///
/// GPUI cancels observations when their [`gpui::Subscription`] is dropped.
/// Storing them here keeps them alive as long as the app runs.
/// The field is never read — its only purpose is preventing the
/// subscriptions from being dropped.
#[allow(dead_code)] // Subscriptions kept alive by ownership, not read access
struct AppSubscriptions(Vec<gpui::Subscription>);

impl gpui::Global for AppSubscriptions {}


fn main() {
    let (_guard, log_receiver) = init_logging();

    // GUI subsystem processes have no console, so ctrlc's SetConsoleCtrlHandler
    // never fires. AttachConsole re-attaches to the parent terminal (if any)
    // when launched via `cargo run`. If that fails (double-click from Explorer,
    // or MSYS2/mintty without a real Windows console), AllocConsole creates a
    // hidden console so SetConsoleCtrlHandler has something to listen on.
    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
            fn AllocConsole() -> i32;
            fn GetConsoleWindow() -> isize;
        }
        #[link(name = "user32")]
        unsafe extern "system" {
            fn ShowWindow(hwnd: isize, cmd_show: i32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = 0xFFFFFFFF;
        const SW_HIDE: i32 = 0;

        unsafe {
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                // No attachable parent console. Create a hidden one so
                // SetConsoleCtrlHandler (used by ctrlc crate) can receive
                // signals. The window is hidden before it has a chance to paint.
                if AllocConsole() != 0 {
                    let hwnd = GetConsoleWindow();
                    if hwnd != 0 {
                        ShowWindow(hwnd, SW_HIDE);
                    }
                }
            }
        }
    }

    // Use _exit() to skip atexit handlers that trigger Metal residency set
    // assertions in llama.cpp's ggml-metal cleanup on macOS.
    ctrlc::set_handler(|| unsafe { libc::_exit(0) }).ok();

    Application::new().run(move |cx: &mut App| {
        if let Err(err) = run_app(cx, log_receiver) {
            tracing::error!(%err, "application startup failed");
            cx.quit();
        }
    });
}

fn run_app(cx: &mut App, log_receiver: vox_core::log_sink::LogReceiver) -> anyhow::Result<()> {
    let data_dir = ensure_data_dirs()?;
    let state = VoxState::new(&data_dir)?;
    let initial_readiness = state.readiness();
    let initial_pipeline = state.pipeline_state();
    cx.set_global(state);
    cx.set_global(VoxTheme::dark());

    // Create persistent LogStore entity that survives settings window close/reopen.
    // Must happen after VoxState is set as global but before any UI opens.
    let log_store = cx.new(|cx| vox_ui::log_panel::LogStore::new(cx, log_receiver));
    cx.set_global(vox_ui::log_panel::SharedLogStore(log_store));

    // Initialize the reactive bridge before opening the overlay window
    cx.set_global(OverlayDisplayState {
        readiness: initial_readiness,
        pipeline_state: initial_pipeline,
    });

    // Initialize recording session tracker (starts inactive)
    cx.set_global(RecordingSession {
        active: Mutex::new(None),
        generation: AtomicU64::new(0),
    });

    register_actions(cx);
    register_key_bindings(cx);
    register_pipeline_actions(cx);

    // Overlay HUD opens immediately — before models load, before GPU init
    open_overlay_window(cx)?;
    setup_system_tray(cx);
    setup_global_hotkey(cx);

    // Reactively sync tray icon/tooltip whenever OverlayDisplayState changes.
    // This catches ALL paths that call `cx.set_global(OverlayDisplayState { .. })`
    // — both from `update_overlay_state` in this file and from overlay_hud.rs
    // (fade timer, copy-to-clipboard). Without this, those overlay_hud paths
    // would leave the tray stale.
    let tray_sync = cx.observe_global::<OverlayDisplayState>(|cx| {
        if let Some(sender) = cx.try_global::<TraySender>() {
            let state = cx.global::<VoxState>();
            let readiness = state.readiness();
            let pipeline_state = state.pipeline_state();
            let tray_state = tray::derive_tray_state(&readiness, &pipeline_state);
            let is_recording = cx.global::<RecordingSession>().active.lock().is_some();
            let _ = sender.0.send(tray::TrayUpdate::SetState {
                state: tray_state,
                is_recording,
            });
        }
    });
    cx.set_global(AppSubscriptions(vec![tray_sync]));

    // Prompt for Accessibility permission on macOS if not already granted.
    // Shows a system dialog directing the user to System Settings; no-op if
    // already trusted or on non-macOS platforms.
    vox_core::injector::prompt_accessibility_if_needed();

    // macOS permission polling (US8): if Accessibility permission isn't granted
    // yet, poll every 2 seconds and auto-proceed when the user grants it.
    // Shows guidance in the overlay while waiting.
    spawn_accessibility_poller(cx);

    // Start system sleep/wake listener and recovery handler (US3).
    spawn_wake_handler(cx);

    // Pipeline initialization runs in background; UI is already visible
    cx.spawn(async move |mut cx| {
        initialize_pipeline(&mut cx).await;
    })
    .detach();

    cx.activate(true);
    Ok(())
}

/// Register ToggleRecording and StopRecording handlers with full pipeline wiring.
///
/// These handlers manage the AudioCapture → VAD → ASR → LLM → Injection
/// lifecycle. Called after `register_actions()` to override the no-op
/// handlers from vox_ui with real pipeline management.
fn register_pipeline_actions(cx: &mut App) {
    cx.on_action(|_: &ToggleRecording, cx| {
        let readiness = cx.global::<VoxState>().readiness();
        if !matches!(readiness, AppReadiness::Ready) {
            tracing::warn!("cannot toggle recording: app not ready");
            return;
        }

        let is_recording = cx.global::<RecordingSession>().active.lock().is_some();

        if is_recording {
            stop_recording(cx);
        } else if let Err(err) = start_recording(cx) {
            tracing::error!(%err, "failed to start recording");
            cx.global::<VoxState>().set_pipeline_state(PipelineState::Error {
                message: err.to_string(),
            });
            update_overlay_state(cx);
        }
    });

    cx.on_action(|_: &StopRecording, cx| {
        let is_recording = cx.global::<RecordingSession>().active.lock().is_some();
        if is_recording {
            stop_recording(cx);
        }
    });

    cx.on_action(|_: &CancelInjectionRetry, cx| {
        let guard = cx.global::<RecordingSession>().active.lock();
        if let Some(active) = guard.as_ref() {
            let _ = active
                .command_tx
                .try_send(PipelineCommand::CancelInjectionRetry);
        }
    });
}

/// Start a new recording session: AudioCapture → VAD → ASR → LLM → Injection.
///
/// Creates all pipeline components, starts audio capture, spawns the
/// orchestrator on the tokio runtime, and starts a GPUI foreground task
/// that forwards pipeline state broadcasts to the overlay HUD.
fn start_recording(cx: &mut App) -> anyhow::Result<()> {
    // Read all needed values from VoxState in one borrow scope
    let (device_name, vad_config, asr, llm, dictionary, transcript_writer, tokio_handle, debug_tap) = {
        let state = cx.global::<VoxState>();

        let settings = state.settings();
        let device_name = settings.input_device.clone();
        let vad_config = VadConfig {
            threshold: settings.vad_threshold,
            min_speech_ms: settings.min_speech_ms,
            min_silence_ms: settings.min_silence_ms,
            max_speech_ms: settings.max_segment_ms,
            bypass_vad: matches!(
                settings.activation_mode,
                vox_core::hotkey_interpreter::ActivationMode::HoldToTalk
            ),
            ..VadConfig::default()
        };
        drop(settings);

        let asr = state
            .clone_asr_engine()
            .ok_or_else(|| anyhow::anyhow!("ASR engine not loaded"))?;
        let llm = state
            .clone_llm_processor()
            .ok_or_else(|| anyhow::anyhow!("LLM processor not loaded"))?;
        let dictionary = state.dictionary().clone();
        let transcript_writer = state.transcript_writer();
        let tokio_handle = state.tokio_runtime().handle().clone();
        let debug_tap = std::sync::Arc::clone(state.debug_tap());

        (
            device_name,
            vad_config,
            asr,
            llm,
            dictionary,
            transcript_writer,
            tokio_handle,
            debug_tap,
        )
    };

    let vad_model_path = models::model_path(models::MODELS[0].filename)?;

    // Create and start audio capture on the GPUI foreground thread.
    // AudioCapture is NOT Send — it stays on this thread, kept alive by
    // the GPUI spawn task below.
    let audio_config = AudioConfig {
        device_name,
        ..AudioConfig::default()
    };
    let mut capture = AudioCapture::new(&audio_config)?;
    capture.start()?;
    let consumer = capture
        .take_consumer()
        .ok_or_else(|| anyhow::anyhow!("audio consumer already taken"))?;
    let native_rate = capture.native_sample_rate();
    let rms_atomic = capture.rms_atomic();

    tracing::info!(
        device = capture.device_name(),
        native_rate,
        "audio capture started"
    );

    // Create pipeline channels
    let (state_tx, _) = tokio::sync::broadcast::channel::<PipelineState>(64);
    let (command_tx, command_rx) = tokio::sync::mpsc::channel::<PipelineCommand>(16);

    // Wire debug tap's error notification channel so write failures
    // surface in the overlay via PipelineState::Error.
    debug_tap.set_state_tx(state_tx.clone());

    // Create pipeline with all components wired in
    let mut pipeline = Pipeline::new(
        asr,
        llm,
        dictionary,
        transcript_writer,
        state_tx,
        command_rx,
        vad_model_path,
        vad_config,
        debug_tap,
    );

    // Start pipeline: spawns VAD thread, broadcasts Listening
    pipeline.start(consumer, native_rate)?;
    let mut state_rx = pipeline.subscribe();

    tracing::info!("pipeline started, spawning orchestrator on tokio");

    // Spawn pipeline.run() on tokio (async loop with spawn_blocking for GPU work)
    tokio_handle.spawn(async move {
        if let Err(err) = pipeline.run().await {
            tracing::error!(%err, "pipeline orchestrator exited with error");
        }
        tracing::info!("pipeline orchestrator shut down");
    });

    // Store command_tx for stop signaling; increment generation so stale
    // forwarding tasks from a previous session won't clear this session's handle
    let generation = cx.global::<RecordingSession>().generation.fetch_add(1, Ordering::Relaxed) + 1;
    *cx.global::<RecordingSession>().active.lock() = Some(ActiveRecording { command_tx });

    // Update UI state to Listening
    cx.global::<VoxState>().set_pipeline_state(PipelineState::Listening);
    update_overlay_state(cx);

    // Spawn GPUI foreground task that:
    // 1. Holds AudioCapture alive (dropping it stops the cpal stream)
    // 2. Forwards real-time RMS amplitude to VoxState for waveform display
    // 3. Polls state broadcasts and forwards them to the overlay
    // 4. Cleans up RecordingSession when the pipeline finishes (generation-gated)
    let executor = cx.background_executor().clone();
    cx.spawn(async move |cx| {
        let _capture = capture;

        // Helper: returns true if this session is still the current one.
        // Must be called inside cx.update closures where &App is available.
        let is_current_gen = |cx: &App| -> bool {
            cx.global::<RecordingSession>()
                .generation
                .load(Ordering::Relaxed)
                == generation
        };

        loop {
            executor.timer(Duration::from_millis(16)).await;

            // Forward real-time RMS from audio callback to VoxState.
            // Generation check and write happen atomically in the same
            // cx.update closure — no window for a stale task to overwrite
            // the current session's state.
            let rms = f32::from_bits(rms_atomic.load(Ordering::Relaxed));
            let is_current = cx.update(|cx| {
                if !is_current_gen(cx) {
                    return false;
                }
                cx.global::<VoxState>().set_latest_rms(rms);
                true
            });
            if !is_current {
                return;
            }

            loop {
                match state_rx.try_recv() {
                    Ok(pipeline_state) => {
                        let is_idle = matches!(pipeline_state, PipelineState::Idle);
                        cx.update(|cx| {
                            if is_current_gen(cx) {
                                cx.global::<VoxState>()
                                    .set_pipeline_state(pipeline_state);
                                update_overlay_state(cx);
                            }
                        });
                        if is_idle {
                            cx.update(|cx| {
                                if is_current_gen(cx) {
                                    cx.global::<RecordingSession>()
                                        .active
                                        .lock()
                                        .take();
                                }
                            });
                            return;
                        }
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                        cx.update(|cx| {
                            if is_current_gen(cx) {
                                cx.global::<VoxState>()
                                    .set_pipeline_state(PipelineState::Idle);
                                cx.global::<RecordingSession>()
                                    .active
                                    .lock()
                                    .take();
                                update_overlay_state(cx);
                            }
                        });
                        return;
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                        tracing::debug!(skipped, "state broadcast receiver lagged");
                        continue;
                    }
                }
            }
        }
    })
    .detach();

    Ok(())
}

/// Stop the current recording session.
///
/// Drops the command channel sender, closing the channel. The pipeline's
/// `run()` loop sees the closed channel, breaks, and executes its shutdown
/// sequence: sets stop flag → drains buffered segments → joins VAD thread →
/// broadcasts `PipelineState::Idle`. The GPUI state-forwarding task picks
/// up the Idle broadcast and cleans up the session.
fn stop_recording(cx: &mut App) {
    let active = cx.global::<RecordingSession>().active.lock().take();
    if active.is_some() {
        tracing::info!("recording stop requested — pipeline shutting down");
    }
}

/// Updates the OverlayDisplayState bridge to match VoxState.
///
/// Must be called after every `VoxState::set_readiness()` or
/// `VoxState::set_pipeline_state()` to trigger overlay re-rendering.
/// Tray synchronization is handled reactively by the `observe_global`
/// callback registered in [`run_app`], which fires on every
/// `set_global(OverlayDisplayState { .. })` — including calls from
/// `overlay_hud.rs` that bypass this function.
fn update_overlay_state(cx: &mut App) {
    let state = cx.global::<VoxState>();
    let readiness = state.readiness();
    let pipeline_state = state.pipeline_state();

    cx.set_global(OverlayDisplayState {
        readiness: readiness.clone(),
        pipeline_state: pipeline_state.clone(),
    });
}

/// Set up the global hotkey with mode-aware press/release handling and
/// runtime re-registration support.
///
/// Instantiates a [`HotkeyInterpreter`] that maps press/release events to
/// recording actions based on the current [`ActivationMode`]. The interpreter
/// reads the latest mode from [`VoxState`] on every event, so settings changes
/// take effect immediately without re-registration.
///
/// Creates a rebind channel (`std::sync::mpsc`) — the [`HotkeyRebindSender`]
/// global lets the settings panel send new hotkey strings for re-registration
/// at runtime. The polling loop unregisters the old hotkey, registers the new
/// one, and updates the event-matching ID.
///
/// Before consulting the interpreter, checks [`AppReadiness`] — if the app is
/// not ready (downloading, loading, error), the hotkey refreshes the overlay
/// to acknowledge the keypress without starting recording (FR-006).
fn setup_global_hotkey(cx: &mut App) {
    let manager = match GlobalHotKeyManager::new() {
        Ok(m) => m,
        Err(err) => {
            tracing::warn!(?err, "failed to create global hotkey manager");
            return;
        }
    };

    let hotkey_str = cx.global::<VoxState>().settings().activation_hotkey.clone();
    let hotkey = match hotkey_str.parse::<HotKey>() {
        Ok(hk) => hk,
        Err(err) => {
            tracing::error!(?err, hotkey = %hotkey_str, "failed to parse hotkey string");
            return;
        }
    };

    let hotkey_registered = match manager.register(hotkey) {
        Ok(()) => {
            tracing::info!(hotkey = %hotkey_str, "global hotkey registered");
            true
        }
        Err(err) => {
            tracing::warn!(?err, hotkey = %hotkey_str, "failed to register global hotkey — will retry in event loop");
            // macOS Input Monitoring permission may not be granted yet.
            if cfg!(target_os = "macos") {
                cx.global::<VoxState>().set_pipeline_state(PipelineState::Error {
                    message: "Input Monitoring permission required — System Settings > Privacy & Security > Input Monitoring".to_string(),
                });
                update_overlay_state(cx);
            }
            false
        }
    };

    let initial_mode = cx.global::<VoxState>().settings().activation_mode;
    let (rebind_tx, rebind_rx) = std::sync::mpsc::channel::<String>();
    cx.global::<VoxState>().set_hotkey_rebind_tx(rebind_tx);

    let executor = cx.background_executor().clone();

    cx.spawn(async move |cx| {
        // Manager must stay alive for the hotkey registration to persist.
        // Not prefixed with _ because we call unregister/register on rebind.
        let manager = manager;
        let mut current_hotkey = hotkey;
        let mut hotkey_id = current_hotkey.id();
        let mut interpreter = HotkeyInterpreter::new(initial_mode);
        let mut hotkey_registered = hotkey_registered;
        let mut retry_timer = Instant::now();

        loop {
            executor.timer(Duration::from_millis(5)).await;

            // Retry hotkey registration every 2 seconds if initial registration failed.
            // Handles macOS Input Monitoring permission being granted after startup.
            if !hotkey_registered && retry_timer.elapsed() >= Duration::from_secs(2) {
                retry_timer = Instant::now();
                match manager.register(current_hotkey) {
                    Ok(()) => {
                        tracing::info!("global hotkey registered after permission grant");
                        hotkey_registered = true;
                        hotkey_id = current_hotkey.id();
                        cx.update(|cx| {
                            let current = cx.global::<VoxState>().pipeline_state();
                            if let PipelineState::Error { ref message } = current {
                                if message.contains("Input Monitoring") {
                                    cx.global::<VoxState>().set_pipeline_state(PipelineState::Idle);
                                    update_overlay_state(cx);
                                }
                            }
                        });
                    }
                    Err(_) => {}
                }
            }

            // Check for hotkey rebind requests from the settings panel
            while let Ok(new_hotkey_str) = rebind_rx.try_recv() {
                match new_hotkey_str.parse::<HotKey>() {
                    Ok(new_hotkey) => {
                        if let Err(err) = manager.unregister(current_hotkey) {
                            tracing::warn!(?err, "failed to unregister old hotkey");
                        }
                        match manager.register(new_hotkey) {
                            Ok(()) => {
                                tracing::info!(
                                    new = %new_hotkey_str,
                                    "hotkey remapped"
                                );
                                current_hotkey = new_hotkey;
                                hotkey_id = new_hotkey.id();
                            }
                            Err(err) => {
                                tracing::warn!(
                                    ?err,
                                    hotkey = %new_hotkey_str,
                                    "failed to register new hotkey, restoring old"
                                );
                                let _ = manager.register(current_hotkey);
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            ?err,
                            hotkey = %new_hotkey_str,
                            "failed to parse new hotkey string"
                        );
                    }
                }
            }

            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.id != hotkey_id {
                    continue;
                }

                cx.update(|cx| {
                    // Sync interpreter mode with current settings on every event
                    let current_mode = cx.global::<VoxState>().settings().activation_mode;
                    interpreter.set_mode(current_mode);

                    // Universal hotkey response (FR-006): if the app is not ready,
                    // acknowledge the keypress by refreshing the overlay (which
                    // already shows download progress, loading stage, or error).
                    let readiness = cx.global::<VoxState>().readiness();
                    if !matches!(readiness, AppReadiness::Ready) {
                        if matches!(event.state, HotKeyState::Pressed) {
                            tracing::info!(?readiness, "hotkey pressed while not ready");
                            update_overlay_state(cx);
                        }
                        return;
                    }

                    let is_recording = cx
                        .global::<RecordingSession>()
                        .active
                        .lock()
                        .is_some();

                    let action = match event.state {
                        HotKeyState::Pressed => interpreter.on_press(is_recording),
                        HotKeyState::Released => interpreter.on_release(is_recording),
                    };

                    match action {
                        HotkeyAction::StartRecording | HotkeyAction::StartHandsFree => {
                            tracing::info!(?action, "hotkey → start recording");
                            if let Err(err) = start_recording(cx) {
                                tracing::error!(%err, "failed to start recording");
                                cx.global::<VoxState>().set_pipeline_state(
                                    PipelineState::Error {
                                        message: err.to_string(),
                                    },
                                );
                                update_overlay_state(cx);
                            }
                        }
                        HotkeyAction::StopRecording => {
                            tracing::info!("hotkey → stop recording");
                            stop_recording(cx);
                        }
                        HotkeyAction::None => {}
                    }
                });
            }
        }
    })
    .detach();
}

/// Set up the system tray with 6-item menu, dynamic icon switching, and
/// dynamic menu text updates.
///
/// Pre-decodes all five icon variants at startup. Creates an
/// `std::sync::mpsc` channel for tray state updates — the sender is stored
/// as a [`TraySender`] GPUI Global so [`update_overlay_state`] can push
/// icon/tooltip changes automatically. The receiver is polled alongside
/// `MenuEvent` in the tray's background task.
fn setup_system_tray(cx: &mut App) {
    let icons = tray::decode_all_tray_icons();
    let (menu, menu_ids, menu_items) = tray::create_tray_menu();
    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<tray::TrayUpdate>();

    // Store sender as global so update_overlay_state can push tray updates
    cx.set_global(TraySender(tray_tx));

    let initial_icon = icons.idle.clone();

    let tray = match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Vox \u{2014} Idle")
        .with_icon(initial_icon)
        .build()
    {
        Ok(t) => t,
        Err(err) => {
            tracing::warn!(?err, "failed to create system tray icon");
            return;
        }
    };

    tracing::info!("system tray icon created (6-item menu, dynamic icons)");

    let executor = cx.background_executor().clone();

    cx.spawn(async move |cx| {
        // Tray and icons must stay alive for the icon to remain visible
        let tray = tray;
        let icons = icons;
        let toggle_item = menu_items.toggle_recording;

        loop {
            executor.timer(Duration::from_millis(10)).await;

            // Process tray state updates (icon + tooltip)
            while let Ok(update) = tray_rx.try_recv() {
                match update {
                    tray::TrayUpdate::SetState {
                        ref state,
                        is_recording,
                    } => {
                        let icon = tray::icon_for_state(state, &icons);
                        if let Err(err) = tray.set_icon(Some(icon.clone())) {
                            tracing::warn!(?err, "failed to set tray icon");
                        }
                        let tooltip = tray::tooltip_for_state(state);
                        if let Err(err) = tray.set_tooltip(Some(&tooltip)) {
                            tracing::warn!(?err, "failed to set tray tooltip");
                        }

                        // Derive label from session activity, not tray icon state.
                        // Recording stays active through Processing/Injecting states.
                        let text = if is_recording {
                            "Stop Recording"
                        } else {
                            "Start Recording"
                        };
                        toggle_item.set_text(text);
                    }
                }
            }

            // Process menu click events
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == menu_ids.toggle_recording {
                    cx.update(|cx| {
                        tracing::info!("ToggleRecording dispatched via tray menu");
                        cx.dispatch_action(&ToggleRecording);
                    });
                } else if event.id == menu_ids.settings {
                    cx.update(|cx| {
                        tracing::info!("OpenSettings via tray menu");
                        open_settings_window(cx);
                    });
                } else if event.id == menu_ids.toggle_overlay {
                    cx.update(|cx| {
                        tracing::info!("ToggleOverlay dispatched via tray menu");
                        cx.dispatch_action(&ToggleOverlay);
                    });
                } else if event.id == menu_ids.about {
                    show_about_dialog();
                } else if event.id == menu_ids.quit {
                    // Use _exit() to skip atexit handlers that trigger Metal
                    // residency set assertions in llama.cpp's ggml-metal cleanup.
                    cx.update(|_cx| unsafe { libc::_exit(0) });
                }
            }
        }
    })
    .detach();
}

/// Spawn the system sleep/wake listener and recovery handler.
///
/// Starts the platform-specific power event listener (Windows: `WM_POWERBROADCAST`,
/// macOS: `IORegisterForSystemPower`) and polls for wake events on the GPUI
/// foreground thread. On wake:
/// 1. Stops any active recording session (the cpal audio stream is invalidated
///    by sleep — the OS tears down audio device handles).
/// 2. Probes the audio device to verify it's accessible.
/// 3. Resets pipeline state to Idle.
///
/// The global hotkey registration persists across sleep/wake (OS message-based).
/// GPU contexts (CUDA/Metal) survive sleep on modern drivers — the existing
/// `retry_once()` mechanism handles any transient GPU failures on the first
/// post-wake transcription.
fn spawn_wake_handler(cx: &mut App) {
    let mut wake_rx = vox_core::power::start_wake_listener();
    let executor = cx.background_executor().clone();

    cx.spawn(async move |cx| {
        loop {
            executor.timer(Duration::from_millis(500)).await;

            while let Ok(event) = wake_rx.try_recv() {
                tracing::info!(
                    elapsed_ms = event.timestamp.elapsed().as_millis() as u64,
                    "system wake detected — starting recovery sequence"
                );

                cx.update(|cx| {
                    // 1. Stop any active recording — audio stream is dead after sleep
                    let had_active = cx
                        .global::<RecordingSession>()
                        .active
                        .lock()
                        .take()
                        .is_some();

                    if had_active {
                        tracing::info!("stopped active recording session after wake");
                    }

                    // 2. Reset pipeline state to Idle
                    cx.global::<VoxState>()
                        .set_pipeline_state(PipelineState::Idle);
                    update_overlay_state(cx);

                    // 3. Probe audio device — create a temporary AudioCapture to
                    // verify the OS audio subsystem is functional after wake.
                    // If it fails, show guidance; the user can retry via hotkey
                    // once the device is available.
                    let audio_config = AudioConfig {
                        device_name: cx.global::<VoxState>().settings().input_device.clone(),
                        ..AudioConfig::default()
                    };
                    match AudioCapture::new(&audio_config) {
                        Ok(_probe) => {
                            tracing::info!("audio device verified after wake");
                        }
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                "audio device not available after wake"
                            );
                            cx.global::<VoxState>().set_pipeline_state(
                                PipelineState::Error {
                                    message: "No microphone detected after wake — connect a device and press the hotkey".to_string(),
                                },
                            );
                            update_overlay_state(cx);
                        }
                    }

                    tracing::info!(
                        elapsed_ms = event.timestamp.elapsed().as_millis() as u64,
                        "wake recovery sequence complete"
                    );
                });
            }
        }
    })
    .detach();
}

/// Poll for macOS Accessibility permission and update the overlay.
///
/// If Accessibility isn't granted after the initial prompt, shows guidance
/// in the overlay and polls every 2 seconds. Auto-dismisses the guidance
/// and proceeds when the user grants permission. No-op on non-macOS
/// platforms or if permission is already granted.
fn spawn_accessibility_poller(cx: &mut App) {
    if vox_core::injector::is_accessibility_granted() {
        return;
    }

    tracing::info!("Accessibility permission not yet granted — starting poll loop");

    // Show guidance only if no other error is already displayed.
    // `setup_global_hotkey()` runs before this function and may have set
    // an Input Monitoring error — overwriting it would hide that guidance.
    let current = cx.global::<VoxState>().pipeline_state();
    if !matches!(current, PipelineState::Error { .. }) {
        cx.global::<VoxState>().set_pipeline_state(PipelineState::Error {
            message: "Accessibility permission required — System Settings > Privacy & Security > Accessibility".to_string(),
        });
        update_overlay_state(cx);
    }

    let executor = cx.background_executor().clone();

    cx.spawn(async move |cx| {
        loop {
            executor.timer(Duration::from_secs(2)).await;

            if vox_core::injector::is_accessibility_granted() {
                tracing::info!("Accessibility permission granted — resuming");
                cx.update(|cx| {
                    // Only clear if the current error is specifically our
                    // Accessibility message. If the displayed error is something
                    // else (e.g., Input Monitoring), leave it — that error's own
                    // handler will clear it when resolved.
                    let current = cx.global::<VoxState>().pipeline_state();
                    if let PipelineState::Error { ref message } = current {
                        if message.contains("Accessibility permission") {
                            cx.global::<VoxState>().set_pipeline_state(PipelineState::Idle);
                            update_overlay_state(cx);
                        }
                    }
                });
                return;
            }
        }
    })
    .detach();
}

/// Show a native About dialog with version and project info.
///
/// Uses Win32 `MessageBoxW` on Windows and `osascript` on macOS.
fn show_about_dialog() {
    let text = format!(
        "Vox v{}\n\n\
         Local-first intelligent voice dictation.\n\
         All processing happens on-device.\n\n\
         https://github.com/willibrandon/vox",
        env!("CARGO_PKG_VERSION")
    );

    #[cfg(target_os = "windows")]
    {
        // Spawn on a background thread so the modal MessageBoxW
        // doesn't block the tray event loop.
        std::thread::spawn(move || {
            let title: Vec<u16> = "About Vox\0".encode_utf16().collect();
            let body: Vec<u16> = format!("{text}\0").encode_utf16().collect();
            const MB_OK: u32 = 0x0000_0000;
            const MB_ICONINFORMATION: u32 = 0x0000_0040;
            unsafe {
                win32_message_box(
                    std::ptr::null(),
                    body.as_ptr(),
                    title.as_ptr(),
                    MB_OK | MB_ICONINFORMATION,
                );
            }
        });
    }

    #[cfg(target_os = "macos")]
    {
        // AppleScript via osascript — simple, no ObjC FFI needed.
        let script = format!(
            "display dialog \"{}\" with title \"About Vox\" buttons {{\"OK\"}} default button \"OK\" with icon note",
            text.replace('\"', "\\\"").replace('\n', "\\n")
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn();
    }
}

#[cfg(target_os = "windows")]
#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "MessageBoxW"]
    fn win32_message_box(
        hwnd: *const (),
        text: *const u16,
        caption: *const u16,
        msg_type: u32,
    ) -> i32;
}

async fn initialize_pipeline(cx: &mut AsyncApp) {
    if let Err(err) = try_initialize_pipeline(cx).await {
        tracing::error!(%err, "pipeline initialization failed");
        cx.update(|cx| {
            cx.global::<VoxState>().set_readiness(AppReadiness::Error {
                message: err.to_string(),
            });
            update_overlay_state(cx);
        });
    }
}

async fn try_initialize_pipeline(cx: &mut AsyncApp) -> anyhow::Result<()> {
    // Tokio handle is used for both async downloads and blocking model loads
    let tokio_handle: tokio::runtime::Handle = cx.update(|cx| {
        cx.global::<VoxState>().tokio_runtime().handle().clone()
    });

    // GPU detection (US7): detect hardware before model loading so we can
    // show actionable guidance if no compatible GPU is found.
    // spawn_blocking so the foreground task yields immediately, letting GPUI
    // paint the overlay before any CPU-intensive work runs.
    match tokio_handle.spawn_blocking(vox_core::gpu::detect_gpu).await? {
        Some(gpu_info) => {
            tracing::info!(
                gpu = %gpu_info,
                "GPU detected"
            );
            cx.update(|cx| {
                cx.global::<VoxState>().set_gpu_info(gpu_info);
            });
        }
        None => {
            let message = if cfg!(target_os = "windows") {
                "No compatible GPU detected. Vox requires an NVIDIA GPU with CUDA support. \
                 Install the latest NVIDIA drivers from nvidia.com/drivers"
            } else {
                "No compatible GPU detected. Vox requires Apple Silicon (M1 or later) with Metal."
            };
            tracing::error!(message, "GPU detection failed");
            cx.update(|cx| {
                cx.global::<VoxState>().set_readiness(AppReadiness::Error {
                    message: message.to_string(),
                });
                update_overlay_state(cx);
            });
            anyhow::bail!(message);
        }
    }

    let missing = tokio_handle
        .spawn_blocking(check_missing_models)
        .await??;

    if !missing.is_empty() {
        tracing::info!(count = missing.len(), "models missing, starting download");

        cx.update(|cx| {
            cx.global::<VoxState>()
                .set_readiness(AppReadiness::Downloading {
                    vad_progress: DownloadProgress::Pending,
                    whisper_progress: DownloadProgress::Pending,
                    llm_progress: DownloadProgress::Pending,
                });
            update_overlay_state(cx);
        });

        let downloader = ModelDownloader::new();
        let download_result = tokio_handle
            .spawn(async move { downloader.download_missing(&missing).await })
            .await?;

        if let Err(download_err) = download_result {
            // Offline fallback (FR-024): show manual download URLs and poll
            // for files to appear on disk. The user can download the models
            // manually and place them in the model directory.
            let err_msg = download_err.to_string();
            tracing::warn!(
                error = %err_msg,
                "model download failed, starting offline fallback (FR-024)"
            );

            let still_missing = tokio_handle
                .spawn_blocking(check_missing_models)
                .await??;
            if !still_missing.is_empty() {
                let missing_filenames: Vec<&str> =
                    still_missing.iter().map(|m| m.filename).collect();

                let err_for_ui = err_msg.clone();
                cx.update(|cx| {
                    let progress = |idx: usize| -> DownloadProgress {
                        if missing_filenames.contains(&models::MODELS[idx].filename) {
                            DownloadProgress::Failed {
                                error: err_for_ui.clone(),
                                manual_url: models::MODELS[idx].url.to_string(),
                            }
                        } else {
                            DownloadProgress::Complete
                        }
                    };

                    cx.global::<VoxState>()
                        .set_readiness(AppReadiness::Downloading {
                            vad_progress: progress(0),
                            whisper_progress: progress(1),
                            llm_progress: progress(2),
                        });
                    update_overlay_state(cx);
                });

                // Poll every 5 seconds for manually-placed model files.
                // Blocks until all required models are detected on disk.
                let poller = ModelDownloader::new();
                tokio_handle
                    .spawn(async move { poller.poll_until_ready().await })
                    .await??;

                tracing::info!("all models detected on disk after offline fallback");
            }
        }
    }

    cx.update(|cx| {
        cx.global::<VoxState>()
            .set_readiness(AppReadiness::Loading {
                stage: "Verifying model integrity...".into(),
            });
        update_overlay_state(cx);
    });

    // SHA-256 verification of all model files (US5). Corrupt files are deleted
    // and flagged for re-download. This catches corruption that occurred while
    // the app was closed (disk errors, partial writes, etc.).
    // spawn_blocking: SHA-256 of ~2.2GB of models takes several seconds.
    let corrupt = tokio_handle
        .spawn_blocking(models::verify_all_models)
        .await??;
    if !corrupt.is_empty() {
        let names: Vec<&str> = corrupt.iter().map(|m| m.name).collect();
        tracing::warn!(?names, "model integrity check failed — re-downloading corrupt models");

        cx.update(|cx| {
            cx.global::<VoxState>()
                .set_readiness(AppReadiness::Loading {
                    stage: format!("Re-downloading {} corrupt model(s)...", corrupt.len()),
                });
            update_overlay_state(cx);
        });

        let downloader = ModelDownloader::new();
        tokio_handle
            .spawn(async move { downloader.download_missing(&corrupt).await })
            .await??;

        // Verify again after re-download
        if !tokio_handle.spawn_blocking(models::all_models_present).await?? {
            anyhow::bail!("models still missing after integrity re-download");
        }
    }

    // Mark all models as Downloaded now that they're verified on disk
    cx.update(|cx| {
        let state = cx.global::<VoxState>();
        for model in models::MODELS {
            state.set_model_runtime(
                model.name.to_string(),
                vox_core::state::ModelRuntimeInfo {
                    state: vox_core::state::ModelRuntimeState::Downloaded,
                    vram_bytes: None,
                    benchmark: None,
                    custom_path: None,
                },
            );
        }
    });

    // Load ASR engine (Whisper → GPU)
    cx.update(|cx| {
        cx.global::<VoxState>()
            .set_readiness(AppReadiness::Loading {
                stage: "Loading ASR model...".into(),
            });
        cx.global::<VoxState>().set_model_runtime(
            models::MODELS[1].name.to_string(),
            vox_core::state::ModelRuntimeInfo {
                state: vox_core::state::ModelRuntimeState::Loading,
                vram_bytes: None,
                benchmark: None,
                custom_path: None,
            },
        );
        update_overlay_state(cx);
    });

    let whisper_path = models::model_path(models::MODELS[1].filename)?;
    let asr_engine = tokio_handle
        .spawn_blocking(move || AsrEngine::new(&whisper_path, true))
        .await??;

    cx.update(|cx| {
        cx.global::<VoxState>().set_asr_engine(asr_engine);
        cx.global::<VoxState>().set_model_runtime(
            models::MODELS[1].name.to_string(),
            vox_core::state::ModelRuntimeInfo {
                state: vox_core::state::ModelRuntimeState::Loaded,
                vram_bytes: Some(573 * 1024 * 1024),
                benchmark: None,
                custom_path: None,
            },
        );
    });

    // Load LLM post-processor (Qwen → GPU)
    cx.update(|cx| {
        cx.global::<VoxState>()
            .set_readiness(AppReadiness::Loading {
                stage: "Loading LLM model...".into(),
            });
        cx.global::<VoxState>().set_model_runtime(
            models::MODELS[2].name.to_string(),
            vox_core::state::ModelRuntimeInfo {
                state: vox_core::state::ModelRuntimeState::Loading,
                vram_bytes: None,
                benchmark: None,
                custom_path: None,
            },
        );
        update_overlay_state(cx);
    });

    let llm_path = models::model_path(models::MODELS[2].filename)?;
    let llm_processor = tokio_handle
        .spawn_blocking(move || PostProcessor::new(&llm_path, true))
        .await??;

    cx.update(|cx| {
        cx.global::<VoxState>().set_llm_processor(llm_processor);
        cx.global::<VoxState>().set_model_runtime(
            models::MODELS[2].name.to_string(),
            vox_core::state::ModelRuntimeInfo {
                state: vox_core::state::ModelRuntimeState::Loaded,
                vram_bytes: Some(2200 * 1024 * 1024),
                benchmark: None,
                custom_path: None,
            },
        );
        cx.global::<VoxState>().set_readiness(AppReadiness::Ready);
        update_overlay_state(cx);
    });

    tracing::info!("pipeline initialization complete");
    Ok(())
}

