//! Vox application entry point.
//!
//! Creates the GPUI [`Application`], initializes structured logging, sets up
//! global state ([`VoxState`] and [`VoxTheme`]), registers actions and
//! keybindings, opens the overlay HUD window, configures the system tray
//! and global hotkey, and kicks off background pipeline initialization.
//! When models are loaded and Ready, the ToggleRecording action creates
//! an AudioCapture → VAD → ASR → LLM → TextInjection pipeline that
//! processes speech and injects polished text into the focused application.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use gpui::{App, AppContext as _, Application, AsyncApp};
use parking_lot::Mutex;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use vox_core::asr::AsrEngine;
use vox_core::audio::{AudioCapture, AudioConfig};
use vox_core::llm::PostProcessor;
use vox_core::logging::init_logging;
use vox_core::models::{self, check_missing_models, DownloadProgress, ModelDownloader};
use vox_core::pipeline::{Pipeline, PipelineCommand, PipelineState};
use vox_core::state::{ensure_data_dirs, AppReadiness, VoxState};
use vox_core::vad::VadConfig;
use vox_ui::key_bindings::{
    register_actions, register_key_bindings, OpenSettings, StopRecording, ToggleRecording,
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
            bypass_vad: settings.hold_to_talk,
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

/// Updates the OverlayDisplayState bridge to match the current VoxState.
///
/// Must be called after every `VoxState::set_readiness()` or
/// `VoxState::set_pipeline_state()` to trigger overlay re-rendering.
fn update_overlay_state(cx: &mut App) {
    let state = cx.global::<VoxState>();
    cx.set_global(OverlayDisplayState {
        readiness: state.readiness(),
        pipeline_state: state.pipeline_state(),
    });
}

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

    let hotkey_id = hotkey.id();
    let executor = cx.background_executor().clone();

    cx.spawn(async move |cx| {
        // Manager must stay alive for the hotkey registration to persist
        let _manager = manager;
        loop {
            executor.timer(Duration::from_millis(50)).await;
            while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                if event.id == hotkey_id {
                    cx.update(|cx| {
                        tracing::info!("ToggleRecording dispatched via global hotkey");
                        cx.dispatch_action(&ToggleRecording);
                    });
                }
            }
        }
    })
    .detach();
}

fn setup_system_tray(cx: &mut App) {
    let menu = Menu::new();

    let toggle_item = MenuItem::new("Toggle Recording", true, None);
    let settings_item = MenuItem::new("Settings", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let toggle_id = toggle_item.id().clone();
    let settings_id = settings_item.id().clone();
    let quit_id = quit_item.id().clone();

    if let Err(err) = menu.append(&toggle_item) {
        tracing::warn!(?err, "failed to append Toggle Recording menu item");
    }
    if let Err(err) = menu.append(&settings_item) {
        tracing::warn!(?err, "failed to append Settings menu item");
    }
    if let Err(err) = menu.append(&quit_item) {
        tracing::warn!(?err, "failed to append Quit menu item");
    }

    let icon = decode_png_icon(include_bytes!("../../../assets/icons/tray-idle.png"));

    let tray = match TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Vox \u{2014} Voice Dictation")
        .with_icon(icon)
        .build()
    {
        Ok(t) => t,
        Err(err) => {
            tracing::warn!(?err, "failed to create system tray icon");
            return;
        }
    };

    tracing::info!("system tray icon created");

    let executor = cx.background_executor().clone();

    cx.spawn(async move |cx| {
        // Tray must stay alive for the icon to remain visible
        let _tray = tray;
        loop {
            executor.timer(Duration::from_millis(50)).await;
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == toggle_id {
                    cx.update(|cx| {
                        tracing::info!("ToggleRecording dispatched via tray menu");
                        cx.dispatch_action(&ToggleRecording);
                    });
                } else if event.id == settings_id {
                    cx.update(|cx| {
                        tracing::info!("OpenSettings dispatched via tray menu");
                        cx.dispatch_action(&OpenSettings);
                    });
                } else if event.id == quit_id {
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
