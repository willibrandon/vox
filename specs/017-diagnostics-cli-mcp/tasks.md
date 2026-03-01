# Tasks: Diagnostics, CLI Tool, and MCP Server

**Input**: Design documents from `/specs/017-diagnostics-cli-mcp/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/diagnostics-protocol.md, quickstart.md

**Tests**: Not explicitly requested in the feature specification. Test tasks included only for the UDS module (critical cross-platform infrastructure) and audio injector (complex pipeline integration) where correctness is hard to verify without automated tests.

**Organization**: Tasks grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace configuration, new crate scaffolding, and dependency additions.

**Key architectural decision**: UDS networking, protocol types, and the shared `DiagnosticsClient` live in a new lightweight `vox_diag` crate — NOT in `vox_core`. This prevents `vox_tool` and `vox_mcp` from pulling in `vox_core`'s heavy ML dependencies (whisper-rs, llama-cpp-2, ort, cpal, etc.), keeping their build times under 10 seconds instead of minutes. Supersedes research R-008 which rejected a separate crate as over-engineering — the ML dependency cost was not accounted for.

- [X] T001 Add `"crates/vox_diag"`, `"crates/vox_tool"`, `"crates/vox_mcp"` to workspace members and add `base64 = "0.22"` to workspace `[dependencies]` in `Cargo.toml`
- [X] T002 [P] Create `crates/vox_diag/` crate: `Cargo.toml` with `name = "vox_diag"`, edition = "2024", `[lib] path = "src/vox_diag.rs"`, deps: serde 1 (derive), serde_json 1, dirs 5, anyhow 1. Windows target dep: `windows = { version = "0.62", features = ["Win32_Networking_WinSock"] }`. Create `src/vox_diag.rs` (`pub mod net; pub mod protocol; pub mod client;`), `src/net.rs` (`pub mod uds;`), and empty source files `src/net/uds.rs`, `src/protocol.rs`, `src/client.rs`
- [X] T003 [P] Create `crates/vox_tool/Cargo.toml` with dependencies (clap 4 derive, serde_json, anyhow, `vox_diag` path dep, base64) and `crates/vox_tool/src/main.rs` with minimal `fn main()` that prints version
- [X] T004 [P] Create `crates/vox_mcp/Cargo.toml` with dependencies (rmcp 0.17 server+macros+transport-io, schemars 1, serde_json, serde derive, anyhow, tokio full, `vox_diag` path dep) and `crates/vox_mcp/src/main.rs` with minimal `#[tokio::main] async fn main()`
- [X] T005 Add `pub mod diagnostics;` to `crates/vox_core/src/vox_core.rs`. Create `crates/vox_core/src/diagnostics.rs` (declares `pub mod listener; pub mod handlers; pub mod audio_injector; pub mod screenshot;`). Add `vox_diag = { path = "../vox_diag" }` and `base64.workspace = true` to `crates/vox_core/Cargo.toml`. Note: NO `pub mod net;` in vox_core — UDS lives in vox_diag. NO `Win32_Networking_WinSock` in vox_core — it's in vox_diag. NO `Win32_Storage_Xps` anywhere — `PrintWindow` uses `Win32_Graphics_Gdi` which is already present

---

## Phase 2: Foundational (UDS + Protocol + Listener Core)

**Purpose**: Cross-platform UDS networking, protocol types, listener infrastructure, shared client, and VoxState wiring. MUST complete before any user story.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### UDS Module (in vox_diag)

- [X] T006 Implement Windows `UnixStream` in `crates/vox_diag/src/net/uds.rs`: struct wrapping `OwnedSocket`, `connect(path)`, `Read`/`Write` trait impls via `recv()`/`send()`, `shutdown()`, `set_nonblocking()`, `set_read_timeout()`, `try_clone() -> io::Result<Self>` (uses `WSADuplicateSocketW` to create a duplicate socket handle, then `WSASocketW` to materialize a new `SOCKET` from the `WSAPROTOCOL_INFOW`, wrap in a new `UnixStream` — required for subscribe's two-thread bidirectional I/O model per FR-028). Use `windows::Win32::Networking::WinSock` APIs. Include `WSAStartup` initialization via `std::sync::Once`. Port from `D:\SRC\rust_uds_windows` patterns per research R-001 (revised: R-001's "no try_clone()" predates FR-028's subscribe requirement)
- [X] T007 Implement Windows `UnixListener` in `crates/vox_diag/src/net/uds.rs`: struct wrapping `OwnedSocket`, `bind(path)` (creates `sockaddr_un`, binds, `listen(128)`), `accept()` returning `(UnixStream, SocketAddr)`, `set_nonblocking()`, `incoming()` iterator. Add `#[cfg(unix)]` re-exports of `std::os::unix::net::{UnixStream, UnixListener}` at top of file
- [X] T008 Add UDS tests in `crates/vox_diag/src/net/uds.rs`: test bind+accept+connect round-trip, test send/recv data, test nonblocking accept returns WouldBlock, test shutdown half-close, test set_read_timeout. Use `tempfile::TempDir` for socket paths. Add `tempfile = "3"` to `[dev-dependencies]` in `crates/vox_diag/Cargo.toml`

### Protocol Types (in vox_diag)

- [X] T009 Implement protocol types in `crates/vox_diag/src/protocol.rs`: `Request` struct (id: u64, method: String, params: Option<Value>) with Deserialize, `Response` struct (id: u64, result/error) with Serialize, `ErrorInfo` struct (code: i32, message: String), `ErrorCode` constants (INVALID_REQUEST=-32600, UNKNOWN_METHOD=-32601, INVALID_PARAMS=-32602, INTERNAL_ERROR=-32603, NOT_READY=-32000, ALREADY_RECORDING=-32001, NOT_RECORDING=-32002, CONNECTION_LIMIT=-32003), `Event` struct (event: String, data: Value) with Serialize, helper constructors `Response::success(id, value)` and `Response::error(id, code, message)`

### Shared Client (in vox_diag)

- [X] T010 Implement `DiagnosticsClient` in `crates/vox_diag/src/client.rs`: struct with `BufReader`/`BufWriter` over `UnixStream` + `AtomicU64` next_id. `connect(path)` connects to specific socket. `connect_auto()` scans `~/.vox/sockets/*.diagnostics.socket` — 0 found = error "No running Vox instance found", 1 found = connect, N found = error listing PIDs. `connect_auto_or_pid(pid: Option<u32>)` — if pid provided, connect to that specific socket; if None, call `connect_auto()`. `request(method, params) -> Result<Value>` sends JSON line, reads response line, returns result or maps error to anyhow. `read_line() -> Result<String>` — public method that reads one raw line from the underlying `BufReader`. Used by the CLI subscribe command to read event notifications after the initial subscribe response. Stale socket handling: if connect fails, delete the socket file and exclude from candidates

### Listener Core (in vox_core, uses vox_diag types)

- [X] T011 Implement `DiagnosticsListener` in `crates/vox_core/src/diagnostics/listener.rs`: struct with `socket_path: PathBuf`, `shutdown: Arc<AtomicBool>`, `handle: Mutex<Option<JoinHandle>>`, `active_connections: Arc<AtomicU32>`. Uses `vox_diag::net::uds::{UnixListener, UnixStream}`. `start(state, socket_dir)` creates `~/.vox/sockets/` dir, cleans stale sockets (try connect → fail → delete), binds `{pid}.diagnostics.socket`, spawns accept thread. `shutdown()` sets AtomicBool, drops/closes the `UnixListener` (which unblocks the `accept()` call — it returns an error), then joins thread. **Accept loop (interruptible)**: call `set_nonblocking(true)` on the listener. Loop: check `shutdown` AtomicBool → if true, break. Call `accept()` → if `WouldBlock`, sleep 100ms, continue. On real accept: check `active_connections` — if >= 4, write `Response::error(0, CONNECTION_LIMIT, "connection limit reached")` + newline to the new stream, close it, do NOT increment counter. If < 4, increment counter, spawn handler thread. The nonblocking + poll pattern ensures `shutdown()` can interrupt the accept loop within ~100ms. **Handler thread**: `BufReader::lines()` loop, parse `Request` via `vox_diag::protocol`, call `dispatch()`, write `Response` + newline, flush. Decrement `active_connections` on thread exit (connection close). Also define `DiagnosticsCommand` enum in this file: `StartRecording { reply: std::sync::mpsc::Sender<Result<()>> }`, `StopRecording { reply: std::sync::mpsc::Sender<Result<()>> }`, `CaptureScreenshot { window: String, reply: std::sync::mpsc::Sender<Result<Vec<u8>>> }`. Use `std::sync::mpsc` oneshot pattern (create channel per command, send the Sender in the command, receiver waits for reply)

### VoxState + LogBuffer Wiring

- [X] T012 Add `LogBuffer` struct to `crates/vox_core/src/log_sink.rs`: `Arc<parking_lot::RwLock<VecDeque<LogEntry>>>` capped at 10,000 entries. Methods: `push(entry)` (push_back, pop_front if over cap), `recent(count, min_level) -> Vec<LogEntry>` (filter by level, return last N). Modify `LogSink` to accept `Option<Arc<LogBuffer>>` and push to it on every event alongside the existing mpsc channel
- [X] T013 Add diagnostics fields to `VoxState` in `crates/vox_core/src/state.rs`: `log_buffer: Arc<LogBuffer>`, `diagnostics_cmd_tx: parking_lot::Mutex<Option<std::sync::mpsc::Sender<DiagnosticsCommand>>>`, `diagnostics_cmd_rx: parking_lot::Mutex<Option<std::sync::mpsc::Receiver<DiagnosticsCommand>>>`, `state_broadcast: tokio::sync::broadcast::Sender<PipelineState>` (capacity 64), `transcript_broadcast: tokio::sync::broadcast::Sender<TranscriptEvent>` (capacity 16). Add `TranscriptEvent` struct (timestamp: String, raw: String, polished: String, latency_ms: u64). Initialize all in `VoxState::new()`. Add accessor methods: `log_buffer()`, `diagnostics_cmd_sender()`, `take_diagnostics_cmd_rx()`, `state_broadcast_subscribe()`, `transcript_broadcast_subscribe()`, `send_state_broadcast()`, `send_transcript_broadcast()`
- [X] T014 Wire `DiagnosticsListener` in `crates/vox/src/main.rs`: after `VoxState::new()`, call `DiagnosticsListener::start(state_arc, socket_dir)` and store the listener for shutdown. Take `diagnostics_cmd_rx` from VoxState, spawn a 50ms GPUI interval timer that polls `try_recv()` — for now just log received commands (actual dispatch added in US3/US4 phases). Call `listener.shutdown()` during app exit
- [X] T015 Feed broadcast channels from pipeline paths in `crates/vox/src/main.rs`: in the existing GPUI forwarding task that receives per-session pipeline state updates, also call `VoxState::send_state_broadcast(state)` for each transition. In `TranscriptWriter::save()` (or the orchestrator completion path in main.rs), also call `VoxState::send_transcript_broadcast(event)` with timestamp/raw/polished/latency_ms. This wiring is separate from T014 (listener startup) because it touches different code paths — the pipeline state forwarding and transcript save paths

**Checkpoint**: Foundation ready — UDS works cross-platform (vox_diag), protocol types defined (vox_diag), client can discover and connect (vox_diag), listener accepts connections and dispatches to handlers (vox_core), broadcast channels fed from pipeline events. User story implementation can now begin.

---

## Phase 3: User Story 1 — Query Running Instance Status (Priority: P1) 🎯 MVP

**Goal**: AI assistants and developers can query status, settings, logs, and transcripts from a running Vox instance.

**Independent Test**: Connect to running Vox via `DiagnosticsClient`, call `status`/`settings`/`logs`/`transcripts`, verify structured JSON responses.

### Implementation for User Story 1

- [X] T016 [US1] Implement `dispatch()` function and `handle_status()` in `crates/vox_core/src/diagnostics/handlers.rs`: dispatch matches method string to handler functions (uses `vox_diag::protocol::{Request, Response, ErrorCode}`), returns `Response::error(UNKNOWN_METHOD)` for unrecognized methods. `handle_status(state)` builds StatusSnapshot JSON from VoxState: pid via `std::process::id()`, readiness (match AppReadiness variants to strings), pipeline_state, activation_mode from settings, recording from `RecordingSession::active`, debug_audio from settings, gpu (name/vram_bytes/platform from GpuInfo), models (iterate model_runtime map), audio (device name/sample_rate/rms), last_latency_ms
- [X] T017 [US1] Implement `handle_settings()` with get action in `crates/vox_core/src/diagnostics/handlers.rs`: parse params for `action` (required — missing → error -32602, invalid → error -32602). For action "get": if no `key`, serialize full Settings to JSON via serde and return as result. If `key` provided, validate key exists (see note below), serialize Settings to serde_json::Value map, extract the single key's value, return as `{"key": value}`. **Note on key extraction**: serialize Settings to `serde_json::Map`, look up the key. Unknown key (not in map) → error -32602 "unknown setting: '{key}'". This avoids needing a separate field accessor per key
- [X] T018 [US1] Implement `handle_logs()` in `crates/vox_core/src/diagnostics/handlers.rs`: parse optional `count` (default 50) and `min_level` (default "trace") from params. Call `log_buffer.recent(count, min_level)`. Map LogEntry to JSON with timestamp/level/target/message. Return structured error -32602 for invalid min_level string
- [X] T019 [US1] Implement `handle_transcripts()` in `crates/vox_core/src/diagnostics/handlers.rs`: parse optional `count` (default 10) from params. Query `TranscriptStore` for recent entries (use existing `search("")` with limit, or add `recent(count)` method if needed). Map each entry to JSON with timestamp/raw/polished/latency_ms

**Checkpoint**: US1 complete — status, settings read, logs, and transcripts all queryable. AI assistants can observe the running instance.

---

## Phase 4: User Story 6 — Modify Settings Remotely (Priority: P6)

**Goal**: AI assistants can write settings values with type validation and immediate persistence.

**Independent Test**: Set a setting value, read it back, verify it changed. Try setting with wrong type, verify structured error.

**Note**: Implemented early (before US2-US5) because it completes the `settings` method handler started in US1 and is only ~60 lines.

### Implementation for User Story 6

- [X] T020 [US6] Add `SettingType` enum and `Settings::field_type(key) -> Option<SettingType>` method in `crates/vox_core/src/config.rs`: enum variants `Float`, `Integer`, `Bool`, `String` (per research R-009 — all 4 variants). Map each known setting key to its expected type using the ACTUAL struct field names: `vad_threshold`→Float, `noise_gate`→Float, `min_silence_ms`→Integer, `min_speech_ms`→Integer, `language`→String, `whisper_model`→String, `llm_model`→String, `temperature`→Float, `remove_fillers`→Bool, `course_correction`→Bool, `punctuation`→Bool, `activation_hotkey`→String, `activation_mode`→String, `overlay_opacity`→Float, `show_raw_transcript`→Bool, `save_history`→Bool, `debug_audio`→String, `max_segment_ms`→Integer, `overlap_ms`→Integer, `command_prefix`→String. Return `None` for unknown keys. Also add `Settings::set_field(key: &str, value: serde_json::Value) -> Result<()>`: serialize self to `serde_json::Map`, insert `key`/`value`, deserialize back to `Settings`, replace self. This is the mechanism for setting individual fields by key name without a per-field match arm
- [X] T021 [US6] Implement `handle_settings_set()` in `crates/vox_core/src/diagnostics/handlers.rs`: extend the existing `handle_settings()` to handle action "set". Require `key` and `value` params (missing → error -32602). Validate key via `Settings::field_type(key)` — unknown key → error -32602 "unknown setting: '{key}'". Validate value type matches SettingType: Float checks `Value::is_f64()`, Integer checks `Value::is_u64()` or `Value::is_i64()`, Bool checks `Value::is_boolean()`, String checks `Value::is_string()` — mismatch → error -32602 "invalid type for '{key}': expected {expected}, got {actual}". Call `VoxState::update_settings()` which internally uses `Settings::set_field()` to apply the change and `Settings::save()` to persist. Return `{"ok": true}`

**Checkpoint**: US6 complete — settings read AND write operational. Full settings method is done.

---

## Phase 5: User Story 2 — Inject Test Audio and Receive Transcription (Priority: P2)

**Goal**: AI assistants can feed WAV/PCM audio into the pipeline and receive transcript results without keystroke injection.

**Independent Test**: Send a WAV file path to `inject_audio`, verify transcript text and latency_ms returned. Verify no keystrokes injected.

### Implementation for User Story 2

- [X] T022 [US2] Implement `AudioInjector` in `crates/vox_core/src/diagnostics/audio_injector.rs`: static methods `load_wav(path) -> Result<(Vec<f32>, u32)>` reads WAV via hound, converts to mono f32 (average channels if stereo), returns samples + sample_rate. `load_pcm(base64_data, sample_rate) -> Result<(Vec<f32>, u32)>` decodes base64 f32 LE bytes. Struct fields: `samples: Vec<f32>`, `sample_rate: u32` (per data-model.md entity design). `run(&self, state: &VoxState) -> Result<InjectionResult>` clones `AsrEngine` and `PostProcessor` from VoxState via `clone_asr_engine()`/`clone_llm_processor()` (returns error -32000 if either is None = not ready), creates `HeapRb<f32>` ring buffer, pushes all samples to producer then drops producer (signals EOF/end-of-audio), creates Pipeline with a `NoOpInjector`, runs pipeline to completion, collects transcript + latency. `InjectionResult` struct: raw_transcript, polished_text, latency_ms. **NoOpInjector**: define a `NoOpInjector` struct in this file that implements the text injection trait (`TextInjector` or equivalent) as a no-op — `inject()` stores the polished text in an `Arc<Mutex<Option<String>>>` instead of simulating keystrokes. This is how FR-009 is satisfied
- [X] T023 [US2] Implement `handle_inject_audio()` in `crates/vox_core/src/diagnostics/handlers.rs`: parse params — exactly one of `path` or `pcm_base64` required (both present → error -32602 "provide either 'path' or 'pcm_base64', not both"; both absent → error -32602 "must provide either 'path' or 'pcm_base64'"). If `pcm_base64`, also require `sample_rate` (missing → error -32602). Check VoxState readiness (not ready → error -32000). Load audio via `AudioInjector::load_wav()` or `load_pcm()`. Validate non-empty samples (0 samples → error -32602). Construct `AudioInjector { samples, sample_rate }`, call `.run(state)`. Return `{raw_transcript, polished_text, latency_ms, injected: true}`. Map file-not-found, corrupt WAV, invalid base64, and pipeline errors to structured errors per diagnostics-protocol.md
- [X] T024 [US2] Add audio injector tests in `crates/vox_core/src/diagnostics/audio_injector.rs`: test `load_wav()` with `tests/fixtures/speech_sample.wav` (verify samples non-empty, sample_rate correct), test `load_pcm()` with known base64 f32 data (encode 4 f32 values, decode, verify round-trip), test `load_wav()` with nonexistent path returns error, test `load_wav()` with empty WAV (if applicable) returns appropriate error

**Checkpoint**: US2 complete — audio injection through full pipeline returns transcripts. AI assistants can test pipeline behavior.

---

## Phase 6: User Story 3 — Remote Recording Control (Priority: P3)

**Goal**: AI assistants can start and stop recording sessions remotely.

**Independent Test**: Send `record start`, verify pipeline transitions to listening. Send `record stop`, verify return to idle. Test error cases (already recording, not recording, not ready).

### Implementation for User Story 3

- [X] T025 [US3] Implement `handle_record()` in `crates/vox_core/src/diagnostics/handlers.rs`: parse `action` param (required — missing → error -32602, invalid → error -32602 "invalid action: must be 'start' or 'stop'"). Check VoxState readiness (not ready → error -32000). For "start": check not already recording via `RecordingSession::active` (→ error -32001 "already recording"). **Subscribe to `state_broadcast` BEFORE sending the command** — this is critical to avoid a race condition where the pipeline transitions to Listening before the handler starts listening for events. Then send `DiagnosticsCommand::StartRecording` via command channel, await oneshot reply for dispatch acknowledgment. Then wait on the already-subscribed `state_broadcast` receiver for `PipelineState::Listening` (or equivalent active state) with a 1-second timeout per SC-003 — timeout → error -32603 "recording failed to start within timeout". For "stop": check is recording (→ error -32002 "not recording"), subscribe to `state_broadcast` first, then send `StopRecording`, await reply, then wait for `PipelineState::Idle` on the pre-subscribed receiver with 1-second timeout. Return `{"ok": true}` only after state transition is confirmed. **Ordering invariant**: always subscribe → send command → await confirmation. Never send → subscribe (the broadcast has no replay buffer — missed events are lost forever)
- [X] T026 [US3] Wire `StartRecording`/`StopRecording` command dispatch in `crates/vox/src/main.rs`: in the GPUI 50ms poll timer (from T014), match `DiagnosticsCommand::StartRecording` → dispatch `ToggleRecording` action if idle, send Ok via reply channel. Match `StopRecording` → dispatch `ToggleRecording` if recording, send Ok via reply. Reply with Err if wrong state (e.g., start when already recording)

**Checkpoint**: US3 complete — recording can be started and stopped remotely with confirmed state transitions.

---

## Phase 7: User Story 4 — Capture Window Screenshots (Priority: P4)

**Goal**: AI assistants can capture screenshots of overlay and settings windows as PNG images.

**Independent Test**: Request screenshot of visible overlay window, verify PNG bytes returned. Request screenshot of closed window, verify error.

### Implementation for User Story 4

- [X] T027 [P] [US4] Implement Windows screenshot capture in `crates/vox_core/src/diagnostics/screenshot.rs`: `capture_window(hwnd: HWND) -> Result<Vec<u8>>` — `GetClientRect` for dimensions, create compatible DC + bitmap via `Win32_Graphics_Gdi` (already in vox_core's Cargo.toml features), `PrintWindow(hwnd, hdc, PRINT_WINDOW_FLAGS(2))` (PW_RENDERFULLCONTENT per research R-004), `GetDIBits` to read BGRA pixels, flip rows (DIB is bottom-up), encode as PNG via `png` crate. Add `#[cfg(unix)]` stub that returns error "not implemented on this platform"
- [X] T028 [P] [US4] Implement macOS screenshot capture in `crates/vox_core/src/diagnostics/screenshot.rs`: `capture_window(window_id: u32) -> Result<Vec<u8>>` — `CGWindowListCreateImage` with `kCGWindowImageBoundsIgnoreFraming`, convert `CGImage` to pixel data, encode as PNG. Add `#[cfg(windows)]` stub for macOS-only function
- [X] T029 [US4] Implement `handle_screenshot()` in `crates/vox_core/src/diagnostics/handlers.rs`: parse optional `window` param (default "overlay"). Validate window name ("overlay" or "settings", unknown → error -32602 "unknown window: '{name}'"). Send `DiagnosticsCommand::CaptureScreenshot { window, reply }` via command channel, await oneshot reply. On success: encode PNG bytes as base64, return `{format: "png", data: base64_string}`. On error: return -32603 with reason. Wire `CaptureScreenshot` dispatch in `crates/vox/src/main.rs` GPUI poll timer: extract HWND (Windows) or CGWindowID (macOS) from the appropriate window handle (`OverlayWindowHandle` global for overlay, settings window handle for settings), call `capture_window()`, send result via reply channel

**Checkpoint**: US4 complete — screenshots of any visible Vox window available to AI assistants.

---

## Phase 8: User Story 5 — Stream Real-Time Pipeline Events (Priority: P5)

**Goal**: Clients can subscribe to live pipeline state, audio RMS, and transcript events.

**Independent Test**: Subscribe to events, trigger a recording, verify state transitions and transcript event arrive. Send unsubscribe, verify events stop.

### Implementation for User Story 5

- [X] T030 [US5] Implement `handle_subscribe()` in `crates/vox_core/src/diagnostics/handlers.rs`: parse `events` array param (required, non-empty → error -32602 if empty, valid types: "pipeline_state", "audio_rms", "transcript" → error -32602 for unknown type). Send initial `Response::success` with `{subscribed: [...]}`. Then call `stream.try_clone()?` to create separate reader/writer handles for the two threads (on macOS this uses `std::os::unix::net::UnixStream::try_clone()` from std; on Windows this uses our `WSADuplicateSocketW`-based impl from T006). Switch connection to event-streaming mode with two threads sharing an `AtomicBool` shutdown flag: **Writer thread (owns the cloned stream)**: **always subscribes to `state_broadcast` internally** (via `VoxState::state_broadcast_subscribe()`) regardless of which events the client requested — this is required for RMS gating (knowing when recording is active). Also subscribes to `transcript_broadcast` (via `VoxState::transcript_broadcast_subscribe()`) if "transcript" is in client's events list. **Event forwarding is filtered by the client's subscription**: pipeline_state transitions are only forwarded to the client if "pipeline_state" is in the subscribed events list, but the writer thread always observes them internally for RMS gating. For "audio_rms" events: when a PipelineState::Listening is received from state_broadcast, start polling `VoxState::latest_rms()` at 30Hz (33ms sleep loop) and push `Event { event: "audio_rms", data: {rms: f32} }`; stop RMS polling when PipelineState::Idle is received. For "transcript" events: forward each TranscriptEvent as `Event { event: "transcript", data: {raw, polished, latency_ms} }`. **Reader thread**: read lines from client socket. Parse raw JSON (NOT via Request deserialization — unsubscribe messages have no `id` field per the protocol contract). Check if `method` == `"unsubscribe"` → set shutdown flag → break. On any I/O error → set shutdown flag → break. On shutdown: both threads exit, connection handler returns (connection closes)
- [X] T031 [US5] Verify broadcast channel integration (from T015) works end-to-end: confirm that starting a recording triggers pipeline_state events on subscribers, that transcript completion triggers transcript events, and that RMS polling activates only during active recording. This is a validation/integration step, not new code — T015 already wired the broadcast feeds

**Checkpoint**: US5 complete — live event streaming works for pipeline state, RMS, and transcripts.

---

## Phase 9: User Story 7 — CLI Tool for Human Developers (Priority: P7)

**Goal**: Developers can interact with running Vox from the terminal via `vox-tool` commands.

**Independent Test**: Run `vox-tool status` against running Vox, verify formatted output. Run `vox-tool inject speech.wav`, verify transcript printed.

### Implementation for User Story 7

- [X] T032 [US7] Implement clap command structure in `crates/vox_tool/src/main.rs`: top-level `#[derive(Parser)]` with `--pid <PID>` optional arg. Subcommands: `Status`, `Settings { action: SettingsAction }` (Get with optional key, Set with key+value args), `Logs { count: Option<u32>, level: Option<String> }`, `Record { action: RecordAction }` (Start/Stop), `Inject { path: PathBuf }`, `Screenshot { window: Option<String>, output: Option<PathBuf> }`, `Subscribe { events: Option<String> }`, `Transcripts { count: Option<u32> }`. Uses `vox_diag::client::DiagnosticsClient` (NOT vox_core)
- [X] T033 [US7] Implement command handlers in `crates/vox_tool/src/main.rs`: connect via `DiagnosticsClient::connect_auto_or_pid(pid)` from vox_diag. For each subcommand: build params JSON, call `client.request(method, params)`, format and print result to stdout. `status`: pretty-print JSON. `settings get`: print value. `settings set`: print ok. `logs`: print entries line by line. `record start/stop`: print ok. `inject`: print transcript result. `screenshot`: if `--output`, decode base64 and write PNG bytes to file; else print base64 size. `subscribe`: call `client.request("subscribe", params)` to get the initial ack, then loop calling `client.read_line()` (from T010) to read raw event JSON lines, parse each as `vox_diag::protocol::Event`, format and print to stdout, until Ctrl+C or I/O error. `transcripts`: print entries. All errors to stderr, exit code 1. Exit code 0 on success (FR-031)
- [X] T034 [US7] Handle multi-instance discovery in `crates/vox_tool/src/main.rs`: when `connect_auto()` finds multiple sockets and no `--pid` was provided, print "Multiple Vox instances found:" with PID list extracted from socket filenames, then "Use --pid <PID> to specify which instance." and exit 1. This behavior is built into `DiagnosticsClient::connect_auto()` (T010), so the CLI just formats the error message

**Checkpoint**: US7 complete — full CLI tool for human developer interaction with all commands.

---

## Phase 10: User Story 8 — MCP Server for AI Assistants (Priority: P8)

**Goal**: AI assistants call MCP tools to interact with Vox via the standard Model Context Protocol.

**Independent Test**: Launch `vox-mcp` with Vox running, connect via MCP protocol, call `vox_status` tool, verify JSON result.

### Implementation for User Story 8

- [X] T035 [US8] Implement `VoxMcp` struct and `#[tool_router]` impl in `crates/vox_mcp/src/main.rs`: struct holds `Mutex<DiagnosticsClient>` from vox_diag (plain `std::sync::Mutex`, no `Arc` wrapper — rmcp's `ServerHandler` does not require `Clone` on the handler struct; tool methods take `&self` and lock the mutex). Define parameter structs with `#[derive(Deserialize, JsonSchema)]` for each tool that needs params. Implement 9 `#[tool]` methods: `vox_status` (no params), `vox_settings_get` (optional key), `vox_settings_set` (key + value), `vox_logs` (optional count + level), `vox_record_start` (no params), `vox_record_stop` (no params), `vox_inject_audio` (path), `vox_screenshot` (optional window), `vox_transcripts` (optional count). Each tool: lock client, call `request()`, wrap result in `CallToolResult::success(vec![Content::text(json)])`. For screenshot: use `Content::image(base64, "image/png")`. Map diagnostics errors to `McpError::internal_error()`
- [X] T036 [US8] Implement `ServerHandler` trait and main function in `crates/vox_mcp/src/main.rs`: `#[tool_handler] impl ServerHandler for VoxMcp` with `get_info()` returning server name "vox-mcp", version from `env!("CARGO_PKG_VERSION")`, description "Control and inspect a running Vox dictation engine instance", `ServerCapabilities::builder().enable_tools().build()`. `#[tokio::main] async fn main()`: parse `--pid <PID>` from `std::env::args()` (simple manual parsing — no clap needed for one optional arg). Init tracing to stderr (MCP uses stdout for protocol). Call `DiagnosticsClient::connect_auto_or_pid(pid)` from vox_diag — if `--pid` provided, connect to that specific instance; if omitted and multiple found, fail with PID list error. Create `VoxMcp`, `.serve(stdio()).await`, `.waiting().await`. MCP config usage: `"args": ["--pid", "12345"]` when multiple instances run. Multi-instance handling matches CLI behavior — fail with actionable error listing PIDs, never silently connect to "first found"
- [X] T037 [US8] Verify no subscribe tool is exposed (FR-032): confirm `#[tool_router]` impl has exactly 9 tools (no vox_subscribe). Add doc comment on `VoxMcp` explaining that subscribe is excluded per FR-032

**Checkpoint**: US8 complete — MCP server exposes all 9 tools. AI assistants can use Vox diagnostics natively.

---

## Phase 11: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, validation, and cross-cutting improvements

- [X] T038 [P] Add `///` doc comments to all `pub` items across all new files: `vox_diag/src/net/uds.rs`, `vox_diag/src/protocol.rs`, `vox_diag/src/client.rs`, `vox_core/src/diagnostics/listener.rs`, `vox_core/src/diagnostics/handlers.rs`, `vox_core/src/diagnostics/audio_injector.rs`, `vox_core/src/diagnostics/screenshot.rs` (Constitution principle VII)
- [X] T039 [P] Verify builds compile without warnings: `cargo build -p vox_diag`, `cargo build -p vox_tool`, `cargo build -p vox_mcp` (should be fast — no ML deps), `cargo build -p vox_core --features cuda` (includes diagnostics module). Verify `vox_tool` and `vox_mcp` do NOT depend on vox_core (check with `cargo tree -p vox_tool` and `cargo tree -p vox_mcp`)
- [X] T040 Run quickstart.md validation: start Vox, run `vox-tool status`, `vox-tool logs --count 5`, `vox-tool settings`, `vox-tool transcripts`. Verify all return valid JSON output. Test MCP server launches and responds to tool list query

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — can start immediately
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories
- **US1 (Phase 3)**: Depends on Foundational — the MVP
- **US6 (Phase 4)**: Depends on US1 (extends settings handler)
- **US2 (Phase 5)**: Depends on Foundational — independent of US1/US6
- **US3 (Phase 6)**: Depends on Foundational — independent of US1/US2
- **US4 (Phase 7)**: Depends on Foundational — independent of US1/US2/US3
- **US5 (Phase 8)**: Depends on Foundational — independent of other stories
- **US7 (Phase 9)**: Depends on ALL handler stories (US1-US6) — wraps all methods in CLI commands
- **US8 (Phase 10)**: Depends on ALL handler stories (US1-US6) — wraps all methods in MCP tools
- **Polish (Phase 11)**: Depends on all stories being complete

### User Story Dependencies

```text
Setup ──→ Foundational ──┬──→ US1 (status/settings-get/logs/transcripts) ──→ US6 (settings-set)
                         │
                         ├──→ US2 (audio injection) ─────────────────────────────────────────┐
                         │                                                                    │
                         ├──→ US3 (recording control) ───────────────────────────────────────┤
                         │                                                                    │
                         ├──→ US4 (screenshots) ─────────────────────────────────────────────┤
                         │                                                                    │
                         └──→ US5 (event streaming) ─────────────────────────────────────────┤
                                                                                              │
                                                                         US7 (CLI) ◄──────────┤
                                                                         US8 (MCP) ◄──────────┘
```

### Within Each User Story

- Handler implementation before integration
- Core logic before error handling
- Story complete before moving to CLI/MCP consumers

### Parallel Opportunities

- **Phase 1**: T002, T003, T004 can all run in parallel
- **Phase 2**: T006+T007 (UDS) can run parallel with T009 (protocol), T010 (client), T012 (LogBuffer); all are in different files/crates
- **Phase 3-8**: US2, US3, US4, US5 can all run in parallel after Foundational (they touch different handler functions and different files)
- **Phase 7**: T027 (Windows screenshot) and T028 (macOS screenshot) are parallel
- **Phase 9-10**: US7 and US8 can run in parallel (different crates entirely)
- **Phase 11**: T038 and T039 can run in parallel

---

## Parallel Example: After Foundational Phase

```text
# These can all launch simultaneously (different files, no dependencies between them):
Agent A: US2 — T022 AudioInjector in vox_core/src/diagnostics/audio_injector.rs
Agent B: US3 — T025 record handler in vox_core/src/diagnostics/handlers.rs (handle_record fn)
Agent C: US4 — T027 Windows screenshot in vox_core/src/diagnostics/screenshot.rs
Agent D: US5 — T030 subscribe handler in vox_core/src/diagnostics/handlers.rs (handle_subscribe fn)

# Note: US3 and US5 both add functions to handlers.rs but touch different functions,
# so they CAN run in parallel if using separate worktrees or careful merging.
# Safer to run US3 and US5 sequentially within handlers.rs.
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (including vox_diag crate)
2. Complete Phase 2: Foundational (UDS + Protocol + Client + Listener)
3. Complete Phase 3: US1 (status, settings read, logs, transcripts)
4. **STOP and VALIDATE**: Connect with `DiagnosticsClient`, query all 4 methods
5. Immediate value: AI assistants can observe Vox state

### Incremental Delivery

1. Setup + Foundational → Infrastructure ready
2. US1 → Read-only diagnostics (MVP!)
3. US6 → Settings write (completes the settings method)
4. US2 → Audio injection (highest-value diagnostic)
5. US3 → Recording control
6. US4 → Screenshots
7. US5 → Event streaming
8. US7 → CLI tool (wraps everything for humans)
9. US8 → MCP server (wraps everything for AI assistants)
10. Polish → Doc comments, validation

### Parallel Team Strategy

With multiple agents after Foundational:
- Agent A: US1 + US6 (settings handler, sequential)
- Agent B: US2 (audio injection, independent)
- Agent C: US3 + US4 (recording + screenshot, sequential — both use command channel)
- Agent D: US5 (subscribe, independent)
- Then: US7 + US8 in parallel (different crates)

---

## Crate Dependency Graph

```text
vox_diag (lightweight: serde, dirs, windows/WinSock)
  ├── used by: vox_core (for UnixStream/UnixListener in listener, protocol types in handlers)
  ├── used by: vox_tool (for DiagnosticsClient, no ML deps!)
  └── used by: vox_mcp (for DiagnosticsClient, no ML deps!)

vox_core (heavy: whisper-rs, llama-cpp-2, ort, cpal, ...)
  ├── depends on: vox_diag
  └── used by: vox (the binary)
```

---

## Review Findings Addressed

This revision addresses 14 review findings from the initial tasks.md:

| # | Finding | Resolution |
|---|---------|------------|
| 1 | Record handler oneshot doesn't wait for state confirmation (SC-003) | T025: now waits on `state_broadcast` for state transition with 1s timeout after oneshot dispatch |
| 2 | SettingType missing Bool variant | T020: includes all 4 variants (Float, Integer, Bool, String) per R-009, maps bool fields like `save_history`, `remove_fillers` |
| 3 | handle_settings_set has no mechanism to set individual fields by key | T020: adds `Settings::set_field(key, value)` using serialize→modify→deserialize pattern |
| 4 | AudioInjector::run() signature diverges from data-model | T022: `run(&self, state: &VoxState)` with samples/sample_rate as struct fields, matching data-model.md |
| 5 | Subscribe RMS polling doesn't specify how to detect active recording | T030: writer thread observes PipelineState transitions from state_broadcast — starts RMS polling on Listening, stops on Idle |
| 6 | Connection limit check has race condition | T011: always `accept()` first, THEN check `active_connections`, reject with error response if full |
| 7 | Unsubscribe request has no id but Request requires id | T030: reader thread parses raw JSON (not Request deserialization) to detect `"method":"unsubscribe"` |
| 8 | vox_tool/vox_mcp pull all ML deps via vox_core | **New `vox_diag` crate** (T002): UDS, protocol, client extracted. vox_tool/vox_mcp depend on vox_diag only |
| 9 | Win32_Storage_Xps is wrong for PrintWindow | T005: removed entirely. PrintWindow uses Win32_Graphics_Gdi (already in vox_core Cargo.toml line 63) |
| 10 | Arc\<Mutex\> unnecessary for DiagnosticsClient in MCP | T035: uses `Mutex<DiagnosticsClient>` directly, no Arc, no Clone derive |
| 11 | MCP silently connects to first on multi-instance | T036: uses `connect_auto_or_pid(None)` which fails with PID list on multi-instance |
| 12 | NoOpInjector struct never created | T022: explicitly defines `NoOpInjector` implementing text injection trait |
| 13 | T015 and T031 overlap on broadcast wiring | T015: feeds broadcast channels from pipeline. T031: validates integration (no new code, just verification) |
| 14 | Settings get response format | T017: specifies format — get-one returns `{"key": value}`, get-all returns full Settings object |
| 15 | T025 subscribes to state_broadcast AFTER sending command — guaranteed race | T025: **subscribe BEFORE sending command**. Ordering invariant: subscribe → send → await. Broadcast has no replay buffer |
| 16 | T033 subscribe CLI can't read event stream — DiagnosticsClient has no public line reader | T010: adds `read_line() -> Result<String>` public method. T033: uses `client.read_line()` in loop after initial ack |
| 17 | T036 MCP has no --pid argument but fails with "specify PID" on multi-instance | T036: parses `--pid <PID>` from `std::env::args()`. MCP config: `"args": ["--pid", "12345"]` |
| 18 | T011 accept() blocks indefinitely — shutdown() can't interrupt it | T011: `set_nonblocking(true)` + poll loop with 100ms sleep + check shutdown AtomicBool each iteration |
| 19 | T030 subscribe writer needs state_broadcast for RMS gating even if client didn't request pipeline_state | T030: writer **always subscribes to state_broadcast internally** for RMS gating; only forwards pipeline_state events if in client's subscribed list |
| 20 | T006/T030 Windows UDS missing try_clone() but subscribe needs two-thread bidirectional I/O | T006: adds `try_clone()` via `WSADuplicateSocketW`. T030: explicitly uses `stream.try_clone()?`. R-001 "no try_clone()" predates FR-028 |

### Upstream Document Updates Needed

These design documents should be updated to match the task revisions:

- **research.md R-001**: Revise "no try_clone() / WSADuplicateSocketW — not needed" to "try_clone() required for subscribe's two-thread bidirectional I/O model (FR-028). Implemented via WSADuplicateSocketW (~15 lines)"
- **research.md R-008**: Revise to explain `vox_diag` crate (replaces "rejected separate crate" with "accepted — ML dependency cost justifies extraction")
- **plan.md**: Update project structure for 3 new crates (vox_diag, vox_tool, vox_mcp) instead of 2. Remove `Win32_Storage_Xps` reference. Move `net/uds.rs` and `diagnostics/client.rs` and `diagnostics/protocol.rs` from vox_core to vox_diag
- **data-model.md**: Update DiagnosticsClient location to vox_diag. Clarify Settings field names match actual struct (not the approximate names in the current field type table)
- **contracts/diagnostics-protocol.md**: Note that unsubscribe messages have no `id` field (intentional deviation from Request struct). Clarify connection limit flow (accept-first, then check)
- **quickstart.md**: Update build commands to include `cargo build -p vox_diag`. Note MCP --pid support for multi-instance

---

## Notes

- [P] tasks = different files, no dependencies
- [Story] label maps task to specific user story for traceability
- US6 placed early (Phase 4) because it's a ~60-line extension of the settings handler from US1
- US7 and US8 are consumers of ALL handler stories — they must come last before Polish
- handlers.rs is touched by multiple stories — when parallelizing, use separate functions to avoid merge conflicts
- The `dispatch()` function in handlers.rs routes all methods — it grows as each story adds handlers
- vox_tool and vox_mcp depend on vox_diag ONLY — fast builds, no ML compilation
