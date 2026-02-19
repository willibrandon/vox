# Feature 009: Application State & Settings

**Status:** Not Started
**Dependencies:** 001-workspace-scaffolding
**Design Reference:** Sections 10 (Settings & Configuration), 14 (Security & Privacy)
**Estimated Scope:** VoxState, settings schema, SQLite database, platform data directories

---

## Overview

Implement the global application state and settings persistence layer. VoxState is the central state object that holds all runtime state and is accessible from anywhere in the app via GPUI's `cx.global::<VoxState>()`. Settings are stored as a JSON file. Transcript history and dictionary data live in SQLite. All data stays local — no cloud sync, no telemetry.

---

## Requirements

### FR-001: VoxState (Global Application State)

```rust
// crates/vox_core/src/state.rs

use parking_lot::RwLock;
use std::sync::Arc;

pub struct VoxState {
    /// User settings (persisted to JSON)
    settings: RwLock<Settings>,
    /// SQLite database for dictionary and history
    db: rusqlite::Connection,
    /// Application readiness (downloading, loading, ready)
    readiness: RwLock<AppReadiness>,
    /// Pipeline state (idle, listening, processing, etc.)
    pipeline_state: RwLock<PipelineState>,
    /// Tokio runtime for async operations
    tokio_runtime: tokio::runtime::Runtime,
    /// Data directory path
    data_dir: PathBuf,
}
```

VoxState must implement GPUI's `Global` trait:

```rust
impl gpui::Global for VoxState {}
```

**Thread safety:** Uses `parking_lot::RwLock` for interior mutability. RwLock preferred over Mutex because UI reads vastly outnumber writes.

### FR-002: AppReadiness State Machine

```rust
#[derive(Clone, Debug)]
pub enum AppReadiness {
    /// Models not found, downloading automatically
    Downloading {
        vad_progress: DownloadProgress,
        whisper_progress: DownloadProgress,
        llm_progress: DownloadProgress,
    },
    /// All models downloaded, loading into GPU memory
    Loading { stage: String },
    /// Full pipeline operational (VAD + GPU ASR + GPU LLM)
    Ready,
}
```

Every state has a corresponding UI representation. No state is invisible or broken. The hotkey works in every state — if the pipeline is not ready, the overlay shows why.

### FR-003: Settings Schema

```rust
// crates/vox_core/src/config/settings.rs

#[derive(Serialize, Deserialize, Clone)]
pub struct Settings {
    // Audio
    pub input_device: Option<String>,
    pub noise_gate: f32,              // 0.0–1.0, default 0.0

    // VAD
    pub vad_threshold: f32,           // 0.0–1.0, default 0.5
    pub min_silence_ms: u32,          // default 500
    pub min_speech_ms: u32,           // default 250

    // ASR
    pub language: String,             // "en"
    pub whisper_model: String,        // model filename

    // LLM
    pub llm_model: String,            // model filename
    pub temperature: f32,             // 0.0–1.0, default 0.1
    pub remove_fillers: bool,         // default true
    pub course_correction: bool,      // default true
    pub punctuation: bool,            // default true

    // Hotkey
    pub activation_hotkey: String,    // default "CapsLock"
    pub hold_to_talk: bool,           // true = push-to-talk, false = toggle
    pub hands_free_double_press: bool,// default true

    // Appearance
    pub overlay_position: OverlayPosition,
    pub overlay_opacity: f32,         // 0.0–1.0, default 0.85
    pub show_raw_transcript: bool,    // default false
    pub theme: ThemeMode,             // System, Light, Dark

    // Advanced
    pub max_segment_ms: u32,          // default 10000
    pub overlap_ms: u32,              // default 1000
    pub command_prefix: String,       // default "hey vox"
}

#[derive(Serialize, Deserialize, Clone)]
pub enum OverlayPosition {
    TopCenter,
    TopRight,
    BottomCenter,
    BottomRight,
    Custom { x: f32, y: f32 },
}

#[derive(Serialize, Deserialize, Clone)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            input_device: None,
            noise_gate: 0.0,
            vad_threshold: 0.5,
            min_silence_ms: 500,
            min_speech_ms: 250,
            language: "en".into(),
            whisper_model: "ggml-large-v3-turbo-q5_0.bin".into(),
            llm_model: "Qwen2.5-3B-Instruct-Q4_K_M.gguf".into(),
            temperature: 0.1,
            remove_fillers: true,
            course_correction: true,
            punctuation: true,
            activation_hotkey: "CapsLock".into(),
            hold_to_talk: true,
            hands_free_double_press: true,
            overlay_position: OverlayPosition::TopCenter,
            overlay_opacity: 0.85,
            show_raw_transcript: false,
            theme: ThemeMode::Dark,
            max_segment_ms: 10_000,
            overlap_ms: 1_000,
            command_prefix: "hey vox".into(),
        }
    }
}
```

### FR-004: Settings Persistence (JSON)

Settings stored as a JSON file in the platform app data directory:

```
# Windows
%APPDATA%/com.vox.app/settings.json

# macOS
~/Library/Application Support/com.vox.app/settings.json
```

```rust
impl Settings {
    pub fn load(data_dir: &Path) -> Result<Self>;
    pub fn save(&self, data_dir: &Path) -> Result<()>;
}
```

If the settings file doesn't exist, create it with defaults. If the file is corrupt, log a warning and reset to defaults (don't crash).

### FR-005: SQLite Database (rusqlite 0.38)

```rust
pub fn init_database(data_dir: &Path) -> Result<rusqlite::Connection> {
    let db_path = data_dir.join("vox.db");
    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}
```

Schema:

```sql
-- Transcript history
CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY,
    raw_text TEXT NOT NULL,
    polished_text TEXT NOT NULL,
    target_app TEXT DEFAULT '',
    duration_ms INTEGER DEFAULT 0,
    latency_ms INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now'))
);

-- Custom dictionary (see Feature 010)
CREATE TABLE IF NOT EXISTS dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    spoken TEXT NOT NULL UNIQUE,
    written TEXT NOT NULL,
    category TEXT DEFAULT 'general',
    is_command_phrase INTEGER DEFAULT 0,
    use_count INTEGER DEFAULT 0,
    created_at TEXT DEFAULT (datetime('now'))
);
```

**rusqlite 0.38 note:** No `FromSql` for `chrono::DateTime<Utc>`. Use `String` (ISO 8601) for all timestamps.

### FR-006: Data Directory Setup

```rust
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::config_dir().unwrap().join("com.vox.app")
    }
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir().unwrap().join("com.vox.app")
    }
}

pub fn ensure_data_dirs() -> Result<PathBuf> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::create_dir_all(model_dir())?;
    Ok(dir)
}
```

### FR-007: Transcript History

```rust
impl VoxState {
    pub fn save_transcript(&self, entry: &TranscriptEntry) -> Result<()>;
    pub fn get_transcripts(&self, limit: usize, offset: usize) -> Result<Vec<TranscriptEntry>>;
    pub fn search_transcripts(&self, query: &str) -> Result<Vec<TranscriptEntry>>;
    pub fn delete_transcript(&self, id: &str) -> Result<()>;
    pub fn clear_history(&self) -> Result<()>;
}
```

**Secure delete:** "Clear history" performs overwrite + VACUUM on SQLite to prevent data recovery.

### FR-008: Audio Data Policy

- Audio is processed in memory and immediately discarded after transcription
- No audio is written to disk at any point
- Transcript history can be disabled entirely in settings
- "Clear history" performs a secure delete

---

## Acceptance Criteria

- [ ] VoxState initializes successfully as GPUI Global
- [ ] Settings load from JSON, save to JSON
- [ ] Missing settings file creates defaults
- [ ] Corrupt settings file resets to defaults (no crash)
- [ ] SQLite database creates and migrates schema
- [ ] Transcript history CRUD operations work
- [ ] Transcript search works
- [ ] Secure delete clears all data with VACUUM
- [ ] Data directories created automatically on first run
- [ ] Platform-specific paths are correct
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_settings_default` | Default settings have sane values |
| `test_settings_roundtrip` | Save then load preserves all fields |
| `test_settings_corrupt_file` | Corrupt JSON resets to defaults |
| `test_settings_missing_file` | Missing file creates defaults |
| `test_db_schema_creation` | Database schema creates correctly |
| `test_transcript_save_load` | Save and retrieve transcript |
| `test_transcript_search` | Search finds matching transcripts |
| `test_transcript_clear` | Clear deletes all transcripts |
| `test_data_dir_platform` | Correct path on each platform |

---

## Performance Targets

| Metric | Target |
|---|---|
| Settings load | < 10 ms |
| Settings save | < 10 ms |
| Transcript save | < 5 ms |
| Transcript search | < 50 ms (up to 10,000 entries) |
| Database size (10,000 transcripts) | < 10 MB |
