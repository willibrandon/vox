# Public API: Custom Dictionary

**Input**: spec.md requirements, research.md decisions, data-model.md entities
**Date**: 2026-02-21

## Module: `crate::dictionary`

### Public Types

```rust
/// A single dictionary entry mapping a spoken form to a written form.
///
/// Loaded from SQLite into the in-memory cache. Used for text substitution
/// during dictation and for LLM hint generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub id: i64,
    pub spoken: String,
    pub written: String,
    pub category: String,
    pub is_command_phrase: bool,
    pub use_count: u64,
    pub created_at: String,
}

/// The result of applying dictionary substitutions to text.
///
/// Contains the substituted text and the IDs of all matched entries.
/// Matched IDs may contain duplicates if the same entry matches multiple
/// times (e.g., "vox" appearing twice in input).
#[derive(Clone, Debug)]
pub struct SubstitutionResult {
    pub text: String,
    pub matched_ids: Vec<i64>,
}

/// The outcome of a batch import operation.
#[derive(Clone, Debug)]
pub struct ImportResult {
    pub added: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

/// A serializable entry for import/export operations.
///
/// Excludes id, use_count, and created_at which are installation-specific.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DictionaryExportEntry {
    pub spoken: String,
    pub written: String,
    pub category: String,
    pub is_command_phrase: bool,
}
```

### Public Constants

```rust
/// SQL to create the dictionary table with the current schema.
///
/// Shared between DictionaryCache::load and state::init_database to
/// prevent schema drift from duplicated DDL.
pub const CREATE_TABLE_SQL: &str = "...";
```

### Public Free Functions

```rust
/// Migrate the dictionary table schema from old column names to new.
///
/// Inspects current columns via PRAGMA table_info. If old column names
/// (term, replacement, frequency) are found, renames them and adds
/// category + is_command_phrase columns. Idempotent — safe to call on
/// both fresh and already-migrated databases.
pub fn migrate_schema(db_path: &Path) -> Result<()>;
```

### DictionaryCache Methods

```rust
/// In-memory cache of user-defined vocabulary substitutions and LLM hints.
///
/// Loaded from SQLite on startup. Clone is cheap (Arc-wrapped internals
/// with shared RwLock state). Supports CRUD operations that synchronize
/// both in-memory state and SQLite storage.
#[derive(Clone)]
pub struct DictionaryCache { /* Arc<RwLock<...>> fields */ }

impl DictionaryCache {
    // ── Construction ────────────────────────────────────────────

    /// Load dictionary from SQLite database at the given path.
    ///
    /// Reads all entries into memory and builds optimized substitution
    /// maps. The db_path is stored for subsequent CRUD operations.
    pub fn load(db_path: &Path) -> Result<Self>;

    /// Create an empty dictionary cache with no database backing.
    ///
    /// CRUD operations will return an error on this cache. Intended
    /// for test helpers that only need substitution behavior.
    pub fn empty() -> Self;

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
    ) -> Result<i64>;

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
    ) -> Result<()>;

    /// Delete a dictionary entry by ID.
    ///
    /// Removes from both SQLite and the in-memory cache. Errors if
    /// the ID doesn't exist.
    pub fn delete(&self, id: i64) -> Result<()>;

    /// List all entries, optionally filtered by category.
    ///
    /// Returns entries sorted by spoken form. If category is Some,
    /// only entries matching that category are returned (case-sensitive
    /// category match).
    pub fn list(&self, category: Option<&str>) -> Vec<DictionaryEntry>;

    /// Search entries by partial match on spoken or written text.
    ///
    /// Returns entries where the query appears as a substring in either
    /// the spoken or written field (case-insensitive). Returns entries
    /// sorted by spoken form.
    pub fn search(&self, query: &str) -> Vec<DictionaryEntry>;

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
    pub fn apply_substitutions(&self, text: &str) -> SubstitutionResult;

    /// Format the top N dictionary entries by use count as hints for
    /// the LLM post-processor's system prompt.
    ///
    /// Includes ALL entries (including command phrases). Sorted by
    /// use_count descending. Limited to at most n entries.
    /// Format: "spoken1 → written1, spoken2 → written2, ..."
    pub fn top_hints(&self, n: usize) -> String;

    /// Increment use counts for the given entry IDs.
    ///
    /// Updates both in-memory entries and SQLite. IDs may contain
    /// duplicates — each occurrence increments the count by 1.
    /// Uses a single SQLite transaction for efficiency.
    pub fn increment_use_counts(&self, ids: &[i64]) -> Result<()>;

    // ── Import/Export ───────────────────────────────────────────

    /// Export all dictionary entries as a JSON string.
    ///
    /// Exports spoken, written, category, and is_command_phrase fields.
    /// Excludes id, use_count, and created_at (installation-specific).
    pub fn export_json(&self) -> Result<String>;

    /// Import dictionary entries from a JSON string.
    ///
    /// Entries with spoken forms that already exist are skipped (not
    /// overwritten). Entries with empty spoken forms are reported as
    /// errors. Returns an ImportResult with counts of added, skipped,
    /// and errored entries.
    pub fn import_json(&self, json: &str) -> Result<ImportResult>;

    // ── Accessors ───────────────────────────────────────────────

    /// Number of entries in the cache.
    pub fn len(&self) -> usize;

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool;
}
```

### Integration Points

#### state.rs — VoxState

```rust
impl VoxState {
    /// Access the dictionary cache.
    ///
    /// The returned reference shares mutable state with the pipeline's
    /// clone — CRUD changes are immediately visible to substitution.
    pub fn dictionary(&self) -> &DictionaryCache;
}
```

`VoxState::new()` calls `migrate_schema()` then `DictionaryCache::load()` during initialization. The `init_database()` function is updated to use `migrate_schema()` instead of raw `CREATE TABLE` SQL.

#### pipeline/orchestrator.rs — Pipeline

`Pipeline::new()` receives `DictionaryCache` from VoxState (via clone). The `process_segment()` method changes:

```rust
// Before (current code):
let substituted = self.dictionary.apply_substitutions(&raw_text);
let hints = self.dictionary.top_hints(50);

// After (refactored):
let result = self.dictionary.apply_substitutions(&raw_text);
if !result.matched_ids.is_empty() {
    if let Err(e) = self.dictionary.increment_use_counts(&result.matched_ids) {
        tracing::warn!("failed to increment dictionary use counts: {e}");
    }
}
let hints = self.dictionary.top_hints(50);
```

## Test Requirements

All 13 tests from spec.md plus the existing 10 tests in dictionary.rs. Existing tests are updated for the new API (SubstitutionResult return type, new DictionaryEntry fields).

| Test | Validates |
|---|---|
| `test_add_entry` | FR-001, FR-002, FR-003, FR-004 |
| `test_update_entry` | FR-003, FR-004 |
| `test_delete_entry` | FR-003, FR-004 |
| `test_substitution_basic` | FR-005 |
| `test_substitution_case_insensitive` | FR-005 |
| `test_substitution_whole_word` | FR-005 |
| `test_substitution_command_excluded` | FR-007 |
| `test_top_hints_format` | FR-008, FR-009 |
| `test_top_hints_sorted_by_use` | FR-009 |
| `test_use_count_increment` | FR-010 |
| `test_import_export_roundtrip` | FR-011, FR-012 |
| `test_search_spoken` | FR-014 |
| `test_search_written` | FR-014 |
