# Data Model: Application State & Settings

**Feature Branch**: `009-app-state-settings`
**Date**: 2026-02-21

## Entity Diagram

```
VoxState (Global)
├── settings: RwLock<Settings>
│   ├── Audio (2 fields)
│   ├── VAD (3 fields)
│   ├── ASR (2 fields)
│   ├── LLM (4 fields)
│   ├── Hotkey (3 fields)
│   ├── Appearance (4 fields)
│   └── Advanced (3 fields)
├── transcript_store: Arc<TranscriptStore>
│   └── connection: Mutex<Connection> → vox.db
│       ├── transcripts table
│       └── dictionary table
├── readiness: RwLock<AppReadiness>
│   └── uses DownloadProgress (from models::downloader)
├── pipeline_state: RwLock<PipelineState>
│   └── reused from pipeline::state
├── tokio_runtime: Runtime
└── data_dir: PathBuf
```

## Entities

### VoxState

Central application state. Implements GPUI `Global` trait for
`cx.global::<VoxState>()` access from any GPUI context.

| Field | Type | Mutability | Notes |
|-------|------|------------|-------|
| settings | `RwLock<Settings>` | Interior | RwLock for read-heavy access |
| transcript_store | `Arc<TranscriptStore>` | Shared | Shared with pipeline via Arc |
| readiness | `RwLock<AppReadiness>` | Interior | Updated during init lifecycle |
| pipeline_state | `RwLock<PipelineState>` | Interior | Updated by pipeline orchestrator |
| tokio_runtime | `Runtime` | Immutable | Created once at init |
| data_dir | `PathBuf` | Immutable | Platform-specific app data path |

**Traits**: `Global` (gpui)

**Lifecycle**: Created once during app init → set as GPUI Global →
lives for entire app lifetime → dropped on app shutdown.

**Thread safety**: All mutable fields use `parking_lot::RwLock` or
`parking_lot::Mutex` (inside TranscriptStore). VoxState itself is
`Send + Sync` via these wrappers.

### Settings

User-configurable preferences. Persisted to JSON on disk.

| Field | Type | Default | Category |
|-------|------|---------|----------|
| input_device | `Option<String>` | `None` | Audio |
| noise_gate | `f32` | `0.0` | Audio |
| vad_threshold | `f32` | `0.5` | VAD |
| min_silence_ms | `u32` | `500` | VAD |
| min_speech_ms | `u32` | `250` | VAD |
| language | `String` | `"en"` | ASR |
| whisper_model | `String` | `"ggml-large-v3-turbo-q5_0.bin"` | ASR |
| llm_model | `String` | `"Qwen2.5-3B-Instruct-Q4_K_M.gguf"` | LLM |
| temperature | `f32` | `0.1` | LLM |
| remove_fillers | `bool` | `true` | LLM |
| course_correction | `bool` | `true` | LLM |
| punctuation | `bool` | `true` | LLM |
| activation_hotkey | `String` | `"CapsLock"` | Hotkey |
| hold_to_talk | `bool` | `true` | Hotkey |
| hands_free_double_press | `bool` | `true` | Hotkey |
| overlay_position | `OverlayPosition` | `TopCenter` | Appearance |
| overlay_opacity | `f32` | `0.85` | Appearance |
| show_raw_transcript | `bool` | `false` | Appearance |
| theme | `ThemeMode` | `Dark` | Appearance |
| max_segment_ms | `u32` | `10_000` | Advanced |
| overlap_ms | `u32` | `1_000` | Advanced |
| command_prefix | `String` | `"hey vox"` | Advanced |
| save_history | `bool` | `true` | Advanced |

**Total**: 23 fields across 7 categories (Audio: 2, VAD: 3, ASR: 2,
LLM: 5, Hotkey: 3, Appearance: 4, Advanced: 4).

**Note**: `save_history` (23rd field) enables/disables transcript
persistence per FR-014 and US5. When false, `save_transcript()` is a
no-op.

**Traits**: `Serialize`, `Deserialize`, `Clone`, `Debug`, `Default`

**Serde attributes**:
- `#[serde(default)]` on the struct for backward compatibility (missing
  fields get defaults)
- `#[serde(deny_unknown_fields)]` is NOT used — forward compatibility
  requires ignoring unknown fields (FR-015)

**Validation rules**:
- `noise_gate`: clamped to 0.0–1.0
- `vad_threshold`: clamped to 0.0–1.0
- `temperature`: clamped to 0.0–1.0
- `overlay_opacity`: clamped to 0.0–1.0
- `min_silence_ms`: minimum 100
- `min_speech_ms`: minimum 50

**Persistence**: JSON file at `data_dir/settings.json`. Atomic write
(temp file + rename).

### OverlayPosition

Overlay HUD placement on screen.

| Variant | Data | Notes |
|---------|------|-------|
| TopCenter | — | Default |
| TopRight | — | |
| BottomCenter | — | |
| BottomRight | — | |
| Custom | `{ x: f32, y: f32 }` | Normalized 0.0–1.0 coordinates |

**Traits**: `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`

### ThemeMode

Application theme selection.

| Variant | Notes |
|---------|-------|
| System | Follow OS light/dark preference |
| Light | Force light theme |
| Dark | Force dark theme (default) |

**Traits**: `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`

### AppReadiness

Application lifecycle state machine. Tracks initialization progress.

| Variant | Data | Notes |
|---------|------|-------|
| Downloading | `{ vad: DownloadProgress, whisper: DownloadProgress, llm: DownloadProgress }` | Per-model progress |
| Loading | `{ stage: String }` | Human-readable stage name |
| Ready | — | Full pipeline operational |

**State transitions**: `Downloading → Loading → Ready` (linear, no
backward transitions in normal operation).

**DownloadProgress**: Reused from `crate::models::DownloadProgress`
(already defined in `models/downloader.rs`):
- `Pending` — not started
- `InProgress { bytes_downloaded: u64, bytes_total: u64 }` — in progress
- `Complete` — verified
- `Failed { error: String, manual_url: String }` — failed with recovery info

**Traits**: `Clone`, `Debug`

### PipelineState (existing — no changes)

Reused from `pipeline/state.rs`. Already defined and tested.

| Variant | Data | Notes |
|---------|------|-------|
| Idle | — | Waiting for activation |
| Listening | — | Microphone active, VAD processing |
| Processing | `{ raw_text: Option<String> }` | ASR/LLM in progress |
| Injecting | `{ polished_text: String }` | Text being typed |
| Error | `{ message: String }` | Recoverable error |

### TranscriptEntry (existing — no changes)

Reused from `pipeline/transcript.rs`. Already defined and tested.

| Field | Type | SQL Column | Notes |
|-------|------|------------|-------|
| id | `String` | `TEXT PRIMARY KEY` | UUID v4 |
| raw_text | `String` | `TEXT NOT NULL` | Original ASR output |
| polished_text | `String` | `TEXT NOT NULL` | After LLM processing |
| target_app | `String` | `TEXT NOT NULL` | Focused app at injection |
| duration_ms | `u32` | `INTEGER NOT NULL` | Audio segment duration |
| latency_ms | `u32` | `INTEGER NOT NULL` | End-to-end processing time |
| created_at | `String` | `TEXT NOT NULL` | ISO 8601 timestamp |

## Database Schema

**File**: `data_dir/vox.db`

```sql
-- Transcript history (existing schema from pipeline/transcript.rs)
CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY,
    raw_text TEXT NOT NULL,
    polished_text TEXT NOT NULL,
    target_app TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_transcripts_created_at
    ON transcripts(created_at);

-- Custom dictionary (existing schema from dictionary.rs)
CREATE TABLE IF NOT EXISTS dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    term TEXT UNIQUE NOT NULL COLLATE NOCASE,
    replacement TEXT NOT NULL,
    frequency INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
```

**Notes**:
- Timestamps are ISO 8601 strings (rusqlite 0.38 has no FromSql for
  chrono::DateTime).
- The dictionary table uses the existing schema from `dictionary.rs`.
  Feature 010 may evolve this schema.
- No foreign keys between tables.
- No WAL mode explicitly set — SQLite default (journal mode DELETE) is
  sufficient for single-connection access.

## File Layout

```
# Windows: %LOCALAPPDATA%/com.vox.app/
# macOS: ~/Library/Application Support/com.vox.app/

com.vox.app/
├── models/           # ML model files (existing)
│   ├── silero_vad_v5.onnx
│   ├── ggml-large-v3-turbo-q5_0.bin
│   └── Qwen2.5-3B-Instruct-Q4_K_M.gguf
├── settings.json     # User settings (new)
└── vox.db            # SQLite database (new)
```
