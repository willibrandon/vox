# Data Model: Error Handling, Logging & Packaging

**Branch**: `015-error-logging-packaging` | **Date**: 2026-02-25

## Entities

### VoxError

Typed error enum representing every categorized failure in the pipeline. Each variant carries enough context for the recovery dispatcher to execute the correct action without additional lookups.

**Location**: `crates/vox_core/src/error.rs`

| Variant | Fields | Recovery | FR |
|---|---|---|---|
| `Audio(AudioError)` | `AudioError` enum (below) | Device switch / retry loop | FR-004, FR-005 |
| `ModelMissing` | `model_name: String, expected_path: PathBuf` | Stop pipeline, re-download | FR-006, FR-024 |
| `ModelCorrupt` | `model_name: String, path: PathBuf, reason: String` | Delete + re-download | FR-006 |
| `ModelOom` | `model_name: String, vram_required: u64, vram_available: Option<u64>` | Display guidance | FR-007 |
| `AsrFailure` | `source: anyhow::Error, segment_id: u64` | Retry once, then discard | FR-001 |
| `LlmFailure` | `source: anyhow::Error, segment_id: u64` | Retry once, then discard | FR-001 |
| `InjectionFailure` | `result: InjectionResult, text: String` | Buffer + Copy button + focus retry | FR-003 |
| `GpuCrash` | `source: anyhow::Error, platform: String` | Display restart instructions | FR-007 |

**Traits**: `std::error::Error`, `Display`, `From<AudioError>`, `From<InjectionResult>`

### AudioError

Sub-enum for audio-specific failures. Extends the existing `error_flag` mechanism in `AudioCapture`.

**Location**: `crates/vox_core/src/error.rs`

| Variant | Fields | Description |
|---|---|---|
| `DeviceDisconnected` | `device_name: String` | Device was unplugged or became unavailable |
| `DeviceMissing` | — | No audio input device found on system |
| `PermissionDenied` | `platform_message: String` | OS denied microphone access |
| `StreamError` | `source: anyhow::Error` | cpal stream creation or runtime error |

### RecoveryAction

Enum describing what the recovery dispatcher should do for each error category.

**Location**: `crates/vox_core/src/error.rs`

| Variant | Description | Associated Errors |
|---|---|---|
| `RetrySegment` | Retry the same audio segment through the failed component once | AsrFailure, LlmFailure |
| `DiscardSegment` | Drop the segment and continue listening | AsrFailure (2nd), LlmFailure (2nd) |
| `SwitchAudioDevice` | Switch to default audio device | Audio(DeviceDisconnected) |
| `AudioRetryLoop` | Enter 2-second polling loop for audio device | Audio(DeviceMissing) |
| `RedownloadModel` | Stop pipeline, delete file if exists, re-download | ModelMissing, ModelCorrupt |
| `DisplayGuidance { message: String }` | Show actionable message in overlay | ModelOom, GpuCrash, Audio(PermissionDenied) |
| `BufferAndRetryFocus { text: String }` | Show text in overlay with Copy, poll for focus | InjectionFailure |

### GpuInfo

Detected GPU hardware information, queried once at startup.

**Location**: `crates/vox_core/src/gpu.rs`

| Field | Type | Description |
|---|---|---|
| `name` | `String` | GPU adapter name (e.g., "NVIDIA GeForce RTX 4090", "Apple M4 Pro") |
| `vram_bytes` | `u64` | Dedicated video memory (Windows) or total unified memory (macOS) |
| `driver_version` | `Option<String>` | Driver version string (Windows only, from DXGI) |
| `platform` | `GpuPlatform` | `Cuda` or `Metal` |

### GpuPlatform

**Location**: `crates/vox_core/src/gpu.rs`

| Variant | Description |
|---|---|
| `Cuda` | NVIDIA GPU with CUDA support (Windows) |
| `Metal` | Apple Silicon with Metal (macOS) |

### WakeEvent

Marker event sent when the system resumes from sleep.

**Location**: `crates/vox_core/src/power.rs`

| Field | Type | Description |
|---|---|---|
| `timestamp` | `std::time::Instant` | When the wake was detected |

Sent via `tokio::sync::mpsc::UnboundedSender<WakeEvent>` from the platform listener thread to the main application thread.

### SizeLimitedWriter

Writer wrapper that enforces the 10 MB per-file size cap.

**Location**: `crates/vox_core/src/logging.rs`

| Field | Type | Description |
|---|---|---|
| `inner` | `NonBlocking` | The underlying tracing-appender non-blocking writer |
| `bytes_written` | `Arc<AtomicU64>` | Accumulated bytes in current day's file |
| `max_bytes` | `u64` | Size limit (10,485,760 = 10 MB) |
| `current_date` | `AtomicU32` | Day-of-year for detecting daily rotation |

**Behavior**: Implements `std::io::Write`. Delegates to `inner` when under limit. When `bytes_written >= max_bytes`, returns `Ok(buf.len())` without writing (silent discard). Resets counter when `current_date` changes (daily rotation detected).

## State Machines

### Pipeline Recovery State Machine

Extends the existing `PipelineState` transitions with error recovery paths.

```
                          ┌─────────────────────────┐
                          │    Pipeline Running      │
                          │  (Listening → Processing │
                          │   → Injecting → Listening)│
                          └─────────┬───────────────┘
                                    │ component error
                                    ▼
                          ┌─────────────────────────┐
                          │   Categorize Error       │
                          │   (VoxError matching)    │
                          └─────────┬───────────────┘
                                    │
                    ┌───────────────┼───────────────┐
                    │               │               │
                    ▼               ▼               ▼
            ┌──────────┐   ┌──────────────┐  ┌────────────────┐
            │  Retry   │   │  Device/Model│  │   Display      │
            │  Segment │   │  Recovery    │  │   Guidance     │
            │  (once)  │   │              │  │   (terminal)   │
            └────┬─────┘   └──────┬───────┘  └────────────────┘
                 │                │
          ┌──────┴──────┐        │
          │             │        │
          ▼             ▼        ▼
    ┌──────────┐  ┌──────────┐ ┌──────────────────┐
    │  Success │  │  Discard │ │  Pipeline Stops   │
    │  → back  │  │  Segment │ │  → Downloading /  │
    │  to Run  │  │  → Listen│ │    Loading state  │
    └──────────┘  └──────────┘ └──────────────────┘
```

### Audio Device Recovery State Machine

```
    ┌─────────────┐
    │  Healthy    │ ◄──────────────────────────┐
    │  (streaming)│                             │
    └──────┬──────┘                             │
           │ error_flag set                     │
           ▼                                    │
    ┌─────────────────┐                         │
    │  Switch to      │── success ──────────────┘
    │  Default Device │
    └──────┬──────────┘
           │ no default available
           ▼
    ┌─────────────────┐                         │
    │  Retry Loop     │── device found ─────────┘
    │  (2s interval)  │
    │  overlay msg    │
    └──────┬──────────┘
           │ permission denied (macOS)
           ▼
    ┌─────────────────┐
    │  Show Guidance  │
    │  (terminal)     │
    └─────────────────┘
```

### Wake Recovery Sequence

```
    System Wake Detected
           │
           ▼
    ┌─────────────────┐
    │  1. Audio Check  │── healthy → skip to 2
    │  health_check()  │── failed → run device recovery loop
    └──────┬──────────┘
           ▼
    ┌─────────────────┐
    │  2. GPU Verify   │── accessible → skip to 3
    │  small inference │── failed → reload models (Loading state)
    └──────┬──────────┘
           ▼
    ┌─────────────────┐
    │  3. Hotkey Check │── registered → skip to 4
    │  re-register     │── failed → show permission guidance (macOS)
    └──────┬──────────┘
           ▼
    ┌─────────────────┐
    │  4. Reset to     │
    │     Idle         │
    └─────────────────┘
```

### Model Integrity Flow

```
    ┌─────────────────┐
    │  App Startup     │
    └──────┬──────────┘
           ▼
    ┌─────────────────┐
    │  verify_all()    │── all OK → proceed to Loading
    │  SHA-256 check   │── missing → download
    └──────┬──────────┘── corrupt → delete + download
           │
           ▼ (during runtime)
    ┌─────────────────┐
    │  Inference Call   │── success → normal flow
    │  (ASR or LLM)    │── file error → check model file
    └──────┬───────────┘
           │ file missing or wrong size
           ▼
    ┌─────────────────┐
    │  Stop Pipeline   │
    │  → Downloading   │
    │  → Re-download   │
    │  → Reload        │
    │  → Resume        │
    └─────────────────┘
```

## Relationships

```
VoxState
├── gpu_info: Option<GpuInfo>           # Set once at startup
├── readiness: AppReadiness             # Downloading → Loading → Ready → Error
├── pipeline_state: PipelineState       # Existing 6-state enum
└── (existing fields unchanged)

Pipeline (orchestrator)
├── uses VoxError for error categorization
├── calls RecoveryAction dispatcher
├── owns retry_once() wrapper for ASR/LLM
└── spawns injection focus retry task

AudioCapture
├── error_flag: AtomicBool              # Existing — set by cpal error callback
├── health_check() → Result<()>         # NEW — checks flag + stream validity
├── switch_to_default() → Result<()>    # NEW — reconnects to default device
└── reconnect() → Result<()>            # NEW — reconnects to specific device

ModelDownloader
├── download_all() → existing flow
├── set_readonly() → NEW after verify    # FR-027
└── (SHA-256 verify already exists)

SizeLimitedWriter
├── wraps NonBlocking (tracing-appender)
├── enforces 10 MB per-file cap
└── resets counter on daily rotation

WakeEvent → triggers recovery sequence in main.rs
GpuInfo → stored in VoxState, displayed in overlay/model panel
```

## Validation Rules

| Entity | Rule | Enforcement |
|---|---|---|
| VoxError | Every variant maps to exactly one RecoveryAction | Match exhaustiveness in recovery dispatcher |
| GpuInfo | vram_bytes > 0 on Windows (GPU required) | Startup check, Error state if 0 |
| SizeLimitedWriter | bytes_written resets on date change | AtomicU32 date comparison on each write |
| AudioCapture | health_check() called before segment processing | Orchestrator calls at loop start |
| Model files | SHA-256 verified on startup + on inference error | verify_all_models() + error categorization |
| Read-only permissions | Set immediately after download + verify | downloader.rs post-verify step |
| Log files | Max 7 days retention, max 10 MB per file | cleanup_old_logs() + SizeLimitedWriter |
