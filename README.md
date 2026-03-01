# Vox

Local-first intelligent voice dictation engine. Pure Rust, [GPUI](https://www.gpui.rs/) frontend, GPU-accelerated ML inference. Transforms speech into polished text injected into any application.

**Pipeline:** Audio Capture → Ring Buffer → Silero VAD (ONNX) → Whisper ASR → Qwen LLM post-processing → Text Injection

All processing happens on-device. Audio never leaves the machine.

## Status

Alpha — nearing daily-driver use. Full voice dictation pipeline runs end-to-end on Windows (CUDA) and macOS (Metal).

### Implemented

- **Audio capture** — cpal input with lock-free ring buffer, rubato resampler, real-time RMS amplitude via AtomicU32
- **Voice activity detection** — Silero VAD v5 via ONNX Runtime, split pre/post padding (300ms/100ms) for soft speech onset capture
- **Speech recognition** — Whisper Large V3 Turbo via whisper.cpp, 200ms silence pre-padding, energy-based hallucination guard
- **LLM post-processing** — Qwen 2.5 3B Instruct via llama.cpp (filler removal, punctuation, course correction, number/date/email formatting, voice command detection, tone adaptation, token streaming, command misclassification guard)
- **Text injection** — OS-level keystroke simulation with voice command mapping (Windows SendInput with UIPI elevation detection, macOS CGEvent with UTF-16 chunking and AX focus detection)
- **Pipeline orchestration** — Tokio select loop, state broadcasting, transcript persistence, activation modes, dictionary substitution, generation-gated session lifecycle
- **Model management** — Registry with platform-specific directories, concurrent downloading with SHA-256 verification, atomic file writes, GGUF/GGML/ONNX format detection, per-instance model directory for test isolation
- **Application state** — VoxState as GPUI Global, JSON settings with atomic write and corrupt-file recovery, SQLite transcript history with search/delete/secure-clear, AppReadiness state machine, privacy-enforced transcript writes
- **Custom dictionary** — SQLite-backed word mappings with in-memory cache, case-insensitive whole-word substitution, LLM hint integration, use count tracking, command phrase exclusion, JSON import/export
- **GPUI application shell** — System tray with PNG icons, global hotkey dispatch, structured logging with daily rotation, async pipeline initialization loading ASR and LLM onto GPU before marking Ready
- **System tray & global hotkeys** — Dynamic tray icon (5 states), 6-item context menu with recording-aware label, three activation modes (hold-to-talk, toggle, hands-free with double-press), runtime hotkey remapping, universal hotkey response in all app states
- **Overlay HUD** — Always-on-top draggable pill window with state-dependent rendering (download progress, waveform visualizer, transcript preview, injected text fade, error display, quick settings), position persistence with display bounds clamping
- **Settings window** — Full management window with sidebar navigation, configurable audio/VAD/hotkey/LLM/appearance settings, transcript history browser, dictionary editor, model status, and live log viewer
- **Error handling & self-healing** — Typed error taxonomy (8 categories mapping to 7 recovery actions), retry-once-then-discard for ASR/LLM failures, injection focus retry with 500ms polling, audio device disconnect recovery with 2s polling loop, system sleep/wake resilience (Windows WM_POWERBROADCAST, macOS IOKit), GPU detection at startup (Windows DXGI, macOS sysctl) with actionable guidance
- **Diagnostic logging** — Structured tracing spans with per-stage timing (ASR, LLM, injection), 10 MB file size cap with silent discard, daily rotation with 7-day retention, configurable via VOX_LOG environment variable
- **Security & model integrity** — SHA-256 verification of all models at startup, corrupt model re-download, read-only file permissions after download, no audio written to disk by default (opt-in debug audio tap with auto-cleanup), zero network after model download
- **macOS permissions** — Accessibility and Input Monitoring permission polling (2s interval), auto-proceed on grant without restart, overlay guidance with exact System Settings paths
- **Audio debug tap** — WAV recording at 4 pipeline stages (raw capture, post-resample, VAD segment, ASR input) for diagnosing audio quality, VAD boundaries, and resampling artifacts. Three-level setting (Off/Segments/Full), bounded channel with backpressure drop, 500 MB storage cap with 24h auto-cleanup, session/segment correlation across tap files
- **Diagnostics & tooling** — Unix domain socket server exposing pipeline state, settings, logs, transcripts, audio injection, and screenshot capture. CLI tool (`vox-tool`) with 8 subcommands for scripting and debugging. MCP server (`vox-mcp`) exposing 11 tools for AI assistant integration via stdio transport
- **Packaging** — Windows MSI installer (WiX v4), macOS .app bundle with DMG, platform-standard data directories, zero-click first launch. All three binaries (vox, vox-tool, vox-mcp) included

## Prerequisites

### Both Platforms
- Rust 1.85+ (`rustup update`)
- CMake 4.0+

### Windows
- Visual Studio 2022 Build Tools (C++ workload)
- CUDA Toolkit 12.8+ with cuDNN 9.x
- Environment variables (persistent user-level):
  ```
  CMAKE_GENERATOR=Visual Studio 17 2022
  CUDA_PATH=C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8
  ```

### macOS
- Xcode 26.x + Command Line Tools
- Metal Toolchain: `xcodebuild -downloadComponent MetalToolchain`

## Build & Run

```bash
# Run (development)
cargo run -p vox --features vox_core/cuda     # Windows (CUDA)
cargo run -p vox --features vox_core/metal    # macOS (Metal)

# Build only
cargo build -p vox --features vox_core/cuda   # Windows
cargo build -p vox --features vox_core/metal  # macOS

# Tests
cargo test -p vox_core --features cuda        # Windows
cargo test -p vox_core --features metal       # macOS

# Diagnostics tools (CLI + MCP server)
cargo build -p vox_tool -p vox_mcp

# Release
cargo build --release -p vox --features vox_core/cuda
```

## Project Structure

```
assets/icons/   Icon assets
crates/
  vox/          Binary entry point — GPUI app shell, tray, hotkeys
  vox_core/     Backend — audio, VAD, ASR, LLM, text injection, diagnostics server
  vox_ui/       GPUI UI components — overlay, panels, controls
  vox_diag/     Diagnostics protocol library — shared types, UDS client, socket discovery
  vox_tool/     CLI tool (vox-tool) — 8 subcommands for inspecting/controlling Vox
  vox_mcp/      MCP server (vox-mcp) — 11 tools for AI assistant integration
packaging/
  windows/      WiX MSI installer (main.wxs, build-msi.ps1)
  macos/        .app bundle and DMG scripts (Info.plist, entitlements, build scripts)
tests/          Integration tests
scripts/        Model download scripts
specs/          Feature specifications
```

## Architecture

Six-crate Cargo workspace:

- **vox** — Binary. GPUI application shell, window setup, system tray, global hotkeys.
- **vox_core** — Library. Audio pipeline, VAD, ASR, LLM, text injection, dictionary, config, state, model management, diagnostics socket server. Feature-gated for `cuda` and `metal`.
- **vox_ui** — Library. GPUI UI components. Overlay HUD, settings, history, dictionary editor, model manager, log viewer.
- **vox_diag** — Library. Diagnostics protocol (Request/Response/Event types), cross-platform UDS client, socket auto-discovery.
- **vox_tool** — Binary (`vox-tool`). CLI for inspecting and controlling a running Vox instance. Connects via diagnostics socket.
- **vox_mcp** — Binary (`vox-mcp`). MCP server exposing Vox diagnostics as tools for AI assistants (Claude, etc.) via stdio transport.

## Diagnostics Tools

Vox exposes a diagnostics socket that `vox-tool` and `vox-mcp` connect to for inspecting and controlling a running instance.

### CLI (`vox-tool`)

```bash
vox-tool launch                          # Start Vox in the background
vox-tool list                            # List all running Vox instances
vox-tool status                          # Pipeline state, GPU, models, audio, latency
vox-tool settings                        # Read all settings
vox-tool settings get vad_threshold      # Read one setting
vox-tool settings set vad_threshold 0.4  # Write a setting
vox-tool logs -n 20 --level warn         # Recent log entries
vox-tool transcripts -n 5               # Recent transcripts
vox-tool record start                    # Start recording
vox-tool record stop                     # Stop recording
vox-tool inject path/to/audio.wav        # Inject WAV into pipeline
vox-tool screenshot --output shot.png    # Capture overlay window
vox-tool subscribe --events transcript   # Live event stream (Ctrl+C to stop)
vox-tool quit                            # Shut down Vox gracefully
vox-tool quit --pid 12345                # Shut down a specific instance
```

Use `--pid <PID>` to target a specific instance when multiple are running.

### MCP Server (`vox-mcp`)

Add to your MCP client configuration (e.g., Claude Code `.mcp.json`):

```json
{
  "mcpServers": {
    "vox": {
      "command": "vox-mcp",
      "args": []
    }
  }
}
```

Exposes 12 tools: `vox_status`, `vox_settings_get`, `vox_settings_set`, `vox_logs`, `vox_record_start`, `vox_record_stop`, `vox_inject_audio`, `vox_screenshot`, `vox_transcripts`, `vox_launch`, `vox_quit`, `vox_list`.

## Data Directories

All user data is stored in platform-standard locations, never alongside the executable.

| Data | Windows | macOS |
|---|---|---|
| Settings | `%LOCALAPPDATA%/com.vox.app/settings.json` | `~/Library/Application Support/com.vox.app/settings.json` |
| Models | `%LOCALAPPDATA%/com.vox.app/models/` | `~/Library/Application Support/com.vox.app/models/` |
| Transcripts | `%LOCALAPPDATA%/com.vox.app/vox.db` | `~/Library/Application Support/com.vox.app/vox.db` |
| Logs | `%LOCALAPPDATA%/com.vox.app/logs/` | `~/Library/Logs/com.vox.app/` |
| Debug Audio | `%LOCALAPPDATA%/com.vox.app/debug_audio/` | `~/Library/Application Support/com.vox.app/debug_audio/` |

Models download automatically on first launch (~2.5 GB total):
- `silero_vad_v5.onnx` (2.3 MB) — Voice activity detection
- `ggml-large-v3-turbo-q5_0.bin` (547 MB) — Whisper ASR
- `qwen2.5-3b-instruct-q4_k_m.gguf` (1.93 GB) — Qwen LLM

All models are SHA-256 verified at download and again at every startup. Corrupt models are automatically re-downloaded. Model files are set read-only after verification.

## Logging

Log files are written to the platform log directory with daily rotation and 7-day retention.

**File pattern:** `vox.YYYY-MM-DD` (e.g., `vox.2026-02-25`)

**Size cap:** 10 MB per day. Once reached, subsequent log entries are silently discarded to the file. The in-app log viewer (Settings > Logs) is unaffected by the file cap.

**Log level configuration:**

```bash
# Set via environment variable (VOX_LOG takes priority over RUST_LOG)
VOX_LOG=trace cargo run -p vox --features vox_core/cuda    # All trace output
VOX_LOG=error ./vox                                         # Errors only
VOX_LOG=info,vox_core=debug ./vox                           # Debug for core, info for rest
```

Default level: `info` for all crates. Each log entry includes structured fields with per-stage timing (ASR duration, LLM duration, injection duration, total latency).

## Error Recovery

Vox self-heals from failures without user intervention. The overlay displays actionable guidance when manual steps are needed.

| Failure | Recovery | User sees |
|---|---|---|
| ASR/LLM crash | Retry once, discard segment on second failure | Brief pause, then listening resumes |
| Audio device disconnect | Switch to default device, or poll every 2s until a device appears | "No microphone detected" in overlay |
| Text injection blocked | Poll for focused window every 500ms for 30s | Buffered text with Copy button |
| Model file corrupt | Delete and re-download with SHA-256 verification | Download progress in overlay |
| GPU out of memory | Display VRAM requirements | "Close other GPU apps" guidance |
| System sleep/wake | Re-check audio device, GPU context, hotkey registration | Automatic, no restart needed |

## macOS Permissions

On macOS, Vox requires two system permissions. The overlay guides you through granting each one, and Vox detects the grant automatically (no restart needed).

**Accessibility** (required for text injection):
1. On first launch, macOS shows a system prompt for Accessibility access
2. If denied, the overlay shows: *"Accessibility permission required — System Settings > Privacy & Security > Accessibility"*
3. Grant the permission in System Settings — Vox detects it within 2 seconds and proceeds

**Input Monitoring** (required for global hotkey):
1. If hotkey registration fails, the overlay shows: *"Input Monitoring permission required — System Settings > Privacy & Security > Input Monitoring"*
2. Grant the permission — Vox re-registers the hotkey within 2 seconds

## Packaging

### Windows — MSI Installer

Requires [WiX Toolset v4](https://wixtoolset.org/):
```
dotnet tool install --global wix
```

Build the installer:
```powershell
.\packaging\windows\build-msi.ps1
```

This builds release binaries for all three executables (vox, vox-tool, vox-mcp), compiles the WiX source, and produces `packaging/windows/output/vox.msi`. The MSI installs to `Program Files\Vox` with a Start Menu shortcut and Add/Remove Programs entry. Models are not bundled — they download on first launch.

### macOS — DMG

Build the .app bundle, then wrap it in a DMG:
```bash
./packaging/macos/build-app.sh
./packaging/macos/build-dmg.sh
```

This builds all three executables (vox, vox-tool, vox-mcp) and bundles them into `Vox.app/Contents/MacOS/`. The app bundle is ad-hoc signed with entitlements for microphone access and Apple Events. Output: `packaging/macos/output/Vox.dmg`. Drag `Vox.app` to Applications to install.

## Target Hardware

| Platform | GPU | Backend |
|---|---|---|
| Windows | NVIDIA RTX 4090 | CUDA |
| macOS | Apple M4 Pro | Metal |

## License

MIT
