//! Model status and management panel.
//!
//! Provides [`ModelPanel`] as an entity with `Render` impl. Displays the
//! status of each ML model (VAD, ASR, LLM) with file size, VRAM usage,
//! download state, benchmark results, and management actions (retry, open
//! folder, swap model). Refreshes from VoxState on every render.

use gpui::{
    div, prelude::*, px, App, Context, EntityId, IntoElement, PathPromptOptions, Render,
    ScrollHandle, SharedString, Window,
};

use vox_core::models::{detect_format, format_to_slot, model_dir, ModelDownloader, ModelInfo, MODELS};
use vox_core::state::{ModelRuntimeInfo, ModelRuntimeState, VoxState};

use crate::layout::{radius, spacing};
use crate::scrollbar::{new_drag_state, Scrollbar, ScrollbarDragState};
use crate::theme::{ThemeColors, VoxTheme};

/// Per-model display data combining static info and runtime state.
struct ModelDisplay {
    /// Static model metadata.
    info: &'static ModelInfo,
    /// Runtime information snapshot.
    runtime: Option<ModelRuntimeInfo>,
}

/// Model management panel showing status of all ML models.
///
/// Uses the scrollbar-as-sibling pattern for proper scroll behavior.
/// Refreshes model states from VoxState on every render to pick up
/// download progress, load events, and benchmark results.
pub struct ModelPanel {
    /// Display data for each registered model.
    models: Vec<ModelDisplay>,
    /// Scroll handle shared between scroll container and Scrollbar.
    scroll_handle: ScrollHandle,
    /// Drag state for scrollbar thumb.
    scrollbar_drag: ScrollbarDragState,
}

impl ModelPanel {
    /// Create a new model panel, reading current model states.
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let models = Self::load_models(cx);
        Self {
            models,
            scroll_handle: ScrollHandle::new(),
            scrollbar_drag: new_drag_state(),
        }
    }

    /// Load model display data from VoxState.
    fn load_models(cx: &App) -> Vec<ModelDisplay> {
        let state = cx.global::<VoxState>();
        let runtime_map = state.all_model_runtime();

        MODELS
            .iter()
            .map(|info| {
                let runtime = runtime_map.get(info.name).cloned();
                ModelDisplay { info, runtime }
            })
            .collect()
    }

    /// Format bytes as human-readable size (MB or GB).
    fn format_size(bytes: u64) -> String {
        if bytes >= 1_073_741_824 {
            format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
        } else {
            format!("{:.1} MB", bytes as f64 / 1_048_576.0)
        }
    }

    /// Render a single model card with status, actions, and benchmark.
    fn render_model_card(
        model: &ModelDisplay,
        colors: &ThemeColors,
        panel_id: EntityId,
    ) -> impl IntoElement {
        let state = model
            .runtime
            .as_ref()
            .map(|r| &r.state)
            .unwrap_or(&ModelRuntimeState::Missing);

        let (state_text, status_color) = match state {
            ModelRuntimeState::Missing => ("Missing", colors.text_muted),
            ModelRuntimeState::Downloading => ("Downloading...", colors.accent),
            ModelRuntimeState::Downloaded => ("Downloaded", colors.status_success),
            ModelRuntimeState::Loading => ("Loading...", colors.accent),
            ModelRuntimeState::Loaded => ("Loaded", colors.status_success),
            ModelRuntimeState::Error(_) => ("Error", colors.status_error),
        };

        let is_error = matches!(state, ModelRuntimeState::Error(_));
        let is_loaded = matches!(state, ModelRuntimeState::Loaded);
        let size_text = Self::format_size(model.info.size_bytes);

        let mut card = div()
            .flex()
            .flex_col()
            .gap(spacing::SM)
            .p(spacing::MD)
            .rounded(radius::SM)
            .bg(colors.elevated_surface)
            .border_1()
            .border_color(if is_error {
                colors.status_error
            } else {
                colors.border
            });

        // Name + status row
        card = card.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(14.0))
                        .text_color(colors.text)
                        .child(SharedString::from(model.info.name.to_string())),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(status_color)
                        .px(spacing::SM)
                        .py(px(2.0))
                        .rounded(radius::SM)
                        .child(SharedString::from(state_text)),
                ),
        );

        // Error message detail
        if let Some(ModelRuntimeInfo {
            state: ModelRuntimeState::Error(msg),
            ..
        }) = &model.runtime
        {
            card = card.child(
                div()
                    .text_size(px(11.0))
                    .text_color(colors.status_error)
                    .child(SharedString::from(msg.clone())),
            );
        }

        // Download progress bar (when downloading)
        if let Some(ModelRuntimeInfo {
            state: ModelRuntimeState::Downloading,
            ..
        }) = &model.runtime
        {
            // Use size_bytes as total, show indeterminate-style bar
            // (actual progress requires subscribing to DownloadEvent which
            // would need the download system wired here; for now show an
            // animated-style bar indicating activity)
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::SM)
                    .child(
                        div()
                            .flex_1()
                            .h(px(4.0))
                            .rounded(radius::SM)
                            .bg(colors.surface)
                            .child(
                                div()
                                    .w(px(60.0))
                                    .h_full()
                                    .rounded(radius::SM)
                                    .bg(colors.accent),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(colors.text_muted)
                            .child(SharedString::from("downloading...")),
                    ),
            );
        }

        // Info row: file size + VRAM (if loaded)
        let mut info_row = div()
            .flex()
            .items_center()
            .gap(spacing::MD)
            .child(
                div()
                    .text_size(px(12.0))
                    .text_color(colors.text_muted)
                    .child(SharedString::from(size_text)),
            );

        if let Some(vram_bytes) = model.runtime.as_ref().and_then(|r| r.vram_bytes) {
            info_row = info_row.child(
                div()
                    .text_size(px(12.0))
                    .text_color(colors.text_muted)
                    .child(SharedString::from(format!(
                        "VRAM: {}",
                        Self::format_size(vram_bytes)
                    ))),
            );
        }

        card = card.child(info_row);

        // Benchmark result (T043)
        if is_loaded {
            if let Some(bench) = model.runtime.as_ref().and_then(|r| r.benchmark.as_ref()) {
                card = card.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.text_muted)
                        .child(SharedString::from(format!(
                            "{}: {:.1}",
                            bench.metric_name, bench.value
                        ))),
                );
            } else {
                card = card.child(
                    div()
                        .text_size(px(11.0))
                        .text_color(colors.text_muted)
                        .child(SharedString::from("Benchmark: pending")),
                );
            }
        }

        // Action buttons row (T042, T044)
        let model_name = model.info.name;
        let model_info_static: &'static ModelInfo = model.info;
        let mut actions = div().flex().items_center().gap(spacing::SM);
        let is_downloaded = matches!(state, ModelRuntimeState::Downloaded);
        let has_actions = is_error || is_loaded || is_downloaded;

        if is_error {
            let retry_text_color = colors.accent;
            let retry_border = colors.accent;
            actions = actions.child(
                div()
                    .id(SharedString::from(format!("retry-{model_name}")))
                    .px(spacing::SM)
                    .py(px(2.0))
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(retry_border)
                    .text_size(px(11.0))
                    .text_color(retry_text_color)
                    .cursor_pointer()
                    .child(SharedString::from("Retry Download"))
                    .on_click(move |_, _window, cx| {
                        // Delete any partial file so the downloader sees it as missing
                        if let Ok(dir) = model_dir() {
                            let model_file = dir.join(model_info_static.filename);
                            if model_file.exists() {
                                if let Err(err) = std::fs::remove_file(&model_file) {
                                    tracing::warn!(%err, "failed to remove model file before retry");
                                }
                            }
                            if let Err(err) = vox_core::models::cleanup_tmp_files() {
                                tracing::warn!(%err, "failed to clean up .tmp files before retry");
                            }
                        }

                        // Set state to Downloading for immediate UI feedback
                        cx.global::<VoxState>().set_model_runtime(
                            model_name.to_string(),
                            ModelRuntimeInfo {
                                state: ModelRuntimeState::Downloading,
                                vram_bytes: None,
                                benchmark: None,
                                custom_path: None,
                            },
                        );
                        cx.notify(panel_id);

                        // Spawn actual download on the tokio runtime
                        let tokio_handle = cx
                            .global::<VoxState>()
                            .tokio_runtime()
                            .handle()
                            .clone();
                        let name_for_result = model_name.to_string();

                        cx.spawn(async move |cx| {
                            let downloader = ModelDownloader::new();
                            let result = tokio_handle
                                .spawn(async move {
                                    downloader
                                        .download_missing(&[model_info_static])
                                        .await
                                })
                                .await;

                            cx.update(|cx| {
                                let new_state = match result {
                                    Ok(Ok(())) => ModelRuntimeState::Downloaded,
                                    Ok(Err(err)) => {
                                        tracing::warn!(%err, "retry download failed");
                                        ModelRuntimeState::Error(
                                            format!("{err:#}"),
                                        )
                                    }
                                    Err(err) => {
                                        tracing::warn!(%err, "retry download task panicked");
                                        ModelRuntimeState::Error(
                                            format!("Download task failed: {err}"),
                                        )
                                    }
                                };
                                cx.global::<VoxState>().set_model_runtime(
                                    name_for_result,
                                    ModelRuntimeInfo {
                                        state: new_state,
                                        vram_bytes: None,
                                        benchmark: None,
                                        custom_path: None,
                                    },
                                );
                                cx.notify(panel_id);
                            });
                        })
                        .detach();
                    }),
            );
        }

        // Swap Model button — available for downloaded or loaded models
        if matches!(
            state,
            ModelRuntimeState::Downloaded | ModelRuntimeState::Loaded
        ) {
            let swap_text = colors.text_muted;
            let swap_border = colors.border;
            actions = actions.child(
                div()
                    .id(SharedString::from(format!("swap-{model_name}")))
                    .px(spacing::SM)
                    .py(px(2.0))
                    .rounded(radius::SM)
                    .border_1()
                    .border_color(swap_border)
                    .text_size(px(11.0))
                    .text_color(swap_text)
                    .cursor_pointer()
                    .child(SharedString::from("Swap Model"))
                    .on_click(move |_, _window, cx| {
                        let receiver = cx.prompt_for_paths(PathPromptOptions {
                            files: true,
                            directories: false,
                            multiple: false,
                            prompt: Some(
                                SharedString::from(format!("Select replacement for {model_name}")),
                            ),
                        });

                        cx.spawn(async move |cx| {
                            let paths = match receiver.await {
                                Ok(Ok(Some(paths))) => paths,
                                _ => return,
                            };
                            let Some(selected) = paths.first() else {
                                return;
                            };

                            // Validate format via magic bytes and map to model slot
                            let format = match detect_format(selected) {
                                Ok(f) => f,
                                Err(err) => {
                                    cx.update(|cx| {
                                        tracing::warn!(%err, "failed to detect model format");
                                        cx.notify(panel_id);
                                    });
                                    return;
                                }
                            };
                            let Some(slot_index) = format_to_slot(format) else {
                                cx.update(|cx| {
                                    tracing::warn!(
                                        "unrecognized model format, expected GGUF, GGML, or ONNX"
                                    );
                                    cx.notify(panel_id);
                                });
                                return;
                            };

                            // Copy to the expected slot filename so the loader finds it
                            let slot_filename = MODELS[slot_index].filename;
                            let dest = match model_dir() {
                                Ok(dir) => dir.join(slot_filename),
                                Err(err) => {
                                    cx.update(|cx| {
                                        tracing::warn!(%err, "failed to resolve model directory");
                                        cx.notify(panel_id);
                                    });
                                    return;
                                }
                            };

                            if let Err(err) = std::fs::copy(selected, &dest) {
                                cx.update(|cx| {
                                    tracing::warn!(%err, "failed to copy model file");
                                    cx.global::<VoxState>().set_model_runtime(
                                        model_name.to_string(),
                                        ModelRuntimeInfo {
                                            state: ModelRuntimeState::Error(
                                                format!("Copy failed: {err}"),
                                            ),
                                            vram_bytes: None,
                                            benchmark: None,
                                            custom_path: None,
                                        },
                                    );
                                    cx.notify(panel_id);
                                });
                                return;
                            }

                            cx.update(|cx| {
                                cx.global::<VoxState>().set_model_runtime(
                                    model_name.to_string(),
                                    ModelRuntimeInfo {
                                        state: ModelRuntimeState::Downloaded,
                                        vram_bytes: None,
                                        benchmark: None,
                                        custom_path: Some(dest),
                                    },
                                );
                                cx.notify(panel_id);
                            });
                        })
                        .detach();
                    }),
            );
        }

        if has_actions {
            card = card.child(actions);
        }

        card
    }
}

impl Render for ModelPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Refresh model data from VoxState
        self.models = Self::load_models(cx);

        let panel_id = cx.entity_id();
        let theme = cx.global::<VoxTheme>();
        let scrollbar_thumb = theme.colors.scrollbar_thumb;
        let scrollbar_track = theme.colors.scrollbar_track;
        let text_color = theme.colors.text;
        let text_muted = theme.colors.text_muted;
        let border_color = theme.colors.border;
        let colors = theme.colors.clone();

        let mut scroll_content = div()
            .id("model-scroll")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .flex()
            .flex_col()
            .p(spacing::LG)
            .gap(spacing::MD);

        // Header with Open Folder button
        scroll_content = scroll_content.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb(spacing::SM)
                .child(
                    div()
                        .text_size(px(16.0))
                        .text_color(text_color)
                        .child(SharedString::from("Models")),
                )
                .child(
                    div()
                        .id("open-model-folder")
                        .px(spacing::MD)
                        .py(px(3.0))
                        .rounded(radius::SM)
                        .border_1()
                        .border_color(border_color)
                        .text_size(px(11.0))
                        .text_color(text_muted)
                        .cursor_pointer()
                        .child(SharedString::from("Open Folder"))
                        .on_click(move |_, _window, _cx| {
                            if let Ok(dir) = model_dir() {
                                #[cfg(target_os = "windows")]
                                {
                                    let _ = std::process::Command::new("explorer")
                                        .arg(&dir)
                                        .spawn();
                                }
                                #[cfg(target_os = "macos")]
                                {
                                    let _ =
                                        std::process::Command::new("open").arg(&dir).spawn();
                                }
                                #[cfg(target_os = "linux")]
                                {
                                    let _ =
                                        std::process::Command::new("xdg-open").arg(&dir).spawn();
                                }
                            }
                        }),
                ),
        );

        // Model cards
        for model in &self.models {
            scroll_content =
                scroll_content.child(Self::render_model_card(model, &colors, panel_id));
        }

        // Scrollbar as SIBLING of scroll container
        div()
            .size_full()
            .child(scroll_content)
            .child(Scrollbar::new(
                self.scroll_handle.clone(),
                cx.entity_id(),
                self.scrollbar_drag.clone(),
                scrollbar_thumb,
                scrollbar_track,
            ))
    }
}
