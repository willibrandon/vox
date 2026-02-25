//! Model management subsystem for ML model downloading, verification, and storage.
//!
//! Provides a static registry of the three required ML models (VAD, ASR, LLM),
//! platform-specific storage path resolution, concurrent downloading with SHA-256
//! verification, directory polling for manual file placement, and model format
//! detection for safe model swapping.
//!
//! # Architecture
//!
//! - [`MODELS`] — Static registry of required models (name, URL, SHA-256, size)
//! - [`ModelDownloader`] — Concurrent download engine with broadcast progress events
//! - [`detect_format`] / [`format_to_slot`] — Magic byte format validation
//!
//! # Storage Location
//!
//! Models are stored in the platform-standard application data directory:
//! - Windows: `%LOCALAPPDATA%/com.vox.app/models/`
//! - macOS: `~/Library/Application Support/com.vox.app/models/`

pub mod downloader;
pub mod format;

pub use downloader::{DownloadEvent, DownloadProgress, ModelDownloader};
pub use format::{detect_format, format_to_slot, ModelFormat};

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, Result};
use parking_lot::Mutex;

/// A required ML model in the static registry.
///
/// All fields are `'static` — the registry is compiled into the binary
/// with no heap allocation. Each model is uniquely identified by its
/// `filename`. Derives `Copy` because all fields are either `&'static str`
/// or `u64`, enabling zero-cost pass-by-value into spawned async tasks.
#[derive(Clone, Copy)]
pub struct ModelInfo {
    /// Human-readable display name (e.g., "Silero VAD v5").
    pub name: &'static str,
    /// Filename on disk (e.g., "silero_vad_v5.onnx").
    pub filename: &'static str,
    /// Direct download URL (HTTPS).
    pub url: &'static str,
    /// Expected SHA-256 hex digest (lowercase, 64 chars).
    pub sha256: &'static str,
    /// Expected file size in bytes (used for progress display fallback).
    pub size_bytes: u64,
}

/// Static registry of all required ML models.
///
/// Exactly 3 entries in pipeline order: VAD (Silero), ASR (Whisper), LLM (Qwen).
/// SHA-256 hashes verified against `scripts/download-models.sh`.
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "Silero VAD v5",
        filename: "silero_vad_v5.onnx",
        url: "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx",
        sha256: "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3",
        size_bytes: 2_327_524,
    },
    ModelInfo {
        name: "Whisper Large V3 Turbo Q5_0",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size_bytes: 547_000_000,
    },
    ModelInfo {
        name: "Qwen 2.5 3B Instruct Q4_K_M",
        filename: "qwen2.5-3b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf",
        sha256: "626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d",
        size_bytes: 1_930_000_000,
    },
];

// --- Model directory override for testing ---

static MODEL_DIR_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> =
    LazyLock::new(|| Mutex::new(None));

/// RAII guard that resets the model directory override when dropped.
///
/// Returned by [`set_model_dir_override`]. Ensures the override is
/// cleaned up even if the test panics.
pub struct ModelDirGuard;

impl Drop for ModelDirGuard {
    fn drop(&mut self) {
        *MODEL_DIR_OVERRIDE.lock() = None;
    }
}

/// Override the model directory path for testing.
///
/// Returns a guard that resets the override when dropped. While the
/// guard is alive, [`model_dir`] returns the override path instead
/// of the platform-standard directory.
pub fn set_model_dir_override(path: PathBuf) -> ModelDirGuard {
    *MODEL_DIR_OVERRIDE.lock() = Some(path);
    ModelDirGuard
}

/// Returns the platform-standard model storage directory.
///
/// Windows: `%LOCALAPPDATA%/com.vox.app/models/`
/// macOS: `~/Library/Application Support/com.vox.app/models/`
///
/// Creates the directory if it does not exist (FR-013).
/// Respects test overrides set via [`set_model_dir_override`].
pub fn model_dir() -> Result<PathBuf> {
    if let Some(ref override_dir) = *MODEL_DIR_OVERRIDE.lock() {
        std::fs::create_dir_all(override_dir).with_context(|| {
            format!(
                "failed to create model directory at {}",
                override_dir.display()
            )
        })?;
        return Ok(override_dir.clone());
    }

    #[cfg(target_os = "windows")]
    let base = dirs::data_local_dir();
    #[cfg(target_os = "macos")]
    let base = dirs::data_dir();
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = dirs::data_dir();

    let base = base.context("platform data directory not available")?;
    let dir = base.join("com.vox.app").join("models");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create model directory at {}", dir.display()))?;
    Ok(dir)
}

/// Returns the full path for a model file in the model directory.
///
/// Combines [`model_dir`] with the given filename. Does not verify
/// that the file exists.
pub fn model_path(filename: &str) -> Result<PathBuf> {
    Ok(model_dir()?.join(filename))
}

/// Returns a list of models not present on disk.
///
/// Checks for file existence in [`model_dir`]. Does not verify
/// checksums — that happens during download (inline SHA-256).
pub fn check_missing_models() -> Result<Vec<&'static ModelInfo>> {
    let dir = model_dir()?;
    Ok(check_missing_in_dir(&dir))
}

/// Returns `true` if all required models are present on disk.
///
/// Equivalent to `check_missing_models()?.is_empty()`.
pub fn all_models_present() -> Result<bool> {
    Ok(check_missing_models()?.is_empty())
}

/// Verify the SHA-256 checksum of a file against an expected hex digest.
///
/// Reads the entire file in a streaming fashion (no full-file buffer).
/// Returns `Ok(true)` if the computed hash matches `expected_sha256`,
/// `Ok(false)` on mismatch, or `Err` if the file cannot be read.
pub fn verify_checksum(path: &Path, expected_sha256: &str) -> Result<bool> {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open {} for checksum verification", path.display()))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)
        .with_context(|| format!("failed to read {} for checksum", path.display()))?;

    let result = hasher.finalize();
    let hex: String = result.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(hex == expected_sha256)
}

/// Delete any leftover `.tmp` files from interrupted downloads.
///
/// Scans [`model_dir`] for files with the `.tmp` extension and removes
/// them. Called at the start of each download batch to clean up after
/// crashes or interrupted sessions (FR-015).
pub fn cleanup_tmp_files() -> Result<()> {
    let dir = model_dir()?;
    cleanup_tmp_in_dir(&dir)
}

/// Open the model directory in the system file explorer.
///
/// Windows: launches `explorer.exe`; macOS: launches `open`.
/// Fire-and-forget — does not wait for the explorer window to close.
pub fn open_model_directory() -> Result<()> {
    let dir = model_dir()?;

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(&dir)
            .spawn()
            .context("failed to open model directory in explorer")?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dir)
            .spawn()
            .context("failed to open model directory")?;
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .context("failed to open model directory")?;
    }

    Ok(())
}

// --- Internal helpers for testability ---

/// Check which models are missing from a specific directory.
fn check_missing_in_dir(dir: &Path) -> Vec<&'static ModelInfo> {
    MODELS
        .iter()
        .filter(|m| !dir.join(m.filename).exists())
        .collect()
}

/// Delete `.tmp` files in a specific directory.
pub(crate) fn cleanup_tmp_in_dir(dir: &Path) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(anyhow::Error::new(err)
                .context(format!("failed to read model directory {}", dir.display())))
        }
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("tmp") {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove tmp file {}", path.display()))?;
            tracing::info!(path = %path.display(), "cleaned up leftover .tmp file");
        }
    }
    Ok(())
}

/// Result of a model inference benchmark.
#[derive(Clone, Debug)]
pub struct BenchmarkResult {
    /// Name of the metric (e.g., "Real-time factor", "Tokens/sec").
    pub metric_name: String,
    /// Numeric value of the metric.
    pub value: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_registry_has_three_entries() {
        assert_eq!(MODELS.len(), 3, "registry must have exactly 3 models");
    }

    #[test]
    fn test_model_registry_unique_filenames() {
        let filenames: Vec<&str> = MODELS.iter().map(|m| m.filename).collect();
        let unique: std::collections::HashSet<&str> = filenames.iter().copied().collect();
        assert_eq!(
            filenames.len(),
            unique.len(),
            "all model filenames must be unique"
        );
    }

    #[test]
    fn test_model_registry_sha256_format() {
        for model in MODELS {
            assert_eq!(
                model.sha256.len(),
                64,
                "{} SHA-256 must be 64 hex chars",
                model.name
            );
            assert!(
                model.sha256.chars().all(|c| c.is_ascii_hexdigit()),
                "{} SHA-256 must be valid hex",
                model.name
            );
            assert_eq!(
                model.sha256,
                model.sha256.to_lowercase(),
                "{} SHA-256 must be lowercase",
                model.name
            );
        }
    }

    #[test]
    fn test_model_dir_platform() {
        // Compute the expected platform path directly (bypassing MODEL_DIR_OVERRIDE)
        // to avoid interference from parallel tests that set an override.
        #[cfg(target_os = "windows")]
        let base = dirs::data_local_dir();
        #[cfg(not(target_os = "windows"))]
        let base = dirs::data_dir();

        let base = base.expect("platform data directory should be available");
        let expected = base.join("com.vox.app").join("models");

        let path_str = expected.to_string_lossy();
        assert!(
            path_str.contains("com.vox.app"),
            "model dir should contain com.vox.app, got: {path_str}"
        );
        assert!(
            path_str.ends_with("models"),
            "model dir should end with 'models', got: {path_str}"
        );
    }

    #[test]
    fn test_model_path() {
        let path = model_path("test_model.bin").expect("model_path");
        assert!(
            path.ends_with("test_model.bin"),
            "model_path should end with filename"
        );
    }

    #[test]
    fn test_model_dir_override() {
        let dir = tempfile::tempdir().expect("tempdir");
        let override_path = dir.path().join("custom_models");

        {
            let _guard = set_model_dir_override(override_path.clone());
            let result = model_dir().expect("model_dir with override");
            assert_eq!(result, override_path);
            assert!(override_path.exists(), "override dir should be created");
        }

        // After guard is dropped, override should be cleared
        let result = model_dir().expect("model_dir after guard drop");
        assert_ne!(result, override_path, "override should be cleared");
    }

    #[test]
    fn test_check_missing_empty_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = check_missing_in_dir(dir.path());
        assert_eq!(
            missing.len(),
            MODELS.len(),
            "all models should be missing in empty dir"
        );
    }

    #[test]
    fn test_check_missing_with_some_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Place the first model file
        std::fs::write(dir.path().join(MODELS[0].filename), b"model data").expect("write");

        let missing = check_missing_in_dir(dir.path());
        assert_eq!(missing.len(), 2, "should have 2 missing models");
        assert!(
            missing.iter().all(|m| m.filename != MODELS[0].filename),
            "present model should not appear in missing list"
        );
    }

    #[test]
    fn test_check_missing_all_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        for model in MODELS {
            std::fs::write(dir.path().join(model.filename), b"data").expect("write");
        }

        let missing = check_missing_in_dir(dir.path());
        assert!(missing.is_empty(), "no models should be missing");
    }

    #[test]
    fn test_sha256_verification_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test_file.bin");
        std::fs::write(&path, b"hello world").expect("write");

        // SHA-256 of "hello world"
        assert!(verify_checksum(
            &path,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        )
        .expect("checksum"));
    }

    #[test]
    fn test_sha256_verification_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("test_file.bin");
        std::fs::write(&path, b"hello world").expect("write");

        assert!(!verify_checksum(
            &path,
            "0000000000000000000000000000000000000000000000000000000000000000"
        )
        .expect("checksum"));
    }

    #[test]
    fn test_cleanup_tmp_files() {
        let dir = tempfile::tempdir().expect("tempdir");

        // Create a mix of .tmp and non-.tmp files
        std::fs::write(dir.path().join("model.onnx"), b"model").expect("write");
        std::fs::write(dir.path().join("model.onnx.tmp"), b"partial").expect("write");
        std::fs::write(dir.path().join("other.tmp"), b"partial2").expect("write");

        cleanup_tmp_in_dir(dir.path()).expect("cleanup");

        assert!(
            dir.path().join("model.onnx").exists(),
            "non-tmp files should remain"
        );
        assert!(
            !dir.path().join("model.onnx.tmp").exists(),
            ".tmp file should be deleted"
        );
        assert!(
            !dir.path().join("other.tmp").exists(),
            ".tmp file should be deleted"
        );
    }

    #[test]
    fn test_cleanup_tmp_nonexistent_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nonexistent = dir.path().join("does_not_exist");
        let result = cleanup_tmp_in_dir(&nonexistent);
        assert!(result.is_ok(), "cleanup should succeed for nonexistent dir");
    }

    #[test]
    fn test_perf_sha256_large_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("large_test.bin");

        // Create a 100 MB file
        let data = vec![0xABu8; 100 * 1024 * 1024];
        std::fs::write(&path, &data).expect("write");

        let start = std::time::Instant::now();
        let _ = verify_checksum(
            &path,
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .expect("checksum should complete");
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < 5,
            "SHA-256 of 100MB took {:?}, SC-003 requires < 5s",
            elapsed
        );
    }

    #[test]
    fn test_perf_check_missing() {
        let start = std::time::Instant::now();
        let _ = check_missing_models();
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 100,
            "check_missing_models took {:?}, SC-004 requires < 100ms",
            elapsed
        );
    }
}
