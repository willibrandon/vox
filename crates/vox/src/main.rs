//! Vox application entry point.
//!
//! Creates the GPUI [`Application`], initializes structured logging, sets up
//! global state ([`VoxState`] and [`VoxTheme`]), registers actions and
//! keybindings, opens the overlay HUD window, configures the system tray
//! and global hotkey, and kicks off background pipeline initialization.

use std::time::Duration;

use global_hotkey::hotkey::{Code, HotKey};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};
use gpui::{
    size, App, AppContext as _, Application, AsyncApp, Bounds, TitlebarOptions,
    WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
};
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use vox_core::asr::AsrEngine;
use vox_core::llm::PostProcessor;
use vox_core::logging::init_logging;
use vox_core::models::{self, check_missing_models, DownloadProgress, ModelDownloader};
use vox_core::state::{ensure_data_dirs, AppReadiness, VoxState};
use vox_ui::key_bindings::{register_actions, register_key_bindings, OpenSettings, ToggleRecording};
use vox_ui::layout::size as layout_size;
use vox_ui::overlay_hud::OverlayHud;
use vox_ui::theme::VoxTheme;

fn main() {
    let _guard = init_logging();

    // Ctrl+C in the terminal should exit cleanly with code 0
    ctrlc::set_handler(|| std::process::exit(0)).ok();

    Application::new().run(|cx: &mut App| {
        if let Err(err) = run_app(cx) {
            tracing::error!(%err, "application startup failed");
            cx.quit();
        }
    });
}

fn run_app(cx: &mut App) -> anyhow::Result<()> {
    let data_dir = ensure_data_dirs()?;
    let state = VoxState::new(&data_dir)?;
    cx.set_global(state);
    cx.set_global(VoxTheme::dark());

    register_actions(cx);
    register_key_bindings(cx);

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

fn open_overlay_window(cx: &mut App) -> anyhow::Result<()> {
    let window_size = size(layout_size::OVERLAY_WIDTH, layout_size::OVERLAY_HEIGHT);
    let bounds = Bounds::centered(None, window_size, cx);

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: None,
        }),
        focus: false,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: true,
        is_resizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };

    cx.open_window(options, |window, cx| {
        // Deferred quit prevents Windows WM_ACTIVATE race condition (Tusk pattern)
        window.on_window_should_close(cx, |_window, cx| {
            cx.defer(|cx| cx.quit());
            false
        });
        cx.new(|cx| OverlayHud::new(cx))
    })?;

    tracing::info!("overlay HUD window opened");
    Ok(())
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
    let code = parse_hotkey_code(&hotkey_str);
    let hotkey = HotKey::new(None, code);

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
    });

    if !models::all_models_present()? {
        anyhow::bail!("not all models available after download");
    }

    // Load ASR engine (Whisper → GPU)
    cx.update(|cx| {
        cx.global::<VoxState>()
            .set_readiness(AppReadiness::Loading {
                stage: "Loading ASR model...".into(),
            });
    });

    let whisper_path = models::model_path(models::MODELS[1].filename)?;
    let asr_engine = tokio_handle
        .spawn_blocking(move || AsrEngine::new(&whisper_path, true))
        .await??;

    cx.update(|cx| {
        cx.global::<VoxState>().set_asr_engine(asr_engine);
    });

    // Load LLM post-processor (Qwen → GPU)
    cx.update(|cx| {
        cx.global::<VoxState>()
            .set_readiness(AppReadiness::Loading {
                stage: "Loading LLM model...".into(),
            });
    });

    let llm_path = models::model_path(models::MODELS[2].filename)?;
    let llm_processor = tokio_handle
        .spawn_blocking(move || PostProcessor::new(&llm_path, true))
        .await??;

    cx.update(|cx| {
        cx.global::<VoxState>().set_llm_processor(llm_processor);
        cx.global::<VoxState>().set_readiness(AppReadiness::Ready);
    });

    tracing::info!("pipeline initialization complete");
    Ok(())
}

fn parse_hotkey_code(key: &str) -> Code {
    match key.to_lowercase().as_str() {
        "capslock" | "caps_lock" | "capital" => Code::CapsLock,
        "f1" => Code::F1,
        "f2" => Code::F2,
        "f3" => Code::F3,
        "f4" => Code::F4,
        "f5" => Code::F5,
        "f6" => Code::F6,
        "f7" => Code::F7,
        "f8" => Code::F8,
        "f9" => Code::F9,
        "f10" => Code::F10,
        "f11" => Code::F11,
        "f12" => Code::F12,
        "space" => Code::Space,
        "escape" | "esc" => Code::Escape,
        other => {
            tracing::warn!(key = other, "unknown hotkey code, defaulting to CapsLock");
            Code::CapsLock
        }
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
