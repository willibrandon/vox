# Research: Custom Dictionary

**Input**: Technical context from plan.md, existing code in dictionary.rs, state.rs, orchestrator.rs
**Date**: 2026-02-21

## RD-001: Schema Migration Strategy

**Decision**: Use ALTER TABLE statements to migrate the existing dictionary table schema. Rename existing columns and add new ones. Wrap in a `migrate_schema(db_path: &Path) -> Result<()>` function that inspects current columns via `PRAGMA table_info(dictionary)` and applies only the needed changes.

**Rationale**: The dictionary table was created by Feature 009's `init_database()` using `dictionary::CREATE_TABLE_SQL`. The existing schema has columns `term`, `replacement`, `frequency`, `created_at`. The new schema needs `spoken`, `written`, `use_count`, `category`, `is_command_phrase` plus indexes. SQLite 3.25+ (bundled in rusqlite 0.38 as ≥3.45) supports `ALTER TABLE RENAME COLUMN`. The migration function is idempotent — safe to run on both fresh and existing databases.

**Migration steps** (only applied if old column names detected):
```sql
ALTER TABLE dictionary RENAME COLUMN term TO spoken;
ALTER TABLE dictionary RENAME COLUMN replacement TO written;
ALTER TABLE dictionary RENAME COLUMN frequency TO use_count;
ALTER TABLE dictionary ADD COLUMN category TEXT NOT NULL DEFAULT 'general';
ALTER TABLE dictionary ADD COLUMN is_command_phrase INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_dictionary_spoken ON dictionary(spoken);
CREATE INDEX IF NOT EXISTS idx_dictionary_category ON dictionary(category);
```

**Alternatives considered**:
- Drop and recreate table: Loses existing data. Rejected — users may have entries from development.
- Keep old column names, map in Rust: Creates terminology mismatch between code and SQL. Rejected for clarity.
- Create new table, migrate data, drop old: More complex than ALTER TABLE for this simple schema change. Rejected.

## RD-002: Cache Architecture

**Decision**: Evolve existing `DictionaryCache` in-place. Switch from immutable `Arc<HashMap>` to shared `Arc<RwLock<HashMap>>` for CRUD support. Add a primary `entries` HashMap keyed by spoken form (lowercase) containing full `DictionaryEntry` objects. Maintain separate `word_subs` and `phrase_subs` structures for fast two-pass substitution. Rebuild substitution structures on any mutation via `rebuild_substitution_maps()`. Store `db_path: Option<PathBuf>` internally.

**Rationale**: The two-pass substitution algorithm (phrases longest-first, then single words via HashMap) meets the <1ms substitution target. CRUD operations are rare (user editing in UI) vs substitutions (every dictation cycle), so rebuilding optimized structures on mutation is acceptable overhead. Storing db_path internally simplifies the API — callers don't pass a database path for every CRUD call. `Arc<RwLock<...>>` makes the struct `Clone`-able (via `Arc::clone`) and shares mutable state across VoxState and Pipeline clones.

**Alternatives considered**:
- Single HashMap for everything: Loses the O(1) word substitution and longest-first phrase ordering. Rejected for performance.
- Separate `DictionaryStore` wrapper: Unnecessary abstraction. Rejected per over-engineering principle.
- Immutable snapshots with reload: Requires coordinating reload across all holders. The shared mutable approach is simpler.

## RD-003: Substitution Result and Use Count Tracking

**Decision**: `apply_substitutions` returns `SubstitutionResult { text: String, matched_ids: Vec<i64> }`. The `matched_ids` vector contains one ID per match occurrence (so if "vox" matches 3 times, the ID appears 3 times). The caller (pipeline orchestrator) calls `increment_use_counts(&result.matched_ids)` to update both in-memory entries and SQLite. The SQLite update uses a single transaction with batched UPDATE statements.

**Rationale**: Separating match tracking from persistence gives the caller control over when IO occurs. In-memory increments are immediate so hint ordering stays current. SQLite persistence ensures counts survive restarts. The return type change from `String` to `SubstitutionResult` requires updating the orchestrator and tests, but this is expected for a refactor.

**Alternatives considered**:
- Auto-increment inside apply_substitutions: Couples substitution with database IO, making it harder to test and potentially adding latency to the hot path. Rejected.
- Accumulate pending increments, flush periodically: Adds complexity (flush timing, crash consistency). Rejected for simplicity.
- Only increment in memory, persist on shutdown: Loses counts on crash. Rejected for data integrity.

## RD-004: Substitution Map Entry Type

**Decision**: Substitution maps use a small internal `SubEntry { id: i64, written: String }` struct instead of storing full `DictionaryEntry` objects. `word_subs: Arc<RwLock<HashMap<String, SubEntry>>>` and `phrase_subs: Arc<RwLock<Vec<(String, SubEntry)>>>`.

**Rationale**: Substitution only needs the written form (for replacement) and ID (for use count tracking). Storing full entries would waste memory and require cloning category, is_command_phrase, created_at fields that are never used during substitution. The SubEntry struct is internal (not pub) and exists only as an optimization.

**Alternatives considered**:
- Store full DictionaryEntry: Works but wastes memory. For 1000 entries the difference is negligible, but the cleaner separation of concerns is worth the small struct.
- Use tuples `(i64, String)`: Works but less readable. Named fields are clearer.

## RD-005: Import/Export Format

**Decision**: JSON array of objects. Each object has fields: `spoken`, `written`, `category`, `is_command_phrase`. Excludes `id`, `use_count`, and `created_at` — these are installation-specific and regenerated on import. Uses a separate `DictionaryExportEntry` struct with serde `Serialize`/`Deserialize` derives.

**Rationale**: JSON is human-readable, widely supported, and the project already has serde_json as a dependency. Excluding server-side fields makes the export portable across different Vox installations. Use count is per-installation usage data, not transferable. Import sets `use_count = 0` and generates a fresh `created_at` timestamp.

**Alternatives considered**:
- CSV: Less structured, no native boolean support, quoting issues with commas in text. Rejected.
- Include all fields in export: Clutters export with non-portable data. Rejected.
- Custom binary format: Unnecessary complexity for a small dataset. Rejected.

## RD-006: Command Phrase Filtering

**Decision**: Command phrases are filtered out during `rebuild_substitution_maps()` — they are never added to `word_subs` or `phrase_subs`. They remain in the `entries` HashMap. `top_hints()` includes ALL entries (including command phrases) sorted by use count.

**Rationale**: Filtering at the rebuild step is efficient — it happens once per mutation, not on every substitution call. Including command phrases in hints but not substitution matches the spec requirement (FR-007, FR-008). The rebuild function iterates all entries once, partitioning into word_subs and phrase_subs while skipping command phrases.

**Alternatives considered**:
- Filter during apply_substitutions: Adds a conditional check on every match iteration. Less efficient than pre-filtering. Rejected.
- Separate data structure for command phrases: Over-engineering for a boolean flag. Rejected.

## RD-007: Connection Management

**Decision**: `DictionaryCache` stores `db_path: Option<PathBuf>` internally. CRUD methods open a new `rusqlite::Connection` for each operation using `self.db_path`. The `empty()` constructor sets `db_path = None`; CRUD methods return an error if called on a cache with no db_path.

**Rationale**: SQLite connections are lightweight (~1ms to open). Opening per-operation avoids holding connections across async boundaries (rusqlite::Connection is not Send+Sync) and simplifies the API. Dictionary CRUD operations are user-initiated (rare) so the connection overhead is negligible.

**Alternatives considered**:
- Pass `&Connection` to each method: Requires callers to manage connections. More error-prone. Rejected.
- Store a persistent Connection in the struct: rusqlite::Connection is not Send+Sync, making it impossible to share across async tasks. Rejected.
- Connection pool: Overkill for a single-user desktop app. Rejected.

## RD-008: VoxState Integration

**Decision**: Add `dictionary: DictionaryCache` field to `VoxState`. Initialize by calling `migrate_schema()` then `DictionaryCache::load()` during `VoxState::new()`. Expose via `pub fn dictionary(&self) -> &DictionaryCache`. The pipeline orchestrator receives the DictionaryCache from VoxState (via clone) instead of creating `DictionaryCache::empty()`.

**Rationale**: VoxState is the central state container (GPUI Global). Housing the dictionary here keeps it alongside the transcript store and settings. The orchestrator currently creates an empty cache, which means dictionaries are never loaded in production — this must be fixed by passing the loaded cache from VoxState.

**Alternatives considered**:
- Keep dictionary creation in orchestrator: Would require passing db_path through the pipeline constructor. Less cohesive. Rejected.
- Lazy-load dictionary on first substitution: Adds latency to the first dictation. Rejected per latency budget.
