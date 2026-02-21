# API Contract: vox_core::models

**Branch**: `008-model-management` | **Date**: 2026-02-21

This document defines the public API surface of the `vox_core::models` module. Other modules (pipeline, vox binary, vox_ui) depend on these types and functions.

## Module: `models` (root)

### Constants

```rust
/// Static registry of all required ML models.
/// Exactly 3 entries: VAD (Silero), ASR (Whisper), LLM (Qwen).
pub const MODELS: &[ModelInfo]
```

### Types

```rust
/// A required ML model in the static registry.
/// All fields are 'static — no heap allocation.
pub struct ModelInfo {
    pub name: &'static str,       // Human-readable display name
    pub filename: &'static str,   // Filename on disk
    pub url: &'static str,        // Direct download URL
    pub sha256: &'static str,     // Expected SHA-256 hex digest (lowercase)
    pub size_bytes: u64,          // Expected file size in bytes
}
```

### Functions

```rust
/// Returns the platform-standard model storage directory.
/// Windows: %LOCALAPPDATA%/com.vox.app/models/
/// macOS: ~/Library/Application Support/com.vox.app/models/
/// Creates the directory if it does not exist (FR-013).
pub fn model_dir() -> Result<PathBuf>

/// Returns the full path for a model file in the model directory.
pub fn model_path(filename: &str) -> Result<PathBuf>

/// Returns a list of models not present on disk.
/// Checks for file existence only (no checksum verification).
pub fn check_missing_models() -> Result<Vec<&'static ModelInfo>>

/// Returns true if all required models are present on disk.
pub fn all_models_present() -> Result<bool>

/// Verifies the SHA-256 checksum of a file against an expected hex digest.
/// Returns Ok(true) if match, Ok(false) if mismatch.
pub fn verify_checksum(path: &Path, expected_sha256: &str) -> Result<bool>

/// Deletes any leftover .tmp files from interrupted downloads (FR-015).
pub fn cleanup_tmp_files() -> Result<()>

/// Opens the model directory in the system file explorer (FR-010).
/// Windows: explorer.exe; macOS: open.
pub fn open_model_directory() -> Result<()>
```

## Module: `models::downloader`

### Types

```rust
/// Event emitted during model download operations.
/// Distributed via tokio::sync::broadcast channel.
#[derive(Clone, Debug)]
pub enum DownloadEvent {
    /// Download started. total_bytes from Content-Length header.
    Started { model: String, total_bytes: u64 },
    /// Progress update (throttled to 500ms per model).
    Progress { model: String, downloaded: u64, total: u64 },
    /// Download complete, SHA-256 verified, file in final location.
    Complete { model: String },
    /// Download or verification failed.
    Failed { model: String, error: String },
    /// SHA-256 verification failed (corrupt download).
    VerificationFailed { model: String },
    /// Model detected on disk via directory polling (manual placement).
    DetectedOnDisk { model: String },
}

/// Aggregate download state for a single model (UI display).
#[derive(Clone, Debug)]
pub enum DownloadProgress {
    Pending,
    InProgress { bytes_downloaded: u64, bytes_total: u64 },
    Complete,
    Failed { error: String, manual_url: String },
}

/// Manages concurrent model downloads with progress reporting.
pub struct ModelDownloader { .. }
```

### Functions

```rust
impl ModelDownloader {
    /// Create a new downloader with a broadcast channel for progress events.
    pub fn new() -> Self

    /// Subscribe to download progress events.
    /// Multiple subscribers are supported (broadcast pattern).
    pub fn subscribe(&self) -> broadcast::Receiver<DownloadEvent>

    /// Download all specified models concurrently (FR-004).
    /// Each download: stream to .tmp, SHA-256 verify, rename to final (FR-006).
    /// On checksum failure: delete and retry once (FR-008).
    /// On second failure: emit Failed event (FR-009).
    /// Returns Ok(()) if all models downloaded successfully.
    pub async fn download_missing(&self, missing: &[&ModelInfo]) -> Result<()>

    /// Poll the model directory every 5 seconds until all models are present (FR-012).
    /// Emits DetectedOnDisk events when manually-placed files are found.
    /// Returns Ok(()) when all models are present.
    pub async fn poll_until_ready(&self) -> Result<()>
}
```

## Module: `models::format`

### Types

```rust
/// Detected model file format based on magic byte inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    /// GGUF format (magic: 0x47475546). Used by LLM models (Qwen).
    Gguf,
    /// GGML/GGMF/GGJT format. Used by ASR models (Whisper).
    Ggml,
    /// ONNX protobuf format. Used by VAD models (Silero).
    Onnx,
    /// Unrecognized file format.
    Unknown,
}
```

### Functions

```rust
/// Detect the model file format by reading the first 4 bytes (FR-016).
/// Returns ModelFormat::Unknown if the file header does not match any
/// known magic bytes.
pub fn detect_format(path: &Path) -> Result<ModelFormat>

/// Maps a ModelFormat to the model slot it can fill.
/// Returns the index into MODELS (0=VAD, 1=ASR, 2=LLM),
/// or None for Unknown format.
pub fn format_to_slot(format: ModelFormat) -> Option<usize>
```

## Dependencies Between Modules

```
vox (binary)
  └── calls models::check_missing_models() on startup
  └── creates ModelDownloader if models are missing
  └── subscribes to DownloadEvent for overlay HUD updates
  └── calls models::open_model_directory() for "Open Folder" action
  └── calls poll_until_ready() when downloads fail

vox_core::pipeline
  └── uses models::model_path() to resolve model file locations
  └── blocks construction until models::all_models_present() returns true

vox_core::models::downloader
  └── uses models::MODELS registry for URLs, checksums, filenames
  └── uses models::model_path() for download destinations
  └── uses models::verify_checksum() after download
  └── uses models::cleanup_tmp_files() before starting

vox_core::models::format
  └── standalone — no dependencies on other models submodules
  └── used by model swapping logic in the app layer

vox_ui::model_panel
  └── subscribes to DownloadEvent for progress display
  └── calls models::open_model_directory() for "Open Folder" button
  └── uses models::format::detect_format() for model swap validation
```

## Error Conditions

| Function | Error Condition | Error Type |
|----------|----------------|------------|
| `model_dir()` | Platform data directory not available | `anyhow::Error` |
| `model_dir()` | Cannot create directory (permissions) | `std::io::Error` |
| `check_missing_models()` | Cannot access model directory | `std::io::Error` |
| `verify_checksum()` | Cannot open file | `std::io::Error` |
| `open_model_directory()` | Explorer/open command not found | `std::io::Error` |
| `download_missing()` | Network unreachable | `reqwest::Error` |
| `download_missing()` | HTTP 4xx/5xx response | `anyhow::Error` |
| `download_missing()` | SHA-256 mismatch after retry | `anyhow::Error` |
| `download_missing()` | Disk full during write | `std::io::Error` |
| `detect_format()` | File too small (< 4 bytes) | `std::io::Error` |
| `detect_format()` | Cannot read file | `std::io::Error` |
