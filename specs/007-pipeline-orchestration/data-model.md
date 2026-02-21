# Data Model: Pipeline Orchestration

**Feature**: 007-pipeline-orchestration
**Date**: 2026-02-20

## Entities

### PipelineState

The operational state of the pipeline, broadcast to all UI subscribers on every transition.

| Field | Type | Description |
|-------|------|-------------|
| variant | enum | One of: Idle, Listening, Processing, Injecting, Error |
| raw_text | Option\<String\> | Present only in Processing state after ASR completes |
| polished_text | String | Present only in Injecting state |
| message | String | Present only in Error state; human-readable |

**Derives**: Clone, Debug, PartialEq

**State transitions**:
```
Idle ──(start)──► Listening ──(segment)──► Processing { raw_text: None }
  ▲                   ▲                         │
  │                   │                    (ASR done)
  │                   │                         ▼
  │                   │              Processing { raw_text: Some(_) }
  │                   │                         │
  │                   │                    (LLM done)
  │                   │                         ▼
  │                   └──(done)───── Injecting { polished_text }
  │                   │
  │                   └──(error)──── Error { message } ──► Listening or Idle
  │
  └──────────(stop)──────────────────────────────────────┘
```

### ActivationMode

The user's configured recording trigger behavior. Determines how hotkey events map to pipeline start/stop.

| Field | Type | Description |
|-------|------|-------------|
| variant | enum | One of: HoldToTalk, Toggle, HandsFree |

**Derives**: Clone, Debug, PartialEq, Serialize, Deserialize

**Default**: HoldToTalk

**Persistence**: Stored in SQLite settings table as string ("hold_to_talk", "toggle", "hands_free").

### TranscriptEntry

A historical record of a completed dictation. Created after each successful text injection. NOT created for voice command executions.

| Field | Type | Description |
|-------|------|-------------|
| id | String | UUID v4, unique identifier |
| raw_text | String | Original ASR output before dictionary/LLM processing |
| polished_text | String | Final text after LLM post-processing |
| target_app | String | Name of focused application at injection time |
| duration_ms | u32 | Audio segment duration in milliseconds. Computed as `segment.len() * 1000 / 16000` — the total sample count of the `Vec<f32>` segment multiplied by 1000 first to preserve precision in integer arithmetic, then divided by the sample rate (16kHz), yielding the wall-clock duration in milliseconds. Note: the multiply-first ordering avoids integer truncation that would produce 0ms for segments shorter than 1 second (e.g., 8000 samples: `8000 / 16000 * 1000 = 0` vs `8000 * 1000 / 16000 = 500`). |
| latency_ms | u32 | End-to-end processing time in milliseconds. Measured from the moment the pipeline's run loop receives the segment from `segment_rx.recv()` to the moment text injection (or command execution) completes. Does not include VAD processing time or channel transit time. |
| created_at | String | ISO 8601 timestamp (e.g., "2026-02-20T14:30:00Z") |

**Persistence**: SQLite table `transcripts`. Auto-pruned after 30 days on startup.

### DictionaryEntry

A user-defined vocabulary substitution. Stored in SQLite, loaded into memory on startup.

| Field | Type | Description |
|-------|------|-------------|
| id | i64 | Auto-increment primary key |
| term | String | The word/phrase to match (case-insensitive) |
| replacement | String | The substitution text |
| frequency | u32 | Usage count for ranking hints. Incremented by the pipeline each time a substitution matches during `apply_substitutions()`. Updated in SQLite asynchronously (batched on pipeline stop or periodic flush) to avoid per-substitution DB writes on the hot path. |
| created_at | String | ISO 8601 timestamp |

**Uniqueness**: `term` is unique (COLLATE NOCASE).

### DictionaryCache

In-memory cache of all dictionary entries. Supports both single-word O(1) lookups and multi-word phrase substitutions via a two-pass algorithm (see research.md R-004).

| Field | Type | Description |
|-------|------|-------------|
| word_substitutions | HashMap\<String, String\> | Single-word terms: lowercase key → replacement. O(1) lookup. |
| phrase_substitutions | Vec\<(String, String)\> | Multi-word phrases: sorted longest-first to prevent partial matches. |
| hints | Vec\<DictionaryEntry\> | All entries sorted by frequency descending. |

**Thread safety**: Fields wrapped in `Arc` for cheap cloning across threads.

**Memory bounds**: The dictionary is expected to contain at most ~10,000 entries for a power user. At an average of ~50 bytes per entry (term + replacement strings + overhead), the in-memory HashMap consumes ~500 KB. The phrase Vec and hints Vec add ~200 KB each. Total DictionaryCache memory: < 1 MB for 10,000 entries. No upper bound is enforced — if the dictionary grows beyond 10,000 entries, the only impact is proportionally more memory and slightly slower phrase-pass substitution.

**Derives**: Clone

### PipelineCommand

Commands sent from PipelineController to Pipeline via mpsc channel. Decouples hotkey handling from the async run loop (see research.md R-010).

| Field | Type | Description |
|-------|------|-------------|
| variant | enum | One of: Stop |

### Pipeline

The orchestrator that coordinates the audio-to-text flow. Owns the Send+Sync components directly and manages the VAD thread via handles. Receives control commands via mpsc channel (not direct method calls during run).

| Field | Type | Description |
|-------|------|-------------|
| asr | AsrEngine | Speech recognition engine (Clone via Arc) |
| llm | PostProcessor | LLM post-processor (Clone via Arc) |
| dictionary | DictionaryCache | In-memory substitution cache (Clone via Arc) |
| transcript_store | TranscriptStore | SQLite-backed transcript persistence |
| state_tx | broadcast::Sender\<PipelineState\> | State change broadcaster |
| command_rx | Receiver\<PipelineCommand\> | Receives control commands from PipelineController |
| stop_flag | Arc\<AtomicBool\> | Signal to stop VAD thread |
| segment_rx | Option\<Receiver\<Vec\<f32\>\>\> | Receives speech segments from VAD thread |
| vad_handle | Option\<JoinHandle\<Result\<()\>\>\> | VAD thread handle |
| vad_model_path | PathBuf | Path to Silero VAD ONNX model |
| vad_config | VadConfig | VAD thresholds and timing |

### PipelineController

Translates hotkey activation events into pipeline start/stop commands based on the active ActivationMode. Sends commands to Pipeline via mpsc channel (never holds `&mut Pipeline`).

| Field | Type | Description |
|-------|------|-------------|
| command_tx | Sender\<PipelineCommand\> | Channel to send commands to Pipeline::run() |
| mode | ActivationMode | Current activation mode (read from settings) |
| is_active | bool | Whether dictation is currently running |
| last_press_time | Option\<Instant\> | For double-press detection (300ms window) |

### TranscriptStore

SQLite-backed persistent storage for transcript history.

| Field | Type | Description |
|-------|------|-------------|
| connection | Mutex\<Connection\> | SQLite connection (parking_lot::Mutex for thread safety) |

## Database Location

All tables (dictionary, transcripts, settings) reside in a **single SQLite file** named `vox.db`, located in the platform-specific application data directory:

- **Windows**: `%APPDATA%\vox\vox.db` (e.g., `C:\Users\<user>\AppData\Roaming\vox\vox.db`)
- **macOS**: `~/Library/Application Support/vox/vox.db`

The directory is created automatically on first launch if it does not exist. The database file is created by `rusqlite::Connection::open()` if absent. A single file simplifies backup, migration, and avoids cross-file transaction complexity.

## Database Schema

### Table: `dictionary`

```sql
CREATE TABLE IF NOT EXISTS dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    term TEXT UNIQUE NOT NULL COLLATE NOCASE,
    replacement TEXT NOT NULL,
    frequency INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
```

### Table: `transcripts`

```sql
CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY,
    raw_text TEXT NOT NULL,
    polished_text TEXT NOT NULL,
    target_app TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_transcripts_created_at ON transcripts(created_at);
```

### Table: `settings`

```sql
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

Used for persisting `activation_mode` and future user preferences.

## Relationships

```
PipelineController ──uses──► Pipeline
Pipeline ──owns──► AsrEngine, PostProcessor, DictionaryCache, TranscriptStore
Pipeline ──broadcasts──► PipelineState ──received by──► UI subscribers
Pipeline ──creates──► TranscriptEntry ──stored in──► TranscriptStore
DictionaryCache ──loaded from──► dictionary table
TranscriptStore ──reads/writes──► transcripts table
ActivationMode ──persisted in──► settings table
```
