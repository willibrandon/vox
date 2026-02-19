# Feature 008: Model Management

**Status:** Not Started
**Dependencies:** 001-workspace-scaffolding
**Design Reference:** Section 11 (Model Management), Section 5.0 (First Launch)
**Estimated Scope:** Auto-download, SHA-256 verification, concurrent downloads, storage, swapping

---

## Overview

Implement the model management subsystem that handles automatic downloading, verification, storage, and swapping of ML models. On first launch, all three models download concurrently with no user interaction required. This is Constitution Principle V (Zero-Click First Launch) — no setup wizards, no confirmation dialogs, no "click to download" buttons. It just starts.

---

## Requirements

### FR-001: Model Registry

```rust
// crates/vox_core/src/models.rs

pub struct ModelInfo {
    pub name: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size_bytes: u64,
}

pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "Silero VAD v5",
        filename: "silero_vad_v5.onnx",
        url: "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx",
        sha256: "TBD",  // Fill with actual hash after first download
        size_bytes: 1_100_000, // ~1.1 MB
    },
    ModelInfo {
        name: "Whisper Large V3 Turbo Q5_0",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "TBD",
        size_bytes: 900_000_000, // ~900 MB
    },
    ModelInfo {
        name: "Qwen 2.5 3B Instruct Q4_K_M",
        filename: "Qwen2.5-3B-Instruct-Q4_K_M.gguf",
        url: "https://huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF/resolve/main/Qwen2.5-3B-Instruct-Q4_K_M.gguf",
        sha256: "TBD",
        size_bytes: 1_600_000_000, // ~1.6 GB
    },
];
```

### FR-002: Model Storage Paths

```rust
pub fn model_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        // %LOCALAPPDATA%/com.vox.app/models/
        dirs::data_local_dir().unwrap().join("com.vox.app").join("models")
    }
    #[cfg(target_os = "macos")]
    {
        // ~/Library/Application Support/com.vox.app/models/
        dirs::data_dir().unwrap().join("com.vox.app").join("models")
    }
}

pub fn model_path(filename: &str) -> PathBuf {
    model_dir().join(filename)
}
```

### FR-003: Model Detection

```rust
pub fn check_models() -> Vec<&'static ModelInfo> {
    MODELS.iter()
        .filter(|m| !model_path(m.filename).exists())
        .collect()
}

pub fn all_models_present() -> bool {
    MODELS.iter().all(|m| model_path(m.filename).exists())
}
```

### FR-004: Concurrent Download

All three models download concurrently using `reqwest` 0.13 with streaming:

```rust
use reqwest::Client;
use tokio::io::AsyncWriteExt;

pub struct ModelDownloader {
    client: Client,
    progress_tx: broadcast::Sender<DownloadEvent>,
}

#[derive(Clone, Debug)]
pub enum DownloadEvent {
    Started { model: String, total_bytes: u64 },
    Progress { model: String, downloaded: u64, total: u64 },
    Complete { model: String },
    Failed { model: String, error: String },
    VerificationFailed { model: String },
}

impl ModelDownloader {
    pub async fn download_missing(&self, missing: Vec<&ModelInfo>) -> Result<()> {
        // Download all missing models concurrently
        let tasks: Vec<_> = missing.iter()
            .map(|model| self.download_model(model))
            .collect();
        futures::future::join_all(tasks).await;
        Ok(())
    }

    async fn download_model(&self, model: &ModelInfo) -> Result<()> {
        // 1. Create model directory if not exists
        // 2. Download to .tmp file (atomic write)
        // 3. Verify SHA-256 checksum
        // 4. Rename .tmp to final filename
        // 5. Report progress via broadcast channel
    }
}
```

### FR-005: SHA-256 Verification

After download, verify file integrity:

```rust
use sha2::{Sha256, Digest};

pub fn verify_checksum(path: &Path, expected_sha256: &str) -> Result<bool> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    let hash = format!("{:x}", hasher.finalize());
    Ok(hash == expected_sha256)
}
```

If verification fails:
- Delete the corrupt file
- Re-download automatically
- If re-download also fails, show manual download instructions

### FR-006: Download Progress Reporting

```rust
#[derive(Clone, Debug)]
pub enum DownloadProgress {
    Pending,
    InProgress { bytes_downloaded: u64, bytes_total: u64 },
    Complete,
    Failed { error: String, manual_url: String },
}
```

Progress updates emit every 500ms (not every chunk, to avoid UI thrashing).

### FR-007: Download Failure Handling

If download fails (no internet, server error, timeout):

1. Overlay shows: model directory path + direct download URLs
2. "Open Folder" button opens the model directory in file explorer
3. "Retry Download" button retries the failed model(s)
4. App keeps running and polls for model files every 5 seconds
5. The moment models appear on disk (manual download, USB transfer), they're detected and loaded

### FR-008: Atomic File Operations

Downloads write to a `.tmp` file, then rename to the final name. This prevents partial files from being mistaken for valid models:

```
ggml-large-v3-turbo-q5_0.bin.tmp  → downloading
ggml-large-v3-turbo-q5_0.bin      → complete and verified
```

### FR-009: Model Swapping

Users can swap models via the Model panel in the settings window:

1. User places a new model file in the model directory (or downloads via UI)
2. Vox validates the file format (GGUF/GGML/ONNX header check)
3. Quick benchmark: run a test inference to verify GPU acceleration works
4. If validation passes, the pipeline restarts with the new model

### FR-010: Disk Monitoring

Poll the model directory every 5 seconds when models are missing. This enables:
- Manual file placement detection
- Recovery after network outage (user downloads via browser)
- USB transfer scenarios

---

## Network Policy

The only permitted network operation is model download during first-run setup (Constitution Principle I). After all models are downloaded:
- Zero network calls
- No telemetry
- No update checks
- No analytics

---

## Acceptance Criteria

- [ ] Missing models detected on startup
- [ ] All three models download concurrently
- [ ] Download progress reported to UI (bytes/total)
- [ ] SHA-256 verification passes for valid files
- [ ] Corrupt downloads are re-downloaded automatically
- [ ] Atomic write prevents partial file detection
- [ ] Download failure shows manual instructions with model URLs
- [ ] File polling detects manually placed models within 5 seconds
- [ ] Pre-existing models skip download entirely
- [ ] Model directory created automatically on first run
- [ ] No network calls after all models are downloaded
- [ ] Zero compiler warnings

---

## Testing Requirements

### Unit Tests

| Test | Description |
|---|---|
| `test_model_dir_platform` | Correct path on each platform |
| `test_check_models_all_present` | Returns empty when all models exist |
| `test_check_models_missing` | Returns missing models correctly |
| `test_sha256_verification` | Known file matches expected hash |
| `test_sha256_mismatch` | Corrupt file detected |
| `test_atomic_write` | .tmp file renamed to final on success |
| `test_atomic_write_cleanup` | .tmp file cleaned up on failure |

### Integration Tests

| Test | Description |
|---|---|
| `test_download_small_model` | Download VAD model (~1 MB), verify checksum |
| `test_concurrent_download` | Three models download concurrently |
| `test_resume_after_failure` | Download fails, retry succeeds |

---

## Performance Targets

| Metric | Target |
|---|---|
| Download speed | Limited by network (no artificial throttling) |
| SHA-256 verification | < 5 seconds for 1.8 GB file |
| Model directory detection | < 100 ms |
| Disk polling interval | 5 seconds |
| Total download (all models, 100 Mbps) | ~5 minutes |
