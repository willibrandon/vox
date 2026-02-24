# Data Model: Settings Window & Panels

**Feature Branch**: `013-settings-window`
**Date**: 2026-02-23

## Existing Entities (in vox_core — modifications noted)

### Settings (config.rs)

Existing struct with 23 fields. This feature adds 4 new fields for window position persistence.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| *...existing 23 fields...* | | | Unchanged |
| `window_x` | `Option<f32>` | `None` | Settings window X position in pixels. None = centered. |
| `window_y` | `Option<f32>` | `None` | Settings window Y position in pixels. None = centered. |
| `window_width` | `Option<f32>` | `None` | Settings window width in pixels. None = default (800). |
| `window_height` | `Option<f32>` | `None` | Settings window height in pixels. None = default (600). |

**Validation**: If saved position is outside current display bounds, reset to `None` (centered). Minimum window size enforced by GPUI `window_min_size` (400x300).

**Serialization**: serde with `#[serde(default)]` on new fields ensures backward compatibility — existing settings.json files without these fields deserialize to `None`.

### TranscriptEntry (pipeline/transcript.rs)

No modifications. Existing fields used directly by History panel:

| Field | Type | UI Usage |
|-------|------|----------|
| `id` | `String` (UUID v4) | Delete target identifier |
| `raw_text` | `String` | Shown when "show raw transcript" enabled |
| `polished_text` | `String` | Primary display text, clipboard copy target |
| `target_app` | `String` | Displayed in entry metadata |
| `duration_ms` | `u32` | Not displayed (processing metric) |
| `latency_ms` | `u32` | Displayed as "XXms" in entry metadata |
| `created_at` | `String` (ISO 8601) | Displayed as formatted timestamp |

### DictionaryEntry (dictionary.rs)

No modifications. Existing fields used directly by Dictionary panel:

| Field | Type | UI Usage |
|-------|------|----------|
| `id` | `i64` | Edit/delete target identifier |
| `spoken` | `String` | Editable inline, search target |
| `written` | `String` | Editable inline, search target |
| `category` | `String` | Editable inline, filter/sort target |
| `is_command_phrase` | `bool` | Toggle control per entry |
| `use_count` | `u64` | Sort target, displayed in entry |
| `created_at` | `String` (ISO 8601) | Not displayed |

### ModelInfo (models.rs)

No modifications. Existing static data used by Model panel:

| Field | Type | UI Usage |
|-------|------|----------|
| `name` | `&'static str` | Model display name |
| `filename` | `&'static str` | Reference filename |
| `url` | `&'static str` | Download source (shown on failure) |
| `sha256` | `&'static str` | Verification (not displayed) |
| `size_bytes` | `u64` | Display as "XX MB" |

### DownloadProgress (models/downloader.rs)

No modifications. Existing enum used by Model panel:

| Variant | Fields | UI Rendering |
|---------|--------|-------------|
| `Pending` | — | "Waiting..." |
| `InProgress` | `bytes_downloaded: u64, bytes_total: u64` | Progress bar with "XX / YY MB" |
| `Complete` | — | Checkmark icon |
| `Failed` | `error: String, manual_url: String` | Error message + Retry button |

## New Entities

### ModelRuntimeInfo (state.rs — new struct in VoxState)

Runtime state for each model, tracked by model name. Stored in `VoxState` as `HashMap<String, ModelRuntimeInfo>`.

| Field | Type | Description |
|-------|------|-------------|
| `state` | `ModelRuntimeState` | Current lifecycle state |
| `vram_bytes` | `Option<u64>` | GPU memory usage when loaded |
| `benchmark` | `Option<BenchmarkResult>` | Inference speed when loaded |
| `custom_path` | `Option<PathBuf>` | Non-default model file path (from swap) |

### ModelRuntimeState (state.rs — new enum)

| Variant | Meaning |
|---------|---------|
| `Missing` | File not on disk |
| `Downloading` | Download in progress (progress tracked by `DownloadProgress`) |
| `Downloaded` | File on disk, not loaded into GPU |
| `Loading` | Currently being loaded into GPU memory |
| `Loaded` | Active in GPU memory, ready for inference |
| `Error(String)` | Load or download failed with error message |

### BenchmarkResult (models.rs — new struct)

| Field | Type | Description |
|-------|------|-------------|
| `metric_name` | `String` | Human-readable metric label (e.g., "tokens/sec", "real-time factor", "inferences/sec") |
| `value` | `f64` | Numeric benchmark value |

### LogEntry (log_sink.rs — new struct in vox_core)

Ephemeral log record captured from the tracing subscriber and displayed in the Log panel.

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | `String` (ISO 8601) | When the event occurred |
| `level` | `LogLevel` | Severity level |
| `target` | `String` | Source component (tracing target, e.g., "vox_core::audio") |
| `message` | `String` | Human-readable log message |

### LogLevel (log_sink.rs — new enum)

Maps directly to `tracing::Level`. Ordered by severity (Error is most severe).

| Variant | Tracing Equivalent | Display Color |
|---------|-------------------|---------------|
| `Error` | `tracing::Level::ERROR` | Red (`log_error` theme color) |
| `Warn` | `tracing::Level::WARN` | Amber (`log_warn` theme color) |
| `Info` | `tracing::Level::INFO` | White (`log_info` theme color) |
| `Debug` | `tracing::Level::DEBUG` | Gray (`log_debug` theme color) |
| `Trace` | `tracing::Level::TRACE` | Dim gray (`log_trace` theme color) |

**Filter semantics** (FR-041): "at or above selected level" means selecting Warn shows Error + Warn. Ordering: Error > Warn > Info > Debug > Trace.

## Entity Relationships

```
VoxState (GPUI Global — singleton)
├── settings: RwLock<Settings>
│   ├── Audio: input_device, noise_gate
│   ├── VAD: vad_threshold, min_silence_ms, min_speech_ms
│   ├── Hotkey: activation_hotkey, hold_to_talk, hands_free_double_press
│   ├── LLM: temperature, remove_fillers, course_correction, punctuation
│   ├── Appearance: theme, overlay_opacity, overlay_position, show_raw_transcript
│   ├── Advanced: max_segment_ms, overlap_ms, command_prefix
│   └── Window: window_x, window_y, window_width, window_height (NEW)
│
├── transcript_store: Arc<TranscriptStore>
│   └── SQLite: TranscriptEntry[] (persistent, queryable)
│
├── dictionary: DictionaryCache
│   └── SQLite + in-memory: DictionaryEntry[] (persistent, cached)
│
├── model_runtime: HashMap<String, ModelRuntimeInfo> (NEW)
│   └── Per-model: state, vram_bytes, benchmark, custom_path
│
└── (implicit) MODELS constant: &[ModelInfo; 3] (static)

LogStore (GPUI Entity — singleton, in vox_ui)
├── entries: VecDeque<LogEntry> (bounded, capacity 10,000)
├── rx: mpsc::UnboundedReceiver<LogEntry>
└── emits: Event::NewLogEntry

LogSink (tracing Layer — in vox_core)
├── tx: mpsc::UnboundedSender<LogEntry>
└── implements: tracing_subscriber::Layer
```

## State Transitions

### Model Lifecycle

```
Missing ──[download starts]──→ Downloading
Downloading ──[complete]──→ Downloaded
Downloading ──[failed]──→ Error
Downloaded ──[load starts]──→ Loading
Loading ──[loaded]──→ Loaded (+ benchmark computed)
Loading ──[failed]──→ Error
Error ──[retry]──→ Downloading
Loaded ──[swap initiated]──→ Loading
```

### Settings Window Lifecycle

```
Closed ──[open action]──→ Open (restore bounds or centered)
Open ──[close button / on_window_should_close]──→ Closed (save bounds)
Open ──[open action again]──→ Open (focus existing, no-op)
```

### Inline Confirmation Lifecycle

```
Idle ──[delete clicked]──→ Confirming (5s timer starts)
Confirming ──[yes clicked]──→ Idle (item deleted)
Confirming ──[no clicked]──→ Idle (cancelled)
Confirming ──[5s timeout]──→ Idle (auto-cancelled)
```
