# Implementation Plan: Error Handling, Logging & Packaging

**Branch**: `015-error-logging-packaging` | **Date**: 2026-02-25 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `/specs/015-error-logging-packaging/spec.md`

## Summary

Implement the "never stop working" error recovery system, extend structured logging with per-stage timing and size caps, add GPU detection, system sleep/wake resilience, model integrity monitoring, and create distributable packages (portable .exe + MSI on Windows, .app in .dmg on macOS). Core architecture: a typed `VoxError` enum categorizes every failure into one of eight categories, each mapped to a `RecoveryAction` that the orchestrator executes automatically. The retry-once-then-discard pattern applies to ASR and LLM failures. Audio device recovery polls every 2 seconds. Sleep/wake listeners on platform-specific threads trigger a full component verification sequence.

## Technical Context

**Language/Version**: Rust 2024 edition (1.85+)
**Primary Dependencies**: gpui (git rev 89e9ab97, v0.2.2), tracing 0.1, tracing-subscriber 0.3, tracing-appender 0.2, cpal 0.17, windows 0.62 (expanded features), anyhow 1, tokio 1.49, sha2 (existing transitive), parking_lot 0.12, libc 0.2 (macOS GPU/power detection)
**New Dependencies**: None at runtime. cargo-wix (build tool only, not compiled into binary).
**Storage**: SQLite via rusqlite 0.38 (existing — transcripts, history deletion), JSON settings (existing — config.rs)
**Testing**: `cargo test -p vox_core --features cuda` (Windows), `cargo test -p vox_core --features metal` (macOS)
**Target Platform**: Windows x86_64 (NVIDIA CUDA) + macOS aarch64 (Apple Metal)
**Project Type**: Three-crate Rust workspace (vox binary, vox_core library, vox_ui components)
**Performance Goals**: E2E < 300ms (RTX 4090), < 750ms (M4 Pro), binary < 15 MB excluding models
**Constraints**: < 6 GB VRAM, < 500 MB RAM, < 2% CPU idle, zero network after model download
**Scale/Scope**: Single-user desktop application, 3 ML models (~3.4 GB total), 8 error categories

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| # | Principle | Status | Notes |
|---|---|---|---|
| I | Local-Only Processing | PASS | No new network calls. GPU detection uses local OS APIs (DXGI, sysctl). All error recovery is on-device. |
| II | Real-Time Latency Budget | PASS | Retry-once adds at most one duplicate operation per segment (same cost as original). Recovery logic runs on processing threads, never audio callback. Log size cap adds negligible overhead (atomic counter check per write). |
| III | Full Pipeline — No Fallbacks | PASS | Error recovery reinforces this principle. Missing/corrupt model → re-download (not degrade). No CPU fallback. Pipeline doesn't start until all components are ready. |
| IV | Pure Rust / GPUI — No Web Tech | PASS | All new code is pure Rust. Packaging uses OS-native tools: WiX (Windows MSI via .NET tool) and hdiutil (macOS built-in). No Node.js, no web toolchain. |
| V | Zero-Click First Launch | PASS | No new setup steps. GPU detection automatic. Permission polling automatic. Model integrity checks transparent. First-run overlay appears < 100ms. |
| VI | Scope Only Increases | PASS | All 27 FRs from spec addressed. MSI installer and read-only model permissions added during clarify (scope increase). |
| VII | Public API Documentation | PASS | All new public items (VoxError, RecoveryAction, GpuInfo, health_check, etc.) will have `///` doc comments. |
| VIII | No Test Skipping | PASS | All tests run unconditionally. Tests for device recovery use mock/simulated conditions, not hardware-dependent skips. |
| IX | Explicit Commit Only | PASS | No automatic commits. |
| X | No Deferral | PASS | All technical decisions resolved in research.md. No items deferred. |
| XI | No Optional Compilation | PASS | No new optional dependencies. DXGI features are platform-conditional (`cfg(target_os = "windows")`), which is the allowed platform-specific pattern. Sleep/wake handlers same. |
| XII | No Blame Attribution | N/A | Planning phase. |
| XIII | No Placeholders | PASS | All implementations will be complete, working code. |

**Gate result**: ALL PASS. No violations. Complexity Tracking table empty.

## Project Structure

### Documentation (this feature)

```text
specs/015-error-logging-packaging/
├── spec.md              # Feature specification (27 FRs, 10 SCs, 8 user stories)
├── plan.md              # This file
├── research.md          # Phase 0 output (10 research decisions)
├── data-model.md        # Phase 1 output (entities, state machines)
├── quickstart.md        # Phase 1 output (test scenarios)
├── checklists/
│   └── requirements.md  # Spec quality checklist (all passing)
└── tasks.md             # Phase 2 output (/speckit.tasks — NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
crates/vox_core/src/
├── error.rs                    # NEW — VoxError enum, error categories, RecoveryAction
├── recovery.rs                 # NEW — Recovery dispatcher, retry wrapper, device recovery loop
├── gpu.rs                      # NEW — GPU detection (DXGI on Windows, sysctl on macOS)
├── power.rs                    # NEW — Sleep/wake listener (WM_POWERBROADCAST / IOKit)
├── audio/
│   └── capture.rs              # MODIFIED — Add health_check(), switch_to_default(), reconnect()
├── models/
│   ├── downloader.rs           # MODIFIED — Set read-only permissions after download
│   └── (models.rs parent)      # MODIFIED — Add verify_all_models(), startup integrity check
├── pipeline/
│   └── orchestrator.rs         # MODIFIED — Integrate retry, error categorization, focus retry
├── injector.rs                 # MODIFIED — Add retry_on_focus() polling task
├── logging.rs                  # MODIFIED — Add SizeLimitedWriter, structured span macros
├── state.rs                    # MODIFIED — Add GpuInfo to VoxState, wake recovery method
└── lib.rs                      # MODIFIED — Add pub mod error, recovery, gpu, power

crates/vox/src/
└── main.rs                     # MODIFIED — GPU detection at startup, sleep/wake listener,
                                #            wake recovery handler, MSI metadata

crates/vox_ui/src/
└── overlay_hud.rs              # MODIFIED — Permission polling overlay messages,
                                #            GPU error display, device recovery status

packaging/
├── windows/
│   ├── wix/
│   │   └── main.wxs            # NEW — WiX installer source (MSI definition)
│   └── build-msi.ps1           # NEW — PowerShell script to build MSI
└── macos/
    ├── Info.plist               # NEW — App bundle metadata
    ├── entitlements.plist       # NEW — macOS sandbox entitlements
    ├── build-app.sh             # NEW — Create .app bundle
    └── build-dmg.sh             # NEW — Create DMG disk image
```

**Structure Decision**: Extends the existing three-crate workspace. New modules (`error.rs`, `recovery.rs`, `gpu.rs`, `power.rs`) are added to `vox_core` as top-level modules because they represent distinct cross-cutting concerns. Packaging scripts live in a new `packaging/` directory at workspace root (build-time only, not compiled into the binary).

## Architecture

### Error Categorization & Recovery

All pipeline errors flow through a typed `VoxError` enum with eight categories matching the spec (FR-001):

```
Audio → switch_to_default() → retry loop (2s) → overlay message
ModelMissing → stop pipeline → re-download → reload → resume
ModelCorrupt → delete file → re-download → reload → resume
ModelOom → display guidance (close GPU apps / smaller quantization)
AsrFailure → retry segment once → if fails, discard + continue
LlmFailure → retry segment once → if fails, discard + continue
InjectionFailure → buffer text + Copy button → retry on focus (500ms poll, 30s timeout)
GpuCrash → display error with restart instructions
```

The orchestrator's `process_segment()` wraps ASR and LLM calls with `retry_once()` — a generic async wrapper that catches errors, logs the first failure, and retries once. On second failure, it returns `Err` which the orchestrator handles by discarding the segment and broadcasting `PipelineState::Listening`.

### Audio Device Recovery

`AudioCapture` gains three methods:
- `health_check() -> Result<()>`: Tests error_flag + attempts a zero-length read to verify stream liveness
- `switch_to_default() -> Result<()>`: Drops current stream, enumerates default device, creates new stream
- `reconnect(device_name: Option<&str>) -> Result<()>`: Reconnects to a specific device or default

The recovery loop is an async function in `recovery.rs`:
1. Call `health_check()`. If OK, return.
2. On `DeviceDisconnected`: Try `switch_to_default()`. If OK, return.
3. On failure: Broadcast "No microphone detected" → sleep 2s → retry from step 1.
4. On `PermissionDenied`: Broadcast overlay guidance → return (macOS only).

### Sleep/Wake Detection

**Windows**: A dedicated thread creates a message-only window (`HWND_MESSAGE`). The window proc handles `WM_POWERBROADCAST`:
- `PBT_APMRESUMEAUTOMATIC` → send `WakeEvent` via mpsc channel
- Thread runs for the lifetime of the application

**macOS**: `IORegisterForSystemPower` registers a C callback:
- `kIOMessageSystemHasPoweredOn` → send `WakeEvent` via mpsc channel
- Uses IOKit framework (pure C API, no ObjC needed)

**Wake recovery handler** (in main.rs, triggered by channel receive):
1. Run audio device recovery loop
2. Verify GPU context: attempt a small inference. If fails, reload models (re-enter Loading state).
3. Re-register global hotkey (verify registration succeeds)
4. Reset pipeline state to Idle
5. Update overlay to show ready state

### GPU Detection

Runs once at startup, before model loading.

**Windows**: `CreateDXGIFactory1` → enumerate adapters → `DXGI_ADAPTER_DESC1`:
- Extract `Description` (GPU name as UTF-16)
- Extract `DedicatedVideoMemory` (VRAM in bytes)
- If no adapter found: set `AppReadiness::Error` with driver installation guidance
- Store `GpuInfo` in `VoxState`

**macOS**: Apple Silicon always has Metal.
- GPU name: `sysctl -n machdep.cpu.brand_string` (e.g., "Apple M4 Pro")
- Memory: `sysctl hw.memsize` via `libc::sysctl` (returns total unified memory)
- Store `GpuInfo` in `VoxState`

### Logging Extension

**Size cap**: `SizeLimitedWriter` wraps `NonBlocking` from tracing-appender:
- Tracks bytes written via `AtomicU64`
- When > 10 MB: `write()` returns `Ok(buf.len())` without forwarding (silent discard)
- Counter resets on daily rotation (new file detected by date change)
- LogSink (UI) and stderr continue receiving all events

**Structured spans**: Replace ad-hoc `tracing::info!` calls with `#[instrument]` and explicit spans:
- Pipeline-level span: `pipeline_segment` with `segment_id`, `total_duration_ms`
- Stage spans: `asr_transcribe`, `llm_process`, `text_inject` with timing + metadata
- Recovery spans: `recovery_attempt` with `error_category`, `action`, `success`

### Model Integrity

**On startup**: Before loading models, run `verify_all_models()`:
- For each model in registry: check file exists, verify SHA-256 checksum
- If any fails: delete corrupt file, set that model's state to `Missing`, trigger download

**On inference error**: When ASR or LLM returns an error:
- Check if model file exists and has expected size
- If missing or wrong size: categorize as `ModelMissing`/`ModelCorrupt`
- Recovery: stop pipeline → delete corrupt file → re-download → reload → resume

**Read-only permissions**: After successful download + verify in `downloader.rs`:
- `std::fs::set_permissions(path, Permissions::readonly())`
- Before re-download: remove read-only flag first

### Packaging

**Windows MSI** (cargo-wix + WiX 4.x):
- Installs `vox.exe` to `Program Files\Vox\`
- Start Menu shortcut, Add/Remove Programs entry
- Does NOT bundle models (Principle V: auto-download on first launch)
- Build: `cargo wix` after `cargo build --release`

**macOS DMG** (hdiutil):
- `.app` bundle: `Vox.app/Contents/{MacOS/vox, Info.plist, Resources/AppIcon.icns}`
- `LSUIElement: true` (no dock icon — overlay-only app)
- `NSMicrophoneUsageDescription` for microphone permission string
- Code signing with Developer ID + entitlements
- DMG with .app + /Applications symlink for drag-drop

**Binary size**: Release profile already configured (opt-level="s", LTO, strip, codegen-units=1). No changes needed. Target: < 15 MB.

### Injection Focus Retry

After `InjectionResult::Blocked`:
1. Store buffered text
2. Spawn `retry_on_focus()` task (tokio background task)
3. Every 500ms: check if focused window accepts text input
4. On focus detected: re-attempt `inject_text()`
5. On success: broadcast completion, clean up
6. On timeout (30s): cancel task, retain Copy button in overlay
7. Cancellation: `CancellationToken` from `tokio_util` (already a transitive dep) or `watch` channel

### macOS Permission Polling

Extends existing `prompt_accessibility_if_needed()`:
1. If `AXIsProcessTrusted()` returns `false` after initial prompt:
   - Show overlay: "Accessibility permission required — System Settings > Privacy & Security > Accessibility"
   - Spawn polling task: check every 2s
   - On `true`: dismiss message, proceed
2. Input Monitoring: detected indirectly through hotkey registration:
   - If `GlobalHotKeyManager::register()` fails with permission error → show guidance
   - Re-attempt registration every 2s
   - On success: proceed

## Dependency Changes

### vox_core/Cargo.toml

**Windows features to add**:
```toml
[target.'cfg(target_os = "windows")'.dependencies.windows]
version = "0.62"
features = [
    # Existing
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Foundation",
    "Win32_System_Threading",
    "Win32_Security",
    # New — GPU detection
    "Win32_Graphics_Dxgi",
    "Win32_Graphics_Dxgi_Common",
    # New — Sleep/wake
    "Win32_System_Power",
]
```

**macOS dependency**:
```toml
[target.'cfg(target_os = "macos")'.dependencies]
libc = "0.2"
```

Note: `libc` is already in `crates/vox/Cargo.toml` (for Metal atexit fix). Adding to `vox_core` for macOS sysctl and IOKit FFI.

### Build tools (not runtime dependencies)

- `cargo-wix`: `cargo install cargo-wix` (developer machine only)
- WiX 4.x: `dotnet tool install --global wix` (developer machine only)

## Complexity Tracking

> No Constitution violations. Table empty.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| (none) | — | — |
