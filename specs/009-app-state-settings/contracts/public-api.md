# Public API Contract: Application State & Settings

**Feature Branch**: `009-app-state-settings`
**Date**: 2026-02-21

## Module: `vox_core::state`

### VoxState

```rust
/// Central application state accessible via GPUI's Global trait.
///
/// Holds all runtime state: user settings, database connection,
/// application readiness, pipeline state, async runtime, and data
/// directory path. Created once during app initialization and set
/// as a GPUI Global for `cx.global::<VoxState>()` access.
pub struct VoxState { /* fields private */ }

impl gpui::Global for VoxState {}

impl VoxState {
    /// Create and initialize VoxState from a data directory path.
    ///
    /// Creates the data directory if it doesn't exist, loads or creates
    /// settings, initializes the SQLite database with schema, and starts
    /// the tokio runtime. Initial readiness is `AppReadiness::Downloading`
    /// with all models pending.
    pub fn new(data_dir: &Path) -> Result<Self>;

    // --- Settings access ---

    /// Read current settings (acquires read lock).
    pub fn settings(&self) -> parking_lot::RwLockReadGuard<'_, Settings>;

    /// Update settings via closure, then persist to disk.
    pub fn update_settings<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Settings);

    // --- Transcript history ---

    /// Save a transcript entry to the database.
    /// No-op if `settings.save_history` is false.
    pub fn save_transcript(&self, entry: &TranscriptEntry) -> Result<()>;

    /// List transcript entries, newest first (paginated).
    pub fn get_transcripts(&self, limit: usize, offset: usize)
        -> Result<Vec<TranscriptEntry>>;

    /// Search transcripts by text content (raw or polished).
    pub fn search_transcripts(&self, query: &str)
        -> Result<Vec<TranscriptEntry>>;

    /// Delete a single transcript by ID.
    pub fn delete_transcript(&self, id: &str) -> Result<()>;

    /// Securely delete all transcript history.
    /// Overwrites text fields, deletes rows, executes VACUUM.
    pub fn clear_history(&self) -> Result<()>;

    // --- Readiness state ---

    /// Read current application readiness state.
    pub fn readiness(&self) -> AppReadiness;

    /// Update application readiness state.
    pub fn set_readiness(&self, state: AppReadiness);

    // --- Pipeline state ---

    /// Read current pipeline state.
    pub fn pipeline_state(&self) -> PipelineState;

    /// Update pipeline state.
    pub fn set_pipeline_state(&self, state: PipelineState);

    // --- Accessors ---

    /// Get the application data directory path.
    pub fn data_dir(&self) -> &Path;

    /// Get a reference to the tokio runtime.
    pub fn tokio_runtime(&self) -> &tokio::runtime::Runtime;

    /// Get shared reference to TranscriptStore for pipeline use.
    pub fn transcript_store(&self) -> Arc<TranscriptStore>;
}
```

### AppReadiness

```rust
/// Application lifecycle state tracking initialization progress.
///
/// Transitions linearly: Downloading → Loading → Ready. The hotkey
/// responds in every state — if not Ready, the overlay shows why.
#[derive(Clone, Debug)]
pub enum AppReadiness {
    /// Models are being downloaded. Per-model progress is tracked.
    Downloading {
        vad_progress: DownloadProgress,
        whisper_progress: DownloadProgress,
        llm_progress: DownloadProgress,
    },
    /// All models downloaded, loading into GPU memory.
    Loading {
        /// Human-readable description of current loading stage.
        stage: String,
    },
    /// Full pipeline operational. Ready for dictation.
    Ready,
}
```

## Module: `vox_core::config`

### Settings

```rust
/// User-configurable application settings.
///
/// Persisted to JSON at `data_dir/settings.json`. All fields have
/// sensible defaults via `Default` impl. Forward-compatible (ignores
/// unknown fields) and backward-compatible (uses defaults for missing
/// fields) via `#[serde(default)]`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings { /* 23 public fields */ }

impl Default for Settings { /* sensible defaults */ }

impl Settings {
    /// Load settings from the data directory.
    ///
    /// Returns defaults if file doesn't exist. Logs a warning and
    /// returns defaults if file is corrupt (never crashes).
    pub fn load(data_dir: &Path) -> Result<Self>;

    /// Save settings to the data directory using atomic write.
    ///
    /// Writes to a temporary file, then renames to settings.json.
    pub fn save(&self, data_dir: &Path) -> Result<()>;
}
```

### OverlayPosition

```rust
/// Overlay HUD placement on screen.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum OverlayPosition {
    TopCenter,
    TopRight,
    BottomCenter,
    BottomRight,
    Custom { x: f32, y: f32 },
}
```

### ThemeMode

```rust
/// Application theme selection.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}
```

## Module: `vox_core::pipeline::transcript` (extended)

### New Methods on TranscriptStore

```rust
impl TranscriptStore {
    // Existing: open, save, list, prune_older_than, count

    /// Search transcripts by text content.
    ///
    /// Matches against both raw_text and polished_text using SQL LIKE.
    /// Returns results ordered by created_at DESC.
    pub fn search(&self, query: &str) -> Result<Vec<TranscriptEntry>>;

    /// Delete a single transcript by ID.
    pub fn delete(&self, id: &str) -> Result<()>;

    /// Securely delete all transcripts.
    ///
    /// Overwrites text fields with empty strings, deletes all rows,
    /// then executes VACUUM to reclaim free pages.
    pub fn clear_secure(&self) -> Result<()>;
}
```

## Free Functions

### `vox_core::state::data_dir`

```rust
/// Resolve the platform-specific application data directory.
///
/// Windows: `%LOCALAPPDATA%/com.vox.app/`
/// macOS: `~/Library/Application Support/com.vox.app/`
///
/// Does NOT create the directory. Use `ensure_data_dirs()` for that.
pub fn data_dir() -> Result<PathBuf>;
```

### `vox_core::state::ensure_data_dirs`

```rust
/// Create the application data directory and models subdirectory.
///
/// Idempotent — safe to call multiple times.
pub fn ensure_data_dirs() -> Result<PathBuf>;
```

### `vox_core::state::init_database`

```rust
/// Open or create the SQLite database at `data_dir/vox.db`.
///
/// Creates the transcripts and dictionary tables if they don't exist.
/// Returns the wrapped connection for use by TranscriptStore.
pub fn init_database(data_dir: &Path) -> Result<TranscriptStore>;
```
