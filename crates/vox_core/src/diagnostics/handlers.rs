//! Diagnostics method handlers for processing UDS requests.
//!
//! Each handler function takes a parsed [`Request`] and [`VoxState`] reference,
//! reads the relevant state, and returns a [`Response`] with the result or error.
//! The [`dispatch`] function routes requests by method name to the appropriate handler.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::json;
use vox_diag::net::uds::UnixStream;
use vox_diag::protocol::{self, Event, Request, Response};

use crate::diagnostics::listener::DiagnosticsCommand;
use crate::log_sink::LogLevel;
use crate::pipeline::state::PipelineState;
use crate::state::{AppReadiness, ModelRuntimeState, VoxState};

/// Route a request to the appropriate handler based on its method name.
///
/// Returns `Response::error(UNKNOWN_METHOD)` for unrecognized methods.
pub fn dispatch(request: &Request, state: &VoxState) -> Response {
    match request.method.as_str() {
        "status" => handle_status(request, state),
        "settings" => handle_settings(request, state),
        "logs" => handle_logs(request, state),
        "transcripts" => handle_transcripts(request, state),
        "inject_audio" => handle_inject_audio(request, state),
        "record" => handle_record(request, state),
        "screenshot" => handle_screenshot(request, state),
        "quit" => handle_quit(request, state),
        unknown => Response::error(
            request.id,
            protocol::error_code::UNKNOWN_METHOD,
            format!("unknown method: {unknown}"),
        ),
    }
}

/// Build a full application status snapshot from VoxState.
///
/// Returns pid, readiness, pipeline state, activation mode, recording flag,
/// debug audio level, GPU info, model runtime info, audio device info,
/// and last pipeline latency.
fn handle_status(request: &Request, state: &VoxState) -> Response {
    let readiness = match state.readiness() {
        AppReadiness::Downloading { .. } => "downloading",
        AppReadiness::Loading { .. } => "loading",
        AppReadiness::Ready => "ready",
        AppReadiness::Error { .. } => "error",
    };

    let pipeline_state = match state.pipeline_state() {
        crate::pipeline::state::PipelineState::Idle => "idle".to_string(),
        crate::pipeline::state::PipelineState::Listening => "listening".to_string(),
        crate::pipeline::state::PipelineState::Processing { .. } => "processing".to_string(),
        crate::pipeline::state::PipelineState::Injecting { .. } => "injecting".to_string(),
        crate::pipeline::state::PipelineState::Error { message } => {
            format!("error: {message}")
        }
        crate::pipeline::state::PipelineState::InjectionFailed { error, .. } => {
            format!("injection_failed: {error}")
        }
    };

    let settings = state.settings();
    let activation_mode = serde_json::to_value(&settings.activation_mode)
        .unwrap_or(serde_json::Value::String("unknown".into()));
    let debug_audio = serde_json::to_value(&settings.debug_audio)
        .unwrap_or(serde_json::Value::String("off".into()));
    drop(settings);

    let gpu = match state.gpu_info() {
        Some(info) => json!({
            "name": info.name,
            "vram_bytes": info.vram_bytes,
            "platform": info.platform.to_string(),
        }),
        None => serde_json::Value::Null,
    };

    let model_runtime = state.all_model_runtime();
    let models: serde_json::Map<String, serde_json::Value> = model_runtime
        .into_iter()
        .map(|(name, info)| {
            let state_str = match &info.state {
                ModelRuntimeState::Missing => "missing",
                ModelRuntimeState::Downloading => "downloading",
                ModelRuntimeState::Downloaded => "downloaded",
                ModelRuntimeState::Loading => "loading",
                ModelRuntimeState::Loaded => "loaded",
                ModelRuntimeState::Error(_) => "error",
            };
            let value = json!({
                "state": state_str,
                "vram_bytes": info.vram_bytes,
            });
            (name, value)
        })
        .collect();

    let audio = json!({
        "device": state.audio_device_name(),
        "sample_rate": state.audio_sample_rate(),
        "rms": state.latest_rms(),
    });

    Response::success(
        request.id,
        json!({
            "pid": std::process::id(),
            "readiness": readiness,
            "pipeline_state": pipeline_state,
            "activation_mode": activation_mode,
            "recording": state.is_recording(),
            "debug_audio": debug_audio,
            "gpu": gpu,
            "models": models,
            "audio": audio,
            "last_latency_ms": state.last_latency_ms(),
        }),
    )
}

/// Read settings (all or a single key).
///
/// Params: `action` (required, must be "get" for this handler — "set" is in T021),
/// optional `key` for a single field.
fn handle_settings(request: &Request, state: &VoxState) -> Response {
    let params = request.params.as_ref().unwrap_or(&serde_json::Value::Null);

    let action = match params.get("action").and_then(|v| v.as_str()) {
        Some(action) => action,
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INVALID_PARAMS,
                "missing required param: 'action'",
            );
        }
    };

    match action {
        "get" => handle_settings_get(request, state, params),
        "set" => handle_settings_set(request, state, params),
        unknown => Response::error(
            request.id,
            protocol::error_code::INVALID_PARAMS,
            format!("invalid action: '{unknown}' (expected 'get' or 'set')"),
        ),
    }
}

/// Handle settings get — return all settings or a single key.
fn handle_settings_get(
    request: &Request,
    state: &VoxState,
    params: &serde_json::Value,
) -> Response {
    let settings = state.settings();
    let settings_value = match serde_json::to_value(&*settings) {
        Ok(v) => v,
        Err(err) => {
            return Response::error(
                request.id,
                protocol::error_code::INTERNAL_ERROR,
                format!("failed to serialize settings: {err}"),
            );
        }
    };
    drop(settings);

    match params.get("key").and_then(|v| v.as_str()) {
        None => Response::success(request.id, settings_value),
        Some(key) => {
            let map = match settings_value.as_object() {
                Some(map) => map,
                None => {
                    return Response::error(
                        request.id,
                        protocol::error_code::INTERNAL_ERROR,
                        "settings did not serialize as object",
                    );
                }
            };
            match map.get(key) {
                Some(value) => Response::success(request.id, json!({ key: value })),
                None => Response::error(
                    request.id,
                    protocol::error_code::INVALID_PARAMS,
                    format!("unknown setting: '{key}'"),
                ),
            }
        }
    }
}

/// Handle settings set — update a single setting by key.
///
/// Validates key existence, type compatibility, then applies and persists.
fn handle_settings_set(
    request: &Request,
    state: &VoxState,
    params: &serde_json::Value,
) -> Response {
    use crate::config::SettingType;

    let key = match params.get("key").and_then(|v| v.as_str()) {
        Some(key) => key,
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INVALID_PARAMS,
                "missing required param: 'key'",
            );
        }
    };

    let value = match params.get("value") {
        Some(value) => value.clone(),
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INVALID_PARAMS,
                "missing required param: 'value'",
            );
        }
    };

    let expected_type = match crate::config::Settings::field_type(key) {
        Some(t) => t,
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INVALID_PARAMS,
                format!("unknown setting: '{key}'"),
            );
        }
    };

    let type_ok = match expected_type {
        SettingType::Float => value.is_f64() || value.is_i64() || value.is_u64(),
        SettingType::Integer => value.is_u64() || value.is_i64(),
        SettingType::Bool => value.is_boolean(),
        SettingType::String => value.is_string(),
    };

    if !type_ok {
        let actual = if value.is_boolean() {
            "bool"
        } else if value.is_u64() || value.is_i64() {
            "integer"
        } else if value.is_f64() {
            "float"
        } else if value.is_string() {
            "string"
        } else {
            "other"
        };
        let expected = match expected_type {
            SettingType::Float => "float",
            SettingType::Integer => "integer",
            SettingType::Bool => "bool",
            SettingType::String => "string",
        };
        return Response::error(
            request.id,
            protocol::error_code::INVALID_PARAMS,
            format!("invalid type for '{key}': expected {expected}, got {actual}"),
        );
    }

    let mut field_error: Option<anyhow::Error> = None;
    match state.update_settings(|settings| {
        if let Err(err) = settings.set_field(key, value) {
            field_error = Some(err);
        }
    }) {
        Ok(()) => {
            if let Some(err) = field_error {
                return Response::error(
                    request.id,
                    protocol::error_code::INVALID_PARAMS,
                    format!("failed to apply setting '{key}': {err}"),
                );
            }
            Response::success(request.id, json!({"ok": true}))
        }
        Err(err) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            format!("failed to save settings: {err}"),
        ),
    }
}

/// Return recent log entries from the in-memory log buffer.
///
/// Params: optional `count` (default 50), optional `min_level` (default "trace").
fn handle_logs(request: &Request, state: &VoxState) -> Response {
    let params = request.params.as_ref().unwrap_or(&serde_json::Value::Null);

    let count = params
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let min_level = match params.get("min_level").and_then(|v| v.as_str()) {
        None => None,
        Some(level_str) => match parse_log_level(level_str) {
            Some(level) => Some(level),
            None => {
                return Response::error(
                    request.id,
                    protocol::error_code::INVALID_PARAMS,
                    format!(
                        "invalid min_level: '{level_str}' (expected error, warn, info, debug, or trace)"
                    ),
                );
            }
        },
    };

    let entries = state.log_buffer().recent(count, min_level);
    let json_entries: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|entry| {
            json!({
                "timestamp": entry.timestamp,
                "level": entry.level.to_string().to_lowercase(),
                "target": entry.target,
                "message": entry.message,
            })
        })
        .collect();

    Response::success(request.id, json!({ "entries": json_entries }))
}

/// Parse a log level string (case-insensitive).
fn parse_log_level(s: &str) -> Option<LogLevel> {
    match s.to_lowercase().as_str() {
        "error" => Some(LogLevel::Error),
        "warn" => Some(LogLevel::Warn),
        "info" => Some(LogLevel::Info),
        "debug" => Some(LogLevel::Debug),
        "trace" => Some(LogLevel::Trace),
        _ => None,
    }
}

/// Return recent transcript entries from the database.
///
/// Params: optional `count` (default 10).
fn handle_transcripts(request: &Request, state: &VoxState) -> Response {
    let params = request.params.as_ref().unwrap_or(&serde_json::Value::Null);

    let count = params
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(10) as usize;

    let entries = match state.get_transcripts(count, 0) {
        Ok(entries) => entries,
        Err(err) => {
            return Response::error(
                request.id,
                protocol::error_code::INTERNAL_ERROR,
                format!("failed to query transcripts: {err}"),
            );
        }
    };

    let json_entries: Vec<serde_json::Value> = entries
        .into_iter()
        .map(|entry| {
            json!({
                "timestamp": entry.created_at,
                "raw": entry.raw_text,
                "polished": entry.polished_text,
                "latency_ms": entry.latency_ms,
            })
        })
        .collect();

    Response::success(request.id, json!({ "entries": json_entries }))
}

/// Inject audio from a WAV file or base64 PCM and run through ASR + LLM.
///
/// Params: exactly one of `path` (WAV file) or `pcm_base64` (base64 f32 LE).
/// If `pcm_base64`, `sample_rate` is also required.
fn handle_inject_audio(request: &Request, state: &VoxState) -> Response {
    let params = request.params.as_ref().unwrap_or(&serde_json::Value::Null);

    let has_path = params.get("path").and_then(|v| v.as_str()).is_some();
    let has_pcm = params.get("pcm_base64").and_then(|v| v.as_str()).is_some();

    if has_path && has_pcm {
        return Response::error(
            request.id,
            protocol::error_code::INVALID_PARAMS,
            "provide either 'path' or 'pcm_base64', not both",
        );
    }
    if !has_path && !has_pcm {
        return Response::error(
            request.id,
            protocol::error_code::INVALID_PARAMS,
            "must provide either 'path' or 'pcm_base64'",
        );
    }

    if !matches!(state.readiness(), AppReadiness::Ready) {
        return Response::error(
            request.id,
            protocol::error_code::NOT_READY,
            "app is not ready — models may still be loading",
        );
    }

    let samples = if has_path {
        let path_str = params["path"].as_str().expect("checked above");
        let path = std::path::Path::new(path_str);
        match crate::diagnostics::audio_injector::load_wav(path) {
            Ok(s) => s,
            Err(err) => {
                return Response::error(
                    request.id,
                    protocol::error_code::INVALID_PARAMS,
                    format!("failed to load WAV: {err}"),
                );
            }
        }
    } else {
        let pcm_data = params["pcm_base64"].as_str().expect("checked above");
        let sample_rate = match params.get("sample_rate").and_then(|v| v.as_u64()) {
            Some(rate) => rate as u32,
            None => {
                return Response::error(
                    request.id,
                    protocol::error_code::INVALID_PARAMS,
                    "missing required param: 'sample_rate' (required with 'pcm_base64')",
                );
            }
        };
        match crate::diagnostics::audio_injector::load_pcm_base64(pcm_data, sample_rate) {
            Ok(s) => s,
            Err(err) => {
                return Response::error(
                    request.id,
                    protocol::error_code::INVALID_PARAMS,
                    format!("failed to decode PCM: {err}"),
                );
            }
        }
    };

    if samples.is_empty() {
        return Response::error(
            request.id,
            protocol::error_code::INVALID_PARAMS,
            "audio contains no samples",
        );
    }

    match crate::diagnostics::audio_injector::run(&samples, state) {
        Ok(result) => Response::success(
            request.id,
            json!({
                "raw_transcript": result.raw_transcript,
                "polished_text": result.polished_text,
                "is_command": result.is_command,
                "latency_ms": result.latency_ms,
                "injected": true,
            }),
        ),
        Err(err) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            format!("injection failed: {err}"),
        ),
    }
}

/// Start or stop a recording session remotely.
///
/// Params: `action` (required, "start" or "stop").
/// Sends a command to the GPUI thread and waits for confirmation.
fn handle_record(request: &Request, state: &VoxState) -> Response {
    let params = request.params.as_ref().unwrap_or(&serde_json::Value::Null);

    let action = match params.get("action").and_then(|v| v.as_str()) {
        Some(action) => action,
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INVALID_PARAMS,
                "missing required param: 'action'",
            );
        }
    };

    if !matches!(state.readiness(), AppReadiness::Ready) {
        return Response::error(
            request.id,
            protocol::error_code::NOT_READY,
            "app is not ready — models may still be loading",
        );
    }

    match action {
        "start" => handle_record_start(request, state),
        "stop" => handle_record_stop(request, state),
        unknown => Response::error(
            request.id,
            protocol::error_code::INVALID_PARAMS,
            format!("invalid action: '{unknown}' (expected 'start' or 'stop')"),
        ),
    }
}

/// Send a start-recording command to the GPUI thread.
fn handle_record_start(request: &Request, state: &VoxState) -> Response {
    if state.is_recording() {
        return Response::error(
            request.id,
            protocol::error_code::ALREADY_RECORDING,
            "already recording",
        );
    }

    let cmd_tx = match state.diagnostics_cmd_sender() {
        Some(tx) => tx,
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INTERNAL_ERROR,
                "diagnostics command channel not available",
            );
        }
    };

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    if cmd_tx
        .send(DiagnosticsCommand::StartRecording { reply: reply_tx })
        .is_err()
    {
        return Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            "failed to send start recording command",
        );
    }

    match reply_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => Response::success(request.id, json!({"ok": true})),
        Ok(Err(err)) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            format!("recording failed to start: {err}"),
        ),
        Err(_) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            "recording start timed out",
        ),
    }
}

/// Send a stop-recording command to the GPUI thread.
fn handle_record_stop(request: &Request, state: &VoxState) -> Response {
    if !state.is_recording() {
        return Response::error(
            request.id,
            protocol::error_code::NOT_RECORDING,
            "not recording",
        );
    }

    let cmd_tx = match state.diagnostics_cmd_sender() {
        Some(tx) => tx,
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INTERNAL_ERROR,
                "diagnostics command channel not available",
            );
        }
    };

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    if cmd_tx
        .send(DiagnosticsCommand::StopRecording { reply: reply_tx })
        .is_err()
    {
        return Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            "failed to send stop recording command",
        );
    }

    match reply_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => Response::success(request.id, json!({"ok": true})),
        Ok(Err(err)) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            format!("recording failed to stop: {err}"),
        ),
        Err(_) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            "recording stop timed out",
        ),
    }
}

/// Valid event types for the subscribe method.
const VALID_EVENT_TYPES: &[&str] = &["pipeline_state", "audio_rms", "transcript"];

/// Handle the subscribe method by transitioning to event-streaming mode.
///
/// Validates the requested event types, sends an initial ack response, then
/// enters a two-thread streaming loop: the writer thread forwards events from
/// broadcast channels, while the reader thread watches for an unsubscribe
/// message or client disconnect.
pub fn handle_subscribe_streaming(
    request: &Request,
    state: &VoxState,
    reader: BufReader<UnixStream>,
    mut writer: BufWriter<UnixStream>,
) {
    let params = request.params.as_ref().unwrap_or(&serde_json::Value::Null);

    let events = match params.get("events").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => {
            let response = Response::error(
                request.id,
                protocol::error_code::INVALID_PARAMS,
                "missing required param: 'events' (array of event types)",
            );
            let _ = write_event_line(&mut writer, &response);
            return;
        }
    };

    if events.is_empty() {
        let response = Response::error(
            request.id,
            protocol::error_code::INVALID_PARAMS,
            "'events' array must not be empty",
        );
        let _ = write_event_line(&mut writer, &response);
        return;
    }

    let mut subscribed: HashSet<String> = HashSet::new();
    for event_val in events {
        let event_name = match event_val.as_str() {
            Some(name) => name,
            None => {
                let response = Response::error(
                    request.id,
                    protocol::error_code::INVALID_PARAMS,
                    "each element of 'events' must be a string",
                );
                let _ = write_event_line(&mut writer, &response);
                return;
            }
        };
        if !VALID_EVENT_TYPES.contains(&event_name) {
            let response = Response::error(
                request.id,
                protocol::error_code::INVALID_PARAMS,
                format!(
                    "unknown event type: '{event_name}' (valid: {})",
                    VALID_EVENT_TYPES.join(", ")
                ),
            );
            let _ = write_event_line(&mut writer, &response);
            return;
        }
        subscribed.insert(event_name.to_string());
    }

    // Send initial ack
    let subscribed_list: Vec<&str> = subscribed.iter().map(|s| s.as_str()).collect();
    let ack = Response::success(request.id, json!({"subscribed": subscribed_list}));
    if write_event_line(&mut writer, &ack).is_err() {
        return;
    }

    // Shared shutdown flag between reader and writer threads
    let shutdown = Arc::new(AtomicBool::new(false));

    // Always subscribe to state broadcast (needed for RMS gating even if client
    // didn't request pipeline_state events)
    let mut state_rx = state.state_broadcast_subscribe();

    // Subscribe to transcript broadcast if requested
    let want_transcripts = subscribed.contains("transcript");
    let mut transcript_rx = if want_transcripts {
        Some(state.transcript_broadcast_subscribe())
    } else {
        None
    };

    let want_pipeline_state = subscribed.contains("pipeline_state");
    let want_rms = subscribed.contains("audio_rms");

    // Spawn reader thread to watch for unsubscribe or disconnect
    let reader_shutdown = Arc::clone(&shutdown);
    let reader_handle = std::thread::Builder::new()
        .name("diagnostics-subscribe-reader".into())
        .spawn(move || {
            subscribe_reader_loop(reader, reader_shutdown);
        });

    // Initialize RMS gating from current pipeline state so mid-session
    // subscribers immediately receive audio_rms events if already listening.
    let mut rms_active = matches!(state.pipeline_state(), PipelineState::Listening);
    let poll_interval = std::time::Duration::from_millis(33); // ~30 Hz

    while !shutdown.load(Ordering::Acquire) {
        // Try to receive pipeline state events (non-blocking)
        match state_rx.try_recv() {
            Ok(pipeline_state) => {
                // Update RMS gating based on pipeline state
                match &pipeline_state {
                    PipelineState::Listening => rms_active = true,
                    PipelineState::Idle
                    | PipelineState::Error { .. }
                    | PipelineState::InjectionFailed { .. } => rms_active = false,
                    _ => {} // Processing/Injecting — keep current rms_active
                }

                // Forward pipeline_state event if client subscribed
                if want_pipeline_state {
                    let event = Event {
                        event: "pipeline_state".into(),
                        data: pipeline_state_to_json(&pipeline_state),
                    };
                    if write_event_line(&mut writer, &event).is_err() {
                        shutdown.store(true, Ordering::Release);
                        break;
                    }
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                tracing::debug!(skipped, "subscribe: state broadcast lagged");
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {}
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                shutdown.store(true, Ordering::Release);
                break;
            }
        }

        // Try to receive transcript events (non-blocking)
        if let Some(ref mut rx) = transcript_rx {
            match rx.try_recv() {
                Ok(transcript) => {
                    let event = Event {
                        event: "transcript".into(),
                        data: json!({
                            "raw": transcript.raw,
                            "polished": transcript.polished,
                            "latency_ms": transcript.latency_ms,
                        }),
                    };
                    if write_event_line(&mut writer, &event).is_err() {
                        shutdown.store(true, Ordering::Release);
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Lagged(skipped)) => {
                    tracing::debug!(skipped, "subscribe: transcript broadcast lagged");
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => {}
                Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                    shutdown.store(true, Ordering::Release);
                    break;
                }
            }
        }

        // Poll RMS at ~30 Hz when recording is active
        if want_rms && rms_active {
            let rms = state.latest_rms();
            let event = Event {
                event: "audio_rms".into(),
                data: json!({"rms": rms}),
            };
            if write_event_line(&mut writer, &event).is_err() {
                shutdown.store(true, Ordering::Release);
                break;
            }
        }

        std::thread::sleep(poll_interval);
    }

    // Wait for reader thread to finish
    if let Ok(handle) = reader_handle {
        let _ = handle.join();
    }
}

/// Reader loop for subscribe connections: watches for unsubscribe or disconnect.
///
/// Sets a 500ms read timeout on the stream so the loop can check the shutdown
/// flag periodically without blocking forever on `read_line`.
fn subscribe_reader_loop(mut reader: BufReader<UnixStream>, shutdown: Arc<AtomicBool>) {
    // Set read timeout so we can check shutdown flag periodically
    let _ = reader
        .get_ref()
        .set_read_timeout(Some(std::time::Duration::from_millis(500)));

    let mut line_buf = String::new();
    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }

        line_buf.clear();
        match reader.read_line(&mut line_buf) {
            Ok(0) => break, // EOF
            Ok(_) => {
                // Parse raw JSON to check for "unsubscribe" method
                // (unsubscribe messages have no `id` field per protocol)
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line_buf.trim()) {
                    if value.get("method").and_then(|v| v.as_str()) == Some("unsubscribe") {
                        tracing::debug!("subscribe: client sent unsubscribe");
                        shutdown.store(true, Ordering::Release);
                        break;
                    }
                }
            }
            Err(ref err) if err.kind() == std::io::ErrorKind::TimedOut
                || err.kind() == std::io::ErrorKind::WouldBlock => {
                continue; // Timeout — loop back to check shutdown flag
            }
            Err(_) => break, // Real I/O error
        }
    }
    shutdown.store(true, Ordering::Release);
}

/// Convert a PipelineState to a JSON value for event payloads.
fn pipeline_state_to_json(state: &PipelineState) -> serde_json::Value {
    match state {
        PipelineState::Idle => json!({"state": "idle"}),
        PipelineState::Listening => json!({"state": "listening"}),
        PipelineState::Processing { raw_text } => {
            json!({"state": "processing", "raw_text": raw_text})
        }
        PipelineState::Injecting { polished_text } => {
            json!({"state": "injecting", "polished_text": polished_text})
        }
        PipelineState::Error { message } => {
            json!({"state": "error", "message": message})
        }
        PipelineState::InjectionFailed {
            polished_text,
            error,
        } => {
            json!({"state": "injection_failed", "polished_text": polished_text, "error": error})
        }
    }
}

/// Serialize any serializable value as a JSON line and write it.
fn write_event_line<T: serde::Serialize>(
    writer: &mut BufWriter<UnixStream>,
    value: &T,
) -> std::io::Result<()> {
    let json = serde_json::to_string(value)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    write!(writer, "{json}\n")?;
    writer.flush()
}

/// Capture a screenshot of the specified window.
///
/// Sends a `CaptureScreenshot` command to the GPUI thread (which has access
/// to window handles), waits for the PNG bytes via a oneshot reply channel.
fn handle_screenshot(request: &Request, state: &VoxState) -> Response {
    let window = request
        .params
        .as_ref()
        .and_then(|p| p.get("window"))
        .and_then(|v| v.as_str())
        .unwrap_or("overlay")
        .to_string();

    if window != "overlay" && window != "settings" {
        return Response::error(
            request.id,
            protocol::error_code::INVALID_PARAMS,
            format!("unknown window: '{window}'"),
        );
    }

    let cmd_tx = match state.diagnostics_cmd_sender() {
        Some(tx) => tx,
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INTERNAL_ERROR,
                "diagnostics command channel not available",
            );
        }
    };

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    let cmd = DiagnosticsCommand::CaptureScreenshot {
        window: window.clone(),
        reply: reply_tx,
    };

    if cmd_tx.send(cmd).is_err() {
        return Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            "failed to send screenshot command",
        );
    }

    match reply_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(png_bytes)) => {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
            Response::success(
                request.id,
                json!({
                    "format": "png",
                    "data": encoded,
                }),
            )
        }
        Ok(Err(err)) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            format!("screenshot failed: {err}"),
        ),
        Err(_) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            "screenshot timed out",
        ),
    }
}

/// Quit the Vox application gracefully via the GPUI thread.
///
/// Sends a quit command to the foreground thread and waits for acknowledgement.
fn handle_quit(request: &Request, state: &VoxState) -> Response {
    let cmd_tx = match state.diagnostics_cmd_sender() {
        Some(tx) => tx,
        None => {
            return Response::error(
                request.id,
                protocol::error_code::INTERNAL_ERROR,
                "diagnostics command channel not available",
            );
        }
    };

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    let cmd = DiagnosticsCommand::Quit { reply: reply_tx };

    if cmd_tx.send(cmd).is_err() {
        return Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            "failed to send quit command",
        );
    }

    match reply_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(Ok(())) => Response::success(request.id, json!({"ok": true})),
        Ok(Err(err)) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            format!("quit failed: {err}"),
        ),
        Err(_) => Response::error(
            request.id,
            protocol::error_code::INTERNAL_ERROR,
            "quit timed out",
        ),
    }
}
