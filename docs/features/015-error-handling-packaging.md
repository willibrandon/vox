# Feature 015: Error Handling, Logging & Packaging

**Status:** Not Started
**Dependencies:** 007-pipeline-orchestration, 011-gpui-application-shell
**Design Reference:** Sections 12 (Error Handling), 14 (Security & Privacy), 16 (Packaging & Distribution)
**Estimated Scope:** Error recovery, logging system, security policy, build artifacts, installers

---

## Overview

Implement the error handling strategy, structured logging, security measures, and packaging for distribution. The core principle is "never stop working" — every error has an automatic recovery path. The app should never show a dead state. If something breaks, the pipeline restarts itself. If a component crashes, it is restarted and retried — not skipped.

---

## Requirements

### FR-001: Error Categories and Recovery

| Category | Examples | Recovery Strategy |
|---|---|---|
| **Audio** | Device disconnected, permission denied | Switch to default device. If no device: pause pipeline, show message, retry every 2s |
| **Model missing** | File not found on disk | Auto-download. If no internet: show manual instructions, poll every 5s |
| **Model corrupt** | Bad GGML/GGUF/ONNX header | Delete corrupt file and re-download automatically |
| **Model OOM** | GPU out of memory | Show error with guidance (close other GPU apps, use smaller quantization) |
| **ASR failure** | Whisper crash on a segment | Log error, retry segment once. If retry fails, discard and continue listening |
| **LLM failure** | Timeout, garbled output | Log error, retry segment once. If retry fails, discard and continue listening |
| **Injection** | Focus lost, permission denied | Buffer text, show in overlay with "Copy" button. Retry on next focus event |
| **GPU crash** | CUDA error, driver crash | Show error with restart instructions |

### FR-002: Pipeline Recovery State Machine

```
Pipeline Running (VAD + GPU ASR + GPU LLM)
        │ component crashes on a segment
        ▼
Retry Segment (restart component, reprocess same audio)
        │ retry succeeds → back to running
        │ retry fails → discard segment, log error
        ▼
Continue Listening (pipeline stays active for next segment)
```

If a model file becomes corrupted or deleted while the app is running, the pipeline stops and re-enters the downloading state. Once the model is back on disk, it reloads and resumes.

### FR-003: Audio Device Recovery

```rust
pub async fn audio_recovery_loop(capture: &mut AudioCapture) {
    loop {
        match capture.health_check() {
            Ok(()) => break,
            Err(AudioError::DeviceDisconnected) => {
                tracing::warn!("Audio device disconnected, switching to default");
                match capture.switch_to_default() {
                    Ok(()) => break,
                    Err(_) => {
                        tracing::warn!("No audio device available, retrying in 2s");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                }
            }
            Err(AudioError::PermissionDenied) => {
                tracing::error!("Microphone permission denied");
                // Show permission guidance in overlay
                break;
            }
            Err(e) => {
                tracing::error!("Audio error: {}", e);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
```

### FR-004: Structured Logging (tracing)

```rust
// Log levels by component
// ERROR: Always logged, indicates a failure that impacts user
// WARN:  Default level, something unexpected but recovered
// INFO:  Verbose, general operational messages
// DEBUG: Development, detailed state transitions
// TRACE: Pipeline timing, per-segment latency breakdown

// Examples:
tracing::info!(
    model = "whisper-large-v3-turbo",
    duration_ms = 35,
    audio_duration_ms = 2300,
    "ASR transcription complete"
);

tracing::warn!(
    device = "MacBook Pro Microphone",
    error = %e,
    "Audio device error, attempting recovery"
);

tracing::trace!(
    phase = "asr",
    latency_ms = 35,
    segment_duration_ms = 2300,
    "Pipeline stage complete"
);
```

### FR-005: Log Rotation and Retention

```
# Windows
%LOCALAPPDATA%/com.vox.app/logs/vox.YYYY-MM-DD.log

# macOS
~/Library/Logs/com.vox.app/vox.YYYY-MM-DD.log
```

- Rotated daily
- 7-day retention (auto-delete older logs)
- Max 10 MB per log file

### FR-006: GPU Detection

**Windows:**
```rust
pub fn detect_gpu_windows() -> Result<GpuInfo> {
    // Query nvidia-smi --query-gpu=name,memory.total --format=csv,noheader
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader"])
        .output()?;
    // Parse output: "NVIDIA GeForce RTX 4090, 24564 MiB"
}
```

GPU is required on Windows. If not detected, show error with driver installation instructions.

**macOS:**
Metal is always available on Apple Silicon. Query unified memory via `sysctl hw.memsize`.

### FR-007: Security Measures

**Model integrity:**
- SHA-256 checksum verification on every download
- Models are read-only after download (file permissions)
- Only download from pinned Hugging Face / GitHub URLs

**Audio privacy:**
- Audio processed in memory, immediately discarded after transcription
- No audio written to disk at any point
- No network calls after model download
- No telemetry, no analytics, no update checks

**Transcript privacy:**
- History stored locally in SQLite
- "Clear history" performs overwrite + VACUUM (secure delete)
- History can be disabled in settings

### FR-008: macOS Permission Handling

macOS requires two permissions granted manually:

1. **Accessibility** (System Settings → Privacy & Security → Accessibility) — for CGEvent text injection
2. **Input Monitoring** (System Settings → Privacy & Security → Input Monitoring) — for global hotkeys

When permissions are missing:
- Show a clear message in the overlay with the exact path to the setting
- Do not crash or hang
- Poll for permission status and proceed automatically when granted

### FR-009: System Sleep/Wake Recovery

When the system sleeps and wakes:
- Audio device may be lost — trigger audio recovery loop (FR-003)
- GPU context may be invalidated — detect and reload models
- Pipeline state resets to Idle on wake
- Hotkey registration must be verified (re-register if lost)

```rust
// Listen for OS sleep/wake events
// Windows: WM_POWERBROADCAST with PBT_APMRESUMEAUTOMATIC
// macOS: NSWorkspace.willSleepNotification / didWakeNotification

pub async fn on_system_wake(pipeline: &mut Pipeline) {
    tracing::info!("System wake detected, verifying pipeline components");
    // 1. Re-check audio device
    audio_recovery_loop(&mut pipeline.audio_capture).await;
    // 2. Verify GPU is still accessible
    if let Err(e) = pipeline.asr.health_check() {
        tracing::warn!(error = %e, "GPU context lost after sleep, reloading models");
        // Re-enter loading state
    }
    // 3. Re-register hotkey
    // 4. Resume pipeline in Idle state
}
```

### FR-010: Build Artifacts

| Platform | Format | Size Target |
|---|---|---|
| Windows | Single `.exe` (portable) + `.msi` installer | < 15 MB |
| macOS | `.app` bundle in `.dmg` | < 15 MB |

Release profile in workspace `Cargo.toml`:

```toml
[profile.release]
opt-level = "s"      # Optimize for size
lto = true           # Link-time optimization
strip = "symbols"    # Strip debug symbols
codegen-units = 1    # Single codegen unit for better optimization
```

### FR-011: First-Run Experience

1. User installs Vox (< 15 MB)
2. First launch: overlay HUD appears **instantly** (< 100ms)
3. All three models auto-download concurrently (~3.4 GB total)
4. Models load onto GPU
5. Pipeline activates. Overlay shows "IDLE — Press [Fn] to start dictating."
6. macOS: Accessibility permission prompt fires on first dictation (OS-triggered)

**Zero-click setup.** Install → launch → wait for download → dictate.

### FR-012: GPU Memory Budget

Combined VRAM usage must stay under 6 GB:

| Component | VRAM |
|---|---|
| Silero VAD (CPU) | 0 GB |
| Whisper Large V3 Turbo Q5_0 | ~1.8 GB |
| Qwen 2.5 3B Instruct Q4_K_M | ~2.2 GB |
| ONNX Runtime | ~0.1 GB |
| Overhead | ~0.5 GB |
| **Total** | **~4.6 GB** |

Leaves 19+ GB free on both target machines (24 GB each).

---

## Acceptance Criteria

### Error Handling
- [ ] Audio device disconnection triggers auto-recovery
- [ ] ASR failure on a segment retries once, then discards
- [ ] LLM failure on a segment retries once, then discards
- [ ] Injection failure buffers text with copy fallback
- [ ] Model corruption triggers re-download
- [ ] GPU OOM shows actionable guidance
- [ ] System sleep/wake recovers audio device and GPU context

### Logging
- [ ] Logs write to platform-specific directory
- [ ] Log rotation works (daily, 7-day retention)
- [ ] All pipeline stages emit tracing spans with timing
- [ ] Log level configurable via environment variable

### Security
- [ ] SHA-256 verification on all downloaded models
- [ ] No audio written to disk
- [ ] No network calls after models are downloaded
- [ ] Secure history deletion works (VACUUM)

### Packaging
- [ ] Release binary < 15 MB (excluding models)
- [ ] Windows .exe runs without installer
- [ ] macOS .app launches from .dmg
- [ ] Combined VRAM < 6 GB

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_audio_recovery_default_device` | Disconnection switches to default |
| `test_asr_retry_on_failure` | Failed segment retried once |
| `test_llm_retry_on_failure` | Failed segment retried once |
| `test_injection_buffer_on_failure` | Failed injection buffers text |
| `test_model_corrupt_detection` | Corrupt model detected and flagged |
| `test_sha256_verification` | Valid file passes, corrupt fails |
| `test_log_rotation` | Old logs cleaned up |

### Integration Tests

| Test | Description |
|---|---|
| `test_pipeline_recovery_asr_crash` | Pipeline continues after ASR crash |
| `test_pipeline_recovery_llm_crash` | Pipeline continues after LLM crash |
| `test_pipeline_recovery_audio_disconnect` | Pipeline recovers after device disconnect |
| `test_pipeline_recovery_sleep_wake` | Pipeline recovers after system sleep/wake |

### Performance Tests

| Test | Target |
|---|---|
| `bench_e2e_latency_4090` | < 300 ms |
| `bench_e2e_latency_m4pro` | < 750 ms |
| `bench_vram_usage` | < 6 GB |
| `bench_ram_usage` | < 500 MB |
| `bench_cpu_idle` | < 2% |
| `bench_memory_leak_1000_segments` | RSS within 2x baseline |

---

## Performance Targets (Final Verification)

| Metric | RTX 4090 | M4 Pro |
|---|---|---|
| End-to-end latency | < 300 ms | < 750 ms |
| VRAM / Unified Memory | < 6 GB | < 6 GB |
| System RAM | < 500 MB | < 500 MB |
| CPU (idle) | < 2% | < 2% |
| CPU (active) | < 15% | < 20% |
| Binary size | < 15 MB | < 15 MB |
| Incremental build | < 10 s | < 10 s |
