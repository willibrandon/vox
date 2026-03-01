# Data Model: Diagnostics, CLI Tool, and MCP Server

**Feature**: 017-diagnostics-cli-mcp
**Date**: 2026-02-28

## Entities

### DiagnosticsListener

The server-side component that accepts connections and dispatches requests.

| Field | Type | Description |
|-------|------|-------------|
| socket_path | PathBuf | Full path to the UDS socket file (`~/.vox/sockets/{pid}.diagnostics.socket`) |
| shutdown | Arc\<AtomicBool\> | Cooperative shutdown signal for listener and handler threads |
| handle | Mutex\<Option\<JoinHandle\<()\>\>\> | Listener thread handle for join on shutdown |
| active_connections | Arc\<AtomicU32\> | Current connection count (max 4, FR-013) |

**Lifecycle**: Created → Listening → Shutdown

| State | Transition | Trigger |
|-------|-----------|---------|
| Created | → Listening | `DiagnosticsListener::start()` binds socket, spawns accept thread |
| Listening | → Shutdown | `DiagnosticsListener::shutdown()` sets AtomicBool, closes socket |

**Relationships**: Owns 0..4 connection handler threads. Reads from VoxState (Arc). Sends DiagnosticsCommand via command channel to GPUI thread.

---

### Request

A client-to-server message representing a diagnostics operation.

| Field | Type | Description |
|-------|------|-------------|
| id | u64 | Request/response correlation ID (integer, echoed in response) |
| method | String | Method name: status, settings, logs, record, inject_audio, screenshot, subscribe, transcripts |
| params | Option\<serde_json::Value\> | Method-specific parameters (absent for parameterless methods) |

**Validation**: `id` must be a positive integer. `method` must be one of the 8 known methods. `params` validated per-method by handlers.

---

### Response

A server-to-client message returning a result or error for a request.

| Field | Type | Description |
|-------|------|-------------|
| id | u64 | Echoed from the request |
| result | Option\<serde_json::Value\> | Success payload (present on success, absent on error) |
| error | Option\<ErrorInfo\> | Error details (present on error, absent on success) |

**Invariant**: Exactly one of `result` or `error` is present, never both, never neither.

---

### ErrorInfo

Structured error details within a Response.

| Field | Type | Description |
|-------|------|-------------|
| code | i32 | JSON-RPC error code (see Error Codes below) |
| message | String | Human-readable error description |

**Error Codes**:

| Code | Constant | Meaning |
|------|----------|---------|
| -32600 | INVALID_REQUEST | Malformed JSON or missing required fields |
| -32601 | UNKNOWN_METHOD | Method name not recognized |
| -32602 | INVALID_PARAMS | Parameters missing, wrong type, or invalid value |
| -32603 | INTERNAL_ERROR | Internal failure (pipeline crash, I/O error) |
| -32000 | NOT_READY | App still downloading/loading models |
| -32001 | ALREADY_RECORDING | record start when already recording |
| -32002 | NOT_RECORDING | record stop when not recording |
| -32003 | CONNECTION_LIMIT | Max 4 concurrent connections reached |

---

### Event (Subscribe Notification)

A server-to-client push message during an active subscription. No `id` field — these are unsolicited notifications.

| Field | Type | Description |
|-------|------|-------------|
| event | String | Event type: `pipeline_state`, `audio_rms`, `transcript` |
| data | serde_json::Value | Event-specific payload |

**Event Types**:

| Event | Data Fields | Push Condition |
|-------|------------|----------------|
| pipeline_state | state: String, raw_text?: String | On every PipelineState transition |
| audio_rms | rms: f32 | 30 Hz polling during active recording only |
| transcript | raw: String, polished: String, latency_ms: u64 | On transcript completion |

---

### DiagnosticsCommand

Commands sent from the diagnostics handler thread to the GPUI foreground thread via mpsc channel.

| Variant | Fields | Description |
|---------|--------|-------------|
| StartRecording | reply: oneshot::Sender\<Result\<()\>\> | Triggers ToggleRecording if idle |
| StopRecording | reply: oneshot::Sender\<Result\<()\>\> | Triggers ToggleRecording if recording |
| CaptureScreenshot | window: String, reply: oneshot::Sender\<Result\<Vec\<u8\>\>\> | Captures window as PNG bytes |

**Flow**: Diagnostics handler sends command → GPUI 50ms timer polls `try_recv()` → executes on GPUI thread → sends result back via oneshot.

---

### DiagnosticsClient

Shared client used by both `vox-tool` (CLI) and `vox-mcp` (MCP server).

| Field | Type | Description |
|-------|------|-------------|
| stream | BufReader\<UnixStream\> + BufWriter\<UnixStream\> | Buffered UDS connection |
| next_id | AtomicU64 | Auto-incrementing request ID counter |

**Methods**:
- `connect(path: &Path)` — Connect to a specific socket path
- `connect_auto()` — Scan `~/.vox/sockets/*.diagnostics.socket`, connect to the sole instance or error on 0/N
- `request(method, params)` — Send request, read response, return result or error

---

### LogBuffer

Thread-safe ring buffer of log entries accessible from diagnostics threads (bypasses GPUI-bound SharedLogStore).

| Field | Type | Description |
|-------|------|-------------|
| entries | Arc\<parking_lot::RwLock\<VecDeque\<LogEntry\>\>\> | Capped at 10,000 entries |

**LogEntry** (existing struct in log_sink.rs, reused):

| Field | Type | Description |
|-------|------|-------------|
| timestamp | String | ISO 8601 UTC timestamp |
| level | String | Tracing level: trace, debug, info, warn, error |
| target | String | Rust module path (e.g., `vox_core::asr`) |
| message | String | Log message text |

**Population**: LogSink tracing layer pushes to both the existing GPUI mpsc channel AND this LogBuffer on every tracing event.

---

### AudioInjector

Manages synthetic audio injection into a temporary pipeline.

| Field | Type | Description |
|-------|------|-------------|
| samples | Vec\<f32\> | Loaded audio samples (mono f32) |
| sample_rate | u32 | Original sample rate of the loaded audio |

**Methods**:
- `load_wav(path)` — Read WAV file via hound, convert to mono f32, return samples + sample rate
- `load_pcm(base64_data, sample_rate)` — Decode base64 f32 samples
- `run(state: &VoxState)` — Clone ASR + LLM from VoxState, create Pipeline with no-op injector, push all samples into ring buffer, run pipeline to completion, return transcript + latency

**State Machine**:

| State | Transition | Trigger |
|-------|-----------|---------|
| Loading | → Injecting | Audio loaded, pipeline created, samples pushed to ring buffer |
| Injecting | → Complete | Pipeline finishes processing (audio source closed, drain complete) |
| Injecting | → Error | Pipeline failure, model not available |

---

### StatusSnapshot

The response payload for the `status` method.

| Field | Type | Source |
|-------|------|--------|
| pid | u32 | `std::process::id()` |
| readiness | String | VoxState readiness (downloading, loading, ready, error) |
| pipeline_state | String | VoxState pipeline_state (idle, listening, processing) |
| activation_mode | String | Settings activation mode |
| recording | bool | RecordingSession::active.lock().is_some() |
| debug_audio | String | Settings debug_audio level |
| gpu | GpuInfo | VoxState gpu_info |
| models | Map\<String, ModelInfo\> | VoxState model_runtime |
| audio | AudioInfo | VoxState audio device + latest_rms |
| last_latency_ms | Option\<u64\> | VoxState last_latency_ms |

---

## Entity Relationships

```text
DiagnosticsListener
  ├── owns 0..4 connection handler threads
  ├── reads Arc<VoxState>
  │     ├── Settings (RwLock)
  │     ├── TranscriptStore (Arc<Mutex<Connection>>)
  │     ├── LogBuffer (Arc<RwLock<VecDeque>>)
  │     ├── AsrEngine (RwLock<Option>, Clone for injection)
  │     ├── PostProcessor (RwLock<Option>, Clone for injection)
  │     ├── state_broadcast (broadcast::Sender<PipelineState>)
  │     └── transcript_broadcast (broadcast::Sender<TranscriptEvent>)
  ├── sends DiagnosticsCommand → GPUI thread (mpsc)
  └── creates AudioInjector (per inject_audio request)

DiagnosticsClient
  ├── used by vox-tool (CLI binary)
  └── used by vox-mcp (MCP server binary)

VoxMcp (MCP Server)
  ├── owns DiagnosticsClient
  └── exposes 9 #[tool] methods (no subscribe)
```

## Settings Field Types (for FR-004 validation)

| Key | Expected Type | SettingType Enum |
|-----|--------------|-----------------|
| vad_threshold | f32 | Float |
| vad_min_silence_ms | u32 | Integer |
| vad_min_speech_ms | u32 | Integer |
| activation_mode | String | String |
| hotkey | String | String |
| asr_language | String | String |
| llm_system_prompt | String | String |
| llm_temperature | f32 | Float |
| overlay_opacity | f32 | Float |
| overlay_position_x | f32 | Float |
| overlay_position_y | f32 | Float |
| debug_audio | String | String |
| inject_delay_ms | u32 | Integer |
| audio_device | String | String |

Diagnostics handler validates `serde_json::Value` variant against expected `SettingType` before calling `update_settings()`. Mismatch returns error code -32602 with expected type.
