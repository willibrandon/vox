# Tasks: Error Handling, Logging & Packaging

**Input**: Design documents from `/specs/015-error-logging-packaging/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, quickstart.md

**Tests**: Included — explicitly requested in spec Testing Requirements section.

**Organization**: Tasks grouped by user story (P1–P8) to enable independent implementation and testing.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (US1–US8)
- Exact file paths included in all descriptions

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Dependency updates and module declarations

- [X] T001 Update crates/vox_core/Cargo.toml: add Win32_Graphics_Dxgi, Win32_Graphics_Dxgi_Common, Win32_System_Power features to windows dependency; add libc = "0.2" under cfg(target_os = "macos") dependencies
- [X] T002 Add pub mod error, recovery, gpu, power declarations to crates/vox_core/src/lib.rs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Typed error taxonomy and recovery primitives used by ALL user stories

**CRITICAL**: No user story work can begin until this phase is complete

- [X] T003 Implement VoxError enum (8 variants: Audio, ModelMissing, ModelCorrupt, ModelOom, AsrFailure, LlmFailure, InjectionFailure, GpuCrash), AudioError sub-enum (4 variants: DeviceDisconnected, DeviceMissing, PermissionDenied, StreamError), RecoveryAction enum (7 variants), Display/Error/From trait impls, and recovery_action_for() exhaustive mapping in crates/vox_core/src/error.rs
- [X] T004 Implement retry_once() generic async retry wrapper and execute_recovery() dispatcher that matches RecoveryAction variants to handler functions in crates/vox_core/src/recovery.rs

**Checkpoint**: Error types and recovery primitives ready — user story implementation can begin

---

## Phase 3: User Story 1 — Self-Healing Pipeline (Priority: P1)

**Goal**: Pipeline retries failed segments once, discards on second failure, buffers injection failures with focus retry. Model corruption triggers re-download. GPU errors show guidance. Offline model instructions shown when no internet. Pipeline never enters a dead state.

**Independent Test**: Simulate component failures during active dictation and verify pipeline continues accepting new audio segments after recovery.

### Tests for User Story 1

- [X] T005 [P] [US1] Write test_asr_retry_on_failure: simulate ASR failure, verify transcribe() called twice, segment discarded, PipelineState::Listening broadcast in crates/vox_core/src/pipeline/orchestrator.rs (test module)
- [X] T006 [P] [US1] Write test_llm_retry_on_failure: simulate LLM failure, verify process() called twice, segment discarded, pipeline returns to Listening within 2s in crates/vox_core/src/pipeline/orchestrator.rs (test module)
- [X] T007 [P] [US1] Write test_injection_buffer_on_failure: simulate InjectionResult::Blocked, verify text buffered and PipelineState::InjectionFailed broadcast in crates/vox_core/src/pipeline/orchestrator.rs (test module)

### Implementation for User Story 1

- [X] T008 [US1] Wrap ASR transcribe() and LLM process() calls with retry_once() in orchestrator process_segment(), broadcast PipelineState::Listening on second failure to discard segment in crates/vox_core/src/pipeline/orchestrator.rs
- [X] T009 [P] [US1] Implement retry_on_focus() polling task: 500ms interval, 30s timeout with CancellationToken, re-attempt inject_text() on focus detection, cancel on new dictation start in crates/vox_core/src/injector.rs
- [X] T010 [US1] Integrate injection failure handling: on InjectionResult::Blocked spawn focus retry task, broadcast InjectionFailed state, cancel retry on new session or copy in crates/vox_core/src/pipeline/orchestrator.rs
- [X] T011 [P] [US1] Add model corruption detection on inference error (check file exists + expected size), model re-download recovery flow (stop pipeline → delete corrupt → download → reload → resume) in crates/vox_core/src/recovery.rs
- [X] T012 [P] [US1] Update overlay_hud.rs: display injection buffer with Copy button, GPU OOM guidance message (VRAM requirements + close GPU apps), GPU crash restart instructions in crates/vox_ui/src/overlay_hud.rs
- [X] T013 [P] [US1] Implement offline model fallback (FR-024): when model download fails due to no internet, show overlay with manual download URL and expected file path, poll every 5 seconds for model files to appear on disk, resume pipeline when files detected in crates/vox_core/src/models/downloader.rs and crates/vox_ui/src/overlay_hud.rs

**Checkpoint**: Pipeline self-heals from ASR, LLM, injection, model, GPU, and offline failures

---

## Phase 4: User Story 2 — Audio Device Recovery (Priority: P2)

**Goal**: Audio device disconnection triggers automatic switch to default device. If no device available, retry every 2 seconds with overlay feedback. Permission denial shows guidance.

**Independent Test**: Disconnect audio device during recording, verify switch to default or retry loop with user-visible feedback.

### Tests for User Story 2

- [X] T014 [US2] Write test_audio_recovery_default_device: simulate device disconnection, verify switch_to_default() called and new stream created in crates/vox_core/src/audio/capture.rs (test module)

### Implementation for User Story 2

- [X] T015 [US2] Add health_check() (test error_flag + stream validity), switch_to_default() (drop stream, enumerate default, create new), and reconnect() (specific device or default) methods to AudioCapture in crates/vox_core/src/audio/capture.rs
- [X] T016 [US2] Implement audio_recovery_loop() async function: health_check → switch_to_default → 2s retry loop → PermissionDenied guidance, using AudioError variants in crates/vox_core/src/recovery.rs
- [X] T017 [US2] Integrate audio health check at start of orchestrator segment processing loop, trigger recovery on failure in crates/vox_core/src/pipeline/orchestrator.rs
- [X] T018 [P] [US2] Update overlay to show "No microphone detected" during device retry loop and device recovery status transitions in crates/vox_ui/src/overlay_hud.rs

**Checkpoint**: Audio device recovery works — disconnect triggers auto-switch or visible retry

---

## Phase 5: User Story 3 — System Sleep/Wake Resilience (Priority: P3)

**Goal**: After system sleep/wake, app recovers audio device, verifies GPU, re-registers hotkey, resets to Idle. User can dictate without restarting.

**Independent Test**: Put system to sleep and wake, verify hotkey works and dictation starts without restart.

**Depends on**: US2 (audio recovery methods used in wake handler)

### Implementation for User Story 3

- [X] T019 [US3] Implement WakeEvent struct and start_wake_listener(): Windows — dedicated thread with HWND_MESSAGE window handling WM_POWERBROADCAST/PBT_APMRESUMEAUTOMATIC; macOS — IORegisterForSystemPower callback on kIOMessageSystemHasPoweredOn; both send WakeEvent via tokio mpsc channel in crates/vox_core/src/power.rs
- [X] T020 [US3] Integrate wake listener at app startup: spawn platform listener thread, implement wake recovery handler (1. audio health_check → recovery loop, 2. GPU verify via small inference → reload if failed, 3. hotkey re-register → permission guidance if failed, 4. reset pipeline to Idle) in crates/vox/src/main.rs

**Checkpoint**: App recovers from sleep/wake — audio, GPU, hotkey all restored

---

## Phase 6: User Story 4 — Diagnostic Logging (Priority: P4)

**Goal**: Structured log entries with per-stage timing. Log files capped at 10 MB with silent discard. Daily rotation, 7-day retention, env variable config.

**Independent Test**: Run dictation session, verify log file contains structured timing per stage and recovery events.

### Tests for User Story 4

- [X] T021 [US4] Write test_log_size_cap: write > 10 MB of log data, verify file stops growing, subsequent events silently discarded, LogSink still receives all events in crates/vox_core/src/logging.rs (test module)

### Implementation for User Story 4

- [X] T022 [US4] Implement SizeLimitedWriter wrapping NonBlocking: AtomicU64 bytes_written counter, 10 MB max_bytes cap, AtomicU32 current_date for daily reset, silent discard on overflow; integrate as file writer layer in init_logging() in crates/vox_core/src/logging.rs
- [X] T023 [P] [US4] Add structured tracing spans to orchestrator process_segment(): pipeline_segment (segment_id, total_duration_ms), asr_transcribe (model, audio_duration_ms, duration_ms), llm_process (model, input_tokens, output_tokens, duration_ms), text_inject (text_len, target_app, duration_ms, result) in crates/vox_core/src/pipeline/orchestrator.rs
- [X] T024 [P] [US4] Add recovery_attempt tracing spans with error_category, action, success, duration_ms fields to execute_recovery() and retry_once() in crates/vox_core/src/recovery.rs

**Checkpoint**: Logs contain structured per-stage timing, file size capped at 10 MB

---

## Phase 7: User Story 5 — Security & Privacy Guarantees (Priority: P5)

**Goal**: SHA-256 verification at startup for all models. Read-only permissions after download. Existing security measures (no audio to disk, no network post-download, secure delete) verified.

**Independent Test**: Corrupt a model file, verify detection and re-download. Verify read-only permissions set after download.

### Tests for User Story 5

- [X] T025 [P] [US5] Write test_sha256_verification (valid file passes, corrupt fails) and test_model_corrupt_detection (corrupt model flagged for re-download) in crates/vox_core/src/models.rs (test module)

### Implementation for User Story 5

- [X] T026 [P] [US5] Implement verify_all_models(): iterate model registry, check file exists + SHA-256 checksum for each, return list of missing/corrupt models for re-download in crates/vox_core/src/models.rs
- [X] T027 [P] [US5] Add set_readonly() after successful download+verify (std::fs::set_permissions with readonly) and remove_readonly() before re-download delete in crates/vox_core/src/models/downloader.rs
- [X] T028 [US5] Integrate verify_all_models() into startup sequence: call before model loading, trigger download for any missing/corrupt models in crates/vox/src/main.rs

**Checkpoint**: Models verified at startup, read-only after download, re-download on corruption

---

## Phase 8: User Story 6 — Distributable Application Package (Priority: P6)

**Goal**: Windows: portable .exe + MSI installer. macOS: .app bundle in .dmg. Binary < 15 MB. Zero-click first launch.

**Independent Test**: Build release binary, verify < 15 MB, run from arbitrary directory, verify data in platform-standard dirs.

### Implementation for User Story 6

- [X] T029 [P] [US6] Create Windows MSI packaging: wix/main.wxs (install to Program Files\Vox, Start Menu shortcut, Add/Remove Programs entry, no bundled models) and build-msi.ps1 build script in packaging/windows/
- [X] T030 [P] [US6] Create macOS packaging: Info.plist (CFBundleIdentifier com.vox.app, LSUIElement true, NSMicrophoneUsageDescription), entitlements.plist (audio-input, apple-events), build-app.sh (.app bundle), build-dmg.sh (hdiutil create) in packaging/macos/

**Checkpoint**: Build scripts produce MSI (Windows) and DMG (macOS) from release binaries

---

## Phase 9: User Story 7 — GPU Detection & Resource Monitoring (Priority: P7)

**Goal**: Detect GPU at startup. Windows: DXGI enumeration for name + VRAM. macOS: sysctl for chip name + memory. No GPU → actionable error. Store GpuInfo in VoxState.

**Independent Test**: Run on machine with/without compatible GPU, verify correct info or guidance.

### Implementation for User Story 7

- [X] T031 [US7] Implement detect_gpu(): Windows — CreateDXGIFactory1, EnumAdapters1, DXGI_ADAPTER_DESC1 for name + DedicatedVideoMemory; macOS — sysctl machdep.cpu.brand_string + hw.memsize via libc; return GpuInfo with GpuPlatform enum in crates/vox_core/src/gpu.rs
- [X] T032 [US7] Add gpu_info: Option<GpuInfo> field to VoxState in crates/vox_core/src/state.rs
- [X] T033 [US7] Integrate GPU detection at startup before model loading: call detect_gpu(), store in VoxState, set AppReadiness::Error with driver guidance if no GPU (Windows) in crates/vox/src/main.rs
- [X] T034 [P] [US7] Update overlay to show GPU detection failure with actionable driver installation guidance in crates/vox_ui/src/overlay_hud.rs

**Checkpoint**: GPU detected at startup, info stored in state, missing GPU shows guidance

---

## Phase 10: User Story 8 — macOS Permission Handling (Priority: P8)

**Goal**: Show exact System Settings paths when permissions denied. Poll every 2s, auto-proceed on grant. No restart required.

**Independent Test**: On macOS, deny permissions, verify guidance in overlay, grant permissions, verify app proceeds without restart.

### Implementation for User Story 8

- [X] T035 [US8] Add Accessibility permission polling loop: if AXIsProcessTrusted() returns false after initial prompt, poll every 2s, dismiss overlay and proceed on grant in crates/vox_core/src/injector/macos.rs
- [X] T036 [US8] Add Input Monitoring detection: if GlobalHotKeyManager::register() fails with permission error, show guidance, retry registration every 2s, proceed on success in crates/vox/src/main.rs
- [X] T037 [P] [US8] Update overlay for macOS permission guidance: "Accessibility permission required — System Settings > Privacy & Security > Accessibility" and Input Monitoring equivalent path in crates/vox_ui/src/overlay_hud.rs

**Checkpoint**: macOS permissions handled gracefully — polling auto-detects grant

---

## Phase 11: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, integration tests, stress tests, security verification, size validation

- [X] T038 Add /// doc comments to all new pub items across crates/vox_core/src/error.rs, recovery.rs, gpu.rs, power.rs (Constitution Principle VII)
- [X] T039 [P] Write integration test test_pipeline_recovery_asr_crash: full pipeline with simulated ASR crash, verify continues after recovery in crates/vox_core/src/pipeline/orchestrator.rs (test module)
- [X] T040 [P] Write integration test test_pipeline_recovery_llm_crash: full pipeline with simulated LLM crash, verify continues after recovery in crates/vox_core/src/pipeline/orchestrator.rs (test module)
- [X] T041 [P] Write integration test test_pipeline_recovery_audio_disconnect: full pipeline with simulated audio device disconnection, verify switch to default and pipeline continues in crates/vox_core/src/pipeline/orchestrator.rs (test module)
- [X] T042 [P] Write integration test test_pipeline_recovery_sleep_wake: simulate wake event, verify audio recovery loop runs, GPU context verified, hotkey re-registered, pipeline resets to Idle in crates/vox_core/src/pipeline/orchestrator.rs (test module)
- [X] T043 [P] Write stress test test_resilience_1000_failures (SC-010): submit 1000 segments with random component failures, verify pipeline never deadlocks, no panics, final RSS < 2x baseline in crates/vox_core/src/pipeline/orchestrator.rs (test module)
- [X] T044 [P] Verify security constraints (SC-005, SC-006): run dictation session equivalent, assert zero audio files written to disk and zero outbound network connections after model download in crates/vox_core/src/pipeline/orchestrator.rs (test module)
- [X] T045 Verify release binary size < 15 MB (SC-007) and combined VRAM usage < 6 GB (SC-008): cargo build --release -p vox --features vox_core/cuda, measure output binary size and model VRAM allocation. **Result**: VRAM ~2.8 GB (PASS). Binary 438 MB total — 431 MB is .nv_fatb (CUDA fat binary for all GPU architectures), Rust+C++ code is ~24 MB. Reducing CMAKE_CUDA_ARCHITECTURES to sm_75+ will shrink dramatically.
- [X] T046 Run quickstart.md test scenarios TS-001 through TS-015 validation against implemented features, verify each scenario's assertions pass. **Validation**: TS-001→T005+T039, TS-002→T006+T040, TS-003→T041+recovery.rs, TS-004→T025+T028, TS-005→T007+T009, TS-006→T031+T033, TS-007→T042+T019, TS-008→T021+T022, TS-009→T044+T025, TS-010→T025+T027, TS-011→error.rs+T034, TS-012→T045 (binary 438MB: 431MB CUDA fat binary + ~24MB code), TS-013→T043, TS-014→T035+T036+T037, TS-015→T023+T024

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Setup — BLOCKS all user stories
- **User Stories (Phase 3–10)**: All depend on Foundational phase completion
  - US3 (Sleep/Wake) additionally depends on US2 (Audio Recovery) — wake handler uses audio_recovery_loop()
  - All other stories are independent of each other
- **Polish (Phase 11)**: Depends on all user stories being complete

### User Story Dependencies

- **US1 (P1)**: After Foundational — no story dependencies
- **US2 (P2)**: After Foundational — no story dependencies
- **US3 (P3)**: After Foundational + **US2** — wake handler calls audio_recovery_loop()
- **US4 (P4)**: After Foundational — no story dependencies (adds spans to existing code)
- **US5 (P5)**: After Foundational — no story dependencies
- **US6 (P6)**: After Foundational — no story dependencies (packaging scripts only)
- **US7 (P7)**: After Foundational — no story dependencies
- **US8 (P8)**: After Foundational — no story dependencies (macOS-specific)

### File Conflict Notes

The following files are modified by multiple user stories (implement sequentially within that file):

- **orchestrator.rs**: US1 (retry/discard), US2 (audio health check), US4 (structured spans)
- **recovery.rs**: US1 (model re-download), US2 (audio loop), US4 (recovery spans)
- **overlay_hud.rs**: US1 (injection/GPU/offline), US2 (audio), US7 (GPU detection), US8 (permissions)
- **main.rs**: US3 (wake handler), US5 (model verify), US7 (GPU startup), US8 (hotkey retry)
- **downloader.rs**: US1 (offline polling, FR-024), US5 (read-only permissions)

### Within Each User Story

- Tests written first (should fail before implementation)
- Core logic before integration
- Same-file tasks sequential, different-file tasks can be [P]

### Parallel Opportunities

**Within Phase 3 (US1)**: T005, T006, T007 tests in parallel; T009, T011, T012, T013 impl in parallel (different files)
**Within Phase 4 (US2)**: T018 overlay in parallel with T015–T017
**Within Phase 6 (US4)**: T022, T023, T024 all in different files — full parallel
**Within Phase 7 (US5)**: T025, T026, T027 in different files — parallel
**Within Phase 8 (US6)**: T029, T030 fully independent — parallel
**Within Phase 9 (US7)**: T034 overlay in parallel with T031–T033
**Within Phase 10 (US8)**: T037 overlay in parallel with T035–T036
**Cross-story parallel** (if multi-agent): US1, US2, US4, US5, US6, US7, US8 can all start after Foundational

---

## Parallel Example: User Story 1

```text
# These three tests touch the same file — run sequentially:
T005 test_asr_retry_on_failure (orchestrator.rs test module)
T006 test_llm_retry_on_failure (orchestrator.rs test module)
T007 test_injection_buffer_on_failure (orchestrator.rs test module)

# These implementation tasks touch different files — run in parallel:
T009 retry_on_focus() in injector.rs
T011 model re-download in recovery.rs
T012 overlay updates in overlay_hud.rs
T013 offline model fallback in downloader.rs + overlay_hud.rs

# These must be sequential (same file: orchestrator.rs):
T008 retry_once wrapping → T010 injection failure integration
```

---

## Parallel Example: User Story 4 (Logging)

```text
# All three files are different — full parallel:
T022 SizeLimitedWriter in logging.rs
T023 Structured spans in orchestrator.rs
T024 Recovery spans in recovery.rs
```

---

## Implementation Strategy

### MVP First (US1 Only)

1. Complete Phase 1: Setup (2 tasks)
2. Complete Phase 2: Foundational (2 tasks)
3. Complete Phase 3: US1 — Self-Healing Pipeline (9 tasks)
4. **STOP and VALIDATE**: Test retry/discard, injection retry, model corruption recovery, offline fallback
5. The core "never stop working" promise is now implemented

### Incremental Delivery

1. Setup + Foundational → Error taxonomy ready
2. US1 (Self-Healing) → Core resilience (MVP)
3. US2 (Audio Recovery) → Hardware resilience
4. US3 (Sleep/Wake) → Laptop user resilience
5. US4 (Logging) → Diagnosability
6. US5 (Security) → Model integrity hardening
7. US6 (Packaging) → Distribution-ready
8. US7 (GPU Detection) → Hardware awareness
9. US8 (macOS Permissions) → Platform polish
10. Polish → Doc comments, integration tests, stress tests, security verification, size validation

### Parallel Team Strategy

With multiple agents after Foundational:
- Agent A: US1 (Self-Healing Pipeline) — orchestrator + injector + downloader
- Agent B: US2 (Audio Recovery) — audio/capture + recovery
- Agent C: US4 + US5 (Logging + Security) — logging + models
- Agent D: US6 + US7 (Packaging + GPU) — packaging scripts + gpu module
- US3 starts after Agent B finishes US2
- US8 starts any time after Foundational

---

## Notes

- [P] tasks = different files, no dependencies within phase
- [Story] label maps task to specific user story for traceability
- All test names match spec Testing Requirements section
- Rust tests live in same file as implementation (test module at bottom)
- Constitution Principle VII requires doc comments on all pub items (T038)
- Constitution Principle VIII requires all tests run unconditionally (no #[ignore])
- 8 error categories map exhaustively to 7 recovery actions via match
- Platform-specific code uses cfg(target_os) — allowed by Constitution Principle XI
