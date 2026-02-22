# Research: Application State & Settings

**Feature Branch**: `009-app-state-settings`
**Date**: 2026-02-21

## R1: gpui as Required Dependency in vox_core

**Decision**: Add `gpui.workspace = true` to `crates/vox_core/Cargo.toml`
as a required (non-optional) dependency.

**Rationale**: VoxState implements GPUI's `Global` trait (FR-001). The
Global trait is defined in gpui. Constitution Principle XI prohibits
making required dependencies optional via feature flags. The spec
explicitly states: "gpui is a required (non-optional) dependency of
vox_core for the Global trait implementation."

**Alternatives considered**:
- **Optional gpui via feature flag** (rejected): Violates Constitution
  Principle XI. Was the mistake that caused the prior Feature 009 branch
  to be deleted.
- **Bridge struct in vox_ui that wraps VoxState** (rejected): Adds
  indirection. The spec requires VoxState itself to implement Global, not
  a wrapper. Principle VI forbids scope reduction.

**Impact**: vox_core gains gpui as a compile-time dependency. This
increases vox_core's dependency tree but is required by the design. Tests
for vox_core will link gpui even when not exercising UI features, which
is acceptable.

**Reference**: Tusk makes gpui optional in tusk_core
(`optional = true, features = ["gpui"]`). Vox does NOT follow this
pattern — Constitution Principle XI explicitly forbids it.

---

## R2: Data Directory Path Consistency

**Decision**: Use `dirs::data_local_dir()` on Windows and
`dirs::data_dir()` on macOS, matching the existing `model_dir()` pattern
in `models.rs`.

**Rationale**: The models module already resolves paths using
`data_local_dir()` (Windows → `%LOCALAPPDATA%`) and `data_dir()`
(macOS → `~/Library/Application Support`). Settings and database files
belong alongside the models directory under `com.vox.app/`.

**Path layout**:
```
# Windows: %LOCALAPPDATA%/com.vox.app/
com.vox.app/
├── models/           # existing (models.rs)
├── settings.json     # new (Feature 009)
└── vox.db            # new (Feature 009)

# macOS: ~/Library/Application Support/com.vox.app/
com.vox.app/
├── models/           # existing (models.rs)
├── settings.json     # new (Feature 009)
└── vox.db            # new (Feature 009)
```

**Spec note**: The spec text mentions `%APPDATA%` (which maps to
`dirs::config_dir()` on Windows → Roaming profile). This is corrected to
`%LOCALAPPDATA%` for consistency with the already-deployed model
directory. `%LOCALAPPDATA%` is semantically correct for local-only data
(Principle I: no cloud sync).

**Alternatives considered**:
- **config_dir() on Windows** (rejected): Would place settings in
  `%APPDATA%/Roaming` while models live in `%LOCALAPPDATA%`. Splits
  application data across two directories. Confusing for users who want
  to back up or reset Vox data.
- **Separate data_dir() function** (rejected): Duplicates the path
  resolution logic already in `models::model_dir()`. Instead, extract
  a shared `data_dir()` function that both use.

---

## R3: Database Architecture

**Decision**: VoxState creates a single `vox.db` SQLite database. It
holds an `Arc<TranscriptStore>` that is shared with the pipeline
orchestrator for background transcript writes. The database initialization
function creates both the `transcripts` and `dictionary` tables.

**Rationale**:
- `TranscriptStore` already exists in `pipeline/transcript.rs` with
  tested CRUD operations and a `Mutex<Connection>` wrapper.
- The pipeline orchestrator runs on a background tokio task and calls
  `transcript_store.save()` — it cannot access GPUI Global.
- `Arc<TranscriptStore>` allows VoxState (UI thread) and Pipeline
  (background) to share the same connection.
- SQLite handles concurrent access from a single connection via internal
  locking. The `Mutex<Connection>` serializes all operations.

**TranscriptStore extensions** (new methods for Feature 009):
- `search(query: &str) -> Result<Vec<TranscriptEntry>>` — SQL LIKE query
- `delete(id: &str) -> Result<()>` — single record delete
- `clear_secure() -> Result<()>` — overwrite all text fields with empty
  strings, DELETE all rows, execute VACUUM

**Dictionary table**: Created by the database init function using
`CREATE TABLE IF NOT EXISTS` with the existing schema from
`dictionary.rs`. The `DictionaryCache` continues to manage its own
in-memory state. Feature 010 will handle dictionary schema evolution.

**Alternatives considered**:
- **VoxState owns raw Connection, no TranscriptStore** (rejected):
  Duplicates proven SQL logic. TranscriptStore is well-tested with 7
  passing tests.
- **Separate connections for TranscriptStore and VoxState** (rejected):
  SQLite in-process concurrency with multiple connections adds
  complexity. Single shared connection is simpler.

---

## R4: Settings Atomic Write Pattern

**Decision**: Settings are saved using atomic write: write to a temporary
file in the same directory, then rename over the target file.

**Rationale**: If the process crashes or power is lost during a write, the
rename is atomic on both NTFS (Windows) and APFS/HFS+ (macOS). This
prevents corrupt settings files. The settings file is small (~2KB JSON),
so the write completes quickly.

**Implementation**:
```
1. Serialize Settings to JSON string (serde_json::to_string_pretty)
2. Write JSON to data_dir/settings.json.tmp
3. Rename settings.json.tmp → settings.json (atomic on both platforms)
```

**Error handling**:
- If step 2 fails (disk full): tmp file may be incomplete. The original
  settings.json is untouched. Return error.
- If step 3 fails: rare (permissions issue). Return error. Original
  settings.json untouched.

**Alternatives considered**:
- **Direct write to settings.json** (rejected): Crash during write
  corrupts the file. Violates edge case requirement (disk full must not
  corrupt existing settings).
- **Write-ahead log** (rejected): Overkill for a single JSON file.

---

## R5: Secure Delete Strategy

**Decision**: Clear history performs three steps:
1. `UPDATE transcripts SET raw_text = '', polished_text = '', target_app = ''`
2. `DELETE FROM transcripts`
3. `VACUUM`

**Rationale**: SQLite does not zero-fill deleted pages by default. Simply
deleting rows leaves the text recoverable in the database file's free
pages. Overwriting text fields first ensures the original content is
replaced in-place. VACUUM then rebuilds the database file, releasing the
free pages back to the OS. This satisfies FR-010 and SC-009.

**Alternatives considered**:
- **Delete + VACUUM only** (rejected): Text remains in free pages until
  VACUUM, but VACUUM may not zero-fill freed space on all filesystems.
  Overwriting first is more reliable.
- **Delete the database file entirely** (rejected): Destroys the
  dictionary table and any other tables. Requires re-creating the schema.
  Also doesn't guarantee the OS zeros the freed disk blocks.
- **PRAGMA secure_delete = ON** (rejected): SQLite's secure_delete
  zeroes freed pages, but doesn't overwrite in-place text in live pages.
  Also has performance overhead for all operations, not just clear.

---

## R6: Existing Code Reuse Analysis

| Component | Location | Reuse Strategy |
|-----------|----------|---------------|
| PipelineState | pipeline/state.rs | Direct reuse — VoxState holds `RwLock<PipelineState>` |
| DownloadProgress | models/downloader.rs | Direct reuse — AppReadiness references `crate::models::DownloadProgress` |
| TranscriptEntry | pipeline/transcript.rs | Direct reuse — no changes needed |
| TranscriptStore | pipeline/transcript.rs | Extend — add search, delete, clear_secure methods |
| model_dir() | models.rs | Pattern reference — new data_dir() follows same structure |
| DictionaryCache | dictionary.rs | Unchanged — continues with own connection |
| Pipeline | pipeline/orchestrator.rs | Minor refactor — accept `Arc<TranscriptStore>` |

**New code**:
- `state.rs`: VoxState struct, AppReadiness enum, Global impl, init logic
- `config.rs`: Settings struct (23 fields), OverlayPosition, ThemeMode,
  load/save with atomic write

**Modified code**:
- `pipeline/transcript.rs`: Add search, delete, clear_secure methods
- `pipeline/orchestrator.rs`: Change TranscriptStore to Arc<TranscriptStore>
- `vox_core/Cargo.toml`: Add `gpui.workspace = true`
