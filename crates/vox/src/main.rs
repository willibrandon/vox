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
use std::time::Duration;

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
    register_actions, register_key_bindings, OpenSettings, StopRecording, ToggleOverlay,
    ToggleRecording,
};
use vox_ui::overlay_hud::{open_overlay_window, OverlayDisplayState};
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

    // Ctrl+C in the terminal should exit cleanly with code 0
    ctrlc::set_handler(|| std::process::exit(0)).ok();

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
}

/// Start a new recording session: AudioCapture → VAD → ASR → LLM → Injection.
///
/// Creates all pipeline components, starts audio capture, spawns the
/// orchestrator on the tokio runtime, and starts a GPUI foreground task
/// that forwards pipeline state broadcasts to the overlay HUD.
fn start_recording(cx: &mut App) -> anyhow::Result<()> {
    // Read all needed values from VoxState in one borrow scope
    let (device_name, vad_config, asr, llm, dictionary, transcript_writer, tokio_handle) = {
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

        (
            device_name,
            vad_config,
            asr,
            llm,
            dictionary,
            transcript_writer,
            tokio_handle,
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

    if let Err(err) = manager.register(hotkey) {
        tracing::warn!(?err, hotkey = %hotkey_str, "failed to register global hotkey");
        return;
    }

    tracing::info!(hotkey = %hotkey_str, "global hotkey registered");

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

        loop {
            executor.timer(Duration::from_millis(5)).await;

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
                        tracing::info!("OpenSettings dispatched via tray menu");
                        cx.dispatch_action(&OpenSettings);
                    });
                } else if event.id == menu_ids.toggle_overlay {
                    cx.update(|cx| {
                        tracing::info!("ToggleOverlay dispatched via tray menu");
                        cx.dispatch_action(&ToggleOverlay);
                    });
                } else if event.id == menu_ids.about {
                    tracing::info!("About Vox selected");
                    // About dialog is a future feature — log for now
                } else if event.id == menu_ids.quit {
                    cx.update(|cx| cx.quit());
                }
            }
        }
    })
    .detach();
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

    let missing = check_missing_models()?;

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
        tokio_handle
            .spawn(async move { downloader.download_missing(&missing).await })
            .await??;
    }

    cx.update(|cx| {
        cx.global::<VoxState>()
            .set_readiness(AppReadiness::Loading {
                stage: "Verifying models...".into(),
            });
        update_overlay_state(cx);
    });

    if !models::all_models_present()? {
        anyhow::bail!("not all models available after download");
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

