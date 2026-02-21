# Quickstart: Model Management Integration

**Branch**: `008-model-management` | **Date**: 2026-02-21

## Basic Usage

### Check and download models on startup

```rust
use vox_core::models::{self, ModelDownloader, DownloadEvent};

// Clean up leftover .tmp files from interrupted downloads
models::cleanup_tmp_files()?;

// Check which models are missing
let missing = models::check_missing_models();
if missing.is_empty() {
    // All models present — start pipeline immediately (FR-014: no network calls)
    let vad_path = models::model_path(models::MODELS[0].filename);
    let asr_path = models::model_path(models::MODELS[1].filename);
    let llm_path = models::model_path(models::MODELS[2].filename);
    // ... construct pipeline with these paths ...
    return Ok(());
}

// Create downloader and subscribe to progress events
let downloader = ModelDownloader::new();
let mut progress_rx = downloader.subscribe();

// Monitor progress on a separate task
tokio::spawn(async move {
    loop {
        match progress_rx.recv().await {
            Ok(DownloadEvent::Progress { model, downloaded, total }) => {
                // Update UI progress bar
            }
            Ok(DownloadEvent::Complete { model }) => {
                // Mark model as ready in UI
            }
            Ok(DownloadEvent::Failed { model, error }) => {
                // Show error in UI
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }
});

// Download all missing models concurrently
match downloader.download_missing(&missing).await {
    Ok(()) => {
        // All models downloaded and verified — start pipeline
    }
    Err(e) => {
        // Show manual download instructions (FR-009)
        // Start directory polling (FR-012)
        downloader.poll_until_ready().await?;
    }
}
```

### Model paths for pipeline construction

```rust
use vox_core::models;

// Get paths for each model type
let vad_path = models::model_path("silero_vad_v5.onnx");
let asr_path = models::model_path("ggml-large-v3-turbo-q5_0.bin");
let llm_path = models::model_path("qwen2.5-3b-instruct-q4_k_m.gguf");

// Or iterate the registry
for model in models::MODELS {
    let path = models::model_path(model.filename);
    println!("{}: {}", model.name, path.display());
}
```

### Model format validation (for swapping)

```rust
use vox_core::models::format::{detect_format, ModelFormat};

let format = detect_format(&path)?;
match format {
    ModelFormat::Gguf => println!("LLM model (GGUF format)"),
    ModelFormat::Ggml => println!("ASR model (GGML format)"),
    ModelFormat::Onnx => println!("VAD model (ONNX format)"),
    ModelFormat::Unknown => println!("Unrecognized format"),
}
```

### Open model directory in file explorer

```rust
use vox_core::models;

// Opens the platform-specific model directory in the file explorer
models::open_model_directory()?;
```

## Testing Patterns

### Unit test with temporary directory

```rust
use tempfile::TempDir;

#[test]
fn test_check_missing_models() {
    // Tests use the real model_dir() path
    // Alternatively, test path resolution functions independently
    let dir = models::model_dir();
    assert!(dir.ends_with("com.vox.app/models"));
}
```

### Integration test with real download (VAD only, ~1.1 MB)

```rust
#[tokio::test]
async fn test_download_vad_model() {
    let downloader = ModelDownloader::new();
    let vad = &models::MODELS[0]; // Silero VAD, ~1.1 MB

    // Download to a temp directory to avoid polluting the real model dir
    // (test helper overrides the destination path)
    let result = downloader.download_model_to(vad, &temp_path).await;
    assert!(result.is_ok());

    // Verify file exists and checksum matches
    assert!(temp_path.exists());
    assert!(models::verify_checksum(&temp_path, vad.sha256).unwrap());
}
```

## Error Handling

All functions return `anyhow::Result`. Key error conditions:

| Error | Source | Recovery |
|-------|--------|----------|
| Network unreachable | `reqwest::Error` | Show manual download UI, start polling |
| HTTP 404/500 | `reqwest::Response::status()` | Show error, allow retry |
| SHA-256 mismatch | `verify_checksum()` | Delete file, auto-retry once, then show manual UI |
| Disk full | `std::io::Error` | Delete .tmp, report required space |
| Permission denied | `std::io::Error` | Report model directory path |
| Invalid model format | `detect_format()` | Report to user, do not load |
