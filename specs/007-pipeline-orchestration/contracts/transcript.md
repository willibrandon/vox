# API Contract: TranscriptStore

**Module**: `crates/vox_core/src/pipeline/transcript.rs`

## TranscriptEntry

A historical record of a completed dictation. Created after each successful text injection. NOT created for voice command executions (FR-016).

```rust
#[derive(Clone, Debug)]
pub struct TranscriptEntry {
    /// Unique identifier (UUID v4).
    pub id: String,
    /// Original ASR output before dictionary/LLM processing.
    pub raw_text: String,
    /// Final text after LLM post-processing and injection.
    pub polished_text: String,
    /// Name of the focused application at injection time.
    pub target_app: String,
    /// Audio segment duration in milliseconds.
    pub duration_ms: u32,
    /// End-to-end processing latency in milliseconds.
    pub latency_ms: u32,
    /// When this record was created (ISO 8601).
    pub created_at: String,
}
```

## TranscriptStore

SQLite-backed persistent storage for transcript history. Thread-safe via internal Mutex.

```rust
pub struct TranscriptStore { /* Mutex<Connection> */ }

impl TranscriptStore {
    /// Open or create the transcript database.
    /// Creates table/index if needed. Auto-prunes records > 30 days.
    pub fn open(db_path: &Path) -> Result<Self>;

    /// Save a transcript entry to the database.
    pub fn save(&self, entry: &TranscriptEntry) -> Result<()>;

    /// List transcript entries, newest first (paginated).
    pub fn list(&self, limit: usize, offset: usize) -> Result<Vec<TranscriptEntry>>;

    /// Delete records older than the given number of days.
    /// Returns count deleted.
    pub fn prune_older_than(&self, days: u32) -> Result<usize>;

    /// Total number of transcript records.
    pub fn count(&self) -> Result<usize>;
}
```
