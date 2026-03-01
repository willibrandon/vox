# Research: Diagnostics, CLI Tool, and MCP Server

**Feature**: 017-diagnostics-cli-mcp
**Date**: 2026-02-28

## R-001: Windows UDS Implementation Strategy

**Decision**: Port `uds_windows` patterns to `windows` 0.62, strip to ~200 lines.

**Rationale**: The reference crate (`D:\SRC\rust_uds_windows`) uses `winapi` 0.3 which conflicts with the project's `windows` 0.62. We need only blocking stream I/O for a dedicated thread — no overlapped I/O, no `pair()`, no `try_clone()`.

**Key API calls to port** (from winapi → windows 0.62):
- `WSAStartup(0x202, &mut data)` via `Once::call_once`
- `WSASocketW(AF_UNIX, SOCK_STREAM, 0, null, 0, 0)` — drop `WSA_FLAG_OVERLAPPED` since we don't need async
- `bind()`, `listen(128)`, `accept()`, `connect()`, `recv()`, `send()`, `closesocket()`
- `ioctlsocket(FIONBIO)` for `set_nonblocking`
- `setsockopt(SOL_SOCKET, SO_RCVTIMEO)` for read timeouts
- `SetHandleInformation` to clear `HANDLE_FLAG_INHERIT`

**sockaddr_un**: Manual `#[repr(C)]` struct with `sun_family: u16` + `sun_path: [u8; 108]`. Path encoded as UTF-8 bytes, null-terminated.

**What we strip** (~1,800 lines removed):
- Overlapped I/O (`ext.rs`, 695 lines) — not needed
- `pair()` — uses tempdir hack, not needed
- `try_clone()` / `WSADuplicateSocketW` — not needed
- All `winapi` imports → replace with `windows` 0.62 equivalents

**Required new `windows` feature**: `Win32_Networking_WinSock`

**Alternatives considered**:
- Use `uds_windows` crate directly: rejected (depends on `winapi`, conflicts with `windows` 0.62)
- Use `interprocess` crate: rejected (adds 40+ transitive deps, most unused)
- Named pipes instead of UDS: rejected (different API surface per platform, UDS available on Windows 10 1803+)

---

## R-002: SharedLogStore Accessibility from Diagnostics Thread

**Decision**: Add a `LogBuffer` to `vox_core` — a thread-safe ring buffer of log entries populated by the existing `LogSink` tracing layer. Diagnostics reads from this buffer directly.

**Rationale**: `SharedLogStore` is a GPUI entity in `vox_ui` (`Entity<LogStore>`) — it requires GPUI context to read, which the diagnostics `std::thread` doesn't have. The underlying `LogSink` uses `tokio::sync::mpsc::unbounded_channel` with a single receiver (already consumed by `LogStore`). We need a parallel read path.

**Implementation**:
1. Add `LogBuffer` struct to `vox_core::log_sink`: `Arc<parking_lot::RwLock<VecDeque<LogEntry>>>` capped at 10,000 entries
2. Modify `LogSink::new()` to accept an `Arc<LogBuffer>` — on each tracing event, push to both the mpsc channel (for GPUI) AND the `LogBuffer` (for diagnostics)
3. Store `Arc<LogBuffer>` in `VoxState` — diagnostics handler calls `log_buffer.recent(count, min_level)`
4. GPUI `LogStore` path remains unchanged (still reads from `LogReceiver`)

**Alternatives considered**:
- Read `SharedLogStore` via GPUI dispatch: rejected (adds async hop, defeats purpose of simple thread-safe reads)
- Create a second tracing subscriber Layer: rejected (more complex, duplicates formatting logic)
- Have diagnostics subscribe to a broadcast channel: rejected (unbounded growth if diagnostics is slow)

---

## R-003: GPUI Action Dispatch from Diagnostics Thread

**Decision**: Use a `std::sync::mpsc::channel<DiagnosticsCommand>` from diagnostics → GPUI foreground, polled by a 50ms GPUI timer.

**Rationale**: The `record` method needs to trigger `ToggleRecording` on the GPUI thread. The diagnostics thread runs on a `std::thread` and cannot call GPUI APIs directly. This is the same pattern used by `hotkey_rebind_tx: Mutex<Option<Sender<String>>>` in VoxState for hotkey → GPUI communication.

**Implementation**:
1. Add `diagnostics_cmd_tx: parking_lot::Mutex<Option<std::sync::mpsc::Sender<DiagnosticsCommand>>>` to VoxState
2. Add `diagnostics_cmd_rx` as a take-once field (like `log_receiver`)
3. In `main.rs`, take the receiver, spawn a GPUI timer (50ms interval) that polls `try_recv()` and dispatches actions
4. `DiagnosticsCommand` enum: `StartRecording`, `StopRecording`, `CaptureScreenshot { window: String, reply: oneshot::Sender<Result<Vec<u8>>> }`
5. For screenshot, the GPUI timer handler captures the window and sends the PNG bytes back via oneshot channel

**Alternatives considered**:
- `gpui::App::update_global_from_any_thread()`: not available in GPUI (no such method exists)
- Direct `window.dispatch_action()` from diagnostics thread: rejected (Window requires GPUI context, not Send)
- `crossbeam::channel`: rejected (adds dependency, std::sync::mpsc sufficient for this use case)

---

## R-004: Screenshot Capture Strategy

**Decision**: Use `PrintWindow` with `PW_RENDERFULLCONTENT` (flag=2) on Windows; `CGWindowListCreateImage` on macOS. Both dispatched via the GPUI command channel since HWND/CGWindowID extraction requires a `&Window` reference.

**Rationale**: GPUI has no screenshot API. The overlay is a GPU-composited transparent window — `BitBlt` from screen DC would capture content behind it. `PrintWindow` with `PW_RENDERFULLCONTENT` (Windows 8.1+) is the correct approach for DWM-composited windows.

**Windows implementation**:
1. Request arrives at diagnostics handler → sends `CaptureScreenshot` command via command channel
2. GPUI timer handler receives command, calls `window_handle.update(cx, |_, window, cx| ...)`
3. Inside closure: extract HWND via `raw_window_handle::HasWindowHandle`, `GetClientRect` for dimensions
4. `PrintWindow(hwnd, hdc, PRINT_WINDOW_FLAGS(2))` — note: `PW_RENDERFULLCONTENT` is `2u32`, not a named constant in `windows` 0.62
5. `GetDIBits` to read pixel data, encode as PNG via `png` crate (already a dependency)
6. Send PNG bytes back via oneshot channel

**Required new `windows` feature**: `Win32_Storage_Xps` (for `PrintWindow`)

**macOS implementation**:
1. Same command channel dispatch
2. Extract `NSWindow` via `raw_window_handle::RawWindowHandle::AppKit`
3. Get `CGWindowID` from `NSWindow` (via `windowNumber`)
4. `CGWindowListCreateImage` with `kCGWindowImageBoundsIgnoreFraming`
5. Convert `CGImage` to PNG bytes

**Alternatives considered**:
- GPUI's `scap` screen capture: rejected (captures entire display, not per-window, not enabled in Vox)
- `BitBlt` from `GetWindowDC`: rejected (doesn't capture GPU-composited content)

---

## R-005: Subscribe Event Sourcing

**Decision**: Add a persistent `broadcast::Sender<PipelineState>` and `broadcast::Sender<TranscriptEvent>` to VoxState. Subscribe handler receives from these.

**Rationale**: Currently, each `Pipeline` creates its own `broadcast::channel` per recording session. The GPUI forwarding task in `main.rs` already forwards these to `VoxState::set_pipeline_state()` + `OverlayDisplayState`. For subscribe, the diagnostics handler needs a persistent source that outlives individual recording sessions.

**Implementation**:
1. Add `state_broadcast: broadcast::Sender<PipelineState>` to VoxState (capacity 64)
2. Add `transcript_broadcast: broadcast::Sender<TranscriptEvent>` to VoxState (capacity 16)
3. The GPUI forwarding task (which already receives per-session pipeline state) also sends to `state_broadcast`
4. `TranscriptWriter::save()` also sends to `transcript_broadcast`
5. Subscribe handler: subscribes to `state_broadcast.subscribe()`, `transcript_broadcast.subscribe()`, polls RMS at 30Hz during recording
6. Subscribe connection uses 2 threads: reader thread (handles unsubscribe) + writer thread (pushes events)

**Alternatives considered**:
- Poll VoxState fields periodically: rejected (up to 50ms latency per poll, misses rapid transitions)
- Have Pipeline send to VoxState's broadcast directly: rejected (breaks encapsulation, Pipeline doesn't know about VoxState)

---

## R-006: Audio Injection Pipeline Creation

**Decision**: Clone `AsrEngine` and `PostProcessor` Arc handles from VoxState. Create a new Pipeline with a no-op text injector. Feed audio at original sample rate.

**Rationale**: Both `AsrEngine` (wraps `Arc<Mutex<WhisperContext>>`) and `PostProcessor` (wraps `Arc<LlamaModel>`) implement `Clone` — they clone the `Arc`, not the model weights. Each pipeline segment already creates fresh `WhisperState`/`LlamaContext` per call. Concurrent injection alongside live recording is safe because they use independent inference contexts against the same shared model weights.

**Implementation**:
1. `AudioInjector::load_wav(path)` → reads WAV via `hound`, returns `(Vec<f32>, u32)` (samples + sample_rate)
2. Create `HeapRb<f32>` ring buffer, push all samples to producer, drop producer (signals EOF)
3. Clone `AsrEngine` + `PostProcessor` from VoxState via `clone_asr_engine()` / `clone_llm_processor()`
4. Create Pipeline with a `NoOpInjector` (implements the text injection trait but discards output, returns polished text)
5. Call `Pipeline::start(consumer, original_sample_rate)` — pipeline's own resampler handles 48kHz→16kHz etc.
6. Run pipeline to completion, collect transcript + latency from the `state_rx` broadcast
7. Return result to diagnostics client

**Alternatives considered**:
- Mutex gate (fail if recording active): rejected (spec FR-026 requires concurrent operation)
- Pre-resample to 16kHz in injector: rejected (spec FR-024 requires exercising pipeline's own resampler)
- Reuse existing Pipeline instance: rejected (Pipeline is consumed per session, not reusable)

---

## R-007: rmcp SDK Integration

**Decision**: Use `rmcp` 0.17.0 from crates.io with features `["server", "macros", "transport-io"]`. All tools are thin wrappers forwarding to `DiagnosticsClient`.

**Rationale**: The rmcp SDK (at `D:\SRC\rust-sdk`) provides `#[tool]` / `#[tool_router]` / `#[tool_handler]` macros that generate tool schemas, routing, and JSON-RPC handling automatically. Each tool method is ~5 lines: deserialize params → call diagnostics client → wrap result.

**Key API patterns confirmed from source**:
- `#[tool(description = "...")]` on async/sync fns, parameters via `Parameters<T>` where `T: Deserialize + JsonSchema`
- `#[tool_router]` generates `Self::tool_router() -> ToolRouter<Self>`
- `#[tool_handler]` on `impl ServerHandler` injects `call_tool`/`list_tools`/`get_tool`
- `ServerHandler::get_info()` is the only required method
- `CallToolResult::success(vec![Content::text(s)])` for text results
- `Content::image(base64_data, "image/png")` for screenshot results
- `ErrorData::internal_error(msg, None)` for forwarded errors
- `stdio()` returns `(Stdin, Stdout)` — tracing MUST go to stderr
- `schemars` is re-exported from rmcp
- `ProtocolVersion::V_2024_11_05` for stable baseline compatibility

**Dependencies**:
```toml
rmcp = { version = "0.17", features = ["server", "macros", "transport-io"] }
schemars = "1"  # re-exported by rmcp, but needed for #[derive(JsonSchema)]
tokio = { version = "1", features = ["full"] }  # rmcp requires tokio runtime
```

**Alternatives considered**:
- Build JSON-RPC MCP server from scratch: rejected (1000+ lines of boilerplate, protocol compliance risk)
- Use a different MCP SDK: rejected (rmcp is the official Rust SDK, actively maintained)

---

## R-008: DiagnosticsClient — Shared UDS Client

**Decision**: Implement `DiagnosticsClient` in `vox_core::diagnostics::client` — shared by both CLI tool and MCP server.

**Rationale**: Both `vox-tool` and `vox-mcp` need to: discover socket paths, connect via UDS, send JSON requests, read JSON responses. Duplicating this is wasteful. Placing in `vox_core` means both new crates depend on `vox_core` (which already contains the UDS module and protocol types).

**Implementation**:
```rust
pub struct DiagnosticsClient {
    stream: BufStream<UnixStream>,  // BufReader + BufWriter combined
    next_id: AtomicU64,
}
impl DiagnosticsClient {
    pub fn connect(path: &Path) -> io::Result<Self>;
    pub fn connect_auto() -> Result<Self>;  // discover socket, connect
    pub fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value>;
}
```

`connect_auto()` scans `~/.vox/sockets/*.diagnostics.socket`, handles 0/1/N instances.

**Alternatives considered**:
- Duplicate protocol code in each crate: rejected (maintenance burden, divergence risk)
- Put client in a separate `vox_client` crate: rejected (over-engineering for ~50 lines)

---

## R-009: Settings Type Validation for Diagnostics Write

**Decision**: Validate setting value types against known field types. Return error code -32602 (invalid params) with expected type on mismatch.

**Rationale**: FR-004 requires type checking on settings writes. Settings fields have known types (f32 for thresholds, String for enums, bool for flags, u32 for milliseconds). The diagnostics handler must validate before calling `update_settings()`.

**Implementation**:
1. Add `Settings::field_type(key: &str) -> Option<SettingType>` method that returns the expected type for each known key
2. `SettingType` enum: `Float`, `Integer`, `Bool`, `String`
3. Diagnostics handler checks `serde_json::Value` variant against expected `SettingType` before calling `update_settings`
4. On mismatch: return `{"error":{"code":-32602,"message":"invalid type for 'vad_threshold': expected float, got string"}}`

**Alternatives considered**:
- Rely on serde deserialization errors: rejected (produces cryptic error messages, not user-friendly)
- Allow coercion (string "0.5" → float): rejected (spec says "value type MUST match")
