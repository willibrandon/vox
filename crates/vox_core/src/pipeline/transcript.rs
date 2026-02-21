//! Transcript persistence for the Vox dictation engine.
//!
//! Stores completed dictation records in SQLite with automatic 30-day pruning.
//! Thread-safe via internal `parking_lot::Mutex` — safe to share across the
//! pipeline's async orchestrator and UI threads.

use std::path::Path;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use rusqlite::Connection;

/// A historical record of a completed dictation.
///
/// Created after each successful text injection. NOT created for voice command
/// executions (FR-016). Fields capture the full context of the dictation for
/// history display and analytics.
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

/// SQLite-backed persistent storage for transcript history.
///
/// Wraps a `rusqlite::Connection` in a `parking_lot::Mutex` for thread-safe
/// access. Transcript writes are infrequent (once per dictation segment),
/// so mutex contention is negligible.
pub struct TranscriptStore {
    connection: Mutex<Connection>,
}

const CREATE_TABLE_SQL: &str = "\
    CREATE TABLE IF NOT EXISTS transcripts (\
        id TEXT PRIMARY KEY,\
        raw_text TEXT NOT NULL,\
        polished_text TEXT NOT NULL,\
        target_app TEXT NOT NULL,\
        duration_ms INTEGER NOT NULL,\
        latency_ms INTEGER NOT NULL,\
        created_at TEXT NOT NULL\
    )";

const CREATE_INDEX_SQL: &str =
    "CREATE INDEX IF NOT EXISTS idx_transcripts_created_at ON transcripts(created_at)";

impl TranscriptStore {
    /// Open or create the transcript database at the given path.
    ///
    /// Creates the table and index if they don't exist. Auto-prunes records
    /// older than 30 days on startup.
    pub fn open(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open transcript database at {}", db_path.display()))?;
        conn.execute_batch(CREATE_TABLE_SQL)
            .context("failed to create transcripts table")?;
        conn.execute_batch(CREATE_INDEX_SQL)
            .context("failed to create transcripts index")?;

        let store = Self {
            connection: Mutex::new(conn),
        };

        // Auto-prune on startup
        let pruned = store.prune_older_than(30)?;
        if pruned > 0 {
            tracing::info!("pruned {pruned} transcript records older than 30 days");
        }

        Ok(store)
    }

    /// Save a transcript entry to the database.
    pub fn save(&self, entry: &TranscriptEntry) -> Result<()> {
        let conn = self.connection.lock();
        conn.execute(
            "INSERT INTO transcripts (id, raw_text, polished_text, target_app, duration_ms, latency_ms, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                entry.id,
                entry.raw_text,
                entry.polished_text,
                entry.target_app,
                entry.duration_ms,
                entry.latency_ms,
                entry.created_at,
            ],
        )
        .context("failed to save transcript entry")?;
        Ok(())
    }

    /// List transcript entries, newest first (paginated).
    pub fn list(&self, limit: usize, offset: usize) -> Result<Vec<TranscriptEntry>> {
        let conn = self.connection.lock();
        let mut stmt = conn
            .prepare(
                "SELECT id, raw_text, polished_text, target_app, duration_ms, latency_ms, created_at \
                 FROM transcripts ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
            )
            .context("failed to prepare list query")?;

        let entries = stmt
            .query_map(rusqlite::params![limit as i64, offset as i64], |row| {
                Ok(TranscriptEntry {
                    id: row.get(0)?,
                    raw_text: row.get(1)?,
                    polished_text: row.get(2)?,
                    target_app: row.get(3)?,
                    duration_ms: row.get(4)?,
                    latency_ms: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })
            .context("failed to query transcripts")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read transcript row")?;

        Ok(entries)
    }

    /// Delete records older than the given number of days.
    /// Returns count deleted.
    pub fn prune_older_than(&self, days: u32) -> Result<usize> {
        let conn = self.connection.lock();
        let deleted = conn
            .execute(
                "DELETE FROM transcripts WHERE created_at < datetime('now', ?1)",
                rusqlite::params![format!("-{days} days")],
            )
            .context("failed to prune old transcripts")?;
        Ok(deleted)
    }

    /// Total number of transcript records.
    pub fn count(&self) -> Result<usize> {
        let conn = self.connection.lock();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM transcripts", [], |row| row.get(0))
            .context("failed to count transcripts")?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, created_at: &str) -> TranscriptEntry {
        TranscriptEntry {
            id: id.to_string(),
            raw_text: format!("raw {id}"),
            polished_text: format!("polished {id}"),
            target_app: "TestApp".to_string(),
            duration_ms: 1000,
            latency_ms: 200,
            created_at: created_at.to_string(),
        }
    }

    #[test]
    fn test_save_and_list_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = TranscriptStore::open(&dir.path().join("test.db")).expect("open");

        let entry = make_entry("id-1", "2026-02-20T10:00:00Z");
        store.save(&entry).expect("save");

        let entries = store.list(10, 0).expect("list");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "id-1");
        assert_eq!(entries[0].raw_text, "raw id-1");
        assert_eq!(entries[0].polished_text, "polished id-1");
        assert_eq!(entries[0].target_app, "TestApp");
        assert_eq!(entries[0].duration_ms, 1000);
        assert_eq!(entries[0].latency_ms, 200);
    }

    #[test]
    fn test_list_newest_first() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = TranscriptStore::open(&dir.path().join("test.db")).expect("open");

        store.save(&make_entry("old", "2026-02-19T10:00:00Z")).expect("save");
        store.save(&make_entry("new", "2026-02-20T10:00:00Z")).expect("save");

        let entries = store.list(10, 0).expect("list");
        assert_eq!(entries[0].id, "new");
        assert_eq!(entries[1].id, "old");
    }

    #[test]
    fn test_list_pagination() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = TranscriptStore::open(&dir.path().join("test.db")).expect("open");

        for i in 0..5 {
            store
                .save(&make_entry(&format!("id-{i}"), &format!("2026-02-{:02}T10:00:00Z", 15 + i)))
                .expect("save");
        }

        let page1 = store.list(2, 0).expect("list page 1");
        assert_eq!(page1.len(), 2);

        let page2 = store.list(2, 2).expect("list page 2");
        assert_eq!(page2.len(), 2);

        let page3 = store.list(2, 4).expect("list page 3");
        assert_eq!(page3.len(), 1);
    }

    #[test]
    fn test_count() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = TranscriptStore::open(&dir.path().join("test.db")).expect("open");

        assert_eq!(store.count().expect("count"), 0);

        store.save(&make_entry("id-1", "2026-02-20T10:00:00Z")).expect("save");
        assert_eq!(store.count().expect("count"), 1);

        store.save(&make_entry("id-2", "2026-02-20T11:00:00Z")).expect("save");
        assert_eq!(store.count().expect("count"), 2);
    }

    #[test]
    fn test_prune_older_than() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = TranscriptStore::open(&dir.path().join("test.db")).expect("open");

        // Insert a record with a date far in the past
        store
            .save(&make_entry("old", "2020-01-01T00:00:00Z"))
            .expect("save old");
        store
            .save(&make_entry("recent", "2026-02-20T10:00:00Z"))
            .expect("save recent");

        let pruned = store.prune_older_than(30).expect("prune");
        assert_eq!(pruned, 1);
        assert_eq!(store.count().expect("count"), 1);

        let remaining = store.list(10, 0).expect("list");
        assert_eq!(remaining[0].id, "recent");
    }

    #[test]
    fn test_auto_prune_on_open() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");

        // First open — insert old record
        {
            let store = TranscriptStore::open(&db_path).expect("open 1");
            store
                .save(&make_entry("ancient", "2020-01-01T00:00:00Z"))
                .expect("save");
            assert_eq!(store.count().expect("count"), 1);
        }

        // Second open — should auto-prune the old record
        {
            let store = TranscriptStore::open(&db_path).expect("open 2");
            assert_eq!(store.count().expect("count"), 0);
        }
    }

    #[test]
    fn test_concurrent_save_and_list() {
        use std::sync::Arc;

        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(TranscriptStore::open(&dir.path().join("test.db")).expect("open"));

        let store_writer = Arc::clone(&store);
        let writer = std::thread::spawn(move || {
            for i in 0..10 {
                store_writer
                    .save(&make_entry(
                        &format!("w-{i}"),
                        &format!("2026-02-20T{:02}:00:00Z", i),
                    ))
                    .expect("save");
            }
        });

        let store_reader = Arc::clone(&store);
        let reader = std::thread::spawn(move || {
            for _ in 0..10 {
                let _ = store_reader.list(100, 0);
            }
        });

        writer.join().expect("writer thread panicked");
        reader.join().expect("reader thread panicked");

        assert_eq!(store.count().expect("count"), 10);
    }
}
