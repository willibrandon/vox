# Data Model: Custom Dictionary

**Input**: spec.md functional requirements, research.md technical decisions
**Date**: 2026-02-21

## Entities

### DictionaryEntry

A user-defined mapping from a spoken form to a written form. Used for text substitution during dictation and for LLM hint generation.

| Field | Type | Constraints | Description |
|---|---|---|---|
| id | i64 | Primary key, auto-increment | Unique identifier |
| spoken | String | Unique (COLLATE NOCASE), not null | The term the user speaks during dictation |
| written | String | Not null | The replacement text to inject |
| category | String | Not null, default "general" | Freeform grouping label (general, name, technical, email, etc.) |
| is_command_phrase | bool | Not null, default false | If true, excluded from text substitution but included in LLM hints |
| use_count | u64 | Not null, default 0 | Number of times matched during text substitution |
| created_at | String | Not null, ISO 8601 | Creation timestamp |

**Uniqueness**: Case-insensitive on `spoken` field via SQLite `COLLATE NOCASE`. Adding an entry with a spoken form that matches an existing one (any case) is rejected with an error.

**Validation Rules**:
- `spoken` must not be empty
- `spoken` must be unique (case-insensitive)
- `written` may be empty (used for filler word removal: "um" → "")
- `category` defaults to "general" if not specified
- `use_count` starts at 0 and only increments via substitution matches

**Indexes**:
- `idx_dictionary_spoken` on `spoken` — fast uniqueness checks and spoken-form lookups
- `idx_dictionary_category` on `category` — fast category filtering for list operations

**Derives**: `Clone`, `Debug`, `Serialize`, `Deserialize`

### SubEntry (internal)

Optimized entry stored in the substitution lookup maps. Not public.

| Field | Type | Description |
|---|---|---|
| id | i64 | Entry ID for use count tracking |
| written | String | The replacement text |

### SubstitutionResult

The result of applying dictionary substitutions to text.

| Field | Type | Description |
|---|---|---|
| text | String | The text after all substitutions applied |
| matched_ids | Vec\<i64\> | IDs of dictionary entries that were matched (with duplicates for repeated matches) |

**Derives**: `Clone`, `Debug`

### DictionaryExportEntry

A serializable subset of DictionaryEntry for import/export operations.

| Field | Type | Description |
|---|---|---|
| spoken | String | The spoken form |
| written | String | The written replacement |
| category | String | Category label |
| is_command_phrase | bool | Command phrase flag |

**Excludes**: `id` (auto-generated), `use_count` (per-installation), `created_at` (set on import)

**Derives**: `Clone`, `Debug`, `Serialize`, `Deserialize`

### ImportResult

The outcome of a batch import operation. Not persisted.

| Field | Type | Description |
|---|---|---|
| added | usize | Count of entries successfully imported |
| skipped | usize | Count of entries skipped due to duplicate spoken forms |
| errors | Vec\<String\> | Descriptions of entries that failed validation |

**Derives**: `Clone`, `Debug`

## Relationships

- DictionaryEntry is standalone — no foreign key relationships to other tables
- DictionaryEntry lives in the same SQLite database file (vox.db) as transcript entries
- Multiple DictionaryEntries share the same category value (logical one-to-many: category → entries)

## SQLite Schema (New)

```sql
CREATE TABLE IF NOT EXISTS dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    spoken TEXT UNIQUE NOT NULL COLLATE NOCASE,
    written TEXT NOT NULL,
    category TEXT NOT NULL DEFAULT 'general',
    is_command_phrase INTEGER NOT NULL DEFAULT 0,
    use_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_dictionary_spoken ON dictionary(spoken);
CREATE INDEX IF NOT EXISTS idx_dictionary_category ON dictionary(category);
```

## Migration from Existing Schema

The existing dictionary table (created by Feature 007/009) uses different column names:

| Old Column | New Column | Migration |
|---|---|---|
| term | spoken | `ALTER TABLE dictionary RENAME COLUMN term TO spoken` |
| replacement | written | `ALTER TABLE dictionary RENAME COLUMN replacement TO written` |
| frequency | use_count | `ALTER TABLE dictionary RENAME COLUMN frequency TO use_count` |
| *(absent)* | category | `ALTER TABLE dictionary ADD COLUMN category TEXT NOT NULL DEFAULT 'general'` |
| *(absent)* | is_command_phrase | `ALTER TABLE dictionary ADD COLUMN is_command_phrase INTEGER NOT NULL DEFAULT 0` |
| created_at | created_at | No change |

Migration is idempotent — `migrate_schema()` checks current column names via `PRAGMA table_info(dictionary)` before applying changes. If the table already has `spoken` column, migration is skipped. If the table doesn't exist, `CREATE TABLE IF NOT EXISTS` creates it with the new schema directly.

## Internal Cache Structure

```text
DictionaryCache
├── db_path: Option<PathBuf>
├── entries: Arc<RwLock<HashMap<String, DictionaryEntry>>>     ← keyed by spoken (lowercase)
├── word_subs: Arc<RwLock<HashMap<String, SubEntry>>>          ← single-word, excludes command phrases
└── phrase_subs: Arc<RwLock<Vec<(String, SubEntry)>>>          ← multi-word sorted longest-first, excludes command phrases
```

All three Arc fields are cloned together when DictionaryCache is cloned. Clones share the same underlying RwLock, so mutations from one clone are visible to all others.
