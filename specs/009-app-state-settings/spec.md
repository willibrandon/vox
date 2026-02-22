# Feature Specification: Application State & Settings

**Feature Branch**: `009-app-state-settings`
**Created**: 2026-02-21
**Status**: Draft
**Dependencies**: 001-workspace-scaffolding
**Design Reference**: Sections 10 (Settings & Configuration), 14 (Security & Privacy)

## User Scenarios & Testing

### User Story 1 - Application Initializes with Persistent State (Priority: P1)

When the user launches Vox, the application creates a VoxState object
that holds all runtime state and is accessible from anywhere in the app
via GPUI's Global trait. If no settings file exists, defaults are
applied. If no database exists, it is created with the required schema.
The application data directory is created automatically on the platform-
appropriate path.

**Why this priority**: VoxState is the central state object that every
other feature depends on. Without it, no settings, no transcript
history, no pipeline state tracking.

**Independent Test**: Launch the application on a clean system with no
prior data directory. Verify VoxState initializes, settings file is
created with defaults, database schema is created, and data directory
exists.

**Acceptance Scenarios**:

1. **Given** first launch on a clean system, **When** Vox starts,
   **Then** VoxState initializes as GPUI Global, settings file is
   created with defaults, SQLite database is created with schema, data
   directory exists at the platform-specific path.
2. **Given** a prior launch with existing settings and database,
   **When** Vox starts, **Then** VoxState loads existing settings from
   JSON and opens the existing database.
3. **Given** a corrupt settings file, **When** Vox starts, **Then**
   VoxState logs a warning and resets settings to defaults without
   crashing.

---

### User Story 2 - Settings Persistence (Priority: P1)

Users configure Vox through a settings panel. Settings are grouped
into categories: Audio, VAD, ASR, LLM, Hotkey, Appearance, and
Advanced. All settings persist to a JSON file on disk and survive
application restarts.

**Why this priority**: Settings persistence is required before any UI
settings panel can be built. Without it, user preferences are lost on
every restart.

**Independent Test**: Modify a setting value, save, restart the
application. Verify the setting retains its new value.

**Acceptance Scenarios**:

1. **Given** default settings, **When** the user changes a setting and
   saves, **Then** the settings file is updated and the new value is
   used.
2. **Given** settings saved to disk, **When** the application restarts,
   **Then** all saved settings are restored to their previous values.
3. **Given** settings with all 23 fields populated, **When** saved and
   reloaded, **Then** every field round-trips without data loss.

---

### User Story 3 - Transcript History (Priority: P2)

After each dictation session, the raw transcript and polished text are
saved to a SQLite database with metadata (target app, duration, latency,
timestamp). Users can browse, search, and delete their transcript
history. A "clear history" action securely deletes all data.

**Why this priority**: Transcript history enables the future history
viewer UI and provides value to users who want to review past
dictations. It depends on VoxState and database being operational (US1).

**Independent Test**: Save a transcript entry, retrieve it by listing,
search for it by text content, delete it, verify it is gone. Clear all
history and verify the database is empty and vacuumed.

**Acceptance Scenarios**:

1. **Given** a completed dictation, **When** VoxState saves the
   transcript, **Then** it is stored in SQLite with all metadata fields.
2. **Given** multiple transcripts, **When** the user searches by text,
   **Then** matching transcripts are returned ordered by recency.
3. **Given** transcript history, **When** the user clears history,
   **Then** all transcripts are overwritten and the database is
   vacuumed to prevent data recovery.
4. **Given** a single transcript, **When** the user deletes it,
   **Then** only that transcript is removed.

---

### User Story 4 - Application Readiness Tracking (Priority: P2)

VoxState tracks the application readiness state: Downloading (with
per-model progress), Loading (with stage description), or Ready. The
hotkey responds in every state — if the pipeline is not ready, the
overlay shows why (download progress, loading stage, or error).

**Why this priority**: Readiness tracking is needed before the overlay
HUD can display download/loading state, but the overlay itself is a
separate feature. The state machine is foundational infrastructure.

**Independent Test**: Transition VoxState through Downloading -> Loading
-> Ready states. Verify the readiness value is queryable at each stage
and contains the expected data (download progress percentages, loading
stage name).

**Acceptance Scenarios**:

1. **Given** models not yet downloaded, **When** querying readiness,
   **Then** state is Downloading with per-model progress values.
2. **Given** all models downloaded, **When** models are loading into
   GPU, **Then** state is Loading with a human-readable stage name.
3. **Given** all models loaded, **When** pipeline is operational,
   **Then** state is Ready.

---

### User Story 5 - Audio Data Privacy (Priority: P1)

Audio is processed in memory and immediately discarded after
transcription. No audio is ever written to disk. Transcript history
can be disabled entirely in settings. Clear history performs a secure
delete. No telemetry, no cloud sync.

**Why this priority**: Privacy is non-negotiable per the Constitution
(Principle I). It shapes data handling in every other user story.

**Independent Test**: Verify no audio files exist on disk after a
dictation session. Verify that with history disabled, no transcript
rows are inserted. Verify clear history leaves no recoverable data.

**Acceptance Scenarios**:

1. **Given** a dictation session, **When** transcription completes,
   **Then** no audio data exists on disk anywhere in the data directory.
2. **Given** transcript history disabled in settings, **When** a
   dictation completes, **Then** no transcript entry is saved.
3. **Given** existing transcript history, **When** user clears history,
   **Then** data is overwritten and VACUUM is executed.

---

### Edge Cases

- What happens when the data directory path contains non-ASCII or
  whitespace characters? System MUST handle it correctly.
- What happens when the disk is full during settings save? System MUST
  report the error without crashing or corrupting existing settings.
- What happens when multiple instances of Vox attempt to access the
  same database? rusqlite MUST handle concurrency via its built-in
  locking; second instance gets a clear error.
- What happens when a settings file has extra fields from a newer
  version? System MUST ignore unknown fields (forward compatibility).
- What happens when a settings file is missing fields added in a newer
  version? System MUST use defaults for missing fields (backward
  compatibility).

## Requirements

### Functional Requirements

- **FR-001**: System MUST provide a VoxState struct that implements
  GPUI's Global trait, enabling `cx.global::<VoxState>()` access from
  any GPUI context.
- **FR-002**: VoxState MUST hold: user settings (RwLock-protected),
  SQLite database connection, application readiness state
  (RwLock-protected), pipeline state (RwLock-protected), Tokio runtime,
  and data directory path.
- **FR-003**: System MUST provide a Settings struct with 23 fields
  across 7 categories (Audio: 2, VAD: 3, ASR: 2, LLM: 5, Hotkey: 3,
  Appearance: 4, Advanced: 4) with serde Serialize/Deserialize support.
- **FR-004**: Settings MUST persist to a JSON file at the platform
  data directory (`%LOCALAPPDATA%/com.vox.app/settings.json` on Windows,
  `~/Library/Application Support/com.vox.app/settings.json` on macOS).
  Windows uses `%LOCALAPPDATA%` (not `%APPDATA%`) for consistency with
  the existing `model_dir()` path and to avoid roaming profile sync.
- **FR-005**: System MUST create default settings when no settings file
  exists.
- **FR-006**: System MUST reset to defaults when settings file is
  corrupt, logging a warning.
- **FR-007**: System MUST provide an AppReadiness enum with three
  states: Downloading (with per-model DownloadProgress), Loading (with
  stage description), Ready.
- **FR-008**: System MUST initialize a SQLite database via rusqlite
  0.38 with tables for transcripts and dictionary entries using
  `CREATE TABLE IF NOT EXISTS`.
- **FR-009**: System MUST provide transcript history CRUD: save, list
  (paginated), search (text query), delete (single), clear (all with
  secure delete).
- **FR-010**: Secure delete ("clear history") MUST overwrite transcript
  data and execute VACUUM to prevent data recovery.
- **FR-011**: System MUST create the platform data directory and model
  subdirectory automatically on first run.
- **FR-012**: All timestamps MUST be stored as ISO 8601 strings (no
  chrono DateTime — rusqlite 0.38 has no FromSql for it).
- **FR-013**: VoxState MUST use `parking_lot::RwLock` for interior
  mutability (reads vastly outnumber writes).
- **FR-014**: No audio data may be written to disk at any point.
  Audio is processed in memory and discarded after transcription.
- **FR-015**: Settings MUST support forward and backward compatibility
  (ignore unknown fields, use defaults for missing fields) via serde's
  `#[serde(default)]`.

### Key Entities

- **VoxState**: Central application state object. Holds settings, DB
  connection, readiness state, pipeline state, Tokio runtime, data
  directory path. Implements GPUI Global trait.
- **Settings**: 23-field configuration struct. Serialized to/from JSON.
  Categories: Audio (2), VAD (3), ASR (2), LLM (5), Hotkey (3),
  Appearance (4), Advanced (4). All fields have sensible defaults.
- **AppReadiness**: Three-state enum tracking application lifecycle:
  Downloading -> Loading -> Ready.
- **DownloadProgress**: Tracks per-model download state (bytes
  downloaded, total bytes, completion status).
- **PipelineState**: Tracks dictation pipeline state (idle, listening,
  processing, etc.). Already partially defined in existing codebase.
- **TranscriptEntry**: Single transcript record with id, raw text,
  polished text, target app, duration, latency, timestamp.
- **OverlayPosition**: Enum for overlay placement (TopCenter,
  TopRight, BottomCenter, BottomRight, Custom {x, y}).
- **ThemeMode**: Enum for theme selection (System, Light, Dark).

## Success Criteria

### Measurable Outcomes

- **SC-001**: Application initializes VoxState and is ready to accept
  settings reads/writes within 50ms of startup on both target platforms.
- **SC-002**: Settings load from disk in under 10ms.
- **SC-003**: Settings save to disk in under 10ms.
- **SC-004**: Transcript save completes in under 5ms.
- **SC-005**: Transcript search across 10,000 entries returns results
  in under 50ms.
- **SC-006**: Database size with 10,000 transcripts remains under 10MB.
- **SC-007**: Missing or corrupt settings files are handled without
  any crash or user intervention.
- **SC-008**: All data remains local — zero network calls, zero
  telemetry, zero cloud sync.
- **SC-009**: Clear history leaves no recoverable transcript data
  (overwrite + VACUUM).
- **SC-010**: Zero compiler warnings across the implementation.

## Assumptions

- The `dirs` crate (or equivalent) is available for resolving platform
  data directories.
- `parking_lot` is already a dependency of `vox_core`.
- `rusqlite` 0.38 is the database crate per the design document.
- `gpui` is a required (non-optional) dependency of `vox_core` for
  the Global trait implementation (Constitution Principle XI).
- `serde` and `serde_json` are used for settings serialization.
- `tokio` runtime is already present in the workspace.
- The transcript `id` field uses a string format (e.g., UUID) generated
  by the caller.
- PipelineState is partially defined in existing pipeline code and may
  need consolidation into VoxState.
