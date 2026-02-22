//! User-defined vocabulary substitution cache for the Vox dictation engine.
//!
//! Loads dictionary entries from SQLite into memory and provides fast
//! substitution during dictation. Single-word lookups use O(1) HashMap,
//! multi-word phrases use longest-first string replacement. The cache is
//! cheaply cloneable via Arc-wrapped internals.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use rusqlite::Connection;

/// A single dictionary entry stored in SQLite and loaded into memory.
#[derive(Clone, Debug)]
pub struct DictionaryEntry {
    /// Auto-increment primary key.
    pub id: i64,
    /// The term to match (case-insensitive). May be single-word or multi-word phrase.
    pub term: String,
    /// The replacement text.
    pub replacement: String,
    /// Usage frequency count (for ranking hints).
    pub frequency: u32,
    /// When this entry was created (ISO 8601).
    pub created_at: String,
}

/// In-memory cache of user-defined vocabulary substitutions and LLM hints.
///
/// Loaded from SQLite on startup. Clone is cheap (Arc-wrapped internals).
/// Supports both single-word terms (O(1) HashMap lookup) and multi-word
/// phrases (longest-first string replacement).
#[derive(Clone)]
pub struct DictionaryCache {
    /// Single-word substitutions. Lowercase key → replacement.
    word_substitutions: Arc<HashMap<String, String>>,
    /// Multi-word phrase substitutions. Sorted by descending word count
    /// (longest first) to prevent partial matches.
    phrase_substitutions: Arc<Vec<(String, String)>>,
    /// All entries sorted by frequency descending (for top_hints).
    hints: Arc<Vec<DictionaryEntry>>,
}

/// SQL statement to create the dictionary table.
///
/// Shared between `DictionaryCache::load` and `state::init_database` to
/// prevent schema drift from duplicated DDL.
pub const CREATE_TABLE_SQL: &str = "\
    CREATE TABLE IF NOT EXISTS dictionary (\
        id INTEGER PRIMARY KEY AUTOINCREMENT,\
        term TEXT UNIQUE NOT NULL COLLATE NOCASE,\
        replacement TEXT NOT NULL,\
        frequency INTEGER NOT NULL DEFAULT 0,\
        created_at TEXT NOT NULL\
    )";

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

impl DictionaryCache {
    /// Load dictionary from SQLite database at the given path.
    ///
    /// Creates the table if it doesn't exist. Entries with whitespace in the
    /// term go into phrase_substitutions; single-word entries go into
    /// word_substitutions.
    pub fn load(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open dictionary database at {}", db_path.display()))?;
        conn.execute_batch(CREATE_TABLE_SQL)
            .context("failed to create dictionary table")?;

        Self::load_from_connection(&conn)
    }

    /// Create an empty dictionary cache (no substitutions, no hints).
    pub fn empty() -> Self {
        Self {
            word_substitutions: Arc::new(HashMap::new()),
            phrase_substitutions: Arc::new(Vec::new()),
            hints: Arc::new(Vec::new()),
        }
    }

    /// Apply substitutions to the given text using a two-pass algorithm:
    /// 1. Phrase pass: replace multi-word phrases (longest-first, case-insensitive)
    /// 2. Word pass: split on whitespace, replace single words (O(1) HashMap lookup)
    ///
    /// Returns the original text unchanged if no substitutions match.
    /// If a substitution produces an empty replacement, the matched text is
    /// removed. If the entire result is empty or whitespace-only after
    /// substitution, returns an empty string.
    pub fn apply_substitutions(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }

        // Pass 1: Replace multi-word phrases (longest-first, case-insensitive)
        let mut result = text.to_string();
        for (phrase, replacement) in self.phrase_substitutions.iter() {
            let lowercase_phrase = phrase.to_lowercase();
            let mut new_result = String::with_capacity(result.len());
            let mut remaining = result.as_str();

            loop {
                let remaining_lower = remaining.to_lowercase();
                match remaining_lower.find(&lowercase_phrase) {
                    Some(lc_pos) => {
                        // Map byte positions from the lowercased string back to the
                        // original. to_lowercase() can change byte lengths for some
                        // Unicode characters (e.g., İ → i̇), so we walk char-by-char.
                        let orig_start = lowercase_byte_to_original(remaining, lc_pos);
                        let orig_end =
                            lowercase_byte_to_original(remaining, lc_pos + lowercase_phrase.len());
                        new_result.push_str(&remaining[..orig_start]);
                        new_result.push_str(replacement);
                        remaining = &remaining[orig_end..];
                    }
                    None => {
                        new_result.push_str(remaining);
                        break;
                    }
                }
            }

            result = new_result;
        }

        // Pass 2: Replace single words via HashMap lookup
        let words: Vec<&str> = result.split_whitespace().collect();
        let replaced: Vec<String> = words
            .iter()
            .map(|word| {
                let lower = word.to_lowercase();
                match self.word_substitutions.get(&lower) {
                    Some(replacement) => replacement.clone(),
                    None => word.to_string(),
                }
            })
            .collect();

        let joined: String = replaced
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        joined.trim().to_string()
    }

    /// Format the top N dictionary entries by frequency as a string
    /// for the LLM `dictionary_hints` parameter.
    ///
    /// Format: "term1 → replacement1, term2 → replacement2, ..."
    pub fn top_hints(&self, n: usize) -> String {
        self.hints
            .iter()
            .take(n)
            .map(|entry| format!("{} → {}", entry.term, entry.replacement))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Reload the cache from SQLite, replacing all in-memory data.
    ///
    /// Called when the user adds/edits/removes dictionary entries via the UI.
    /// The running pipeline holds a Clone of DictionaryCache (Arc internals),
    /// so a reload creates new Arc allocations. The pipeline's next call to
    /// apply_substitutions() uses the old snapshot until it re-clones.
    pub fn reload(&mut self, db_path: &Path) -> Result<()> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open dictionary database at {}", db_path.display()))?;
        let fresh = Self::load_from_connection(&conn)?;
        self.word_substitutions = fresh.word_substitutions;
        self.phrase_substitutions = fresh.phrase_substitutions;
        self.hints = fresh.hints;
        Ok(())
    }

    /// Number of entries in the cache (words + phrases).
    pub fn len(&self) -> usize {
        self.word_substitutions.len() + self.phrase_substitutions.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.word_substitutions.is_empty() && self.phrase_substitutions.is_empty()
    }

    fn load_from_connection(conn: &Connection) -> Result<Self> {
        let mut stmt = conn
            .prepare("SELECT id, term, replacement, frequency, created_at FROM dictionary ORDER BY frequency DESC")
            .context("failed to prepare dictionary query")?;

        let entries: Vec<DictionaryEntry> = stmt
            .query_map([], |row| {
                Ok(DictionaryEntry {
                    id: row.get(0)?,
                    term: row.get(1)?,
                    replacement: row.get(2)?,
                    frequency: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .context("failed to query dictionary")?
            .collect::<std::result::Result<Vec<_>, _>>()
            .context("failed to read dictionary row")?;

        let mut word_subs = HashMap::new();
        let mut phrase_subs = Vec::new();

        for entry in &entries {
            if entry.term.contains(char::is_whitespace) {
                phrase_subs.push((entry.term.clone(), entry.replacement.clone()));
            } else {
                word_subs.insert(entry.term.to_lowercase(), entry.replacement.clone());
            }
        }

        // Sort phrases by descending word count (longest first)
        phrase_subs.sort_by(|a, b| {
            let a_words = a.0.split_whitespace().count();
            let b_words = b.0.split_whitespace().count();
            b_words.cmp(&a_words)
        });

        Ok(Self {
            word_substitutions: Arc::new(word_subs),
            phrase_substitutions: Arc::new(phrase_subs),
            hints: Arc::new(entries),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_db(entries: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test_dict.db");
        let conn = Connection::open(&db_path).expect("failed to open test db");
        conn.execute_batch(CREATE_TABLE_SQL)
            .expect("failed to create table");

        for (term, replacement) in entries {
            conn.execute(
                "INSERT INTO dictionary (term, replacement, frequency, created_at) VALUES (?1, ?2, 0, '2026-01-01T00:00:00Z')",
                rusqlite::params![term, replacement],
            )
            .expect("failed to insert test entry");
        }

        (dir, db_path)
    }

    #[test]
    fn test_two_pass_substitution_ordering() {
        // Phrase substitution must run before word substitution
        let (_dir, db_path) = create_test_db(&[
            ("New York City", "NYC"),
            ("new", "fresh"),
        ]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        // "New York City" phrase match should take priority, "new" word match shouldn't apply to it
        let result = cache.apply_substitutions("I love New York City and new ideas");
        assert_eq!(result, "I love NYC and fresh ideas");
    }

    #[test]
    fn test_case_insensitive_matching() {
        let (_dir, db_path) = create_test_db(&[("hello", "hi")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(cache.apply_substitutions("HELLO world"), "hi world");
        assert_eq!(cache.apply_substitutions("Hello world"), "hi world");
        assert_eq!(cache.apply_substitutions("hello world"), "hi world");
    }

    #[test]
    fn test_empty_replacement_removes_text() {
        let (_dir, db_path) = create_test_db(&[("um", ""), ("uh", "")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(cache.apply_substitutions("um let's uh meet"), "let's meet");
    }

    #[test]
    fn test_all_words_removed_returns_empty() {
        let (_dir, db_path) = create_test_db(&[("um", ""), ("uh", "")]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(cache.apply_substitutions("um uh"), "");
    }

    #[test]
    fn test_phrase_longest_first_priority() {
        let (_dir, db_path) = create_test_db(&[
            ("New York", "NY"),
            ("New York City", "NYC"),
        ]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        // "New York City" is longer and should match first
        assert_eq!(cache.apply_substitutions("Visit New York City"), "Visit NYC");
        // "New York" alone should still match
        assert_eq!(cache.apply_substitutions("Visit New York"), "Visit NY");
    }

    #[test]
    fn test_round_trip_load_from_sqlite() {
        let (_dir, db_path) = create_test_db(&[
            ("rust", "Rust"),
            ("hello world", "greetings"),
        ]);
        let cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(cache.len(), 2);
        assert!(!cache.is_empty());
        assert_eq!(cache.apply_substitutions("I love rust and hello world"), "I love Rust and greetings");
    }

    #[test]
    fn test_empty_cache() {
        let cache = DictionaryCache::empty();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.apply_substitutions("hello world"), "hello world");
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
        assert_eq!(cache.apply_substitutions("hello world"), "hello world");
    }

    #[test]
    fn test_reload() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = dir.path().join("test_dict.db");
        let conn = Connection::open(&db_path).expect("failed to open test db");
        conn.execute_batch(CREATE_TABLE_SQL)
            .expect("failed to create table");
        conn.execute(
            "INSERT INTO dictionary (term, replacement, frequency, created_at) VALUES ('foo', 'bar', 0, '2026-01-01T00:00:00Z')",
            [],
        ).expect("insert failed");
        drop(conn);

        let mut cache = DictionaryCache::load(&db_path).expect("load failed");
        assert_eq!(cache.len(), 1);

        // Add another entry
        let conn = Connection::open(&db_path).expect("failed to open test db");
        conn.execute(
            "INSERT INTO dictionary (term, replacement, frequency, created_at) VALUES ('baz', 'qux', 0, '2026-01-01T00:00:00Z')",
            [],
        ).expect("insert failed");
        drop(conn);

        cache.reload(&db_path).expect("reload failed");
        assert_eq!(cache.len(), 2);
    }
}
