# Research: Error Handling, Logging & Packaging

**Branch**: `015-error-logging-packaging` | **Date**: 2026-02-25

## R-001: Sleep/Wake Event Detection

**Decision**: Platform-specific listener threads with channel communication to GPUI main thread.

**Windows**: Dedicated thread creates a message-only window (`CreateWindowExW` with `HWND_MESSAGE`). Window proc handles `WM_POWERBROADCAST` with `PBT_APMRESUMEAUTOMATIC`. Sends `WakeEvent` via `tokio::sync::mpsc` channel to orchestrator thread. Requires adding `Win32_System_Power` feature to the `windows` crate.

**macOS**: Use `IORegisterForSystemPower` from IOKit framework (pure C API, no Objective-C message sending needed). Callback fires on `kIOMessageSystemHasPoweredOn`. Sends `WakeEvent` via channel. Consistent with Vox's existing raw FFI pattern (`#[link(name = "IOKit", kind = "framework")]`).

**Rationale**: GPUI has no built-in sleep/wake support. Zed doesn't handle it either (confirmed by codebase search — only thermal state callbacks exist in GPUI macOS platform). The thermal state registration pattern in `crates/gpui_macos/src/platform.rs` provides a reference for macOS notification setup, but we use IOKit's C API instead of NSWorkspace to avoid ObjC message sending complexity.

**Alternatives rejected**:
- Polling system uptime: Unreliable, can't distinguish sleep from idle.
- GPUI platform trait extension: Would require forking GPUI. Too invasive for one feature.

**On wake recovery sequence** (sequential):
1. Re-check audio device → reconnect or enter device recovery loop
2. Verify GPU context → if lost, reload models (re-enter Loading state)
3. Re-register global hotkey → verify registration succeeded
4. Reset pipeline to Idle state

---

## R-002: GPU Detection Approach

**Decision**: DXGI adapter enumeration on Windows, Metal device query + sysctl on macOS.

**Windows**: Add `Win32_Graphics_Dxgi` and `Win32_Graphics_Dxgi_Common` features to the `windows` crate (already a dependency at v0.62). Use `CreateDXGIFactory1` → `IDXGIFactory1::EnumAdapters1` → `DXGI_ADAPTER_DESC1` for adapter name (`Description`) and dedicated video memory (`DedicatedVideoMemory`). This is the same pattern Zed uses in `crates/gpui_windows/src/directx_devices.rs`.

**macOS**: Metal is always available on Apple Silicon. Use `sysctl hw.memsize` via `libc::sysctl` for total unified memory. For GPU name: read from `system_profiler SPDisplaysDataType` or use IOKit's `IOServiceMatching("AGXAccelerator")`. Simpler: shell out to `sysctl -n machdep.cpu.brand_string` for chip name (e.g., "Apple M4 Pro").

**Rationale**: DXGI is the standard Windows GPU enumeration API with direct bindings via the `windows` crate. More reliable than `nvidia-smi` subprocess (which requires NVIDIA CLI tools to be in PATH and doesn't work if only the driver is installed without the toolkit). The `windows` crate already provides all needed types.

**Alternatives rejected**:
- `nvidia-smi` subprocess: Requires NVIDIA toolkit (not just driver). May not be in PATH. Can fail silently.
- NVML (NVIDIA Management Library): Requires loading `nvml.dll` at runtime. Additional native library dependency.
- WMI queries: Slower, requires COM initialization, verbose.

**Current `windows` crate features** (in vox_core/Cargo.toml):
```
Win32_UI_Input_KeyboardAndMouse, Win32_UI_WindowsAndMessaging,
Win32_Foundation, Win32_System_Threading, Win32_Security
```

**Features to add**:
```
Win32_Graphics_Dxgi, Win32_Graphics_Dxgi_Common, Win32_System_Power
```

---

## R-003: Log File Size Cap (10 MB)

**Decision**: Custom `SizeLimitedWriter` wrapper around `tracing_appender::non_blocking::NonBlocking`.

The wrapper tracks accumulated bytes written via `AtomicU64`. When the counter exceeds 10 MB (10,485,760 bytes), the `Write::write()` implementation returns `Ok(buf.len())` without forwarding to the inner writer — silently discarding further file writes for the current day. The counter resets to 0 when daily rotation creates a new file (detected by tracking the current date).

Log events still flow to:
- UI LogSink layer (for real-time log panel display)
- stderr fallback (for development/debugging)

Only the file writer is capped.

**Rationale**: `tracing-appender` only supports time-based rotation (Hourly, Daily, Minutely, Never). There is no built-in size-based rotation. A size cap with silent discard is simpler than mid-day file rotation (which would conflict with daily rotation naming, e.g., `vox.2026-02-25` vs `vox.2026-02-25.1`).

**Alternatives rejected**:
- Custom `MakeWriter` with mid-day rotation: Complex file naming, conflicts with daily rotation naming scheme.
- Periodic file size check + log level reduction: Loses context from all components indiscriminately, hard to control which events are suppressed.
- External log rotation tool (logrotate): Not available on Windows, requires system-level configuration.

---

## R-004: Model File Integrity Monitoring

**Decision**: On-access integrity checks at pipeline boundaries. No file watcher dependency.

**Verification layers**:
1. **On download** (existing): Inline SHA-256 during streaming write. Already implemented in `models/downloader.rs`.
2. **On pipeline startup** (new): Full SHA-256 verification of all model files before loading. Catches corruption that occurred while app was closed.
3. **On inference error** (new): When ASR or LLM inference fails with a file-related error (IO error, parse error, invalid format), check if model file exists and has expected size. If missing or wrong size, categorize as `ModelCorrupt` or `ModelMissing` → stop pipeline, re-enter downloading state.

**No `notify` crate needed**. Model corruption during runtime is caught by inference failure. The error categorization system (R-001 in spec) routes the error to the correct recovery action (delete + re-download).

**Rationale**: Adding a file watcher dependency for a rare edge case is unnecessary complexity. The on-access check provides detection exactly when it matters — right before the model is used. If the file is corrupted between uses, the next inference attempt will fail, which triggers the recovery path.

**Alternatives rejected**:
- `notify` crate for real-time file watching: Additional dependency, background resource usage, for an extremely rare scenario (model file modified while app is running).
- Periodic polling (every 30-60s): CPU overhead for SHA-256 on large files (1.6 GB), and still delayed detection.

---

## R-005: MSI Installer Tooling (Windows)

**Decision**: WiX Toolset 4.x via `cargo-wix` Cargo subcommand.

**Workflow**:
1. Install WiX 4.x: `dotnet tool install --global wix`
2. Install cargo-wix: `cargo install cargo-wix`
3. Initialize: `cargo wix init` generates `wix/main.wxs` from Cargo.toml metadata
4. Customize `main.wxs`: Add Start Menu shortcut, set install directory, embed icon
5. Build: `cargo wix` produces `.msi` in `target/wix/`

**MSI features**:
- Installs `vox.exe` to `Program Files\Vox\`
- Creates Start Menu shortcut
- Registers Add/Remove Programs entry with uninstaller
- Does NOT add to PATH (app is launched from Start Menu or shortcut)
- Does NOT bundle models (downloaded on first launch per Principle V)

**Rationale**: WiX is the industry standard for MSI creation. `cargo-wix` provides direct Cargo integration, generating WiX source from project metadata. WiX 4.x runs on .NET, available via `dotnet tool`.

**Alternatives rejected**:
- NSIS: Produces `.exe` installer, not `.msi`. MSI is the Windows standard for enterprise deployment.
- Inno Setup: Same as NSIS — produces `.exe`, not `.msi`.
- Advanced Installer: Commercial tool, unnecessary for a simple single-binary installer.

---

## R-006: macOS DMG Creation

**Decision**: Shell script using `hdiutil create` (built-in macOS tool). No external dependencies.

**Workflow**:
1. Build release binary: `cargo build --release -p vox --features vox_core/metal`
2. Create `.app` bundle:
   ```
   Vox.app/Contents/
   ├── Info.plist          (app metadata, bundle ID, version)
   ├── MacOS/vox           (release binary)
   └── Resources/
       └── AppIcon.icns    (app icon)
   ```
3. Code sign: `codesign --sign "Developer ID" --entitlements entitlements.plist Vox.app`
4. Create DMG: `hdiutil create -volname "Vox" -srcfolder build/ -format UDBZ Vox.dmg`

**Info.plist fields**:
- `CFBundleIdentifier`: `com.vox.app`
- `CFBundleName`: `Vox`
- `CFBundleVersion`: From `Cargo.toml`
- `LSMinimumSystemVersion`: `14.0` (macOS Sonoma, Apple Silicon)
- `NSMicrophoneUsageDescription`: Required for microphone access
- `LSUIElement`: `true` (no dock icon — overlay-only app)

**Entitlements**:
- `com.apple.security.device.audio-input`: Microphone access
- `com.apple.security.automation.apple-events`: Accessibility (text injection)

**Rationale**: `hdiutil` is built into every macOS installation. No external tools needed. Avoids Node.js-based tools like `create-dmg` (Constitution Principle IV: No Web Tech).

**Alternatives rejected**:
- `create-dmg` npm package: Requires Node.js. Violates Principle IV.
- `dmgbuild` Python package: Requires Python runtime. Unnecessary dependency.

---

## R-007: Injection Retry on Focus

**Decision**: After injection failure, spawn a background polling task (500ms interval) that checks for a focused text-accepting window. On focus detected, re-attempt injection. Timeout after 30 seconds.

**Flow**:
1. `inject_text()` returns `InjectionResult::Blocked { reason, text }`
2. Orchestrator broadcasts `PipelineState::InjectionFailed { polished_text, error }`
3. Overlay shows buffered text with "Copy" button
4. Orchestrator spawns `retry_injection_on_focus(text)` background task
5. Task polls every 500ms: check if focused window accepts text input
6. On success: broadcast `Injecting` → inject → broadcast `Listening`
7. On timeout (30s): cancel task, leave Copy button active
8. On new dictation start or user copy: cancel task

**Cancellation**: Use a `tokio::sync::watch` channel or `CancellationToken` to signal the retry task to stop.

**Rationale**: No cross-platform "focus change event" API exists. Polling at 500ms is responsive (user barely notices delay) without significant CPU overhead. The 30-second timeout prevents indefinite background work.

**Alternatives rejected**:
- Accessibility API focus observers: Platform-specific, complex, fragile across different target apps.
- Retry only on next dictation: Doesn't meet spec FR-003 ("reattempt injection on the next focus event").

---

## R-008: macOS Permission Polling

**Decision**: Poll permission status every 2 seconds while denied. Proceed automatically when granted.

**Accessibility**: `AXIsProcessTrusted()` — already called at startup via `prompt_accessibility_if_needed()`. Add a polling loop if initial check returns `false`:
- Show overlay: "Accessibility permission required — System Settings > Privacy & Security > Accessibility"
- Poll `AXIsProcessTrusted()` every 2 seconds
- On `true`: dismiss overlay message, proceed

**Input Monitoring**: No direct API to check Input Monitoring status on macOS. Detection is indirect:
- Register global hotkey via `global-hotkey` crate
- If registration fails with permission error → Input Monitoring denied
- Show overlay guidance
- Attempt re-registration every 2 seconds
- On success: proceed

**Rationale**: macOS doesn't provide push notifications for TCC permission changes. Polling is the standard approach used by professional macOS applications. 2-second interval balances responsiveness with minimal CPU usage.

**Alternatives rejected**:
- File watcher on TCC.db: SIP-protected, undocumented, unreliable across macOS versions.
- Require app restart: Violates spec FR-023 ("without requiring a restart").

---

## R-009: Structured Tracing Spans

**Decision**: Extend existing tracing usage with `#[instrument]` attribute macros and explicit `info_span!` for pipeline-level timing.

**Per-stage spans**:
- `vad_process`: Fields: `audio_samples`, `speech_prob`, `duration_ms`
- `asr_transcribe`: Fields: `model`, `audio_duration_ms`, `duration_ms`, `segments`
- `llm_process`: Fields: `model`, `input_tokens`, `output_tokens`, `duration_ms`
- `text_inject`: Fields: `text_len`, `target_app`, `duration_ms`, `result`

**Pipeline span**: Wraps entire segment processing with `pipeline_segment` span containing all sub-spans. Fields: `segment_id`, `total_duration_ms`.

**Recovery spans**: `recovery_attempt` with fields: `error_category`, `action`, `success`, `duration_ms`.

**Existing logging** (already in orchestrator.rs):
- ASR latency logged at line 284 (`tracing::info!`)
- Various `warn!` calls for non-fatal errors

**Extension**: Replace ad-hoc `info!`/`warn!` calls with structured spans that include timing and structured fields.

**Rationale**: `tracing::instrument` is zero-overhead when no subscriber is active. Structured fields enable machine-parseable log analysis for remote debugging (spec User Story 4).

---

## R-010: Existing Infrastructure Reuse

**Components that already exist and will be EXTENDED (not replaced)**:

| Component | Location | What Exists | What's Added |
|---|---|---|---|
| Logging init | `logging.rs` | Daily rotation, 7-day cleanup, env filter, LogSink | Size cap wrapper, structured span helpers |
| SHA-256 verify | `models/downloader.rs` | Inline hash during download | Startup integrity check, runtime error routing |
| Release profile | `Cargo.toml` | opt-level="s", LTO, strip, codegen-units=1 | No changes needed |
| Error types | `injector.rs` | `InjectionError`, `InjectionResult` | Extend with `VoxError` categorization |
| Pipeline state | `pipeline/state.rs` | 6 states including Error, InjectionFailed | No new states — recovery uses existing transitions |
| App readiness | `state.rs` | Downloading, Loading, Ready, Error | Add GpuInfo field |
| History deletion | `transcript.rs` | `clear_secure()` with overwrite + VACUUM | No changes needed |
| macOS AX prompt | `injector/macos.rs` | `prompt_accessibility_if_needed()` | Add polling loop for denied state |
| Audio capture | `audio/capture.rs` | Ring buffer, error_flag, RMS atomic | Add health_check(), switch_to_default() |
| Settings | `config.rs` | 22 fields, atomic write | No changes needed for this feature |
