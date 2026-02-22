//! Central application state for the Vox dictation engine.
//!
//! Provides [`VoxState`] as the single source of truth for all runtime state,
//! accessible from any GPUI context via `cx.global::<VoxState>()`. Manages
//! user settings, SQLite transcript history, application readiness tracking,
//! pipeline state, and the async runtime.
//!
//! Also provides data directory resolution ([`data_dir`], [`ensure_data_dirs`])
//! and database initialization ([`init_database`]).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::RwLock;

use crate::config::Settings;
use crate::models::DownloadProgress;
use crate::pipeline::state::PipelineState;
use crate::pipeline::transcript::{TranscriptEntry, TranscriptStore};

/// Application lifecycle state tracking initialization progress.
///
/// Transitions linearly: Downloading → Loading → Ready. The hotkey responds
/// in every state — if not Ready, the overlay shows why (download progress,
/// loading stage, or error).
#[derive(Clone, Debug)]
pub enum AppReadiness {
    /// Models are being downloaded. Per-model progress is tracked.
    Downloading {
        /// Silero VAD model download progress.
        vad_progress: DownloadProgress,
        /// Whisper ASR model download progress.
        whisper_progress: DownloadProgress,
        /// Qwen LLM model download progress.
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

/// Central application state accessible via GPUI's Global trait.
///
/// Holds all runtime state: user settings, transcript store, application
/// readiness, pipeline state, async runtime, and data directory path.
/// Created once during app initialization and set as a GPUI Global for
/// `cx.global::<VoxState>()` access.
pub struct VoxState {
    /// User settings, protected by RwLock for concurrent read access.
    settings: RwLock<Settings>,
    /// Shared transcript store (also used by pipeline orchestrator).
    transcript_store: Arc<TranscriptStore>,
    /// Application readiness state machine.
    readiness: RwLock<AppReadiness>,
    /// Current pipeline operational state.
    pipeline_state: RwLock<PipelineState>,
    /// Tokio runtime for async operations (model downloads, etc.).
    tokio_runtime: tokio::runtime::Runtime,
    /// Atomic flag synced from `settings.save_history` for lock-free privacy checks.
    save_history: Arc<AtomicBool>,
    /// Platform-specific application data directory.
    data_dir: PathBuf,
}

impl gpui::Global for VoxState {}

impl VoxState {
    /// Create and initialize VoxState from a data directory path.
    ///
    /// Creates the data directory if it doesn't exist, loads or creates
    /// settings, initializes the SQLite database with schema, and starts
    /// the tokio runtime. Initial readiness is `AppReadiness::Downloading`
    /// with all models pending.
    pub fn new(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("failed to create data directory at {}", data_dir.display()))?;

        let settings_path = data_dir.join("settings.json");
        let first_launch = !settings_path.exists();
        let settings = Settings::load(data_dir)?;
        if first_launch {
            settings.save(data_dir)?;
        }
        let save_history = Arc::new(AtomicBool::new(settings.save_history));
        let transcript_store = Arc::new(init_database(data_dir)?);

        let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("failed to create tokio runtime")?;

        let readiness = AppReadiness::Downloading {
            vad_progress: DownloadProgress::Pending,
            whisper_progress: DownloadProgress::Pending,
            llm_progress: DownloadProgress::Pending,
        };

        Ok(Self {
            settings: RwLock::new(settings),
            transcript_store,
            save_history,
            readiness: RwLock::new(readiness),
            pipeline_state: RwLock::new(PipelineState::Idle),
            tokio_runtime,
            data_dir: data_dir.to_path_buf(),
        })
    }

    // --- Settings access ---

    /// Read current settings (acquires read lock).
    pub fn settings(&self) -> parking_lot::RwLockReadGuard<'_, Settings> {
        self.settings.read()
    }

    /// Update settings via closure, then persist to disk.
    ///
    /// Applies changes to a cloned copy, persists it, and only swaps the
    /// in-memory state on success. If the disk write fails, in-memory state
    /// remains unchanged.
    pub fn update_settings<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut Settings),
    {
        let mut guard = self.settings.write();
        let mut updated = guard.clone();
        f(&mut updated);
        updated.save(&self.data_dir)?;
        self.save_history
            .store(updated.save_history, Ordering::Release);
        *guard = updated;
        Ok(())
    }

    // --- Transcript history ---

    /// Save a transcript entry to the database.
    ///
    /// No-op if `settings.save_history` is false.
    pub fn save_transcript(&self, entry: &TranscriptEntry) -> Result<()> {
        if !self.save_history.load(Ordering::Acquire) {
            return Ok(());
        }
        self.transcript_store.save(entry)
    }

    /// List transcript entries, newest first (paginated).
    pub fn get_transcripts(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<TranscriptEntry>> {
        self.transcript_store.list(limit, offset)
    }

    /// Search transcripts by text content (raw or polished).
    pub fn search_transcripts(
        &self,
        query: &str,
    ) -> Result<Vec<TranscriptEntry>> {
        self.transcript_store.search(query)
    }

    /// Delete a single transcript by ID.
    pub fn delete_transcript(&self, id: &str) -> Result<()> {
        self.transcript_store.delete(id)
    }

    /// Securely delete all transcript history.
    ///
    /// Overwrites text fields, deletes rows, executes VACUUM.
    pub fn clear_history(&self) -> Result<()> {
        self.transcript_store.clear_secure()
    }

    // --- Readiness state ---

    /// Read current application readiness state.
    pub fn readiness(&self) -> AppReadiness {
        self.readiness.read().clone()
    }

    /// Update application readiness state.
    pub fn set_readiness(&self, state: AppReadiness) {
        *self.readiness.write() = state;
    }

    // --- Pipeline state ---

    /// Read current pipeline state.
    pub fn pipeline_state(&self) -> PipelineState {
        self.pipeline_state.read().clone()
    }

    /// Update pipeline state.
    pub fn set_pipeline_state(&self, state: PipelineState) {
        *self.pipeline_state.write() = state;
    }

    // --- Accessors ---

    /// Get the application data directory path.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Get a reference to the tokio runtime.
    pub fn tokio_runtime(&self) -> &tokio::runtime::Runtime {
        &self.tokio_runtime
    }

    /// Create a transcript writer for pipeline use.
    ///
    /// The writer enforces the `save_history` privacy setting on all writes.
    /// Pipeline code should use this instead of accessing TranscriptStore
    /// directly.
    pub fn transcript_writer(&self) -> TranscriptWriter {
        TranscriptWriter::new(
            Arc::clone(&self.transcript_store),
            Arc::clone(&self.save_history),
        )
    }
}

/// Privacy-aware transcript persistence wrapper.
///
/// Wraps `Arc<TranscriptStore>` with a `save_history` flag that gates write
/// operations. Read operations (count, list) are always permitted.
/// Prevents pipeline code from bypassing the privacy setting.
#[derive(Clone)]
pub struct TranscriptWriter {
    store: Arc<TranscriptStore>,
    save_history: Arc<AtomicBool>,
}

impl TranscriptWriter {
    /// Create a new transcript writer.
    pub fn new(store: Arc<TranscriptStore>, save_history: Arc<AtomicBool>) -> Self {
        Self { store, save_history }
    }

    /// Save a transcript entry, respecting the save_history setting.
    ///
    /// Returns `Ok(())` without writing if save_history is disabled.
    pub fn save(&self, entry: &TranscriptEntry) -> Result<()> {
        if !self.save_history.load(Ordering::Acquire) {
            return Ok(());
        }
        self.store.save(entry)
    }

    /// Total number of transcript records (read-only, no privacy gate).
    pub fn count(&self) -> Result<usize> {
        self.store.count()
    }

    /// List transcript entries, newest first (read-only, no privacy gate).
    pub fn list(&self, limit: usize, offset: usize) -> Result<Vec<TranscriptEntry>> {
        self.store.list(limit, offset)
    }
}

/// Resolve the platform-specific application data directory.
///
/// Windows: `%LOCALAPPDATA%/com.vox.app/`
/// macOS: `~/Library/Application Support/com.vox.app/`
///
/// Does NOT create the directory. Use [`ensure_data_dirs`] for that.
pub fn data_dir() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    let base = dirs::data_local_dir();
    #[cfg(target_os = "macos")]
    let base = dirs::data_dir();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = dirs::data_dir();

    let base = base.context("platform data directory not available")?;
    Ok(base.join("com.vox.app"))
}

/// Create the application data directory and models subdirectory.
///
/// Idempotent — safe to call multiple times.
pub fn ensure_data_dirs() -> Result<PathBuf> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create data directory at {}", dir.display()))?;
    let models_dir = dir.join("models");
    std::fs::create_dir_all(&models_dir)
        .with_context(|| format!("failed to create models directory at {}", models_dir.display()))?;
    Ok(dir)
}

/// Open or create the SQLite database at `data_dir/vox.db`.
///
/// Creates the transcripts and dictionary tables if they don't exist.
/// Returns the wrapped connection for use by TranscriptStore.
pub fn init_database(data_dir: &Path) -> Result<TranscriptStore> {
    let db_path = data_dir.join("vox.db");
    let store = TranscriptStore::open(&db_path)?;

    // Dictionary table lives in the same vox.db. A second Connection::open is
    // fine — runs once at startup and keeps TranscriptStore focused on transcripts.
    let conn = rusqlite::Connection::open(&db_path)
        .with_context(|| format!("failed to open database for dictionary schema at {}", db_path.display()))?;
    conn.execute_batch(crate::dictionary::CREATE_TABLE_SQL)
        .context("failed to create dictionary table")?;

    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vox_state_init() {
        let dir = tempfile::tempdir().expect("temp dir");
        let state = VoxState::new(dir.path()).expect("VoxState::new should succeed");

        // Settings file should be created with defaults on first launch
        let settings_path = dir.path().join("settings.json");
        assert!(settings_path.exists(), "settings.json should exist after first launch");

        let settings = state.settings();
        assert_eq!(settings.language, "en");
        assert_eq!(settings.vad_threshold, 0.5);
        drop(settings);

        // Database should exist
        let db_path = dir.path().join("vox.db");
        assert!(db_path.exists(), "vox.db should exist after init");

        // Readiness should start at Downloading
        match state.readiness() {
            AppReadiness::Downloading {
                vad_progress,
                whisper_progress,
                llm_progress,
            } => {
                assert!(
                    matches!(vad_progress, DownloadProgress::Pending),
                    "VAD should be Pending"
                );
                assert!(
                    matches!(whisper_progress, DownloadProgress::Pending),
                    "Whisper should be Pending"
                );
                assert!(
                    matches!(llm_progress, DownloadProgress::Pending),
                    "LLM should be Pending"
                );
            }
            other => panic!("expected Downloading, got {other:?}"),
        }

        // Pipeline state should be Idle
        assert_eq!(state.pipeline_state(), PipelineState::Idle);
    }

    #[test]
    fn test_data_dir_platform() {
        let dir = data_dir().expect("data_dir should succeed");
        let path_str = dir.to_string_lossy();
        assert!(
            path_str.contains("com.vox.app"),
            "data dir should contain com.vox.app, got: {path_str}"
        );
    }

    #[test]
    fn test_vox_state_existing_data() {
        let dir = tempfile::tempdir().expect("temp dir");

        // Create settings with non-default values
        let mut settings = Settings::default();
        settings.language = "de".into();
        settings.save(dir.path()).expect("save settings");

        // Create VoxState — it should load existing settings
        let state = VoxState::new(dir.path()).expect("VoxState::new should succeed");
        assert_eq!(state.settings().language, "de");
    }

    #[test]
    fn test_ensure_data_dirs() {
        // This test verifies the actual platform path is created.
        // We only check that the function succeeds and the directory exists.
        let dir = ensure_data_dirs().expect("ensure_data_dirs should succeed");
        assert!(dir.exists(), "data directory should exist");
        assert!(dir.join("models").exists(), "models directory should exist");
    }

    #[test]
    fn test_init_database_creates_both_tables() {
        let dir = tempfile::tempdir().expect("temp dir");
        let _store = init_database(dir.path()).expect("init_database should succeed");

        // Verify both tables exist by opening a raw connection and querying
        let db_path = dir.path().join("vox.db");
        let conn = rusqlite::Connection::open(&db_path).expect("open db");

        // Transcripts table should exist
        let transcript_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcripts", [], |row| row.get(0))
            .expect("transcripts table should exist");
        assert_eq!(transcript_count, 0);

        // Dictionary table should exist
        let dict_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dictionary", [], |row| row.get(0))
            .expect("dictionary table should exist");
        assert_eq!(dict_count, 0);
    }

    #[test]
    fn test_app_readiness_transitions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let state = VoxState::new(dir.path()).expect("VoxState::new");

        // Start: Downloading
        assert!(matches!(state.readiness(), AppReadiness::Downloading { .. }));

        // Transition to Loading
        state.set_readiness(AppReadiness::Loading {
            stage: "Loading VAD model".into(),
        });
        match state.readiness() {
            AppReadiness::Loading { stage } => {
                assert_eq!(stage, "Loading VAD model");
            }
            other => panic!("expected Loading, got {other:?}"),
        }

        // Transition to Ready
        state.set_readiness(AppReadiness::Ready);
        assert!(matches!(state.readiness(), AppReadiness::Ready));
    }

    #[test]
    fn test_pipeline_state_transitions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let state = VoxState::new(dir.path()).expect("VoxState::new");

        assert_eq!(state.pipeline_state(), PipelineState::Idle);

        state.set_pipeline_state(PipelineState::Listening);
        assert_eq!(state.pipeline_state(), PipelineState::Listening);

        state.set_pipeline_state(PipelineState::Processing {
            raw_text: Some("hello".into()),
        });
        assert!(matches!(
            state.pipeline_state(),
            PipelineState::Processing { .. }
        ));

        state.set_pipeline_state(PipelineState::Injecting {
            polished_text: "Hello.".into(),
        });
        assert!(matches!(
            state.pipeline_state(),
            PipelineState::Injecting { .. }
        ));

        state.set_pipeline_state(PipelineState::Idle);
        assert_eq!(state.pipeline_state(), PipelineState::Idle);
    }

    #[test]
    fn test_update_settings() {
        let dir = tempfile::tempdir().expect("temp dir");
        let state = VoxState::new(dir.path()).expect("VoxState::new");

        state
            .update_settings(|s| {
                s.language = "fr".into();
                s.temperature = 0.8;
            })
            .expect("update_settings should succeed");

        // Verify in-memory state
        assert_eq!(state.settings().language, "fr");
        assert_eq!(state.settings().temperature, 0.8);

        // Verify persisted to disk
        let loaded = Settings::load(dir.path()).expect("load");
        assert_eq!(loaded.language, "fr");
        assert_eq!(loaded.temperature, 0.8);
    }

    #[test]
    fn test_save_history_disabled() {
        let dir = tempfile::tempdir().expect("temp dir");
        let state = VoxState::new(dir.path()).expect("VoxState::new");

        // Disable save_history
        state
            .update_settings(|s| s.save_history = false)
            .expect("update");

        let entry = TranscriptEntry {
            id: "test-1".into(),
            raw_text: "hello".into(),
            polished_text: "Hello.".into(),
            target_app: "TestApp".into(),
            duration_ms: 1000,
            latency_ms: 200,
            created_at: "2026-02-21T10:00:00Z".into(),
        };

        state.save_transcript(&entry).expect("save_transcript should succeed");

        // Should have saved nothing
        let transcripts = state.get_transcripts(10, 0).expect("get_transcripts");
        assert!(
            transcripts.is_empty(),
            "no transcripts should be saved when save_history=false"
        );
    }

    #[test]
    fn test_clear_history_vacuum() {
        let dir = tempfile::tempdir().expect("temp dir");
        let state = VoxState::new(dir.path()).expect("VoxState::new");

        // Save some transcripts
        for i in 0..5 {
            let entry = TranscriptEntry {
                id: format!("test-{i}"),
                raw_text: format!("raw text {i} with enough content to take space"),
                polished_text: format!("Polished text {i} with enough content to take space"),
                target_app: "TestApp".into(),
                duration_ms: 1000,
                latency_ms: 200,
                created_at: format!("2026-02-21T10:0{i}:00Z"),
            };
            state.save_transcript(&entry).expect("save");
        }

        let count = state.get_transcripts(100, 0).expect("get").len();
        assert_eq!(count, 5, "should have 5 transcripts before clear");

        // Get db file size before clear
        let db_path = dir.path().join("vox.db");
        let size_before = std::fs::metadata(&db_path).expect("metadata").len();

        state.clear_history().expect("clear_history should succeed");

        let count = state.get_transcripts(100, 0).expect("get").len();
        assert_eq!(count, 0, "should have 0 transcripts after clear");

        // After VACUUM, file size should be <= the pre-clear size
        let size_after = std::fs::metadata(&db_path).expect("metadata").len();
        assert!(
            size_after <= size_before,
            "db file size should not grow after clear+VACUUM (before={size_before}, after={size_after})"
        );
    }
}
