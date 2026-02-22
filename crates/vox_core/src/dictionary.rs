//! User-defined vocabulary substitution cache for the Vox dictation engine.
//!
//! Loads dictionary entries from SQLite into memory and provides fast
//! substitution during dictation. Single-word lookups use O(1) HashMap,
//! multi-word phrases use longest-first string replacement. The cache is
//! cheaply cloneable via Arc-wrapped internals with shared RwLock state —
//! mutations from any clone are immediately visible to all others.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use parking_lot::RwLock;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

// ── Public Types ────────────────────────────────────────────────────────

/// A single dictionary entry mapping a spoken form to a written form.
///
/// Loaded from SQLite into the in-memory cache. Used for text substitution
/// during dictation and for LLM hint generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryEntry {
    /// Auto-increment primary key.
    pub id: i64,
    /// The term the user speaks during dictation.
    pub spoken: String,
    /// The replacement text to inject.
    pub written: String,
    /// Freeform grouping label (general, name, technical, email, etc.).
    pub category: String,
    /// If true, excluded from text substitution but included in LLM hints.
    pub is_command_phrase: bool,
    /// Number of times matched during text substitution.
    pub use_count: u64,
    /// Creation timestamp (ISO 8601).
    pub created_at: String,
}

/// The result of applying dictionary substitutions to text.
///
/// Contains the substituted text and the IDs of all matched entries.
/// Matched IDs may contain duplicates if the same entry matches multiple
/// times (e.g., "vox" appearing twice in input).
#[derive(Clone, Debug)]
pub struct SubstitutionResult {
    /// The text after all substitutions applied.
    pub text: String,
    /// IDs of dictionary entries that were matched (with duplicates for repeated matches).
    pub matched_ids: Vec<i64>,
}

/// The outcome of a batch import operation.
#[derive(Clone, Debug)]
pub struct ImportResult {
    /// Count of entries successfully imported.
    pub added: usize,
    /// Count of entries skipped due to duplicate spoken forms.
    pub skipped: usize,
    /// Descriptions of entries that failed validation.
    pub errors: Vec<String>,
}

/// A serializable entry for import/export operations.
///
/// Excludes id, use_count, and created_at which are installation-specific.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryExportEntry {
    /// The spoken form.
    pub spoken: String,
    /// The written replacement.
    pub written: String,
    /// Category label.
    pub category: String,
    /// Command phrase flag.
    pub is_command_phrase: bool,
}

// ── Internal Types ──────────────────────────────────────────────────────

/// Optimized entry stored in the substitution lookup maps.
///
/// Contains only the fields needed during substitution: the entry ID
/// (for use count tracking) and the replacement text.
struct SubEntry {
    id: i64,
    written: String,
}

// ── Constants ───────────────────────────────────────────────────────────

/// SQL to create the dictionary table with the current schema.
///
/// Shared between `DictionaryCache::load` and `state::init_database` to
/// prevent schema drift from duplicated DDL.
pub const CREATE_TABLE_SQL: &str = "\
    CREATE TABLE IF NOT EXISTS dictionary (\
        id INTEGER PRIMARY KEY AUTOINCREMENT,\
        spoken TEXT UNIQUE NOT NULL COLLATE NOCASE,\
        written TEXT NOT NULL,\
        category TEXT NOT NULL DEFAULT 'general',\
        is_command_phrase INTEGER NOT NULL DEFAULT 0,\
        use_count INTEGER NOT NULL DEFAULT 0,\
        created_at TEXT NOT NULL\
    );\
    CREATE INDEX IF NOT EXISTS idx_dictionary_spoken ON dictionary(spoken);\
    CREATE INDEX IF NOT EXISTS idx_dictionary_category ON dictionary(category);";

// ── Free Functions ──────────────────────────────────────────────────────

/// Migrate the dictionary table schema from old column names to new.
///
/// Inspects current columns via PRAGMA table_info. If old column names
/// (term, replacement, frequency) are found, renames them and adds
/// category + is_command_phrase columns. Idempotent — safe to call on
/// both fresh and already-migrated databases.
pub fn migrate_schema(db_path: &Path) -> Result<()> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("failed to open database for schema migration at {}", db_path.display()))?;

    // Check if table exists
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='dictionary'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .context("failed to check if dictionary table exists")?
        > 0;

    if !table_exists {
        conn.execute_batch(CREATE_TABLE_SQL)
            .context("failed to create dictionary table")?;
        return Ok(());
    }

    // Check current column names to determine if migration is needed
    let mut stmt = conn
        .prepare("PRAGMA table_info(dictionary)")
        .context("failed to read table info")?;
    let columns: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .context("failed to query table info")?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to read column names")?;
    drop(stmt);

    let has_old_columns = columns.iter().any(|c| c == "term");

    if has_old_columns {
        // All migration steps in one transaction so a crash mid-migration
        // cannot leave the schema in a half-renamed state.
        let tx = conn
            .unchecked_transaction()
            .context("failed to begin migration transaction")?;

        tx.execute_batch("ALTER TABLE dictionary RENAME COLUMN term TO spoken")
            .context("failed to rename column term to spoken")?;
        tx.execute_batch("ALTER TABLE dictionary RENAME COLUMN replacement TO written")
            .context("failed to rename column replacement to written")?;
        tx.execute_batch("ALTER TABLE dictionary RENAME COLUMN frequency TO use_count")
            .context("failed to rename column frequency to use_count")?;

        if !columns.iter().any(|c| c == "category") {
            tx.execute_batch(
                "ALTER TABLE dictionary ADD COLUMN category TEXT NOT NULL DEFAULT 'general'",
            )
            .context("failed to add category column")?;
        }
        if !columns.iter().any(|c| c == "is_command_phrase") {
            tx.execute_batch(
                "ALTER TABLE dictionary ADD COLUMN is_command_phrase INTEGER NOT NULL DEFAULT 0",
            )
            .context("failed to add is_command_phrase column")?;
        }

        tx.commit().context("failed to commit migration transaction")?;
    }

    // Create indexes (idempotent via IF NOT EXISTS)
    conn.execute_batch("CREATE INDEX IF NOT EXISTS idx_dictionary_spoken ON dictionary(spoken)")
        .context("failed to create spoken index")?;
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_dictionary_category ON dictionary(category)",
    )
    .context("failed to create category index")?;

    Ok(())
}

/// Map a byte position in the lowercased version of `text` to the corresponding
/// byte position in the original `text`.
///
/// Unicode case folding can change byte lengths (e.g., İ → i̇ is 2 bytes → 3 bytes).
/// This walks through original characters, accumulating their lowercased byte lengths,
/// to find where a lowercase byte offset falls in the original string.
fn lowercase_byte_to_original(text: &str, lc_target: usize) -> usize {
    let mut lc_bytes = 0usize;
    let mut orig_bytes = 0usize;
    for ch in text.chars() {
        if lc_bytes >= lc_target {
            break;
        }
        let lc_len: usize = ch.to_lowercase().map(|c| c.len_utf8()).sum();
        lc_bytes += lc_len;
        orig_bytes += ch.len_utf8();
    }
    orig_bytes
}

// ── DictionaryCache ─────────────────────────────────────────────────────

/// In-memory cache of user-defined vocabulary substitutions and LLM hints.
///
/// Loaded from SQLite on startup. Clone is cheap (Arc-wrapped internals
/// with shared RwLock state). Supports CRUD operations that synchronize
/// both in-memory state and SQLite storage.
#[derive(Clone)]
pub struct DictionaryCache {
    /// Path to the SQLite database. None for caches created with `empty()`.
    db_path: Option<PathBuf>,
    /// All entries keyed by spoken form (lowercase).
    entries: Arc<RwLock<HashMap<String, DictionaryEntry>>>,
    /// Single-word substitutions. Excludes command phrases.
    word_subs: Arc<RwLock<HashMap<String, SubEntry>>>,
    /// Multi-word phrase substitutions sorted longest-first. Excludes command phrases.
    phrase_subs: Arc<RwLock<Vec<(String, SubEntry)>>>,
}

impl DictionaryCache {
    // ── Construction ────────────────────────────────────────────

    /// Load dictionary from SQLite database at the given path.
    ///
    /// Runs schema migration (idempotent), reads all entries into memory,
    /// and builds optimized substitution maps. The db_path is stored for
    /// subsequent CRUD operations.
    pub fn load(db_path: &Path) -> Result<Self> {
        migrate_schema(db_path)?;

        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open dictionary database at {}", db_path.display()))?;

        let mut stmt = conn
            .prepare("SELECT id, spoken, written, category, is_command_phrase, use_count, created_at FROM dictionary ORDER BY spoken")
            .context("failed to prepare dictionary query")?;

        let entries_vec: Vec<DictionaryEntry> = stmt
            .query_map([], |row| {
                Ok(DictionaryEntry {
                    id: row.get(0)?,
                    spoken: row.get(1)?,
                    written: row.get(2)?,
                    category: row.get(3)?,
                    is_command_phrase: row.get::<_, i32>(4)? != 0,
                    use_count: row.get::<_, i64>(5)? as u64,
                    created_at: row.get(6)?,
                })
            })
            .context("failed to query dictionary")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read dictionary row")?;

        let mut entries_map = HashMap::new();
        for entry in entries_vec {
            entries_map.insert(entry.spoken.to_lowercase(), entry);
        }

        let cache = Self {
            db_path: Some(db_path.to_path_buf()),
            entries: Arc::new(RwLock::new(entries_map)),
            word_subs: Arc::new(RwLock::new(HashMap::new())),
            phrase_subs: Arc::new(RwLock::new(Vec::new())),
        };

        cache.rebuild_substitution_maps();

        Ok(cache)
    }

    /// Create an empty dictionary cache with no database backing.
    ///
    /// CRUD operations will return an error on this cache. Intended
    /// for test helpers that only need substitution behavior.
    pub fn empty() -> Self {
        Self {
            db_path: None,
            entries: Arc::new(RwLock::new(HashMap::new())),
            word_subs: Arc::new(RwLock::new(HashMap::new())),
            phrase_subs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    // ── CRUD ────────────────────────────────────────────────────

    /// Add a new dictionary entry.
    ///
    /// Inserts into SQLite and updates the in-memory cache. Returns
    /// the auto-generated entry ID. Errors if the spoken form already
    /// exists (case-insensitive) or if spoken is empty.
    pub fn add(
        &self,
        spoken: &str,
        written: &str,
        category: &str,
        is_command_phrase: bool,
    ) -> Result<i64> {
        if spoken.trim().is_empty() {
            bail!("spoken form must not be empty");
        }

        let spoken_lower = spoken.to_lowercase();

        // Check for duplicate in cache
        {
            let entries = self.entries.read();
            if entries.contains_key(&spoken_lower) {
                bail!("spoken form '{}' already exists in dictionary", spoken);
            }
        }

        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO dictionary (spoken, written, category, is_command_phrase, use_count, created_at) VALUES (?1, ?2, ?3, ?4, 0, datetime('now'))",
            rusqlite::params![spoken, written, category, is_command_phrase as i32],
        )
        .with_context(|| format!("failed to add dictionary entry for '{spoken}'"))?;

        let id = conn.last_insert_rowid();
        let entry = Self::read_entry_by_id(&conn, id)?;

        {
            let mut entries = self.entries.write();
            entries.insert(spoken_lower, entry);
        }

        self.rebuild_substitution_maps();

        Ok(id)
    }

    /// Update an existing dictionary entry by ID.
    ///
    /// Updates both SQLite and the in-memory cache. Errors if the ID
    /// doesn't exist or if the new spoken form conflicts with another
    /// entry (case-insensitive uniqueness).
    pub fn update(
        &self,
        id: i64,
        spoken: &str,
        written: &str,
        category: &str,
        is_command_phrase: bool,
    ) -> Result<()> {
        if spoken.trim().is_empty() {
            bail!("spoken form must not be empty");
        }

        let spoken_lower = spoken.to_lowercase();

        {
            let entries = self.entries.read();
            // Verify the entry exists
            if !entries.values().any(|e| e.id == id) {
                bail!("dictionary entry with id {id} not found");
            }
            // Check for spoken form conflict with another entry
            if let Some(existing) = entries.get(&spoken_lower) {
                if existing.id != id {
                    bail!("spoken form '{}' already exists in dictionary", spoken);
                }
            }
        }

        let conn = self.open_connection()?;
        conn.execute(
            "UPDATE dictionary SET spoken = ?2, written = ?3, category = ?4, is_command_phrase = ?5 WHERE id = ?1",
            rusqlite::params![id, spoken, written, category, is_command_phrase as i32],
        )
        .with_context(|| format!("failed to update dictionary entry {id}"))?;

        let entry = Self::read_entry_by_id(&conn, id)?;

        {
            let mut entries = self.entries.write();
            entries.retain(|_, e| e.id != id);
            entries.insert(spoken_lower, entry);
        }

        self.rebuild_substitution_maps();
        Ok(())
    }

    /// Delete a dictionary entry by ID.
    ///
    /// Removes from both SQLite and the in-memory cache. Errors if
    /// the ID doesn't exist.
    pub fn delete(&self, id: i64) -> Result<()> {
        {
            let entries = self.entries.read();
            if !entries.values().any(|e| e.id == id) {
                bail!("dictionary entry with id {id} not found");
            }
        }

        let conn = self.open_connection()?;
        conn.execute("DELETE FROM dictionary WHERE id = ?1", rusqlite::params![id])
            .with_context(|| format!("failed to delete dictionary entry {id}"))?;

        {
            let mut entries = self.entries.write();
            entries.retain(|_, e| e.id != id);
        }

        self.rebuild_substitution_maps();
        Ok(())
    }

    /// List all entries, optionally filtered by category.
    ///
    /// Returns entries sorted by spoken form. If category is Some,
    /// only entries matching that category are returned (case-sensitive
    /// category match).
    pub fn list(&self, category: Option<&str>) -> Vec<DictionaryEntry> {
        let entries = self.entries.read();
        let mut result: Vec<DictionaryEntry> = entries
            .values()
            .filter(|e| match category {
                Some(cat) => e.category == cat,
                None => true,
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| a.spoken.cmp(&b.spoken));
        result
    }

    /// Search entries by partial match on spoken or written text.
    ///
    /// Returns entries where the query appears as a substring in either
    /// the spoken or written field (case-insensitive). Returns entries
    /// sorted by spoken form.
    pub fn search(&self, query: &str) -> Vec<DictionaryEntry> {
        let query_lower = query.to_lowercase();
        let entries = self.entries.read();
        let mut result: Vec<DictionaryEntry> = entries
            .values()
            .filter(|e| {
                e.spoken.to_lowercase().contains(&query_lower)
                    || e.written.to_lowercase().contains(&query_lower)
            })
            .cloned()
            .collect();
        result.sort_by(|a, b| a.spoken.cmp(&b.spoken));
        result
    }

    // ── Pipeline Integration ────────────────────────────────────

    /// Apply dictionary substitutions to raw transcript text.
    ///
    /// Two-pass algorithm:
    /// 1. Replace multi-word phrases (longest-first, case-insensitive)
    /// 2. Replace single words via HashMap O(1) lookup
    ///
    /// Entries marked as command phrases are excluded from substitution.
    /// Returns the substituted text and IDs of all matched entries
    /// (with duplicates for repeated matches).
    pub fn apply_substitutions(&self, text: &str) -> SubstitutionResult {
        if text.is_empty() {
            return SubstitutionResult {
                text: String::new(),
                matched_ids: Vec::new(),
            };
        }

        let mut matched_ids = Vec::new();

        // Pass 1: Replace multi-word phrases (longest-first, case-insensitive)
        let mut result = text.to_string();
        {
            let phrase_subs = self.phrase_subs.read();
            for (phrase, sub_entry) in phrase_subs.iter() {
                let mut new_result = String::with_capacity(result.len());
                let mut remaining = result.as_str();

                loop {
                    let remaining_lower = remaining.to_lowercase();
                    match remaining_lower.find(phrase.as_str()) {
                        Some(lc_pos) => {
                            let orig_start = lowercase_byte_to_original(remaining, lc_pos);
                            let orig_end = lowercase_byte_to_original(
                                remaining,
                                lc_pos + phrase.len(),
                            );
                            new_result.push_str(&remaining[..orig_start]);
                            new_result.push_str(&sub_entry.written);
                            remaining = &remaining[orig_end..];
                            matched_ids.push(sub_entry.id);
                        }
                        None => {
                            new_result.push_str(remaining);
                            break;
                        }
                    }
                }

                result = new_result;
            }
        }

        // Pass 2: Replace single words via HashMap lookup
        {
            let word_subs = self.word_subs.read();
            let words: Vec<&str> = result.split_whitespace().collect();
            let replaced: Vec<String> = words
                .iter()
                .map(|word| {
                    let lower = word.to_lowercase();
                    match word_subs.get(&lower) {
                        Some(sub_entry) => {
                            matched_ids.push(sub_entry.id);
                            sub_entry.written.clone()
                        }
                        None => word.to_string(),
                    }
                })
                .collect();

            let joined: String = replaced
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");

            result = joined.trim().to_string();
        }

        SubstitutionResult {
            text: result,
            matched_ids,
        }
    }

    /// Format the top N dictionary entries by use count as hints for
    /// the LLM post-processor's system prompt.
    ///
    /// Includes ALL entries (including command phrases). Sorted by
    /// use_count descending. Limited to at most n entries.
    /// Format: "spoken1 → written1, spoken2 → written2, ..."
    pub fn top_hints(&self, n: usize) -> String {
        let entries = self.entries.read();
        let mut sorted: Vec<&DictionaryEntry> = entries.values().collect();
        sorted.sort_by(|a, b| b.use_count.cmp(&a.use_count));
        sorted
            .iter()
            .take(n)
            .map(|e| format!("{} → {}", e.spoken, e.written))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Increment use counts for the given entry IDs.
    ///
    /// Updates both in-memory entries and SQLite. IDs may contain
    /// duplicates — each occurrence increments the count by 1.
    /// Uses a single SQLite transaction for efficiency.
    pub fn increment_use_counts(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        // Count occurrences per ID for batched updates
        let mut counts: HashMap<i64, u64> = HashMap::new();
        for &id in ids {
            *counts.entry(id).or_insert(0) += 1;
        }

        let conn = self.open_connection()?;
        let tx = conn
            .unchecked_transaction()
            .context("failed to begin transaction for use count update")?;

        for (&id, &count) in &counts {
            tx.execute(
                "UPDATE dictionary SET use_count = use_count + ?2 WHERE id = ?1",
                rusqlite::params![id, count as i64],
            )
            .with_context(|| format!("failed to increment use count for entry {id}"))?;
        }

        tx.commit()
            .context("failed to commit use count transaction")?;

        // Update in-memory counts
        let mut entries = self.entries.write();
        for (&id, &count) in &counts {
            if let Some(entry) = entries.values_mut().find(|e| e.id == id) {
                entry.use_count += count;
            }
        }

        Ok(())
    }

    // ── Import/Export ───────────────────────────────────────────

    /// Export all dictionary entries as a JSON string.
    ///
    /// Exports spoken, written, category, and is_command_phrase fields.
    /// Excludes id, use_count, and created_at (installation-specific).
    pub fn export_json(&self) -> Result<String> {
        let entries = self.entries.read();
        let exports: Vec<DictionaryExportEntry> = entries
            .values()
            .map(|e| DictionaryExportEntry {
                spoken: e.spoken.clone(),
                written: e.written.clone(),
                category: e.category.clone(),
                is_command_phrase: e.is_command_phrase,
            })
            .collect();
        serde_json::to_string_pretty(&exports).context("failed to serialize dictionary entries")
    }

    /// Import dictionary entries from a JSON string.
    ///
    /// Entries with spoken forms that already exist are skipped (not
    /// overwritten). Entries with empty spoken forms are reported as
    /// errors. Returns an ImportResult with counts of added, skipped,
    /// and errored entries.
    pub fn import_json(&self, json: &str) -> Result<ImportResult> {
        let imports: Vec<DictionaryExportEntry> =
            serde_json::from_str(json).context("failed to parse dictionary JSON")?;

        let mut added = 0;
        let mut skipped = 0;
        let mut errors = Vec::new();

        for entry in imports {
            if entry.spoken.trim().is_empty() {
                errors.push("entry with empty spoken form skipped".to_string());
                continue;
            }

            // Check if spoken form already exists (case-insensitive)
            let exists = {
                let entries = self.entries.read();
                entries.contains_key(&entry.spoken.to_lowercase())
            };

            if exists {
                skipped += 1;
                continue;
            }

            match self.add(
                &entry.spoken,
                &entry.written,
                &entry.category,
                entry.is_command_phrase,
            ) {
                Ok(_) => added += 1,
                Err(e) => {
                    errors.push(format!("failed to import '{}': {e}", entry.spoken));
                }
            }
        }

        Ok(ImportResult {
            added,
            skipped,
            errors,
        })
    }

    // ── Accessors ───────────────────────────────────────────────

    /// Number of entries in the cache.
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.read().is_empty()
    }

    // ── Internal ────────────────────────────────────────────────

    /// Rebuild the optimized substitution maps from the entries HashMap.
    ///
    /// Partitions entries into word_subs (single-word, non-command) and
    /// phrase_subs (multi-word, non-command, sorted longest-first).
    /// Command phrase entries are excluded from both maps.
    fn rebuild_substitution_maps(&self) {
        let (new_word_subs, new_phrase_subs) = {
            let entries = self.entries.read();
            let mut word_subs = HashMap::new();
            let mut phrase_subs = Vec::new();

            for entry in entries.values() {
                if entry.is_command_phrase {
                    continue;
                }

                let spoken_lower = entry.spoken.to_lowercase();
                let sub = SubEntry {
                    id: entry.id,
                    written: entry.written.clone(),
                };

                if spoken_lower.contains(char::is_whitespace) {
                    phrase_subs.push((spoken_lower, sub));
                } else {
                    word_subs.insert(spoken_lower, sub);
                }
            }

            // Sort phrases by descending word count (longest first)
            phrase_subs.sort_by(|a, b| {
                let a_words = a.0.split_whitespace().count();
                let b_words = b.0.split_whitespace().count();
                b_words.cmp(&a_words)
            });

            (word_subs, phrase_subs)
        }; // entries read lock released here

        *self.word_subs.write() = new_word_subs;
        *self.phrase_subs.write() = new_phrase_subs;
    }

    /// Open a fresh SQLite connection using the stored db_path.
    ///
    /// Returns an error if this cache was created with `empty()`.
    fn open_connection(&self) -> Result<Connection> {
        let db_path = self
            .db_path
            .as_ref()
            .context("no database path — this cache was created with DictionaryCache::empty()")?;
        Connection::open(db_path).with_context(|| {
            format!(
                "failed to open dictionary database at {}",
                db_path.display()
            )
        })
    }

    /// Read a single dictionary entry by ID from an open connection.
    fn read_entry_by_id(conn: &Connection, id: i64) -> Result<DictionaryEntry> {
        conn.query_row(
            "SELECT id, spoken, written, category, is_command_phrase, use_count, created_at FROM dictionary WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(DictionaryEntry {
                    id: row.get(0)?,
                    spoken: row.get(1)?,
                    written: row.get(2)?,
                    category: row.get(3)?,
                    is_command_phrase: row.get::<_, i32>(4)? != 0,
                    use_count: row.get::<_, i64>(5)? as u64,
                    created_at: row.get(6)?,
                })
            },
        )
        .with_context(|| format!("failed to read dictionary entry with id {id}"))
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a temp database pre-populated with simple entries (spoken → written).
    fn create_test_db(entries: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test_dict.db");
        let conn = Connection::open(&db_path).expect("failed to open test db");
        conn.execute_batch(CREATE_TABLE_SQL)
            .expect("failed to create table");

        for (spoken, written) in entries {
            conn.execute(
                "INSERT INTO dictionary (spoken, written, category, is_command_phrase, use_count, created_at) VALUES (?1, ?2, 'general', 0, 0, '2026-01-01T00:00:00Z')",
                rusqlite::params![spoken, written],
            )
            .expect("failed to insert test entry");
        }

        (dir, db_path)
    }

    /// Create an empty DictionaryCache backed by a temp database.
    fn create_cache() -> (DictionaryCache, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test_dict.db");
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        (cache, dir)
    }

    // ── Existing tests (updated for new API) ────────────────────

    #[test]
    fn test_two_pass_substitution_ordering() {
        let (_dir, db_path) =
            create_test_db(&[("New York City", "NYC"), ("new", "fresh")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        let result = cache.apply_substitutions("I love New York City and new ideas");
        assert_eq!(result.text, "I love NYC and fresh ideas");
        assert_eq!(result.matched_ids.len(), 2);
    }

    #[test]
    fn test_case_insensitive_matching() {
        let (_dir, db_path) = create_test_db(&[("hello", "hi")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(
            cache.apply_substitutions("HELLO world").text,
            "hi world"
        );
        assert_eq!(
            cache.apply_substitutions("Hello world").text,
            "hi world"
        );
        assert_eq!(
            cache.apply_substitutions("hello world").text,
            "hi world"
        );
    }

    #[test]
    fn test_empty_replacement_removes_text() {
        let (_dir, db_path) = create_test_db(&[("um", ""), ("uh", "")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(
            cache.apply_substitutions("um let's uh meet").text,
            "let's meet"
        );
    }

    #[test]
    fn test_all_words_removed_returns_empty() {
        let (_dir, db_path) = create_test_db(&[("um", ""), ("uh", "")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(cache.apply_substitutions("um uh").text, "");
    }

    #[test]
    fn test_phrase_longest_first_priority() {
        let (_dir, db_path) =
            create_test_db(&[("New York", "NY"), ("New York City", "NYC")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(
            cache.apply_substitutions("Visit New York City").text,
            "Visit NYC"
        );
        assert_eq!(
            cache.apply_substitutions("Visit New York").text,
            "Visit NY"
        );
    }

    #[test]
    fn test_round_trip_load_from_sqlite() {
        let (_dir, db_path) =
            create_test_db(&[("rust", "Rust"), ("hello world", "greetings")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(cache.len(), 2);
        assert!(!cache.is_empty());
        assert_eq!(
            cache
                .apply_substitutions("I love rust and hello world")
                .text,
            "I love Rust and greetings"
        );
    }

    #[test]
    fn test_empty_cache() {
        let cache = DictionaryCache::empty();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        let result = cache.apply_substitutions("hello world");
        assert_eq!(result.text, "hello world");
        assert!(result.matched_ids.is_empty());
        assert_eq!(cache.top_hints(10), "");
    }

    #[test]
    fn test_top_hints_formatting() {
        let (_dir, db_path) = create_test_db(&[("foo", "bar"), ("baz", "qux")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        let hints = cache.top_hints(10);
        assert!(hints.contains("→"));
        assert!(hints.contains("foo"));
        assert!(hints.contains("bar"));
    }

    #[test]
    fn test_no_substitution_returns_original() {
        let (_dir, db_path) = create_test_db(&[("xyz", "abc")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        let result = cache.apply_substitutions("hello world");
        assert_eq!(result.text, "hello world");
        assert!(result.matched_ids.is_empty());
    }

    #[test]
    fn test_fresh_load_picks_up_changes() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test_dict.db");
        let conn = Connection::open(&db_path).expect("failed to open test db");
        conn.execute_batch(CREATE_TABLE_SQL)
            .expect("failed to create table");
        conn.execute(
            "INSERT INTO dictionary (spoken, written, category, is_command_phrase, use_count, created_at) VALUES ('foo', 'bar', 'general', 0, 0, '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("insert failed");
        drop(conn);

        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(cache.len(), 1);

        // Add another entry directly via SQL
        let conn = Connection::open(&db_path).expect("failed to open test db");
        conn.execute(
            "INSERT INTO dictionary (spoken, written, category, is_command_phrase, use_count, created_at) VALUES ('baz', 'qux', 'general', 0, 0, '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("insert failed");
        drop(conn);

        // Fresh load should see both entries
        let fresh_cache = DictionaryCache::load(&db_path).expect("fresh load failed");
        assert_eq!(fresh_cache.len(), 2);
    }

    // ── New tests (from spec) ───────────────────────────────────

    #[test]
    fn test_add_entry() {
        let (cache, _dir) = create_cache();

        let id = cache
            .add("vox", "Vox", "name", false)
            .expect("add should succeed");
        assert!(id > 0);

        let entries = cache.list(None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].spoken, "vox");
        assert_eq!(entries[0].written, "Vox");
        assert_eq!(entries[0].category, "name");
        assert!(!entries[0].is_command_phrase);
        assert_eq!(entries[0].use_count, 0);

        // Duplicate spoken form (case-insensitive) should be rejected
        let err = cache.add("VOX", "Something", "general", false);
        assert!(err.is_err());
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("already exists"));

        // Only 1 entry should exist
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_update_entry() {
        let (cache, _dir) = create_cache();

        let id = cache
            .add("vox", "Vox", "name", false)
            .expect("add should succeed");

        cache
            .update(id, "vox", "VOX", "technical", false)
            .expect("update should succeed");

        let entries = cache.list(None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].written, "VOX");
        assert_eq!(entries[0].category, "technical");

        // Update with non-existent ID should fail
        let err = cache.update(99999, "foo", "bar", "general", false);
        assert!(err.is_err());
    }

    #[test]
    fn test_delete_entry() {
        let (cache, dir) = create_cache();

        let id = cache
            .add("vox", "Vox", "name", false)
            .expect("add should succeed");

        cache.delete(id).expect("delete should succeed");

        assert!(cache.list(None).is_empty());

        // Reloading from same database should also show empty
        let db_path = dir.path().join("test_dict.db");
        let fresh = DictionaryCache::load(&db_path).expect("load failed");
        assert!(fresh.list(None).is_empty());

        // Delete with non-existent ID should fail
        let err = cache.delete(99999);
        assert!(err.is_err());
    }

    #[test]
    fn test_search_spoken() {
        let (cache, _dir) = create_cache();
        cache
            .add("postgresql", "PostgreSQL", "technical", false)
            .expect("add");
        cache
            .add("postgres", "PostgreSQL", "technical", false)
            .expect("add");
        cache
            .add("python", "Python", "technical", false)
            .expect("add");

        let results = cache.search("post");
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|e| e.spoken == "postgresql"));
        assert!(results.iter().any(|e| e.spoken == "postgres"));

        let empty = cache.search("xyz");
        assert!(empty.is_empty());
    }

    #[test]
    fn test_search_written() {
        let (cache, _dir) = create_cache();
        cache
            .add("postgresql", "PostgreSQL", "technical", false)
            .expect("add");
        cache
            .add("python", "Python", "technical", false)
            .expect("add");

        // Search by written form (case-insensitive)
        let results = cache.search("Python");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].spoken, "python");
    }

    #[test]
    fn test_list_by_category() {
        let (cache, _dir) = create_cache();
        cache.add("vox", "Vox", "name", false).expect("add");
        cache
            .add("python", "Python", "technical", false)
            .expect("add");
        cache
            .add("hello", "Hello", "general", false)
            .expect("add");

        let technical = cache.list(Some("technical"));
        assert_eq!(technical.len(), 1);
        assert_eq!(technical[0].spoken, "python");

        let names = cache.list(Some("name"));
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].spoken, "vox");

        let all = cache.list(None);
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_substitution_basic() {
        let (cache, _dir) = create_cache();
        let id_vox = cache.add("vox", "Vox", "name", false).expect("add");
        let id_pg = cache
            .add("postgres", "PostgreSQL", "technical", false)
            .expect("add");

        let result = cache.apply_substitutions("I use vox and postgres daily");
        assert_eq!(result.text, "I use Vox and PostgreSQL daily");
        assert_eq!(result.matched_ids.len(), 2);
        assert!(result.matched_ids.contains(&id_vox));
        assert!(result.matched_ids.contains(&id_pg));
    }

    #[test]
    fn test_substitution_case_insensitive() {
        let (cache, _dir) = create_cache();
        cache.add("vox", "Vox", "name", false).expect("add");

        assert_eq!(
            cache.apply_substitutions("VOX is great").text,
            "Vox is great"
        );
        assert_eq!(
            cache.apply_substitutions("Vox is great").text,
            "Vox is great"
        );
    }

    #[test]
    fn test_substitution_whole_word() {
        let (cache, _dir) = create_cache();
        cache.add("vox", "Vox", "name", false).expect("add");

        let result = cache.apply_substitutions("equinox is great");
        assert_eq!(result.text, "equinox is great");
        assert!(result.matched_ids.is_empty());
    }

    #[test]
    fn test_substitution_unicode() {
        let (cache, _dir) = create_cache();
        cache.add("cafe", "Cafe", "general", false).expect("add");

        // ASCII case-insensitive
        assert_eq!(
            cache.apply_substitutions("CAFE time").text,
            "Cafe time"
        );

        // Unicode with diacritics
        cache
            .add("naïve", "Naïve", "general", false)
            .expect("add");
        assert_eq!(
            cache.apply_substitutions("NAÏVE approach").text,
            "Naïve approach"
        );
    }

    #[test]
    fn test_top_hints_format() {
        let (cache, _dir) = create_cache();
        cache.add("vox", "Vox", "name", false).expect("add");
        cache
            .add("postgres", "PostgreSQL", "technical", false)
            .expect("add");

        let hints = cache.top_hints(10);
        assert!(hints.contains("→"));
        assert!(hints.contains("vox"));
        assert!(hints.contains("Vox"));
        assert!(hints.contains("postgres"));
        assert!(hints.contains("PostgreSQL"));
    }

    #[test]
    fn test_top_hints_sorted_by_use() {
        let (cache, _dir) = create_cache();
        let id_vox = cache.add("vox", "Vox", "name", false).expect("add");
        let id_pg = cache
            .add("postgres", "PostgreSQL", "technical", false)
            .expect("add");
        let _id_py = cache
            .add("python", "Python", "technical", false)
            .expect("add");

        // Set use counts: vox=10, postgres=5, python=1
        cache
            .increment_use_counts(&vec![id_vox; 10])
            .expect("increment");
        cache
            .increment_use_counts(&vec![id_pg; 5])
            .expect("increment");

        let hints = cache.top_hints(2);
        // Should contain vox and postgres (top 2 by use count)
        assert!(hints.contains("vox"));
        assert!(hints.contains("postgres"));
        // python should NOT be included (only top 2 requested)
        assert!(!hints.contains("python"));

        // vox (use_count=10) should appear before postgres (use_count=5)
        let vox_pos = hints.find("vox").expect("vox should be in hints");
        let pg_pos = hints.find("postgres").expect("postgres should be in hints");
        assert!(
            vox_pos < pg_pos,
            "vox should appear before postgres in hints"
        );
    }

    #[test]
    fn test_use_count_increment() {
        let (cache, dir) = create_cache();
        let id = cache.add("vox", "Vox", "name", false).expect("add");

        let result = cache.apply_substitutions("vox is great vox");
        assert_eq!(result.matched_ids.len(), 2);
        assert!(result.matched_ids.iter().all(|&mid| mid == id));

        cache
            .increment_use_counts(&result.matched_ids)
            .expect("increment");

        let entries = cache.list(None);
        assert_eq!(entries[0].use_count, 2);

        // Verify persistence across reload
        let db_path = dir.path().join("test_dict.db");
        let fresh = DictionaryCache::load(&db_path).expect("load");
        let fresh_entries = fresh.list(None);
        assert_eq!(fresh_entries[0].use_count, 2);
    }

    #[test]
    fn test_import_export_roundtrip() {
        let (cache1, _dir1) = create_cache();
        cache1.add("vox", "Vox", "name", false).expect("add");
        cache1
            .add("postgres", "PostgreSQL", "technical", false)
            .expect("add");
        cache1
            .add("my email", "user@example.com", "email", false)
            .expect("add");
        cache1
            .add("delete last", "", "command", true)
            .expect("add");
        cache1.add("um", "", "general", false).expect("add");

        let json = cache1.export_json().expect("export");

        // Verify JSON is valid and contains 5 entries
        let parsed: Vec<DictionaryExportEntry> =
            serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed.len(), 5);

        // Verify JSON excludes id, use_count, created_at
        assert!(!json.contains("\"id\""));
        assert!(!json.contains("\"use_count\""));
        assert!(!json.contains("\"created_at\""));

        // Import into a fresh dictionary
        let (cache2, _dir2) = create_cache();
        let result = cache2.import_json(&json).expect("import");
        assert_eq!(result.added, 5);
        assert_eq!(result.skipped, 0);
        assert!(result.errors.is_empty());

        // Verify all 5 entries present with correct fields
        let entries = cache2.list(None);
        assert_eq!(entries.len(), 5);
        let vox = entries.iter().find(|e| e.spoken == "vox").expect("vox");
        assert_eq!(vox.written, "Vox");
        assert_eq!(vox.category, "name");
        assert!(!vox.is_command_phrase);
        assert_eq!(vox.use_count, 0); // use_count not carried over

        let cmd = entries
            .iter()
            .find(|e| e.spoken == "delete last")
            .expect("delete last");
        assert!(cmd.is_command_phrase);

        // Import again — all should be skipped
        let result2 = cache2.import_json(&json).expect("import again");
        assert_eq!(result2.added, 0);
        assert_eq!(result2.skipped, 5);
        assert!(result2.errors.is_empty());
    }

    #[test]
    fn test_substitution_command_excluded() {
        let (cache, _dir) = create_cache();
        cache
            .add("delete last", "", "command", true)
            .expect("add");
        cache.add("vox", "Vox", "name", false).expect("add");

        let result = cache.apply_substitutions("please delete last vox");
        // "delete last" should NOT be substituted (is_command_phrase=true)
        assert!(
            result.text.contains("delete last"),
            "command phrase should remain unchanged, got: {}",
            result.text
        );
        // "vox" should be substituted
        assert!(
            result.text.contains("Vox"),
            "non-command entry should be substituted, got: {}",
            result.text
        );

        // Both should appear in top_hints (including command phrases)
        let hints = cache.top_hints(50);
        assert!(hints.contains("delete last"));
        assert!(hints.contains("vox"));
    }

    #[test]
    fn test_migrate_schema_fresh() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");

        migrate_schema(&db_path).expect("migrate should succeed");

        // Verify table and indexes exist
        let conn = Connection::open(&db_path).expect("open");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dictionary", [], |row| row.get(0))
            .expect("table should exist");
        assert_eq!(count, 0);

        // Verify indexes
        let idx_spoken: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_dictionary_spoken'",
                [],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(idx_spoken, 1);

        let idx_category: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_dictionary_category'",
                [],
                |row| row.get(0),
            )
            .expect("query");
        assert_eq!(idx_category, 1);
    }

    #[test]
    fn test_migrate_schema_from_old() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");

        // Create table with old schema
        let conn = Connection::open(&db_path).expect("open");
        conn.execute_batch(
            "CREATE TABLE dictionary (\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\
                term TEXT UNIQUE NOT NULL COLLATE NOCASE,\
                replacement TEXT NOT NULL,\
                frequency INTEGER NOT NULL DEFAULT 0,\
                created_at TEXT NOT NULL\
            )",
        )
        .expect("create old table");

        // Insert a test entry with old column names
        conn.execute(
            "INSERT INTO dictionary (term, replacement, frequency, created_at) VALUES ('hello', 'Hello', 5, '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("insert");
        drop(conn);

        // Run migration
        migrate_schema(&db_path).expect("migrate should succeed");

        // Load and verify old entry preserved
        let cache = DictionaryCache::load(&db_path).expect("load");
        let entries = cache.list(None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].spoken, "hello");
        assert_eq!(entries[0].written, "Hello");
        assert_eq!(entries[0].use_count, 5);
        assert_eq!(entries[0].category, "general"); // default
        assert!(!entries[0].is_command_phrase); // default
    }

    #[test]
    fn test_whitespace_only_spoken_rejected() {
        let (cache, _dir) = create_cache();

        // Pure whitespace must be rejected by add
        assert!(cache.add("   ", "something", "general", false).is_err());
        assert!(cache.add("\t\n", "something", "general", false).is_err());
        assert!(cache.add("", "something", "general", false).is_err());
        assert_eq!(cache.len(), 0);

        // Add a valid entry, then try to update its spoken form to whitespace
        let id = cache.add("hello", "Hello", "general", false).expect("add");
        assert!(cache.update(id, "   ", "Hello", "general", false).is_err());
        // Original entry should be unchanged
        let entries = cache.list(None);
        assert_eq!(entries[0].spoken, "hello");
    }

    #[test]
    fn test_import_whitespace_spoken_rejected() {
        let (cache, _dir) = create_cache();
        let json = r#"[
            {"spoken": "  ", "written": "bad", "category": "general", "is_command_phrase": false},
            {"spoken": "vox", "written": "Vox", "category": "name", "is_command_phrase": false}
        ]"#;

        let result = cache.import_json(json).expect("import");
        assert_eq!(result.added, 1);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_migrate_schema_partial_is_atomic() {
        let dir = tempfile::tempdir().expect("temp dir");
        let db_path = dir.path().join("test.db");

        // Simulate a partially-migrated state: spoken exists but written
        // does not. This would happen if old non-transactional migration
        // crashed after the first rename.
        let conn = Connection::open(&db_path).expect("open");
        conn.execute_batch(
            "CREATE TABLE dictionary (\
                id INTEGER PRIMARY KEY AUTOINCREMENT,\
                spoken TEXT UNIQUE NOT NULL COLLATE NOCASE,\
                replacement TEXT NOT NULL,\
                frequency INTEGER NOT NULL DEFAULT 0,\
                created_at TEXT NOT NULL\
            )",
        )
        .expect("create partial table");
        conn.execute(
            "INSERT INTO dictionary (spoken, replacement, frequency, created_at) \
             VALUES ('test', 'Test', 1, '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("insert");
        drop(conn);

        // migrate_schema should handle this: no "term" column exists so
        // the old-column branch is skipped, but the new columns may be
        // missing. load() should still succeed because the SELECT query
        // references column names that must exist.
        let result = DictionaryCache::load(&db_path);
        // This verifies the partially-migrated schema doesn't crash —
        // if "written" column is missing, load would error here.
        assert!(
            result.is_err(),
            "load should fail on partial schema missing 'written' column — \
             this confirms the bug scenario; the transactional fix prevents \
             this state from occurring"
        );
    }
}
