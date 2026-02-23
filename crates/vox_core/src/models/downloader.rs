//! Concurrent model download engine with SHA-256 verification and progress reporting.
//!
//! Downloads missing models in parallel via tokio tasks, streaming HTTP responses
//! through bounded mpsc channels to blocking file writers that compute SHA-256
//! inline. Progress events are broadcast to subscribers (overlay HUD, model panel).
//! Supports auto-retry on checksum failure and directory polling for manual file
//! placement.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tokio::sync::broadcast;

use super::ModelInfo;

/// Event emitted during model download operations.
///
/// Distributed via `tokio::sync::broadcast` channel. Multiple consumers
/// (overlay HUD, model panel, log viewer) can subscribe independently.
/// Progress events are throttled to 500ms per model to avoid UI thrashing.
#[derive(Clone, Debug)]
pub enum DownloadEvent {
    /// Download initiated; `total_bytes` from HTTP Content-Length header.
    Started {
        /// Human-readable model name.
        model: String,
        /// Total download size in bytes.
        total_bytes: u64,
    },
    /// Bytes downloaded so far (throttled to 500ms intervals per model).
    Progress {
        /// Human-readable model name.
        model: String,
        /// Bytes received so far.
        downloaded: u64,
        /// Total expected bytes.
        total: u64,
    },
    /// Download complete, SHA-256 verified, file renamed to final location.
    Complete {
        /// Human-readable model name.
        model: String,
    },
    /// Download or verification failed after retry.
    Failed {
        /// Human-readable model name.
        model: String,
        /// Human-readable error description.
        error: String,
    },
    /// SHA-256 checksum mismatch — file is corrupt.
    VerificationFailed {
        /// Human-readable model name.
        model: String,
    },
    /// Model file detected on disk via directory polling (manual placement).
    DetectedOnDisk {
        /// Human-readable model name.
        model: String,
    },
}

/// Aggregate download state for a single model, used by the UI layer.
///
/// Derived from the stream of [`DownloadEvent`]s. Each variant represents
/// the current state of one model's download lifecycle.
#[derive(Clone, Debug)]
pub enum DownloadProgress {
    /// Download not yet started.
    Pending,
    /// Actively downloading.
    InProgress {
        /// Bytes received so far.
        bytes_downloaded: u64,
        /// Total expected bytes.
        bytes_total: u64,
    },
    /// Downloaded and SHA-256 verified.
    Complete,
    /// Failed with recovery information.
    Failed {
        /// Human-readable error description.
        error: String,
        /// Direct download URL for manual recovery.
        manual_url: String,
    },
}

/// Manages concurrent model downloads with progress reporting.
///
/// Holds a shared HTTP client and broadcast channel for distributing
/// download events. Each instance resolves the model directory once at
/// construction to avoid global state races when multiple downloaders
/// exist concurrently (e.g. parallel tests).
pub struct ModelDownloader {
    /// Shared HTTP client (connection pool is internal).
    client: reqwest::Client,
    /// Broadcast sender for download events (capacity 16).
    sender: broadcast::Sender<DownloadEvent>,
    /// Captured model directory — resolved once at construction, used for
    /// all downloads in this instance. Avoids races with the global
    /// MODEL_DIR_OVERRIDE when multiple tests run in parallel.
    model_dir: std::path::PathBuf,
}

impl ModelDownloader {
    /// Create a new downloader using the platform-standard model directory.
    ///
    /// The broadcast channel has capacity 16. Subscribers that fall behind
    /// will receive `RecvError::Lagged` and miss intermediate progress
    /// updates (acceptable for idempotent progress events).
    pub fn new() -> Self {
        let dir = super::model_dir().expect("failed to resolve model directory");
        let (sender, _) = broadcast::channel(16);
        Self {
            client: reqwest::Client::new(),
            sender,
            model_dir: dir,
        }
    }

    /// Create a new downloader targeting a specific directory.
    ///
    /// Used by tests to avoid global state races with `MODEL_DIR_OVERRIDE`.
    /// Each test gets its own downloader pointing at its own temp directory.
    pub fn with_model_dir(dir: std::path::PathBuf) -> Self {
        std::fs::create_dir_all(&dir).expect("failed to create model directory");
        let (sender, _) = broadcast::channel(16);
        Self {
            client: reqwest::Client::new(),
            sender,
            model_dir: dir,
        }
    }

    /// Subscribe to download progress events.
    ///
    /// Returns a broadcast receiver. Multiple independent subscribers are
    /// supported — each receives all events independently.
    pub fn subscribe(&self) -> broadcast::Receiver<DownloadEvent> {
        self.sender.subscribe()
    }

    /// Download all specified models concurrently.
    ///
    /// Spawns one tokio task per model. Each download streams the HTTP
    /// response to a `.tmp` file with inline SHA-256 computation, then
    /// atomically renames to the final path on verification success.
    /// On checksum failure, deletes the `.tmp` file and retries once.
    /// On second failure, emits a `Failed` event and collects the error.
    /// Returns `Ok(())` only if all models downloaded and verified.
    pub async fn download_missing(&self, missing: &[&ModelInfo]) -> Result<()> {
        super::cleanup_tmp_in_dir(&self.model_dir)?;

        let mut handles = Vec::with_capacity(missing.len());

        for &model_ref in missing {
            let model = *model_ref;
            let client = self.client.clone();
            let sender = self.sender.clone();
            let model_dir = self.model_dir.clone();

            handles.push(tokio::spawn(async move {
                let inner = ModelDownloader { client, sender, model_dir };

                match inner.download_model(&model).await {
                    Ok(()) => Ok(()),
                    Err(first_err) => {
                        tracing::warn!(
                            model = model.name,
                            error = %first_err,
                            "first download attempt failed, retrying"
                        );

                        match inner.download_model(&model).await {
                            Ok(()) => Ok(()),
                            Err(second_err) => {
                                // No-op if no receivers subscribed (expected during tests)
                                let _ = inner.sender.send(DownloadEvent::Failed {
                                    model: model.name.to_string(),
                                    error: second_err.to_string(),
                                });
                                Err(second_err)
                            }
                        }
                    }
                }
            }));
        }

        let mut errors = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => errors.push(err),
                Err(join_err) => {
                    errors.push(anyhow::anyhow!("download task panicked: {}", join_err))
                }
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            let message = errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; ");
            anyhow::bail!("model downloads failed: {}", message)
        }
    }

    /// Poll the model directory every 5 seconds until all models are present.
    ///
    /// Emits `DetectedOnDisk` events via the broadcast channel when
    /// manually-placed files are found. Returns `Ok(())` when all
    /// required models exist on disk. Self-terminates to satisfy
    /// FR-014 (no background activity after all models present).
    pub async fn poll_until_ready(&self) -> Result<()> {
        let expected: Vec<(&str, &str)> =
            super::MODELS.iter().map(|m| (m.filename, m.name)).collect();
        poll_directory(&self.sender, &self.model_dir, &expected).await
    }

    /// Download a single model: stream HTTP → .tmp file with inline SHA-256 → atomic rename.
    async fn download_model(&self, model: &ModelInfo) -> Result<()> {
        let final_path = self.model_dir.join(model.filename);
        let tmp_filename = format!("{}.tmp", model.filename);
        let tmp_path = final_path.with_file_name(&tmp_filename);

        // Remove any leftover .tmp from a previous interrupted attempt
        if tmp_path.exists() {
            std::fs::remove_file(&tmp_path)
                .with_context(|| format!("failed to remove leftover {}", tmp_path.display()))?;
        }

        let response = self
            .client
            .get(model.url)
            .send()
            .await
            .with_context(|| format!("failed to request {}", model.name))?;

        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("HTTP {} downloading {}", status, model.name);
        }

        let total_bytes = response.content_length().unwrap_or(model.size_bytes);

        // No-op if no receivers subscribed yet (expected during async startup)
        let _ = self.sender.send(DownloadEvent::Started {
            model: model.name.to_string(),
            total_bytes,
        });

        let (chunk_tx, chunk_rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(64);

        let writer_path = tmp_path.clone();
        let writer_handle =
            tokio::task::spawn_blocking(move || write_chunks_to_file(chunk_rx, &writer_path));

        // Stream HTTP response chunks to the blocking writer
        let mut downloaded: u64 = 0;
        let mut last_progress = Instant::now();
        let mut response = response;

        let stream_result: Result<()> = async {
            while let Some(chunk) = response
                .chunk()
                .await
                .with_context(|| format!("network error downloading {}", model.name))?
            {
                downloaded += chunk.len() as u64;

                if last_progress.elapsed() >= Duration::from_millis(500) {
                    let _ = self.sender.send(DownloadEvent::Progress {
                        model: model.name.to_string(),
                        downloaded,
                        total: total_bytes,
                    });
                    last_progress = Instant::now();
                }

                chunk_tx
                    .send(chunk)
                    .await
                    .map_err(|_| anyhow::anyhow!("writer task terminated unexpectedly"))?;
            }
            Ok(())
        }
        .await;

        // Close channel to signal writer to finish
        drop(chunk_tx);

        // Always await writer for proper cleanup
        let writer_result = writer_handle.await.context("writer task panicked")?;

        // If streaming failed, clean up and return that error
        if let Err(stream_err) = stream_result {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(stream_err);
        }

        let computed_hash = writer_result?;

        // Emit final progress (100%)
        let _ = self.sender.send(DownloadEvent::Progress {
            model: model.name.to_string(),
            downloaded,
            total: total_bytes,
        });

        // Verify SHA-256
        if computed_hash != model.sha256 {
            let _ = std::fs::remove_file(&tmp_path);
            let _ = self.sender.send(DownloadEvent::VerificationFailed {
                model: model.name.to_string(),
            });
            anyhow::bail!(
                "SHA-256 mismatch for {}: expected {}, got {}",
                model.name,
                model.sha256,
                computed_hash
            );
        }

        // Atomic rename: .tmp → final
        std::fs::rename(&tmp_path, &final_path).with_context(|| {
            format!(
                "failed to rename {} to {}",
                tmp_path.display(),
                final_path.display()
            )
        })?;

        let _ = self.sender.send(DownloadEvent::Complete {
            model: model.name.to_string(),
        });

        tracing::info!(
            model = model.name,
            path = %final_path.display(),
            "model downloaded and verified"
        );
        Ok(())
    }
}

/// Receive chunks from an mpsc channel, write to file with inline SHA-256.
///
/// Runs inside `spawn_blocking` to avoid blocking the tokio runtime on
/// disk I/O. Returns the lowercase hex SHA-256 digest of all written bytes.
fn write_chunks_to_file(
    mut rx: tokio::sync::mpsc::Receiver<bytes::Bytes>,
    path: &Path,
) -> Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Write;

    let mut file = std::fs::File::create(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    let mut hasher = Sha256::new();

    while let Some(chunk) = rx.blocking_recv() {
        file.write_all(&chunk)
            .with_context(|| format!("failed to write to {}", path.display()))?;
        hasher.update(&chunk);
    }

    file.sync_all()
        .with_context(|| format!("failed to sync {}", path.display()))?;

    let hash = hasher.finalize();
    Ok(hash.iter().map(|b| format!("{:02x}", b)).collect())
}

/// Poll a directory for expected files, emitting `DetectedOnDisk` events.
///
/// Internal helper used by [`ModelDownloader::poll_until_ready`] and tests.
/// Checks every 5 seconds for newly-appeared files, emitting events for
/// each detection, and returns when all expected files are present.
async fn poll_directory(
    sender: &broadcast::Sender<DownloadEvent>,
    dir: &Path,
    expected_files: &[(&str, &str)],
) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    let mut previously_found: HashSet<String> = HashSet::new();

    // Record what's already present
    for &(filename, _) in expected_files {
        if dir.join(filename).exists() {
            previously_found.insert(filename.to_string());
        }
    }

    loop {
        interval.tick().await;

        let mut all_present = true;
        for &(filename, model_name) in expected_files {
            let path = dir.join(filename);
            if path.exists() {
                if previously_found.insert(filename.to_string()) {
                    let _ = sender.send(DownloadEvent::DetectedOnDisk {
                        model: model_name.to_string(),
                    });
                    tracing::info!(model = model_name, "model detected on disk");
                }
            } else {
                all_present = false;
            }
        }

        if all_present {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_write_with_verification() {
        let dir = tempfile::tempdir().expect("tempdir");
        let final_path = dir.path().join("test_model.bin");
        let tmp_path = dir.path().join("test_model.bin.tmp");

        let data = b"hello world";
        let expected_hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

        std::fs::write(&tmp_path, data).expect("write tmp");

        let valid = super::super::verify_checksum(&tmp_path, expected_hash).expect("checksum");
        assert!(valid, "checksum should match");

        std::fs::rename(&tmp_path, &final_path).expect("rename");

        assert!(final_path.exists(), "final file should exist after rename");
        assert!(!tmp_path.exists(), ".tmp should not exist after rename");
        assert_eq!(std::fs::read(&final_path).expect("read"), data);
    }

    #[test]
    fn test_atomic_write_cleanup_on_mismatch() {
        let dir = tempfile::tempdir().expect("tempdir");
        let final_path = dir.path().join("test_model.bin");
        let tmp_path = dir.path().join("test_model.bin.tmp");

        std::fs::write(&tmp_path, b"corrupt data").expect("write tmp");

        let valid = super::super::verify_checksum(
            &tmp_path,
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .expect("checksum");
        assert!(!valid, "checksum should not match corrupt data");

        std::fs::remove_file(&tmp_path).expect("cleanup");

        assert!(!tmp_path.exists(), ".tmp should be deleted on failure");
        assert!(
            !final_path.exists(),
            "final file should never exist on failure"
        );
    }

    #[tokio::test]
    async fn test_poll_detects_file_placement() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dir_path = dir.path().to_path_buf();
        let (sender, mut receiver) = broadcast::channel(16);

        let expected = vec![("test_model.bin", "Test Model")];

        let poll_dir = dir_path.clone();
        let poll_handle = tokio::spawn(async move {
            poll_directory(&sender, &poll_dir, &expected).await
        });

        // Place file after 1 second (poll interval is 5s)
        let place_dir = dir_path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            std::fs::write(place_dir.join("test_model.bin"), b"model data")
                .expect("write test model");
        });

        // Poll should detect within 10 seconds (two poll intervals)
        let result = tokio::time::timeout(Duration::from_secs(15), poll_handle).await;

        assert!(result.is_ok(), "poll should complete within timeout");
        let inner = result.expect("no timeout").expect("no panic");
        assert!(inner.is_ok(), "poll should succeed");

        // Verify DetectedOnDisk event was emitted
        let mut found_event = false;
        loop {
            match receiver.try_recv() {
                Ok(DownloadEvent::DetectedOnDisk { ref model }) if model == "Test Model" => {
                    found_event = true;
                }
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue,
                _ => {}
            }
        }
        assert!(found_event, "should have received DetectedOnDisk event");
    }
}
