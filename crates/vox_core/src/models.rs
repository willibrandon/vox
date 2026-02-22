//! Model management subsystem for automatic download, verification, and storage of ML models.
//!
//! Provides a static registry of required models ([`MODELS`]), platform-standard path resolution
//! ([`model_dir`], [`model_path`]), SHA-256 checksum verification ([`verify_checksum`]),
//! concurrent download orchestration ([`ModelDownloader`]), and model format detection
//! ([`detect_format`], [`ModelFormat`]).

/// Concurrent model download engine with streaming, SHA-256 verification, and progress reporting.
pub mod downloader;
/// Model file format detection via magic byte inspection (GGUF, GGML, ONNX).
pub mod format;

pub use downloader::{DownloadEvent, DownloadProgress, ModelDownloader};
pub use format::{ModelFormat, detect_format, format_to_slot};

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

static MODEL_DIR_OVERRIDE: parking_lot::Mutex<Option<PathBuf>> = parking_lot::Mutex::new(None);
static TEST_DIR_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// RAII guard that resets the model directory override when dropped.
///
/// Holds the serialization lock to prevent concurrent tests from conflicting.
pub struct ModelDirGuard {
    _serialize: parking_lot::MutexGuard<'static, ()>,
}

impl Drop for ModelDirGuard {
    fn drop(&mut self) {
        *MODEL_DIR_OVERRIDE.lock() = None;
    }
}

/// Override the model directory for testing. Returns a guard that resets on drop.
///
/// Tests using this are serialized — only one runs at a time.
pub fn set_model_dir_override(dir: PathBuf) -> ModelDirGuard {
    let serialize = TEST_DIR_LOCK.lock();
    *MODEL_DIR_OVERRIDE.lock() = Some(dir);
    ModelDirGuard { _serialize: serialize }
}

/// A required ML model in the static registry.
///
/// All string fields are `'static` — the registry is compiled into the binary
/// with no heap allocation.
pub struct ModelInfo {
    /// Human-readable display name (e.g., "Silero VAD v5").
    pub name: &'static str,
    /// Filename on disk (e.g., "silero_vad_v5.onnx").
    pub filename: &'static str,
    /// Direct download URL.
    pub url: &'static str,
    /// Expected SHA-256 hex digest (lowercase).
    pub sha256: &'static str,
    /// Expected file size in bytes.
    pub size_bytes: u64,
}

/// Static registry of all required ML models.
///
/// Exactly 3 entries: VAD (Silero), ASR (Whisper), LLM (Qwen).
/// SHA-256 hashes are verified values from `scripts/download-models.sh`.
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "Silero VAD v5",
        filename: "silero_vad_v5.onnx",
        url: "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx",
        sha256: "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3",
        size_bytes: 2_354_596,
    },
    ModelInfo {
        name: "Whisper Large V3 Turbo Q5_0",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        size_bytes: 574_041_068,
    },
    ModelInfo {
        name: "Qwen 2.5 3B Instruct Q4_K_M",
        filename: "qwen2.5-3b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf",
        sha256: "626b4a6678b86442240e33df819e00132d3ba7dddfe1cdc4fbb18e0a9615c62d",
        size_bytes: 2_058_382_272,
    },
];

/// Returns the platform-standard model storage directory.
///
/// On Windows: `%LOCALAPPDATA%/com.vox.app/models/`
/// On macOS: `~/Library/Application Support/com.vox.app/models/`
///
/// Creates the directory tree if it does not exist.
pub fn model_dir() -> Result<PathBuf> {
    let override_dir = MODEL_DIR_OVERRIDE.lock().clone();
    if let Some(dir) = override_dir {
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create model directory: {}", dir.display()))?;
        return Ok(dir);
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
        .with_context(|| format!("failed to create model directory: {}", dir.display()))?;
    Ok(dir)
}

/// Returns the full path for a model file in the model directory.
pub fn model_path(filename: &str) -> Result<PathBuf> {
    Ok(model_dir()?.join(filename))
}

/// Returns a list of models whose files are not present on disk.
///
/// Checks for file existence only — does not verify checksums.
pub fn check_missing_models() -> Result<Vec<&'static ModelInfo>> {
    let dir = model_dir()?;
    Ok(MODELS
        .iter()
        .filter(|m| !dir.join(m.filename).exists())
        .collect())
}

/// Returns `true` if all required models are present on disk.
pub fn all_models_present() -> Result<bool> {
    Ok(check_missing_models()?.is_empty())
}

/// Verifies the SHA-256 checksum of a file against an expected hex digest.
///
/// Streams the file through the hasher to avoid buffering large files in memory.
/// Returns `Ok(true)` if the checksum matches, `Ok(false)` if it differs.
pub fn verify_checksum(path: &Path, expected_sha256: &str) -> Result<bool> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open file for checksum: {}", path.display()))?;
    std::io::copy(&mut file, &mut hasher)
        .with_context(|| format!("failed to read file for checksum: {}", path.display()))?;
    let hash = format!("{:x}", hasher.finalize());
    Ok(hash == expected_sha256)
}

/// Deletes any leftover `.tmp` files from interrupted downloads in the model directory.
pub fn cleanup_tmp_files() -> Result<()> {
    let dir = model_dir()?;
    let entries = std::fs::read_dir(&dir)
        .with_context(|| format!("failed to read model directory: {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("tmp") {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to delete tmp file: {}", path.display()))?;
        }
    }
    Ok(())
}

/// Opens the model directory in the system file explorer.
///
/// Windows: launches `explorer.exe`. macOS: launches `open`.
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
            .context("failed to open model directory in Finder")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_model_dir_platform() {
        // Acquire serialization lock so no other test has MODEL_DIR_OVERRIDE active
        let _serialize = TEST_DIR_LOCK.lock();
        let dir = model_dir().expect("model_dir should succeed");
        let path_str = dir.to_string_lossy();
        assert!(
            path_str.contains("com.vox.app") && path_str.ends_with("models"),
            "model dir should end with com.vox.app/models, got: {path_str}"
        );
    }

    #[test]
    fn test_model_registry_has_three_entries() {
        assert_eq!(MODELS.len(), 3, "registry should contain exactly 3 models");
    }

    #[test]
    fn test_model_path_joins_correctly() {
        let tmp_dir = TempDir::new().expect("create tempdir");
        let _guard = set_model_dir_override(tmp_dir.path().to_path_buf());
        let path = model_path("test_model.bin").expect("model_path should succeed");
        assert!(path.ends_with("test_model.bin"));
    }

    #[test]
    fn test_check_models_missing() {
        let tmp_dir = TempDir::new().expect("create tempdir");
        let _guard = set_model_dir_override(tmp_dir.path().to_path_buf());
        // Empty tempdir means all models are missing
        let missing = check_missing_models().expect("check_missing_models should succeed");
        assert_eq!(missing.len(), MODELS.len(), "all models should be missing in empty tempdir");
        for m in &missing {
            assert!(!m.filename.is_empty());
        }
    }

    #[test]
    fn test_sha256_verification() {
        let dir = TempDir::new().expect("create tempdir");
        let file_path = dir.path().join("test_hash.bin");
        let content = b"hello world";
        std::fs::write(&file_path, content).expect("write test file");

        // Known SHA-256 of "hello world"
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(verify_checksum(&file_path, expected).expect("verify should succeed"));
    }

    #[test]
    fn test_sha256_mismatch() {
        let dir = TempDir::new().expect("create tempdir");
        let file_path = dir.path().join("corrupt.bin");
        std::fs::write(&file_path, b"corrupted data").expect("write test file");

        let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(!verify_checksum(&file_path, wrong_hash).expect("verify should succeed"));
    }

    #[test]
    fn test_cleanup_tmp_files() {
        let tmp_dir = TempDir::new().expect("create tempdir");
        let _guard = set_model_dir_override(tmp_dir.path().to_path_buf());
        let dir = model_dir().expect("model_dir should succeed");

        // Create a .tmp file
        let tmp_path = dir.join("test_cleanup.tmp");
        let mut f = std::fs::File::create(&tmp_path).expect("create tmp file");
        f.write_all(b"partial download").expect("write tmp");
        drop(f);

        assert!(tmp_path.exists(), "tmp file should exist before cleanup");
        cleanup_tmp_files().expect("cleanup should succeed");
        assert!(!tmp_path.exists(), "tmp file should be deleted after cleanup");
    }

    /// SC-003: SHA-256 verification of a large file completes in under 5 seconds.
    #[test]
    fn test_sha256_performance_large_file() {
        let dir = TempDir::new().expect("create tempdir");
        let file_path = dir.path().join("large_file.bin");

        // Create a 100 MB file filled with a repeating pattern
        let chunk = vec![0xABu8; 1024 * 1024]; // 1 MB
        let mut f = std::fs::File::create(&file_path).expect("create large file");
        for _ in 0..100 {
            f.write_all(&chunk).expect("write chunk");
        }
        f.sync_all().expect("sync");
        drop(f);

        let start = std::time::Instant::now();
        let _ = verify_checksum(&file_path, "dummy_hash_for_timing_only");
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "SHA-256 of 100 MB should complete in under 5 seconds, took {:?}",
            elapsed
        );
    }

    /// SC-004: Model directory detection completes in under 100ms.
    #[test]
    fn test_check_missing_models_performance() {
        let tmp_dir = TempDir::new().expect("create tempdir");
        let _guard = set_model_dir_override(tmp_dir.path().to_path_buf());

        let start = std::time::Instant::now();
        let _ = check_missing_models();
        let elapsed = start.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "check_missing_models should complete in under 100ms, took {:?}",
            elapsed
        );
    }
}
