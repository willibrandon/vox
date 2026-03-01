# Diagnostics Protocol Contract

**Feature**: 017-diagnostics-cli-mcp
**Date**: 2026-02-28
**Transport**: Unix Domain Sockets (UDS) at `~/.vox/sockets/{pid}.diagnostics.socket`
**Encoding**: Newline-delimited JSON (one JSON object per line, `\n` terminated)

## Wire Format

### Request

```json
{"id": <u64>, "method": "<string>", "params": <object|null>}
```

- `id`: Positive integer, echoed in response for correlation
- `method`: One of: `status`, `settings`, `logs`, `record`, `inject_audio`, `screenshot`, `subscribe`, `transcripts`
- `params`: Method-specific parameters object, or omitted/null for parameterless methods

### Success Response

```json
{"id": <u64>, "result": <value>}
```

### Error Response

```json
{"id": <u64>, "error": {"code": <i32>, "message": "<string>"}}
```

### Event Notification (subscribe only)

```json
{"event": "<string>", "data": <object>}
```

No `id` field — server-initiated push notifications during active subscriptions.

---

## Methods

### `status`

Returns a full application state snapshot.

**Parameters**: None

**Request**:
```json
{"id": 1, "method": "status"}
```

**Response**:
```json
{"id": 1, "result": {
  "pid": 12345,
  "readiness": "ready",
  "pipeline_state": "idle",
  "activation_mode": "hold-to-talk",
  "recording": false,
  "debug_audio": "off",
  "gpu": {
    "name": "NVIDIA GeForce RTX 4090",
    "vram_bytes": 25769803776,
    "platform": "cuda"
  },
  "models": {
    "silero_vad_v5.onnx": {"state": "loaded", "vram_bytes": null},
    "ggml-large-v3-turbo-q5_0.bin": {"state": "loaded", "vram_bytes": 601882624},
    "qwen2.5-3b-instruct-q4_k_m.gguf": {"state": "loaded", "vram_bytes": 2362232832}
  },
  "audio": {
    "device": "Microphone (Realtek)",
    "sample_rate": 48000,
    "rms": 0.023
  },
  "last_latency_ms": 187
}}
```

**Result Fields**:

| Field | Type | Source | Always Present |
|-------|------|--------|---------------|
| pid | u32 | `std::process::id()` | Yes |
| readiness | string | VoxState readiness enum (downloading, loading, ready, error) | Yes |
| pipeline_state | string | VoxState pipeline state (idle, listening, processing) | Yes |
| activation_mode | string | Settings.activation_mode | Yes |
| recording | bool | RecordingSession::active.lock().is_some() | Yes |
| debug_audio | string | Settings.debug_audio | Yes |
| gpu.name | string | VoxState gpu_info | Yes |
| gpu.vram_bytes | u64 | VoxState gpu_info | Yes |
| gpu.platform | string | "cuda" or "metal" | Yes |
| models | object | VoxState model_runtime (keyed by filename) | Yes |
| models.*.state | string | "downloaded", "loading", "loaded" | Yes |
| models.*.vram_bytes | u64 or null | null for CPU-only models (Silero) | Yes |
| audio.device | string | Current audio device name | Yes |
| audio.sample_rate | u32 | Device sample rate | Yes |
| audio.rms | f32 | Latest RMS level (0.0 if not recording) | Yes |
| last_latency_ms | u64 or null | null if no transcription has occurred | Yes |

**Errors**: None (always succeeds, works in all readiness states per FR-023).

---

### `settings`

Read or write application settings.

**Parameters**:

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| action | string | Yes | `"get"` or `"set"` |
| key | string | No | Setting key (if omitted with "get", returns all) |
| value | any | Conditional | Required when action is "set" |

**Read All Settings**:
```json
{"id": 2, "method": "settings", "params": {"action": "get"}}
```
```json
{"id": 2, "result": {
  "vad_threshold": 0.5,
  "activation_mode": "hold-to-talk",
  "hotkey": "ctrl+shift+space",
  ...
}}
```

**Read One Setting**:
```json
{"id": 3, "method": "settings", "params": {"action": "get", "key": "vad_threshold"}}
```
```json
{"id": 3, "result": {"vad_threshold": 0.5}}
```

**Write Setting**:
```json
{"id": 4, "method": "settings", "params": {"action": "set", "key": "vad_threshold", "value": 0.6}}
```
```json
{"id": 4, "result": {"ok": true}}
```

**Errors**:

| Condition | Code | Message |
|-----------|------|---------|
| Missing action | -32602 | "missing required parameter: action" |
| Invalid action | -32602 | "invalid action: must be 'get' or 'set'" |
| Set without key | -32602 | "missing required parameter: key" |
| Set without value | -32602 | "missing required parameter: value" |
| Unknown key | -32602 | "unknown setting: '{key}'" |
| Type mismatch | -32602 | "invalid type for '{key}': expected {expected}, got {actual}" |
| Save failure | -32603 | "failed to save settings: {reason}" |

---

### `logs`

Retrieve recent log entries.

**Parameters**:

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| count | u32 | No | 50 | Number of entries to return |
| min_level | string | No | "trace" | Minimum severity: trace, debug, info, warn, error |

**Request**:
```json
{"id": 5, "method": "logs", "params": {"count": 20, "min_level": "warn"}}
```

**Response**:
```json
{"id": 5, "result": {"entries": [
  {"timestamp": "2026-02-28T10:30:45Z", "level": "warn", "target": "vox_core::asr", "message": "transcription timeout after 500ms"},
  {"timestamp": "2026-02-28T10:30:44Z", "level": "error", "target": "vox_core::pipeline", "message": "segment processing failed"}
]}}
```

**Errors**:

| Condition | Code | Message |
|-----------|------|---------|
| Invalid min_level | -32602 | "invalid log level: '{value}'" |

---

### `transcripts`

Retrieve recent transcript history from the SQLite store.

**Parameters**:

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| count | u32 | No | 10 | Number of entries to return |

**Request**:
```json
{"id": 6, "method": "transcripts", "params": {"count": 5}}
```

**Response**:
```json
{"id": 6, "result": {"entries": [
  {
    "timestamp": "2026-02-28T10:30:45Z",
    "raw": "hello world",
    "polished": "Hello, world.",
    "latency_ms": 187
  }
]}}
```

**Errors**: None (returns empty entries if no transcripts exist).

---

### `record`

Start or stop a recording session.

**Parameters**:

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| action | string | Yes | `"start"` or `"stop"` |

**Start Recording**:
```json
{"id": 7, "method": "record", "params": {"action": "start"}}
```
```json
{"id": 7, "result": {"ok": true}}
```

**Stop Recording**:
```json
{"id": 8, "method": "record", "params": {"action": "stop"}}
```
```json
{"id": 8, "result": {"ok": true}}
```

**Errors**:

| Condition | Code | Message |
|-----------|------|---------|
| App not ready | -32000 | "app not ready: models still loading" |
| Already recording | -32001 | "already recording" |
| Not recording | -32002 | "not recording" |
| Missing action | -32602 | "missing required parameter: action" |
| Invalid action | -32602 | "invalid action: must be 'start' or 'stop'" |

---

### `inject_audio`

Inject audio into the full pipeline and return the transcript.

**Parameters (WAV file)**:

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| path | string | Conditional | Path to WAV file on disk |

**Parameters (Raw PCM)**:

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| pcm_base64 | string | Conditional | Base64-encoded f32 mono samples |
| sample_rate | u32 | Conditional | Sample rate of the PCM data (required with pcm_base64) |

Exactly one of `path` or `pcm_base64` must be provided.

**Inject WAV File**:
```json
{"id": 9, "method": "inject_audio", "params": {"path": "/tmp/test_speech.wav"}}
```

**Inject Raw PCM**:
```json
{"id": 10, "method": "inject_audio", "params": {"pcm_base64": "AAAAAAAAAIA/...", "sample_rate": 16000}}
```

**Response**:
```json
{"id": 9, "result": {
  "raw_transcript": "hello world",
  "polished_text": "Hello, world.",
  "latency_ms": 245,
  "injected": true
}}
```

**Behavior**:
- Blocking operation on the connection (FR-030). Client connection is occupied for the duration.
- Fast-forward by default (FR-010). All samples pushed to ring buffer immediately.
- Exercises full pipeline: VAD → ASR → LLM (FR-009). No keystroke injection.
- Audio fed at original sample rate; pipeline resamples internally (FR-024).
- Can run concurrently with live recording (FR-026). Clones Arc model handles.

**Errors**:

| Condition | Code | Message |
|-----------|------|---------|
| App not ready | -32000 | "app not ready: models still loading" |
| No path or pcm_base64 | -32602 | "must provide either 'path' or 'pcm_base64'" |
| Both path and pcm_base64 | -32602 | "provide either 'path' or 'pcm_base64', not both" |
| pcm_base64 without sample_rate | -32602 | "sample_rate required with pcm_base64" |
| File not found | -32602 | "file not found: '{path}'" |
| Invalid WAV format | -32602 | "unsupported audio format: {reason}" |
| Corrupt file | -32602 | "corrupt audio file: {reason}" |
| Empty audio (0 samples) | -32602 | "audio contains 0 samples" |
| Invalid base64 | -32602 | "invalid base64 encoding: {reason}" |
| Pipeline failure | -32603 | "pipeline error: {reason}" |

---

### `screenshot`

Capture a window screenshot as PNG.

**Parameters**:

| Param | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| window | string | No | "overlay" | Window to capture: "overlay" or "settings" |

**Request**:
```json
{"id": 11, "method": "screenshot", "params": {"window": "overlay"}}
```

**Response**:
```json
{"id": 11, "result": {"format": "png", "data": "iVBORw0KGgo..."}}
```

**Result Fields**:

| Field | Type | Description |
|-------|------|-------------|
| format | string | Always "png" |
| data | string | Base64-encoded PNG image data |

**Errors**:

| Condition | Code | Message |
|-----------|------|---------|
| Invalid window name | -32602 | "unknown window: '{name}'" |
| Window not visible | -32603 | "window '{name}' is not open or not visible" |
| Capture failure | -32603 | "screenshot capture failed: {reason}" |

---

### `subscribe`

Subscribe to real-time pipeline events. Converts the connection from request/response to event streaming mode.

**Parameters**:

| Param | Type | Required | Description |
|-------|------|----------|-------------|
| events | array\<string\> | Yes | Event types to subscribe to: "pipeline_state", "audio_rms", "transcript" |

**Request**:
```json
{"id": 13, "method": "subscribe", "params": {"events": ["pipeline_state", "audio_rms", "transcript"]}}
```

**Initial Response** (acknowledges subscription):
```json
{"id": 13, "result": {"subscribed": ["pipeline_state", "audio_rms", "transcript"]}}
```

**Subsequent Event Notifications** (no id):
```json
{"event": "pipeline_state", "data": {"state": "listening"}}
{"event": "audio_rms", "data": {"rms": 0.045}}
{"event": "pipeline_state", "data": {"state": "processing", "raw_text": "hello world"}}
{"event": "transcript", "data": {"raw": "hello world", "polished": "Hello, world.", "latency_ms": 187}}
{"event": "pipeline_state", "data": {"state": "idle"}}
```

**Unsubscribe** (client sends during active subscription per FR-028):
```json
{"method": "unsubscribe"}
```

The server stops pushing events and the connection returns to normal request/response mode.

**Threading**: Subscribe connections use 2 threads (reader thread for unsubscribe commands, writer thread for event push). This means 4 subscribe connections = 8 handler threads + 1 listener = 9 total.

**RMS Polling**: Audio RMS events are only pushed during active recording at 30 Hz. No RMS events when idle (FR-012 note).

**Errors**:

| Condition | Code | Message |
|-----------|------|---------|
| Empty events array | -32602 | "events array must not be empty" |
| Unknown event type | -32602 | "unknown event type: '{type}'" |

**Not available via MCP** (FR-032): The MCP server does not expose a subscribe tool. Event streaming is only available through direct UDS connections (CLI tool or custom clients).

---

## Error Code Summary

| Code | Name | Usage |
|------|------|-------|
| -32600 | INVALID_REQUEST | Malformed JSON, missing id, non-JSON input |
| -32601 | UNKNOWN_METHOD | Method name not recognized |
| -32602 | INVALID_PARAMS | Missing/invalid/wrong-type parameters |
| -32603 | INTERNAL_ERROR | Pipeline crash, I/O failure, capture failure |
| -32000 | NOT_READY | App still downloading/loading models |
| -32001 | ALREADY_RECORDING | record start when recording is active |
| -32002 | NOT_RECORDING | record stop when not recording |
| -32003 | CONNECTION_LIMIT | Max 4 concurrent connections reached |

---

## Connection Lifecycle

1. Client connects to UDS socket at `~/.vox/sockets/{pid}.diagnostics.socket`
2. If connection limit (4) reached: server sends error response with code -32003, closes connection
3. Client sends newline-terminated JSON request
4. Server parses, dispatches to handler, sends newline-terminated JSON response
5. Repeat steps 3-4 for request/response methods
6. For subscribe: after initial response, server pushes events until unsubscribe or disconnect
7. Client disconnects (or server shuts down): connection cleaned up, active_connections decremented

---

## MCP Tool Mapping

| MCP Tool | Diagnostics Method | Notes |
|----------|--------------------|-------|
| vox_status | status | No params |
| vox_settings_get | settings (action=get) | Optional key param |
| vox_settings_set | settings (action=set) | key + value params |
| vox_logs | logs | Optional count, level params |
| vox_record_start | record (action=start) | No params |
| vox_record_stop | record (action=stop) | No params |
| vox_inject_audio | inject_audio | path param |
| vox_screenshot | screenshot | Optional window param |
| vox_transcripts | transcripts | Optional count param |

No `vox_subscribe` tool (FR-032).
