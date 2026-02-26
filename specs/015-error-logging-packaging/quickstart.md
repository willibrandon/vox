# Quickstart: Error Handling, Logging & Packaging

**Branch**: `015-error-logging-packaging` | **Date**: 2026-02-25

## Test Scenarios

### TS-001: ASR Retry and Discard (FR-001, SC-001)

**Setup**: Pipeline running with active dictation session.

**Steps**:
1. Simulate ASR failure by providing a malformed audio buffer to `AsrEngine::transcribe()`
2. Verify the orchestrator retries the segment once (second call to `transcribe()`)
3. On second failure, verify the segment is discarded (no text output)
4. Verify `PipelineState::Listening` is broadcast (pipeline continues)
5. Submit a valid audio segment and verify it processes successfully

**Assertions**:
- `transcribe()` called exactly twice for the failing segment
- `tracing::warn!` emitted with error category `AsrFailure` and retry count
- Pipeline state transitions: Processing → Error → Listening → Processing (next segment)
- No memory leak: RSS stable after 100 consecutive failures

---

### TS-002: LLM Retry and Discard (FR-001, SC-001)

**Setup**: Pipeline running, ASR succeeds, LLM fails.

**Steps**:
1. Provide valid audio → ASR produces transcript
2. Simulate LLM failure (e.g., context creation error)
3. Verify retry once
4. On second failure, verify segment discarded with raw transcript lost
5. Submit next segment, verify full pipeline works

**Assertions**:
- `PostProcessor::process()` called twice for failing segment
- Recovery span logged: `recovery_attempt { error_category: "LlmFailure", action: "RetrySegment" }`
- Pipeline returns to Listening within 2 seconds of failure (SC-001)

---

### TS-003: Audio Device Disconnection Recovery (FR-004, FR-005, SC-002)

**Setup**: Pipeline listening on a non-default audio device.

**Steps**:
1. Simulate device disconnection (set `error_flag` to true, drop cpal stream)
2. Verify `health_check()` returns `Err(AudioError::DeviceDisconnected)`
3. Verify `switch_to_default()` is called
4. If default available: verify new stream created, pipeline continues
5. If no device: verify "No microphone detected" shown in overlay
6. Verify retry every 2 seconds
7. Simulate device becoming available → verify auto-recovery

**Assertions**:
- Default device switch completes within 3 seconds (SC-002)
- Overlay message displayed when no device available
- Retry interval is 2 seconds (within 10% tolerance)
- Pipeline resumes listening after device recovery

---

### TS-004: Model Corruption Detection and Re-download (FR-006, FR-012)

**Setup**: All models downloaded and loaded. Pipeline idle.

**Steps**:
1. Corrupt a model file on disk (truncate or modify bytes)
2. Start pipeline → trigger model loading
3. Verify SHA-256 check fails on startup verification
4. Verify corrupt file is deleted
5. Verify re-download is triggered
6. Verify SHA-256 passes on new download
7. Verify pipeline reaches Ready state

**Assertions**:
- `verify_all_models()` detects mismatch
- Corrupt file removed from disk before re-download
- DownloadEvent::Started broadcast for re-download
- DownloadEvent::Complete broadcast with successful verification
- AppReadiness transitions: Downloading → Loading → Ready

---

### TS-005: Injection Failure with Focus Retry (FR-003)

**Setup**: Pipeline active, target window loses focus before injection.

**Steps**:
1. Process a segment through ASR + LLM → get polished text
2. Simulate injection failure (`InjectionResult::Blocked { reason: "NoFocusedWindow", text }`)
3. Verify overlay shows buffered text with "Copy" button
4. Verify focus retry task spawned (polling every 500ms)
5. Simulate focus returning to a text-accepting window
6. Verify injection re-attempted and succeeds
7. Verify overlay clears buffered text

**Assertions**:
- `PipelineState::InjectionFailed` broadcast immediately
- Focus retry task polls at ~500ms intervals
- On focus recovery: `inject_text()` called with original text
- On success: state transitions to Listening
- Retry cancels after 30 seconds if no focus detected

---

### TS-006: GPU Detection at Startup (FR-020, FR-021)

**Setup**: Fresh application launch.

**Steps (Windows)**:
1. Call `detect_gpu()` at startup
2. Verify DXGI adapter enumeration finds at least one adapter
3. Verify `GpuInfo.name` contains adapter description
4. Verify `GpuInfo.vram_bytes` is non-zero
5. Store in `VoxState`

**Steps (macOS)**:
1. Call `detect_gpu()` at startup
2. Verify system memory queried via sysctl
3. Verify `GpuInfo.name` contains chip name
4. Verify `GpuInfo.vram_bytes` is total unified memory

**Steps (Windows, no GPU)**:
1. Simulate no DXGI adapter found
2. Verify `AppReadiness::Error` set with guidance message
3. Verify overlay shows "No compatible GPU detected" with driver install instructions

**Assertions**:
- GpuInfo populated before model loading begins
- VRAM value is reasonable (> 1 GB, < 100 GB)
- Error message is actionable (includes driver install URL or instructions)

---

### TS-007: System Sleep/Wake Recovery (FR-008, SC-003)

**Setup**: Pipeline in Idle state, all components loaded.

**Steps**:
1. Simulate wake event (send `WakeEvent` through channel)
2. Verify audio health check runs
3. Verify GPU context check runs
4. Verify hotkey re-registration
5. Verify pipeline resets to Idle
6. Press hotkey → verify dictation starts normally

**Assertions**:
- Recovery completes within 5 seconds of wake event (SC-003)
- Audio device reconnected or recovery loop started
- Hotkey responsive after recovery
- Pipeline state is Idle (not Error, not stuck in Loading)

---

### TS-008: Log Rotation and Size Cap (FR-009, FR-010, FR-011)

**Setup**: Application running, log directory exists.

**Steps (daily rotation)**:
1. Run application across a date boundary (or mock system time)
2. Verify new log file created with new date
3. Verify old file still exists (within 7-day window)
4. Place a log file dated 8 days ago in log directory
5. Run cleanup → verify old file deleted

**Steps (size cap)**:
1. Write > 10 MB of log data in a single day
2. Verify file stops growing at ~10 MB
3. Verify subsequent log events are silently discarded (no error, no crash)
4. Verify LogSink (UI) still receives all events (not size-capped)

**Steps (env variable)**:
1. Set `VOX_LOG=trace` → verify trace-level output in file
2. Set `VOX_LOG=error` → verify only error-level output in file
3. Unset both → verify default level (info)

**Assertions**:
- Log files follow `vox.YYYY-MM-DD` naming
- Files older than 7 days deleted on startup
- File size never exceeds ~10.5 MB (10 MB + one final write)
- UI log panel unaffected by file size cap

---

### TS-009: Security Verification (FR-012, FR-013, FR-014, FR-015, FR-016)

**Setup**: Application with downloaded models and transcript history.

**Steps (no audio to disk)**:
1. Run a 10-minute dictation session
2. Monitor file system for any new audio files (WAV, PCM, raw)
3. Verify zero audio files created (SC-005)

**Steps (no network after download)**:
1. Complete all model downloads
2. Monitor network traffic for 10 minutes of normal use
3. Verify zero outbound connections (SC-006)

**Steps (SHA-256 verification)**:
1. Download a model → verify checksum matches expected value
2. Corrupt a downloaded file → verify `verify_checksum()` returns `false`

**Steps (secure history delete)**:
1. Create transcript entries
2. Call `clear_secure()`
3. Verify all rows deleted
4. Verify VACUUM executed (database file size reduced)
5. Attempt recovery of deleted text → verify not recoverable

**Steps (history disable)**:
1. Set `save_history = false` in settings
2. Process dictation segments
3. Verify no new transcript entries created
4. Re-enable → verify entries saved again

**Assertions**:
- Zero audio files on disk during and after session
- Zero network calls post-download
- SHA-256 matches for all three models
- Cleared history data not recoverable via SQLite forensics
- Privacy toggle immediately effective

---

### TS-010: Read-Only Model Permissions (FR-027)

**Setup**: Fresh model download.

**Steps**:
1. Download a model via `ModelDownloader`
2. After download + SHA-256 verification completes
3. Check file permissions: verify read-only
4. Attempt to write to file → verify permission denied
5. Trigger re-download (corrupt file scenario) → verify read-only removed before delete

**Assertions**:
- File permissions set to read-only after verify
- Standard file operations (read, mmap) still work
- Re-download path removes read-only before deleting

---

### TS-011: GPU OOM Guidance (FR-007)

**Setup**: Simulate GPU out-of-memory during model loading.

**Steps**:
1. Simulate OOM error from whisper-rs or llama-cpp model loading
2. Verify error categorized as `VoxError::ModelOom`
3. Verify overlay displays actionable guidance
4. Verify guidance message includes: close other GPU apps, model memory requirements

**Assertions**:
- Error message mentions approximate VRAM needed
- Message is user-friendly (no raw error codes)
- App does not crash — remains in Error state with overlay visible

---

### TS-012: Portable Executable (FR-018, FR-025)

**Setup**: Release build of `vox.exe`.

**Steps**:
1. Build release: `cargo build --release -p vox --features vox_core/cuda`
2. Measure binary size → verify < 15 MB (SC-007)
3. Copy `vox.exe` to an arbitrary directory (e.g., Desktop)
4. Run from that directory
5. Verify data stored in `%LOCALAPPDATA%/com.vox.app/` (not alongside exe)
6. Verify models download to standard model directory
7. Verify logs written to standard log directory

**Assertions**:
- Binary < 15 MB
- No files created alongside the executable
- All user data in platform-standard directories

---

### TS-013: Resilience Under Sustained Failures (SC-010)

**Setup**: Pipeline running, automated failure injection.

**Steps**:
1. Submit 1000 audio segments
2. For each segment, randomly fail one component (ASR, LLM, or injection)
3. Verify pipeline never enters a dead state
4. Measure RSS after 1000 segments
5. Compare to baseline RSS (measured before failure injection)

**Assertions**:
- Pipeline processes all 1000 segments (retry or discard each)
- No panics, no hangs, no channel deadlocks
- Final RSS < 2x baseline RSS (no memory leak)
- All 1000 segments produce either output text or discard log entry

---

### TS-014: macOS Permission Handling (FR-022, FR-023, SC-009)

**Setup**: macOS, permissions not yet granted.

**Steps**:
1. Launch app without Accessibility permission
2. Verify overlay shows "Accessibility permission required" with exact path
3. Grant permission in System Settings
4. Verify app detects grant within ~2 seconds
5. Verify dictation works without restart (SC-009)

**Steps (Input Monitoring)**:
1. Launch app without Input Monitoring permission
2. Verify hotkey registration fails gracefully
3. Verify overlay shows guidance for Input Monitoring
4. Grant permission
5. Verify hotkey re-registers within ~2 seconds

**Assertions**:
- Overlay messages include exact System Settings path
- App does not crash when permissions denied
- Auto-detection of permission grant (no restart needed)
- Polling interval is ~2 seconds

---

### TS-015: Structured Logging Content (FR-009, SC-004)

**Setup**: Complete a dictation session.

**Steps**:
1. Record and process one dictation segment
2. Read the log file
3. Verify pipeline-level span with total_duration_ms
4. Verify ASR span with model name, audio_duration_ms, transcription_duration_ms
5. Verify LLM span with model name, input tokens, output tokens, duration_ms
6. Verify injection span with text length, target app, result, duration_ms

**Assertions**:
- All four stages have timing information
- Fields are structured (key=value pairs, not embedded in message text)
- Timing values are plausible (positive, within budget)
- Log level appropriate (info for normal, warn for recovery, error for failures)
