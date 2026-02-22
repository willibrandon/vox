//! Concurrent model download engine with streaming, SHA-256 verification, and progress reporting.
//!
//! Handles downloading missing ML models to the platform-standard model directory,
//! verifying file integrity via SHA-256, and reporting progress via a tokio broadcast channel.
//! Supports atomic file writes (.tmp -> rename) and directory polling for manual file placement.

use std::io::Write;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;

use super::{ModelInfo, check_missing_models, model_dir};

/// Indicates a SHA-256 checksum mismatch (used to distinguish from network/IO errors in retry logic).
#[derive(Debug)]
struct ChecksumMismatch(String);

impl std::fmt::Display for ChecksumMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ChecksumMismatch {}

/// Event emitted during model download operations.
///
/// Distributed via `tokio::sync::broadcast` channel. Multiple subscribers
/// (overlay HUD, model panel, log viewer) can independently observe progress.
#[derive(Clone, Debug)]
pub enum DownloadEvent {
    /// Download started for a model. `total_bytes` from Content-Length header.
    Started { model: String, total_bytes: u64 },
    /// Progress update, throttled to 500ms per model to avoid UI thrashing.
    Progress {
        model: String,
        downloaded: u64,
        total: u64,
    },
    /// Download complete — SHA-256 verified, file moved to final location.
    Complete { model: String },
    /// Download or verification permanently failed after retry.
    Failed { model: String, error: String },
    /// SHA-256 verification failed (corrupt download, will auto-retry once).
    VerificationFailed { model: String },
    /// Model file detected on disk via directory polling (manual placement).
    DetectedOnDisk { model: String },
}

/// Aggregate download state for a single model, suitable for UI display.
#[derive(Clone, Debug)]
pub enum DownloadProgress {
    /// Download not yet started.
    Pending,
    /// Download in progress with byte counters.
    InProgress {
        bytes_downloaded: u64,
        bytes_total: u64,
    },
    /// Download complete and verified.
    Complete,
    /// Download failed — includes error message and direct download URL for manual recovery.
    Failed { error: String, manual_url: String },
}

/// Manages concurrent model downloads with progress reporting.
///
/// Holds a shared `reqwest::Client` and a broadcast channel for distributing
/// [`DownloadEvent`]s to multiple consumers.
pub struct ModelDownloader {
    client: reqwest::Client,
    sender: broadcast::Sender<DownloadEvent>,
}

impl Default for ModelDownloader {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelDownloader {
    /// Create a new downloader with a broadcast channel (capacity 16) for progress events.
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(16);
        Self {
            client: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(30))
                .read_timeout(Duration::from_secs(30))
                .build()
                .expect("failed to build HTTP client"),
            sender,
        }
    }

    /// Subscribe to download progress events.
    ///
    /// Multiple subscribers are supported. If a receiver falls behind,
    /// it will see `RecvError::Lagged` — acceptable for idempotent progress events.
    pub fn subscribe(&self) -> broadcast::Receiver<DownloadEvent> {
        self.sender.subscribe()
    }

    /// Download all specified models concurrently.
    ///
    /// Each download streams to a `.tmp` file, verifies SHA-256, then renames to final path.
    /// On checksum failure, the corrupt file is deleted and the download retries once.
    /// On second failure, a [`DownloadEvent::Failed`] is emitted.
    pub async fn download_missing(&self, missing: &[&ModelInfo]) -> Result<()> {
        let mut handles = Vec::with_capacity(missing.len());

        for model in missing {
            let client = self.client.clone();
            let sender = self.sender.clone();
            let name = model.name;
            let filename = model.filename;
            let url = model.url;
            let sha256 = model.sha256;

            handles.push(tokio::spawn(async move {
                match download_model(&client, &sender, name, filename, url, sha256).await {
                    Ok(()) => Ok(()),
                    Err(err) if err.downcast_ref::<ChecksumMismatch>().is_some() => {
                        // Checksum mismatch — clean up .tmp and retry once
                        if let Ok(dir) = model_dir() {
                            let tmp_path = dir.join(format!("{filename}.tmp"));
                            let _ = std::fs::remove_file(&tmp_path);
                        }
                        if let Err(retry_err) = download_model(&client, &sender, name, filename, url, sha256).await {
                            let _ = sender.send(DownloadEvent::Failed {
                                model: name.to_string(),
                                error: format!("{retry_err:#}"),
                            });
                            return Err(retry_err);
                        }
                        Ok(())
                    }
                    Err(err) => {
                        // Network/IO error — fail immediately, no retry
                        let _ = sender.send(DownloadEvent::Failed {
                            model: name.to_string(),
                            error: format!("{err:#}"),
                        });
                        Err(err)
                    }
                }
            }));
        }

        let mut errors = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => errors.push(err),
                Err(join_err) => errors.push(anyhow::anyhow!("download task panicked: {join_err}")),
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            let messages: Vec<String> = errors.iter().map(|e| format!("{e:#}")).collect();
            bail!("some downloads failed: {}", messages.join("; "))
        }
    }

    /// Poll the model directory every 5 seconds until all models are present.
    ///
    /// Emits [`DownloadEvent::DetectedOnDisk`] for each model file found during polling.
    /// Returns `Ok(())` when all models are present.
    pub async fn poll_until_ready(&self) -> Result<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        let mut previously_found: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Record which models are already present
        let dir = model_dir()?;
        for model in super::MODELS {
            if dir.join(model.filename).exists() {
                previously_found.insert(model.name.to_string());
            }
        }

        loop {
            interval.tick().await;

            // Check for newly appeared models and emit events before checking completeness
            for model in super::MODELS {
                if dir.join(model.filename).exists() && !previously_found.contains(model.name) {
                    previously_found.insert(model.name.to_string());
                    let _ = self.sender.send(DownloadEvent::DetectedOnDisk {
                        model: model.name.to_string(),
                    });
                }
            }

            let missing = check_missing_models()?;
            if missing.is_empty() {
                return Ok(());
            }
        }
    }
}

/// Download a single model: stream to .tmp, hash inline, verify, rename to final.
async fn download_model(
    client: &reqwest::Client,
    sender: &broadcast::Sender<DownloadEvent>,
    name: &str,
    filename: &str,
    url: &str,
    expected_sha256: &str,
) -> Result<()> {
    let dir = model_dir()?;
    let final_path = dir.join(filename);
    let tmp_path = dir.join(format!("{filename}.tmp"));

    // Skip if already present and checksum matches
    if final_path.exists() {
        match super::verify_checksum(&final_path, expected_sha256) {
            Ok(true) => {
                let _ = sender.send(DownloadEvent::Complete {
                    model: name.to_string(),
                });
                return Ok(());
            }
            _ => {
                // File exists but is corrupt or unreadable — delete and re-download
                std::fs::remove_file(&final_path).with_context(|| {
                    format!("failed to delete corrupt model file: {}", final_path.display())
                })?;
            }
        }
    }

    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("HTTP request failed for {name}"))?
        .error_for_status()
        .with_context(|| format!("HTTP error for {name}"))?;

    let total_bytes = response.content_length().unwrap_or(0);
    let _ = sender.send(DownloadEvent::Started {
        model: name.to_string(),
        total_bytes,
    });

    // Set up bounded mpsc channel for feeding chunks to the blocking writer
    let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::channel::<bytes::Bytes>(32);

    // Capture values for the blocking task
    let tmp_path_clone = tmp_path.clone();
    // Spawn blocking writer + hasher
    let writer_handle = tokio::task::spawn_blocking(move || -> Result<String> {
        let mut file = std::fs::File::create(&tmp_path_clone)
            .with_context(|| format!("failed to create tmp file: {}", tmp_path_clone.display()))?;
        let mut hasher = Sha256::new();

        while let Some(chunk) = chunk_rx.blocking_recv() {
            hasher.update(&chunk);
            file.write_all(&chunk)
                .with_context(|| format!("failed to write to: {}", tmp_path_clone.display()))?;
        }

        file.sync_all()
            .with_context(|| format!("failed to sync file: {}", tmp_path_clone.display()))?;

        let hash = format!("{:x}", hasher.finalize());
        Ok(hash)
    });

    // Stream HTTP response chunks, sending to writer via mpsc
    let mut response = response;
    let mut downloaded: u64 = 0;
    let mut last_progress = Instant::now();
    let model_name = name.to_string();
    let sender_clone = sender.clone();

    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                downloaded += chunk.len() as u64;

                // Throttle progress events to 500ms
                if last_progress.elapsed() >= Duration::from_millis(500) {
                    let _ = sender_clone.send(DownloadEvent::Progress {
                        model: model_name.clone(),
                        downloaded,
                        total: total_bytes,
                    });
                    last_progress = Instant::now();
                }

                chunk_tx
                    .send(chunk)
                    .await
                    .context("writer task dropped unexpectedly")?;
            }
            Ok(None) => break,
            Err(err) => {
                // Drop sender to unblock writer, then clean up
                drop(chunk_tx);
                let _ = writer_handle.await;
                let _ = std::fs::remove_file(&tmp_path);
                return Err(err).with_context(|| format!("download stream failed for {name}"));
            }
        }
    }

    // Final progress event at 100%
    let _ = sender.send(DownloadEvent::Progress {
        model: model_name.clone(),
        downloaded,
        total: total_bytes,
    });

    // Drop sender to signal EOF to writer
    drop(chunk_tx);

    // Wait for writer to finish and get hash
    let computed_hash = writer_handle
        .await
        .context("writer task panicked")?
        .with_context(|| format!("writer failed for {name}"))?;

    // Verify checksum
    if computed_hash != expected_sha256 {
        let _ = std::fs::remove_file(&tmp_path);
        let _ = sender.send(DownloadEvent::VerificationFailed {
            model: model_name.clone(),
        });
        return Err(ChecksumMismatch(format!(
            "SHA-256 mismatch for {name}: expected {expected_sha256}, got {computed_hash}"
        ))
        .into());
    }

    // Atomic rename from .tmp to final path
    std::fs::rename(&tmp_path, &final_path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            tmp_path.display(),
            final_path.display()
        )
    })?;

    let _ = sender.send(DownloadEvent::Complete {
        model: model_name,
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_poll_until_ready() {
        let tmp_dir = TempDir::new().expect("create tempdir");
        let _guard = super::super::set_model_dir_override(tmp_dir.path().to_path_buf());
        let dir = model_dir().expect("model_dir should succeed");

        // Create placeholder files for all models so poll_until_ready can eventually succeed.
        for model in super::super::MODELS {
            let path = dir.join(model.filename);
            std::fs::write(&path, b"placeholder").unwrap();
        }

        // Remove one file to simulate a missing model
        let target_model = &super::super::MODELS[0]; // Silero VAD
        let target_path = dir.join(target_model.filename);
        std::fs::remove_file(&target_path).unwrap();

        let downloader = ModelDownloader::new();
        let mut receiver = downloader.subscribe();

        // Spawn poll_until_ready
        let poll_handle = tokio::spawn(async move {
            downloader.poll_until_ready().await
        });

        // Place the file back after 1 second
        let target_path_clone = target_path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(1)).await;
            std::fs::write(&target_path_clone, b"placeholder").unwrap();
        });

        // Wait for DetectedOnDisk event (timeout 12 seconds to cover two poll intervals)
        let mut saw_detected = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
        loop {
            let timeout_result = tokio::time::timeout_at(deadline, receiver.recv()).await;
            match timeout_result {
                Ok(Ok(DownloadEvent::DetectedOnDisk { model })) if model == target_model.name => {
                    saw_detected = true;
                    break;
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Err(_timeout) => break,
                _ => {}
            }
        }

        assert!(saw_detected, "should have received DetectedOnDisk event for VAD model");

        // Poll should complete since all models are now present
        let poll_result: anyhow::Result<()> = poll_handle.await.expect("poll task should not panic");
        assert!(poll_result.is_ok(), "poll_until_ready should succeed");
    }

    #[test]
    fn test_atomic_write() {
        // Simulate the .tmp -> final rename pattern
        let dir = TempDir::new().unwrap();
        let tmp_path = dir.path().join("model.bin.tmp");
        let final_path = dir.path().join("model.bin");

        let mut f = std::fs::File::create(&tmp_path).unwrap();
        f.write_all(b"model data").unwrap();
        f.sync_all().unwrap();
        drop(f);

        assert!(tmp_path.exists());
        assert!(!final_path.exists());

        std::fs::rename(&tmp_path, &final_path).unwrap();

        assert!(!tmp_path.exists());
        assert!(final_path.exists());
        assert_eq!(std::fs::read(&final_path).unwrap(), b"model data".to_vec());
    }

    #[test]
    fn test_atomic_write_cleanup() {
        // Simulate cleanup when verification fails: .tmp is deleted, final never created
        let dir = TempDir::new().unwrap();
        let tmp_path = dir.path().join("corrupt.bin.tmp");
        let final_path = dir.path().join("corrupt.bin");

        let mut f = std::fs::File::create(&tmp_path).unwrap();
        f.write_all(b"corrupt data").unwrap();
        f.sync_all().unwrap();
        drop(f);

        // Simulate checksum failure: delete .tmp, don't rename
        assert!(tmp_path.exists());
        std::fs::remove_file(&tmp_path).unwrap();

        assert!(!tmp_path.exists());
        assert!(!final_path.exists());
    }
}
