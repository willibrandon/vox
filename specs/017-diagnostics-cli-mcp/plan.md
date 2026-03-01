# Implementation Plan: Diagnostics, CLI Tool, and MCP Server

**Branch**: `017-diagnostics-cli-mcp` | **Date**: 2026-02-28 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/017-diagnostics-cli-mcp/spec.md`

## Summary

Enable AI coding assistants and developers to observe, control, and test a running Vox instance remotely. Three components: (1) a diagnostics listener embedded in the Vox app exposing state queries, recording control, audio injection, screenshots, and event streaming over Unix Domain Sockets, (2) a CLI tool (`vox-tool`) for human developers, and (3) an MCP server (`vox-mcp`) for AI assistant integration. All share a newline-delimited JSON protocol over UDS, with the MCP server translating to/from stdio JSON-RPC via the `rmcp` 0.17.0 SDK.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: `windows` 0.62 (UDS on Windows), `std::os::unix::net` (UDS on macOS), `serde`/`serde_json` (protocol), `clap` 4 (CLI), `rmcp` 0.17.0 (MCP server), `hound` 3.5 (WAV reading, already present), `base64` 0.22 (PCM encoding), `png` 0.17 (screenshot encoding, already present)
**Storage**: UDS socket files at `~/.vox/sockets/{pid}.diagnostics.socket`; reads existing SQLite (transcripts), JSON (settings), in-memory (logs, state)
**Testing**: `cargo test -p vox_core --features cuda` (Windows), `cargo test -p vox_core --features metal` (macOS)
**Target Platform**: Windows 10 1803+ (AF_UNIX support), macOS 12+
**Project Type**: Existing three-crate workspace (`vox`, `vox_core`, `vox_ui`) + two new binary crates (`vox_tool`, `vox_mcp`)
**Performance Goals**: < 1ms diagnostics overhead on pipeline (SC-007), < 1s for read queries (SC-001), audio injection wall-clock determined by ASR+LLM speed not audio duration (SC-002)
**Constraints**: Max 4 concurrent connections (FR-013), max 9 handler threads (1 listener + up to 8 for 4 subscribe connections), no blocking of audio pipeline or UI render thread (FR-022)
**Scale/Scope**: ~2,000 new production lines + ~300 test lines across ~12 new files, 2 new crates, ~5 modified files

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Notes |
|---|-----------|--------|-------|
| I | Local-Only Processing | PASS | Diagnostics uses local UDS only. No network calls. Socket accessible only to same-user processes on localhost. |
| II | Real-Time Latency Budget | PASS | Diagnostics listener runs on dedicated std::threads, never touches audio callback or ML inference threads. SC-007 requires < 1ms overhead measured over 100 utterances. |
| III | Full Pipeline — No Fallbacks | PASS | Audio injection exercises the full pipeline (VAD+ASR+LLM). No components skipped. Only text injection is replaced with a no-op (returns result instead of keystrokes). |
| IV | Pure Rust / GPUI — No Web Tech | PASS | All components are pure Rust. CLI uses clap (Rust). MCP server uses rmcp (Rust). No JavaScript, HTML, CSS, or WebView. |
| V | Zero-Click First Launch | PASS | Diagnostics listener starts automatically during app init (FR-023). No setup required. CLI auto-discovers running instances. |
| VI | Scope Only Increases | PASS | All 32 functional requirements from the spec are implemented. No features removed or deferred. Subscribe excluded from MCP per FR-032 (spec-defined scope, not a reduction). |
| VII | Public API Documentation | PASS | All new `pub` items will have `///` doc comments. |
| VIII | No Test Skipping | PASS | All tests run unconditionally. UDS tests use tempdir for socket paths. No `#[ignore]` or conditional compilation. |
| IX | Explicit Commit Only | PASS | No commits without explicit user instruction. |
| X | No Deferral | PASS | All 7 phases (UDS, protocol, record/subscribe, injection, screenshot, CLI, MCP) implemented. No phase deferred. |
| XI | No Optional Compilation | PASS | UDS module uses `#[cfg(windows)]` / `#[cfg(unix)]` — platform-specific backends, not optional features. All other code compiles unconditionally. `Win32_Networking_WinSock` and `Win32_Storage_Xps` are required features on Windows. |
| XII | No Blame Attribution | PASS | N/A at planning stage. |
| XIII | No Placeholders | PASS | All handlers implement real logic. No `todo!()` or stub code. |

## Project Structure

### Documentation (this feature)

```text
specs/017-diagnostics-cli-mcp/
├── spec.md              # Feature specification (32 FRs, 10 SCs)
├── plan.md              # This file
├── research.md          # Phase 0 research (9 decisions)
├── data-model.md        # Phase 1 entity model
├── quickstart.md        # Phase 1 build/run/test guide
├── contracts/           # Phase 1 protocol contracts
│   └── diagnostics-protocol.md
├── checklists/
│   └── requirements.md  # Spec quality checklist
└── tasks.md             # Phase 2 output (by /speckit.tasks)
```

### Source Code (repository root)

```text
crates/vox_core/
├── src/
│   ├── vox_core.rs              # + pub mod net; pub mod diagnostics;
│   ├── net/
│   │   └── uds.rs               # Cross-platform UDS (Windows impl + macOS re-exports)
│   ├── diagnostics/
│   │   ├── protocol.rs          # Request/Response/Method types, error codes
│   │   ├── listener.rs          # DiagnosticsListener (thread, accept loop, connection handler)
│   │   ├── handlers.rs          # Method dispatch: status, settings, logs, transcripts, record, inject, screenshot, subscribe
│   │   ├── client.rs            # DiagnosticsClient (shared by CLI + MCP)
│   │   ├── audio_injector.rs    # AudioInjector (ring buffer, WAV loading, pipeline creation)
│   │   └── screenshot.rs        # Platform-specific window capture (Windows PrintWindow, macOS CGWindowListCreateImage)
│   ├── log_sink.rs              # + LogBuffer for thread-safe log access
│   └── state.rs                 # + diagnostics_cmd_tx, state_broadcast, transcript_broadcast, log_buffer
└── Cargo.toml                   # + Win32_Networking_WinSock, Win32_Storage_Xps, base64

crates/vox/
└── src/
    └── main.rs                  # + DiagnosticsListener wiring, command channel polling

crates/vox_tool/                 # NEW CRATE
├── Cargo.toml                   # clap, serde_json, anyhow, vox_core
└── src/
    └── main.rs                  # CLI entry point, clap commands, formatted output

crates/vox_mcp/                  # NEW CRATE
├── Cargo.toml                   # rmcp, serde_json, schemars, anyhow, tokio, vox_core
└── src/
    └── main.rs                  # VoxMcp struct, #[tool] definitions, stdio server

Cargo.toml                       # + workspace members: vox_tool, vox_mcp
```

**Structure Decision**: Extends the existing three-crate workspace with two new binary crates (`vox_tool`, `vox_mcp`). Core diagnostics infrastructure (UDS, protocol, listener, handlers, client) lives in `vox_core` as a new `diagnostics` module, following the same pattern as existing modules (`audio`, `vad`, `asr`, `llm`, `pipeline`). The `net` module is a separate top-level module in `vox_core` since UDS is a general-purpose networking primitive not specific to diagnostics.

## Dependency Graph

```text
Phase 1 (UDS Module) ─────────────────┐
                                       ▼
Phase 2 (Protocol + Listener) ─────────┬──→ Phase 3 (Record + Subscribe)
                                       │
                                       ├──→ Phase 4 (Audio Injection)
                                       │
                                       ├──→ Phase 5 (Screenshot)
                                       │
                                       ├──→ Phase 6 (CLI Tool)
                                       │
                                       └──→ Phase 7 (MCP Server)
```

Phases 3–7 are independent of each other after Phase 2. They can be implemented in any order or in parallel.

### Phase 1: UDS Module (~290 lines)

| File | Content | Est. Lines |
|------|---------|-----------|
| `crates/vox_core/src/net/uds.rs` | Windows `UnixStream`/`UnixListener` (Winsock2 AF_UNIX wrapper) + macOS `pub use std` re-exports | ~200 |
| `crates/vox_core/src/net/uds.rs` | Tests (bind, connect, send/recv, nonblocking, shutdown) | ~80 |
| `crates/vox_core/src/vox_core.rs` | `pub mod net;` declaration | ~1 |
| `crates/vox_core/Cargo.toml` | Add `Win32_Networking_WinSock` to windows features | ~1 |

### Phase 2: Protocol + Listener (~450 lines)

| File | Content | Est. Lines |
|------|---------|-----------|
| `crates/vox_core/src/diagnostics/protocol.rs` | `Request`, `Response`, `Method` enum, `ErrorCode` constants, serialization | ~120 |
| `crates/vox_core/src/diagnostics/handlers.rs` | `dispatch()` + handlers for status, settings get/set, logs, transcripts | ~180 |
| `crates/vox_core/src/diagnostics/listener.rs` | `DiagnosticsListener` struct, accept loop, connection handler, shutdown | ~150 |
| `crates/vox_core/src/diagnostics/client.rs` | `DiagnosticsClient` (connect, auto-discover, request/response) | ~60 |
| `crates/vox_core/src/log_sink.rs` | Add `LogBuffer` struct + integrate with `LogSink` layer | ~40 |
| `crates/vox_core/src/state.rs` | Add `log_buffer`, `diagnostics_cmd_tx/rx`, `state_broadcast`, `transcript_broadcast` fields | ~30 |
| `crates/vox/src/main.rs` | Wire `DiagnosticsListener::start()`, take command receiver, spawn GPUI poll timer | ~30 |
| `crates/vox_core/src/vox_core.rs` | `pub mod diagnostics;` declaration | ~1 |

### Phase 3: Record + Subscribe (~150 lines)

| File | Content | Est. Lines |
|------|---------|-----------|
| `crates/vox_core/src/diagnostics/handlers.rs` | Record start/stop handler (sends `DiagnosticsCommand` via command channel) | ~40 |
| `crates/vox_core/src/diagnostics/handlers.rs` | Subscribe handler (2-thread push loop: reader + writer) | ~80 |
| `crates/vox/src/main.rs` | Command channel handler dispatches `StartRecording`/`StopRecording` | ~30 |

### Phase 4: Audio Injection (~200 lines)

| File | Content | Est. Lines |
|------|---------|-----------|
| `crates/vox_core/src/diagnostics/audio_injector.rs` | `AudioInjector` (WAV loading, ring buffer, pipeline creation with no-op injector) | ~120 |
| `crates/vox_core/src/diagnostics/handlers.rs` | `inject_audio` handler (parse params, run injector, collect result) | ~60 |
| `crates/vox_core/src/diagnostics/audio_injector.rs` | Tests (inject WAV, verify transcript returned) | ~60 |

### Phase 5: Screenshot (~180 lines)

| File | Content | Est. Lines |
|------|---------|-----------|
| `crates/vox_core/src/diagnostics/screenshot.rs` | Windows: `PrintWindow` + `GetDIBits` + PNG encode | ~80 |
| `crates/vox_core/src/diagnostics/screenshot.rs` | macOS: `CGWindowListCreateImage` + PNG encode | ~80 |
| `crates/vox_core/src/diagnostics/handlers.rs` | `screenshot` handler (sends command, waits for oneshot reply) | ~20 |
| `crates/vox_core/Cargo.toml` | Add `Win32_Storage_Xps` to windows features | ~1 |

### Phase 6: CLI Tool (~350 lines)

| File | Content | Est. Lines |
|------|---------|-----------|
| `crates/vox_tool/Cargo.toml` | Dependencies: clap 4, serde_json, anyhow, vox_core, base64 | ~20 |
| `crates/vox_tool/src/main.rs` | Clap command definitions + handler functions + formatted output | ~300 |
| `Cargo.toml` | Add `"crates/vox_tool"` to workspace members | ~1 |

### Phase 7: MCP Server (~250 lines)

| File | Content | Est. Lines |
|------|---------|-----------|
| `crates/vox_mcp/Cargo.toml` | Dependencies: rmcp 0.17, schemars, serde_json, anyhow, tokio, vox_core | ~20 |
| `crates/vox_mcp/src/main.rs` | `VoxMcp` struct, 9 `#[tool]` methods, `ServerHandler` impl, stdio main | ~250 |
| `Cargo.toml` | Add `"crates/vox_mcp"` to workspace members | ~1 |

## Estimated Total Impact

| Metric | Value |
|--------|-------|
| New files | ~12 |
| New crates | 2 (`vox_tool`, `vox_mcp`) |
| Modified files | ~5 (`Cargo.toml`, `vox_core/Cargo.toml`, `vox_core.rs`, `state.rs`, `main.rs`, `log_sink.rs`) |
| New production lines | ~1,870 |
| New test lines | ~140 |
| New dependencies | `clap` 4 (vox_tool), `rmcp` 0.17 + `schemars` (vox_mcp), `base64` 0.22 (vox_core) |
| Binary size impact (vox) | ~20-30 KB (UDS + diagnostics module) |
| Runtime overhead | 1 listener thread + 1-2 threads per connection (max 9 total for 4 connections) |

## Key Design Decisions (from research.md)

| # | Decision | Rationale |
|---|----------|-----------|
| R-001 | Port uds_windows to `windows` 0.62, strip to ~200 lines | Avoids `winapi` conflict, removes ~1,800 lines of unneeded overlapped I/O |
| R-002 | Add `LogBuffer` to vox_core for thread-safe log reads | SharedLogStore is a GPUI entity, inaccessible from diagnostics std::thread |
| R-003 | Command channel (mpsc) for GPUI action dispatch, 50ms poll timer | Same pattern as existing `hotkey_rebind_tx`, no async GPUI API available |
| R-004 | PrintWindow + PW_RENDERFULLCONTENT for screenshots | Required for GPU-composited transparent windows, BitBlt captures wrong content |
| R-005 | Persistent broadcast channels in VoxState for subscribe events | Per-session Pipeline channels are ephemeral, subscribe needs to outlive sessions |
| R-006 | Clone Arc model handles for injection pipeline | AsrEngine/PostProcessor both Clone via Arc, safe concurrent injection per FR-026 |
| R-007 | rmcp 0.17.0 with #[tool] macros for MCP server | Official SDK, ~5 lines per tool, auto-generates schemas and routing |
| R-008 | DiagnosticsClient in vox_core, shared by CLI + MCP | Avoids protocol code duplication between the two consumer crates |
| R-009 | Explicit type validation on settings writes | FR-004 requires type match, serde errors are too cryptic for diagnostics UX |

## Complexity Tracking

> No constitution violations. No complexity justifications needed.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (none) | — | — |
