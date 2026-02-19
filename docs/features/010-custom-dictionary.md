# Feature 010: Custom Dictionary

**Status:** Not Started
**Dependencies:** 009-application-state-settings
**Design Reference:** Section 9 (Custom Dictionary)
**Estimated Scope:** SQLite schema, dictionary cache, CRUD operations, LLM hint integration

---

## Overview

Implement the custom dictionary system that lets users define spoken-to-written word mappings. The dictionary serves two purposes: (1) direct substitution during pipeline processing, and (2) LLM hints that bias the post-processor toward user-preferred spellings. Examples: "vox" → "Vox", "my email" → "engineer@example.com", technical jargon, names, abbreviations.

---

## Requirements

### FR-001: SQLite Schema

```sql
CREATE TABLE IF NOT EXISTS dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    spoken TEXT NOT NULL UNIQUE,        -- What the user says (lowercase)
    written TEXT NOT NULL,              -- What gets injected
    category TEXT DEFAULT 'general',    -- Grouping: general, name, technical, email, etc.
    is_command_phrase INTEGER DEFAULT 0, -- Excluded from text substitution
    use_count INTEGER DEFAULT 0,        -- Tracks usage for hint prioritization
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_dictionary_spoken ON dictionary(spoken);
CREATE INDEX IF NOT EXISTS idx_dictionary_category ON dictionary(category);
```

**rusqlite 0.38 note:** Use `String` (ISO 8601) for `created_at`, not `chrono::DateTime<Utc>` (no `FromSql` impl).

### FR-002: Dictionary Cache

In-memory HashMap for O(1) lookups during pipeline processing:

```rust
// crates/vox_core/src/dictionary/store.rs

use parking_lot::RwLock;
use std::collections::HashMap;

pub struct DictionaryCache {
    /// spoken (lowercase) → written, O(1) lookup
    cache: RwLock<HashMap<String, DictionaryEntry>>,
}

pub struct DictionaryEntry {
    pub id: i64,
    pub spoken: String,
    pub written: String,
    pub category: String,
    pub is_command_phrase: bool,
    pub use_count: u64,
}
```

The cache is loaded from SQLite at startup and kept in sync on modifications.

### FR-003: CRUD Operations

```rust
impl DictionaryCache {
    /// Load all entries from SQLite into memory.
    pub fn load(db: &rusqlite::Connection) -> Result<Self>;

    /// Add a new dictionary entry. Updates both SQLite and cache.
    pub fn add(&self, db: &rusqlite::Connection, spoken: &str, written: &str, category: &str) -> Result<i64>;

    /// Update an existing entry.
    pub fn update(&self, db: &rusqlite::Connection, id: i64, spoken: &str, written: &str, category: &str) -> Result<()>;

    /// Delete an entry.
    pub fn delete(&self, db: &rusqlite::Connection, id: i64) -> Result<()>;

    /// Get all entries, optionally filtered by category.
    pub fn list(&self, category: Option<&str>) -> Vec<DictionaryEntry>;

    /// Search entries by spoken or written text.
    pub fn search(&self, query: &str) -> Vec<DictionaryEntry>;
}
```

### FR-004: Text Substitution

During pipeline processing, apply direct substitutions before LLM processing:

```rust
impl DictionaryCache {
    /// Apply dictionary substitutions to raw transcript text.
    /// Only applies entries where is_command_phrase == false.
    /// Case-insensitive matching on the spoken form.
    pub fn apply_substitutions(&self, text: &str) -> String;
}
```

Substitution rules:
- Case-insensitive matching on the spoken form
- Whole-word matching only (don't match "vox" inside "equinox")
- Entries marked `is_command_phrase = true` are excluded from substitution
- Substitution happens before LLM processing

### FR-005: LLM Hint Integration

Top dictionary entries are injected into the LLM system prompt as hints:

```rust
impl DictionaryCache {
    /// Get the top N entries formatted as LLM hints.
    /// Sorted by use_count (most used first).
    pub fn top_hints(&self, n: usize) -> String;
}
```

Output format for LLM prompt:

```
Custom dictionary (apply these substitutions):
- "vox" → "Vox"
- "my email" → "engineer@example.com"
- "postgres" → "PostgreSQL"
```

Limited to top 50 entries to keep the prompt under the context window budget.

### FR-006: Use Count Tracking

Increment `use_count` each time a dictionary entry is applied (either via substitution or LLM hint usage):

```rust
impl DictionaryCache {
    pub fn increment_use_count(&self, db: &rusqlite::Connection, id: i64) -> Result<()>;
}
```

Use count determines hint priority — frequently used entries appear first in the LLM prompt.

### FR-007: Import/Export

```rust
impl DictionaryCache {
    /// Export all entries as JSON for backup.
    pub fn export_json(&self) -> Result<String>;

    /// Import entries from JSON, merging with existing.
    pub fn import_json(&self, db: &rusqlite::Connection, json: &str) -> Result<ImportResult>;
}

pub struct ImportResult {
    pub added: usize,
    pub skipped: usize,  // Duplicate spoken forms
    pub errors: Vec<String>,
}
```

### FR-008: Command Phrase Exclusion

Entries marked `is_command_phrase = true` are treated differently:
- NOT applied during text substitution
- Still included as LLM hints (so the LLM knows about them)
- Used for phrases that should trigger voice commands, not text injection

---

## Acceptance Criteria

- [ ] Dictionary loads from SQLite into memory cache
- [ ] Add/update/delete entries persist to SQLite and update cache
- [ ] Text substitution applies correct replacements
- [ ] Whole-word matching only (no partial matches)
- [ ] Case-insensitive matching works
- [ ] Command phrase entries excluded from substitution
- [ ] Top hints format correctly for LLM prompt
- [ ] Use count increments on substitution
- [ ] Import/export round-trip preserves all data
- [ ] Search finds entries by spoken or written text
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_add_entry` | Add entry, verify in cache and SQLite |
| `test_update_entry` | Update entry, verify changes propagated |
| `test_delete_entry` | Delete entry, verify removed from cache and SQLite |
| `test_substitution_basic` | "vox" → "Vox" in text |
| `test_substitution_case_insensitive` | "VOX" also matches |
| `test_substitution_whole_word` | "equinox" does not match "vox" entry |
| `test_substitution_command_excluded` | Command phrases not substituted |
| `test_top_hints_format` | Correct LLM prompt format |
| `test_top_hints_sorted_by_use` | Most-used entries first |
| `test_use_count_increment` | Count increments on substitution |
| `test_import_export_roundtrip` | Export then import preserves data |
| `test_search_spoken` | Search finds by spoken form |
| `test_search_written` | Search finds by written form |

---

## Performance Targets

| Metric | Target |
|---|---|
| Cache load (1000 entries) | < 50 ms |
| Single substitution | < 1 ms |
| Full text substitution (100 words) | < 5 ms |
| Add/update/delete | < 10 ms |
| Search (1000 entries) | < 10 ms |
