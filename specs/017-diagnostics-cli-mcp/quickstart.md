# Quickstart: Diagnostics, CLI Tool, and MCP Server

**Feature**: 017-diagnostics-cli-mcp
**Date**: 2026-02-28

## Prerequisites

- Rust 1.85+ (2024 edition)
- CMake 4.0+
- **Windows**: Visual Studio 2022 Build Tools, CUDA 12.8+ (or Vulkan SDK for AMD)
- **macOS**: Xcode 26.x + Command Line Tools

## Build

### Full Workspace (including new crates)

```bash
# Windows (CUDA)
cargo build -p vox --features vox_core/cuda
cargo build -p vox_tool
cargo build -p vox_mcp

# macOS (Metal)
cargo build -p vox --features vox_core/metal
cargo build -p vox_tool
cargo build -p vox_mcp
```

### Individual Crates

```bash
# Core library only (includes UDS, diagnostics modules)
cargo build -p vox_core --features cuda    # Windows
cargo build -p vox_core --features metal   # macOS

# CLI tool (depends on vox_core for UDS + protocol)
cargo build -p vox_tool

# MCP server (depends on vox_core for DiagnosticsClient)
cargo build -p vox_mcp
```

## Test

```bash
# All vox_core tests (includes UDS, diagnostics, audio injector)
cargo test -p vox_core --features cuda     # Windows
cargo test -p vox_core --features metal    # macOS

# Single test
cargo test -p vox_core test_uds_connect --features cuda -- --nocapture

# Specific module tests
cargo test -p vox_core net:: --features cuda          # UDS module
cargo test -p vox_core diagnostics:: --features cuda  # Diagnostics module
```

## Run

### 1. Start Vox (creates diagnostics socket automatically)

```bash
# Windows
cargo run -p vox --features vox_core/cuda

# macOS
cargo run -p vox --features vox_core/metal
```

The diagnostics listener starts during app initialization (FR-023). Socket created at:
- Windows: `C:\Users\<user>\.vox\sockets\<pid>.diagnostics.socket`
- macOS: `/Users/<user>/.vox/sockets/<pid>.diagnostics.socket`

### 2. Use CLI Tool

```bash
# Auto-discovers running Vox instance
vox-tool status
vox-tool settings
vox-tool settings get vad_threshold
vox-tool settings set vad_threshold 0.6
vox-tool logs --count 20 --level warn
vox-tool record start
vox-tool record stop
vox-tool inject path/to/speech.wav
vox-tool screenshot --window overlay --output capture.png
vox-tool subscribe --events state,rms,transcript
vox-tool transcripts --count 5

# With specific PID (when multiple instances running)
vox-tool --pid 12345 status
```

Exit codes: 0 = success, non-zero = error. Errors printed to stderr (FR-031).

### 3. Use MCP Server (AI Assistant Integration)

Add to your AI assistant's MCP configuration:

```json
{
  "mcpServers": {
    "vox": {
      "command": "path/to/vox-mcp",
      "args": []
    }
  }
}
```

The MCP server auto-discovers the running Vox instance and exposes 9 tools:
`vox_status`, `vox_settings_get`, `vox_settings_set`, `vox_logs`,
`vox_record_start`, `vox_record_stop`, `vox_inject_audio`, `vox_screenshot`, `vox_transcripts`

### 4. Direct UDS Connection (Manual Testing)

```bash
# Connect with socat (macOS/Linux)
socat - UNIX-CONNECT:$HOME/.vox/sockets/$(pgrep vox).diagnostics.socket

# Then type JSON requests:
{"id":1,"method":"status"}
{"id":2,"method":"logs","params":{"count":5}}
{"id":3,"method":"settings","params":{"action":"get","key":"vad_threshold"}}
```

## Socket Path Discovery

The CLI tool and MCP server scan `~/.vox/sockets/` for `*.diagnostics.socket` files:
- **0 found**: Error "No running Vox instance found"
- **1 found**: Auto-connect
- **N found**: CLI lists PIDs and requires `--pid`; MCP server connects to the first

Stale sockets from crashed instances are auto-cleaned: connect attempt fails → delete file → proceed (FR-015).

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| "No running Vox instance found" | Vox not running, or socket not created | Start Vox first; check `~/.vox/sockets/` exists |
| "app not ready: models still loading" | Models downloading/loading | Wait for Vox to reach ready state |
| "connection limit reached" | 4 clients already connected | Disconnect other clients |
| CLI hangs on `inject` | Audio processing in progress | Expected behavior — injection is blocking (FR-030) |
| MCP tools not appearing | MCP server can't find Vox | Ensure Vox is running before MCP server starts |

## Architecture Reference

```
vox (app)                      vox-tool (CLI)       vox-mcp (MCP server)
┌──────────────────┐           ┌──────────┐         ┌──────────────┐
│ DiagnosticsListener│◄── UDS ──┤ DiagnosticsClient  │◄── DiagnosticsClient
│ (std::thread)     │          │ + clap    │         │ + rmcp stdio │
│ JSON req/res      │          └──────────┘         └──────────────┘
└────────┬──────────┘                                      ▲
         │ reads                                           │
┌────────▼──────────┐                              AI Assistant
│ VoxState/Pipeline │                              (Claude Code,
│ Settings/GPU/Logs │                               Cursor, etc.)
└───────────────────┘
```
