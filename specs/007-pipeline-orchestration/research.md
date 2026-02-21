# Research: Pipeline Orchestration

**Feature**: 007-pipeline-orchestration
**Date**: 2026-02-20

## R-001: Pipeline Threading Model

**Decision**: Three-tier threading — cpal OS thread → dedicated VAD std::thread → tokio async orchestrator with spawn_blocking for GPU work.

**Rationale**: Thread safety constraints of the existing components mandate this architecture:

| Component | Send | Sync | Clone | Implication |
|-----------|------|------|-------|-------------|
| AudioCapture | No | No | No | Must stay on creation thread. cpal callback runs on OS thread. |
| HeapCons\<f32\> | Yes | No | No | Ring buffer consumer can move to VAD thread. |
| AudioResampler | No | No | No | Must be created on VAD thread (holds `rubato::Fft`). |
| SileroVad | No | No | No | Must be created on VAD thread (holds `ort::Session`). |
| VadStateMachine | Yes | Yes | Yes | Pure logic, can go anywhere. |
| SpeechChunker | Yes | Yes | No | Can move to VAD thread. |
| AsrEngine | Yes | Yes | Yes | Clone via `Arc<Mutex<WhisperContext>>`. Safe for spawn_blocking. |
| PostProcessor | Yes | Yes | Yes | Clone via Arc. Safe for spawn_blocking. |
| DictionaryCache | Yes | Yes | Yes | In-memory HashMap. Cloneable for shared access. |

**Threading diagram**:

```
┌─────────────────────────────────────────────────┐
│ cpal Audio Callback Thread (OS-managed)          │
│  AudioCapture.start() → producer writes samples  │
│  Lock-free SPSC ring buffer (no allocs/locks)    │
└──────────────────────┬──────────────────────────┘
                       │ HeapCons<f32> (moved to VAD thread)
                       ▼
┌─────────────────────────────────────────────────┐
│ VAD Processing Thread (std::thread::spawn)       │
│  run_vad_loop():                                 │
│    ring buffer → resample → 512-sample windows   │
│    → SileroVad inference → state machine         │
│    → chunker → emit Vec<f32> segments            │
│  Sleeps 5ms when no audio. Exits on stop flag.   │
└──────────────────────┬──────────────────────────┘
                       │ tokio::sync::mpsc::Sender<Vec<f32>>
                       ▼
┌─────────────────────────────────────────────────┐
│ Pipeline Orchestrator (tokio async task)          │
│  while let Some(segment) = segment_rx.recv() {   │
│    1. spawn_blocking → asr.transcribe()          │
│    2. dictionary.apply_substitutions()           │
│    3. get_focused_app_name()                     │
│    4. spawn_blocking → llm.process()             │
│    5. match Text → inject_text()                 │
│           Command → execute_command()            │
│    6. Text → transcript_store.save()             │
│  }                                               │
│  Broadcasts PipelineState on each transition.    │
└──────────────────────┬──────────────────────────┘
                       │ tokio::sync::broadcast
                       ▼
┌─────────────────────────────────────────────────┐
│ UI Subscribers                                   │
│  Overlay HUD, settings panel (future features)   │
│  Latest-wins: lagged subscribers get most recent │
└─────────────────────────────────────────────────┘
```

**Alternatives considered**:
- **Single-threaded pipeline**: Rejected. SileroVad and AudioResampler are NOT Send — can't coexist with async runtime on the same task.
- **All spawn_blocking**: Rejected. `run_vad_loop` is a long-running synchronous loop — `spawn_blocking` is designed for short bursts, not permanent threads. A dedicated thread avoids starving the blocking pool.

**spawn_blocking failure handling**: `tokio::task::spawn_blocking` returns a `JoinHandle` whose `.await` yields `Result<T, JoinError>`. A `JoinError` occurs only if: (1) the spawned task panics, or (2) the runtime is shutting down. If `spawn_blocking` does fail (JoinError), the pipeline treats it identically to an ASR/LLM error: broadcast `Error { message }` with the JoinError description, discard the segment, return to Listening (per R-009 error recovery strategy). No special pool configuration is needed.

**Blocking pool capacity assumption** (documented): Tokio's default blocking pool has a maximum of 512 threads. The pipeline uses at most 2 concurrent blocking tasks (ASR and LLM), and they never overlap since processing is sequential within a segment. Even if future features add more spawn_blocking usage, 512 threads provides >250× headroom. This assumption is valid and does not require a custom pool size configuration.
- **Async VAD loop with tokio::task::spawn**: Rejected. SileroVad is NOT Send, so it can't be held across await points. Would require `unsafe impl Send` with no safety guarantee.

## R-002: AudioCapture Ownership and Lifecycle

**Decision**: Pipeline does NOT own AudioCapture. AudioCapture stays on the thread that created it (main/UI thread). Pipeline receives the ring buffer consumer (`HeapCons<f32>`) when starting.

**Rationale**: `AudioCapture` is NOT Send — it contains `cpal::Stream` and mutable state that cannot safely cross thread boundaries. The ring buffer consumer IS Send and can be moved to the VAD thread. `AudioCapture` must be started/stopped by whoever created it (main thread).

**Lifecycle**:
1. App startup: Create `AudioCapture::new(config)` on main thread
2. Pipeline start: `audio.start()`, then take `audio.consumer()` reference
3. Pipeline spawns VAD thread with consumer (or clones the buffer arrangement)
4. Pipeline stop — **shutdown sequencing** (strict ordering required):
   1. Set `stop_flag` to `true` (AtomicBool, Ordering::Release) — signals VAD thread to exit its loop
   2. Join VAD thread handle (blocks until VAD flushes remaining audio and exits cleanly) — the VAD thread's flush produces final segments into the channel before exiting
   3. Drop or close the segment channel sender (happens automatically when VAD thread exits, since it owns the Sender) — this causes `segment_rx.recv()` in the pipeline's run() loop to return `None`, ending the loop
   4. The caller (main thread) then calls `audio.stop()` — AudioCapture stays on the caller's thread per R-002's ownership model

   **Why this order matters**: Setting stop_flag before join ensures the VAD thread will actually exit. Joining before stopping AudioCapture ensures all buffered audio is flushed through VAD (important for hold-to-talk tail capture). The channel closes naturally when the VAD thread exits, providing a clean signal to the pipeline's run() loop.

**Problem**: `AudioCapture::consumer()` returns `&mut HeapCons<f32>`, not an owned value. The consumer can't be moved out. We need to restructure: the Pipeline should receive an owned `HeapCons<f32>` at construction or start time, meaning AudioCapture needs a method to split off the consumer.

**Solution**: Add `AudioCapture::take_consumer(&mut self) -> Option<HeapCons<f32>>` that moves the consumer out (returns None if already taken). This is safe because the consumer is a separate allocation from the producer. The `switch_device()` method already rebuilds the ring buffer, so it would need to return a new consumer.

**Decision**: Modify `AudioCapture` to expose `take_consumer() -> Option<HeapCons<f32>>`.

**Contract alignment**: `Pipeline::start()` takes the owned consumer and native sample rate as parameters:
```rust
pub fn start(&mut self, consumer: HeapCons<f32>, native_sample_rate: u32) -> Result<()>;
```

The caller creates AudioCapture, starts it, takes the consumer, and passes it to Pipeline::start(). AudioCapture stays on the caller's thread.

## R-003: Focused Application Name Detection

**Decision**: Add `get_focused_app_name()` to the injector module as a public function, using platform-specific APIs already partially available.

**Windows implementation**:
```
GetForegroundWindow() → HWND
GetWindowThreadProcessId(hwnd) → PID
OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, pid) → handle
QueryFullProcessImageNameW(handle) → "C:\\...\\notepad.exe"
Extract filename stem → "notepad"
```

All required Win32 features are already in Cargo.toml: `Win32_System_Threading` (for OpenProcess, QueryFullProcessImageNameW), `Win32_UI_WindowsAndMessaging` (for GetForegroundWindow, GetWindowThreadProcessId), `Win32_Foundation` (for CloseHandle).

**macOS implementation**:
```
NSWorkspace.shared().frontmostApplication() → NSRunningApplication
app.localizedName() → "Safari"
```

Uses `objc2` (already in Cargo.toml). The focused window detection code in `macos.rs` already accesses NSWorkspace — we add a parallel path for app name extraction.

**Fallback**: If any API call fails, return `"Unknown"`. This is non-fatal — the LLM uses the app name for tone hints, not critical logic.

**Alternatives considered**:
- **Window title instead of process name**: Rejected. Window titles vary wildly ("untitled - Notepad", "document.txt - Visual Studio Code") and change with content. Process/app name is stable ("notepad", "Code").
- **New module for app detection**: Rejected. Injector already has platform-specific code with the same APIs. Adding to injector avoids duplicating Win32/macOS imports.

## R-004: DictionaryCache Design

**Decision**: In-memory `HashMap<String, String>` for substitutions with `Vec<DictionaryEntry>` for hints, loaded from SQLite on startup. Clone via Arc for sharing.

**Storage schema** (rusqlite 0.38, already in Cargo.toml):
```sql
CREATE TABLE IF NOT EXISTS dictionary (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    term TEXT UNIQUE NOT NULL COLLATE NOCASE,
    replacement TEXT NOT NULL,
    frequency INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL  -- ISO 8601
);
```

**In-memory structure**:
```rust
pub struct DictionaryCache {
    substitutions: Arc<HashMap<String, String>>,
    hints: Arc<Vec<DictionaryEntry>>,
}

// Clone is cheap (Arc clones)
impl Clone for DictionaryCache { ... }
```

**API design**:
- `DictionaryCache::load(db_path: &Path) -> Result<Self>`: Load from SQLite, build HashMap and sorted hints Vec.
- `DictionaryCache::empty() -> Self`: Create empty cache (for initial state before dictionary is populated).
- `apply_substitutions(&self, text: &str) -> String`: Word-by-word case-insensitive lookup and replacement. O(n) in word count, O(1) per lookup.
- `top_hints(&self, n: usize) -> String`: Format top N entries by frequency as a string for the LLM prompt (matches `PostProcessor::process()` `dictionary_hints: &str` parameter).

**Two-pass substitution algorithm**: The dictionary supports both single-word terms and multi-word phrases. Substitution runs in two passes:

1. **Phrase pass** (multi-word entries only, longest-first): Iterate over multi-word entries sorted by descending word count. For each phrase, perform case-insensitive `str::replace` on the full text. This runs in O(p × n) where p = number of multi-word entries and n = text length. For a typical dictionary (<100 multi-word entries) and typical utterance (<50 words), this is sub-microsecond.

2. **Word pass** (single-word entries): Split remaining text on whitespace, look up each token (lowercased) in the HashMap, replace if found, rejoin. O(w) where w = word count, with O(1) per lookup.

Longest-first ordering in the phrase pass prevents partial matches (e.g., "New York City" matches before "New York"). The HashMap stores only single-word entries; multi-word entries are stored in a separate sorted Vec.

**In-memory structure** (revised):
```rust
pub struct DictionaryCache {
    word_substitutions: Arc<HashMap<String, String>>,  // Single-word, lowercase key
    phrase_substitutions: Arc<Vec<(String, String)>>,   // Multi-word, sorted longest-first
    hints: Arc<Vec<DictionaryEntry>>,                   // All entries by frequency
}
```

**Alternatives considered**:
- **Aho-Corasick automaton**: Rejected. Adds a dependency for multi-pattern matching. The phrase count is small enough that sequential string::replace is sufficient and simpler.
- **Single HashMap only (whitespace split)**: Rejected. Cannot handle multi-word phrases like "New York" → "NYC" because whitespace splitting breaks the phrase into separate tokens.
- **JSON file storage**: Rejected. SQLite is already a dependency and provides ACID transactions for concurrent access (future dictionary editor UI).
- **Regex-based replacement**: Rejected. Over-engineered. Simple string matching is sufficient for exact substitutions.

## R-005: TranscriptStore Design

**Decision**: SQLite-backed persistent store with auto-pruning on startup.

**Storage schema**:
```sql
CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY,          -- UUID v4
    raw_text TEXT NOT NULL,
    polished_text TEXT NOT NULL,
    target_app TEXT NOT NULL,
    duration_ms INTEGER NOT NULL,
    latency_ms INTEGER NOT NULL,
    created_at TEXT NOT NULL       -- ISO 8601
);

CREATE INDEX IF NOT EXISTS idx_transcripts_created_at ON transcripts(created_at);
```

**API**:
- `TranscriptStore::open(db_path: &Path) -> Result<Self>`: Open/create database, run migrations, auto-prune entries older than 30 days.
- `save(&self, entry: &TranscriptEntry) -> Result<()>`: Insert record.
- `list(&self, limit: usize, offset: usize) -> Result<Vec<TranscriptEntry>>`: Paginated list, newest first.
- `prune_older_than(&self, days: u32) -> Result<usize>`: Delete old records, return count deleted.

**Thread safety**: `rusqlite::Connection` is NOT Send/Sync. Options:
1. Wrap in `Mutex` for cross-thread access
2. Use a single-threaded database actor
3. Open a new connection per operation

**Decision**: Use `parking_lot::Mutex<Connection>` (verified workspace dependency: `parking_lot = "0.12"` in workspace Cargo.toml, `parking_lot.workspace = true` in vox_core/Cargo.toml — no new dependencies required). Transcript writes are infrequent (once per dictation segment, not on a hot path), so mutex contention is negligible.

**Concurrent access budget**: The worst case is a transcript `save()` coinciding with a UI `list()` query. parking_lot::Mutex is non-async and blocks the calling thread. `save()` is an INSERT (~50µs for a single row). `list()` is a SELECT with LIMIT/OFFSET (~100µs for 50 rows). Maximum contention: one caller waits ~100µs — far below the 300ms latency budget and imperceptible to the UI. No priority inversion risk because both callers are non-real-time (the pipeline's hot path is ASR/LLM, not transcript storage).

**Auto-prune timing**: On `TranscriptStore::open()`, immediately run `DELETE FROM transcripts WHERE created_at < datetime('now', '-30 days')`. This happens once at startup — no background timer needed.

**Alternatives considered**:
- **In-memory with periodic flush**: Rejected. FR-015 requires persistence across restarts. In-memory with flush risks data loss on crash.
- **Separate database file per day**: Rejected. Over-engineered. Single table with date-based pruning is simpler.
- **chrono for timestamps**: Rejected per CLAUDE.md note: rusqlite 0.38 has no `FromSql` for `chrono::DateTime<Utc>`. Use `String` (ISO 8601) with `uuid::Uuid` for IDs.

## R-006: PipelineController Activation State Machine

**Decision**: Finite state machine with timer-based double-press detection.

**State machine**:
```
                    ActivationMode::HoldToTalk
                    ┌──────────────────────┐
     hotkey_press   │   Idle               │  hotkey_release
    ───────────────►│   ↓ press            │──────────────►
                    │   Listening           │   → stop + process remaining
                    └──────────────────────┘

                    ActivationMode::Toggle
                    ┌──────────────────────┐
     first press    │   Idle               │  second press
    ───────────────►│   ↓ press            │──────────────►
                    │   Listening           │   → stop + process remaining
                    └──────────────────────┘

                    ActivationMode::HandsFree
                    ┌──────────────────────┐
     double-press   │   Idle               │  single press
    ───────────────►│   ↓ double-press     │──────────────►
                    │   Listening           │   → stop + process remaining
                    │   (VAD auto-segments) │
                    └──────────────────────┘
```

**Double-press detection**: Track `last_press_time: Option<Instant>`. On press:
- If `last_press_time` is Some and elapsed < 300ms → double-press detected
- Otherwise → single press, record `last_press_time = Some(Instant::now())`

For HandsFree mode, the double-press starts continuous dictation. Single press during dictation stops it. No timer/timeout needed — the press events drive transitions directly.

**Mode persistence**: Stored in settings (config module). Loaded on startup, updated immediately when user changes via UI. The `PipelineController` reads the current mode from settings on each hotkey event (not cached).

**Alternatives considered**:
- **Dedicated timer thread for double-press**: Rejected. Simple `Instant::elapsed()` check on the event handler thread is sufficient.
- **Mode cached in controller**: Rejected. Reading from settings on each event ensures changes take effect immediately (FR-003, acceptance scenario 4).

## R-007: State Broadcasting Design

**Decision**: `tokio::sync::broadcast` channel with capacity 16. Latest-wins semantics for slow subscribers.

**Channel configuration**: `broadcast::channel::<PipelineState>(16)` — 16 slots provides enough buffer for rapid state transitions during a processing cycle (Listening → Processing(None) → Processing(Some) → Injecting → Listening = 5 transitions per segment).

**Subscriber overflow handling**: `broadcast` uses latest-wins by default. If a subscriber falls behind by more than 16 messages, it receives `RecvError::Lagged(n)` and the next successful recv gets the most recent state. This satisfies the spec's "latest-wins semantics, no crash or deadlock" edge case.

**State transitions per segment**:
1. → `Processing { raw_text: None }` (segment received)
2. → `Processing { raw_text: Some(raw) }` (ASR complete)
3. → `Injecting { polished_text }` (LLM complete, about to inject)
4. → `Listening` (injection complete, ready for next)
Or on error: → `Error { message }` → `Listening` or `Idle`

**Maximum subscriber count**: No hard limit is enforced. In practice, the subscriber count is small (<10): overlay HUD, settings panel status indicator, and potentially a log viewer. Each subscriber adds one receiver handle (~64 bytes) and receives clones of PipelineState (two small Strings). At 10 subscribers × 5 state transitions per segment × ~100 bytes per state = ~5 KB of clone overhead per segment — negligible. If a future design requires >100 subscribers, the broadcast channel remains correct but memory overhead should be re-evaluated.

**PipelineState must be Clone**: broadcast requires `T: Clone`. The enum contains only `String` and `Option<String>`, which are Clone. No issue.

**Alternatives considered**:
- **watch channel (single latest value)**: Rejected. Subscribers would miss intermediate states (Processing → Injecting). The UI needs every transition for smooth HUD updates.
- **mpsc per subscriber**: Rejected. Requires manual fan-out. broadcast handles multi-subscriber natively.
- **Custom observer pattern**: Rejected. tokio::sync::broadcast is purpose-built for this pattern.

## R-008: Existing `run_vad_loop()` Integration

**Decision**: Reuse the existing `run_vad_loop()` function with one modification: change `try_send` to `blocking_send` to prevent segment drops under backpressure (FR-017: strict FIFO, no drops).

**Key observations**:
- `run_vad_loop()` is synchronous and designed for `std::thread::spawn`
- Takes `&mut` references to VAD components (SileroVad, StateMachine, Chunker)
- Exits when `stop: &AtomicBool` is set
- Flushes remaining audio on stop (important for hold-to-talk: captures tail of speech)
- Sleeps 5ms when no audio available (yields CPU time)

**Required modification**: Change `segment_tx.try_send(segment)` to `segment_tx.blocking_send(segment)` in two locations (main loop and flush). `try_send` returns `Err(TrySendError::Full)` when the channel is at capacity, silently dropping the segment. `blocking_send` blocks the VAD thread until space is available, providing natural backpressure. This guarantees no segment is ever dropped (FR-017, spec edge case "Rapid successive utterances").

**Channel capacity justification**: The segment channel uses a capacity of 32. This is derived from worst-case throughput analysis: at ~250ms per segment processing and a minimum segment interval of ~1 second (VAD silence threshold of 500ms + minimum speech), the pipeline can never accumulate more than 1 segment of backlog under normal operation. The 32-slot buffer provides ~32× headroom for transient processing stalls (e.g., a slow LLM inference on a particularly long utterance) without the VAD thread ever blocking. This capacity uses 32 × (size of Vec<f32> pointer) ≈ 768 bytes of channel overhead — negligible. The VAD thread only blocks if the pipeline falls >32 segments behind — effectively impossible given the processing/arrival rate ratio.

**The pipeline spawns a std::thread with**:
```rust
std::thread::spawn(move || {
    let mut vad = SileroVad::new(&vad_model_path)?;
    let mut resampler = AudioResampler::new(native_rate, 16000);
    let mut state_machine = VadStateMachine::new(config.clone());
    let mut chunker = SpeechChunker::new(config);
    run_vad_loop(&mut consumer, resampler.as_mut(), &mut vad, &mut state_machine, &mut chunker, &segment_tx, &stop)
})
```

This creates all non-Send components on the thread where they'll be used, avoiding any thread-safety issues.

**Error return type compatibility**: `run_vad_loop()` returns `anyhow::Result<()>`, which is compatible with the pipeline's error handling. The `std::thread::spawn` closure returns this Result, and the pipeline receives it via `JoinHandle<Result<()>>`. On `Err`, the pipeline broadcasts `Error { message: err.to_string() }` and transitions to Idle (per R-009, "VAD thread panic" row). No type adaptation is needed at the thread boundary.

**Alternatives considered**:
- **Keep try_send with larger buffer**: Rejected. Any finite buffer can overflow under pathological load. The spec requirement is absolute: "no segment may be dropped."
- **Unbounded channel**: Rejected. Removes backpressure entirely, so a stuck pipeline could cause unbounded memory growth. Bounded with blocking_send is safer.
- **Log and continue on try_send failure**: Rejected. Violates FR-017 — a logged warning doesn't recover the lost segment.

## R-010: Controller/Pipeline Command Channel

**Decision**: PipelineController communicates with Pipeline via an `mpsc::channel<PipelineCommand>` (capacity 8), not via `&mut Pipeline` references. This resolves the mutability conflict where `run(&mut self)` holds an exclusive borrow for the duration of the processing loop, while hotkey handlers also need to control the pipeline.

**Command channel capacity justification**: Capacity of 8 is chosen because commands are consumed faster than they can be produced: (1) only one command type (Stop) currently exists, (2) the user physically cannot produce hotkey events faster than ~50ms apart, (3) each command is consumed on the next select! iteration (~microseconds). A capacity of 8 provides room for future command variants (e.g., Pause, Flush, ModeChange) without redesign. Even with rapid hotkey toggling, the pipeline processes commands near-instantly so the channel never approaches capacity.

**Problem**: `Pipeline::run(&mut self)` is an async loop that holds `&mut self` indefinitely. Hotkey handlers need to call `stop()` on the pipeline, but can't obtain `&mut Pipeline` while `run()` is active. Rust's borrow checker prevents this at compile time.

**Solution**: Command channel pattern.
```
PipelineController ──mpsc::Sender<PipelineCommand>──► Pipeline::run()
                                                       (select! on commands + segments)
```

- `Pipeline::new()` takes `command_rx: mpsc::Receiver<PipelineCommand>`
- `PipelineController::new()` takes `command_tx: mpsc::Sender<PipelineCommand>`
- `Pipeline::run()` uses `tokio::select!` to listen for both segments AND commands
- `PipelineController` methods send commands, never borrow Pipeline

**`Pipeline::run()` select loop**:
```rust
loop {
    tokio::select! {
        Some(segment) = self.segment_rx.recv() => {
            self.process_segment(segment).await?;
        }
        Some(cmd) = self.command_rx.recv() => {
            match cmd {
                PipelineCommand::Stop => {
                    // Current segment (if any) already completed
                    // before select! re-entered
                    break;
                }
            }
        }
        else => break, // Both channels closed
    }
}
```

**Key property**: A Stop command received during segment processing waits for the current segment to finish (the select! doesn't interrupt an in-progress process_segment call). This satisfies FR-018: "current segment MUST complete processing and injection before the pipeline goes Idle."

**Alternatives considered**:
- **Arc\<Mutex\<Pipeline\>\>**: Rejected. Mutex around the entire pipeline would serialize all operations and risk deadlocks with the async runtime.
- **Separate Pipeline and PipelineRunner**: Rejected. Splits a cohesive abstraction into two types that must be kept in sync.
- **AtomicBool stop flag only**: Rejected. Works for simple stop but can't support future commands (pause, flush, mode change). The channel is extensible.

## R-009: Error Recovery Strategy

**Decision**: Per-segment error isolation. A failure in ASR or LLM for one segment broadcasts Error state, discards that segment, and returns to Listening. The pipeline itself never crashes from a single-segment failure.

**Error categories and handling**:

| Error Source | Handling | State Transition | Spec Edge Case |
|-------------|----------|-----------------|----------------|
| ASR transcription failure | Log, discard segment | → Error(msg) → Listening | Pipeline error mid-segment |
| LLM processing failure | Log, discard segment | → Error(msg) → Listening | Pipeline error mid-segment |
| spawn_blocking JoinError | Log, discard segment (same as ASR/LLM) | → Error(msg) → Listening | Pipeline error mid-segment |
| Injection blocked (elevated, Windows) | Broadcast error with guidance | → Error(msg) → Listening | Elevated process target |
| Injection blocked (AX revoked, macOS) | Broadcast error with guidance | → Error(msg) → Listening | Accessibility permission revoked |
| Injection blocked (sandboxed app, macOS) | Broadcast error with app name | → Error(msg) → Listening | Sandboxed app target |
| Injection blocked (no focus) | Broadcast error | → Error(msg) → Listening | Target application focus change |
| Audio device disconnect | Broadcast error, stop pipeline | → Error(msg) → Idle | Audio device disconnection |
| VAD thread panic | Propagate, stop pipeline | → Error(msg) → Idle | VAD thread unexpected exit |
| VAD thread error exit (not panic) | Drain remaining segments, then stop | → Error(msg) → Idle | VAD thread unexpected exit |
| Channel closed (VAD thread exited normally) | Stop processing loop | → Idle | (normal shutdown) |
| SQLite database corrupted/locked | open()/load() returns Err, pipeline can't start | Error before start | SQLite database corruption |
| Empty/silent segment (RMS < 1e-3) | Skip ASR/LLM/injection | → Listening | Empty ASR output |
| Broadcast subscriber lagged | Subscriber gets latest state on next recv | (no state change) | Broadcast subscriber overflow |

**Implementation**: Each stage in the processing loop is wrapped in error handling:
```rust
match self.process_segment(segment).await {
    Ok(()) => {} // Transition handled internally
    Err(e) => {
        self.broadcast(PipelineState::Error { message: e.to_string() });
        // Continue loop — next segment will be processed
    }
}
```
