# Data Model: Model Management

**Branch**: `008-model-management` | **Date**: 2026-02-21

## Entities

### ModelInfo

Static registry entry for a required ML model. Compiled into the binary.

| Field | Type | Description |
|-------|------|-------------|
| name | `&'static str` | Human-readable display name (e.g., "Silero VAD v5") |
| filename | `&'static str` | Filename on disk (e.g., "silero_vad_v5.onnx") |
| url | `&'static str` | Direct download URL |
| sha256 | `&'static str` | Expected SHA-256 hex digest (lowercase, 64 chars) |
| size_bytes | `u64` | Expected file size in bytes (used for progress display) |

**Constraints**:
- Registry is a `&'static [ModelInfo]` constant — no runtime mutation
- Exactly 3 entries (VAD, ASR, LLM) — Constitution Principle III
- All fields are `'static` — no heap allocation
- SHA-256 hashes are lowercase hex strings

**Identity**: Uniquely identified by `filename` (no two models share a filename)

### DownloadEvent

Transient state change emitted during model download operations. Distributed via `tokio::sync::broadcast`.

| Variant | Fields | Description |
|---------|--------|-------------|
| Started | `model: String, total_bytes: u64` | Download initiated, total size known from Content-Length |
| Progress | `model: String, downloaded: u64, total: u64` | Bytes downloaded so far (throttled to 500ms intervals) |
| Complete | `model: String` | Download finished, SHA-256 verified, file renamed to final name |
| Failed | `model: String, error: String` | Download or verification failed |
| VerificationFailed | `model: String` | SHA-256 mismatch — file is corrupt |
| DetectedOnDisk | `model: String` | Model file found via directory polling (manual placement) |

**Constraints**:
- Must implement `Clone` (required by broadcast channel)
- Must implement `Debug` (logging)
- `model` field is the human-readable name from `ModelInfo::name`
- Progress events emitted at most once per 500ms per model (FR-007)

### DownloadProgress

Current aggregate download state for a single model. Used by UI to render per-model status.

| Variant | Fields | Description |
|---------|--------|-------------|
| Pending | — | Not yet started |
| InProgress | `bytes_downloaded: u64, bytes_total: u64` | Downloading |
| Complete | — | Downloaded and SHA-256 verified |
| Failed | `error: String, manual_url: String` | Failed with recovery info |

**Constraints**:
- Must implement `Clone` (UI state snapshots)
- `manual_url` in Failed variant is the direct download URL for manual recovery

### ModelFormat

Detected file format based on magic byte inspection. Used for model swapping validation.

| Variant | Magic Bytes | Model Type |
|---------|-------------|------------|
| Gguf | `0x47475546` ("GGUF") | LLM (Qwen) |
| Ggml | `0x67676D6C` ("ggml"), `0x67676D66` ("ggmf"), `0x67676A74` ("ggjt") | ASR (Whisper) |
| Onnx | Protobuf wire format (first byte `0x08`) | VAD (Silero) |
| Unknown | — | Unrecognized format |

**Constraints**:
- Detection reads only the first 4 bytes of the file
- Format uniquely maps to model slot (GGUF = LLM, GGML = ASR, ONNX = VAD)
- Unknown format is an error condition reported to the user

## State Transitions

### Per-Model Download State

```
Pending ──→ InProgress ──→ Complete
                │
                ├──→ VerificationFailed ──→ InProgress (auto-retry)
                │                              │
                │                              └──→ Failed (retry also failed)
                │
                └──→ Failed (network/server error)
                        │
                        ├──→ InProgress (user clicks "Retry")
                        └──→ Complete (manual placement detected by poller)
```

### Application-Level Model State

```
Checking ──→ AllPresent ──→ PipelineReady
    │
    └──→ Downloading ──→ AllPresent ──→ PipelineReady
             │
             └──→ DownloadFailed ──→ Polling
                                       │
                                       ├──→ Downloading (user clicks "Retry")
                                       └──→ AllPresent (manual placement)
```

## Relationships

```
ModelInfo (static, 3 entries)
    │
    ├── ModelDownloader uses ModelInfo to know what to download
    │
    ├── DownloadEvent references ModelInfo::name
    │
    ├── DownloadProgress derived from DownloadEvent stream
    │
    └── ModelFormat maps to ModelInfo slot (GGUF→LLM, GGML→ASR, ONNX→VAD)

ModelDownloader
    │
    ├── owns reqwest::Client (shared across downloads)
    │
    ├── owns broadcast::Sender<DownloadEvent>
    │
    └── spawns one tokio task per concurrent download
```
