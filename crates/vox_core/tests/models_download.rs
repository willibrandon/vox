//! Integration tests for the model download engine.
//!
//! Downloads the real Silero VAD model (~2.3 MB) to verify end-to-end
//! streaming, SHA-256 verification, and atomic file writes.
//! All tests use a temporary directory override to avoid mutating real model cache.

use std::time::Duration;

use tempfile::TempDir;
use vox_core::models::{self, DownloadEvent, ModelDownloader, set_model_dir_override};

/// Download the VAD model to a tempdir, verify checksum matches and file exists
/// at the final path (not .tmp).
#[tokio::test]
async fn test_download_small_model() {
    let tmp_dir = TempDir::new().expect("create tempdir");
    let _guard = set_model_dir_override(tmp_dir.path().to_path_buf());

    let vad = &models::MODELS[0]; // Silero VAD, ~2.3 MB
    let dir = models::model_dir().expect("model_dir should succeed");
    let final_path = dir.join(vad.filename);
    let tmp_path = dir.join(format!("{}.tmp", vad.filename));

    let downloader = ModelDownloader::new();

    let result: anyhow::Result<()> = tokio::time::timeout(
        Duration::from_secs(120),
        downloader.download_missing(&[vad]),
    )
    .await
    .expect("download timed out after 120s");

    assert!(result.is_ok(), "download should succeed: {result:?}");

    assert!(final_path.exists(), "final model file should exist");
    assert!(!tmp_path.exists(), ".tmp file should not remain");

    assert!(
        models::verify_checksum(&final_path, vad.sha256).expect("checksum should succeed"),
        "SHA-256 should match"
    );
}

/// Download two copies of the VAD model concurrently (different filenames, same URL)
/// to verify that download_missing actually spawns parallel tasks.
/// Checks that Started events for both models arrive before either completes.
#[tokio::test]
async fn test_concurrent_download() {
    let tmp_dir = TempDir::new().expect("create tempdir");
    let _guard = set_model_dir_override(tmp_dir.path().to_path_buf());

    // Two ModelInfo entries pointing at the same small file but with different filenames
    let model_a = models::ModelInfo {
        name: "Concurrent Test A",
        filename: "concurrent_a.onnx",
        url: models::MODELS[0].url,
        sha256: models::MODELS[0].sha256,
        size_bytes: models::MODELS[0].size_bytes,
    };
    let model_b = models::ModelInfo {
        name: "Concurrent Test B",
        filename: "concurrent_b.onnx",
        url: models::MODELS[0].url,
        sha256: models::MODELS[0].sha256,
        size_bytes: models::MODELS[0].size_bytes,
    };

    let downloader = ModelDownloader::new();
    let mut receiver = downloader.subscribe();

    // Spawn download of both models
    let download_handle = tokio::spawn(async move {
        downloader.download_missing(&[&model_a, &model_b]).await
    });

    // Collect events — track ordering of Started vs Complete
    let event_handle = tokio::spawn(async move {
        let mut started_a = false;
        let mut started_b = false;
        let mut complete_a = false;
        let mut complete_b = false;
        let mut both_started_before_any_complete = false;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
        loop {
            let timeout_result = tokio::time::timeout_at(deadline, receiver.recv()).await;
            match timeout_result {
                Ok(Ok(DownloadEvent::Started { ref model, .. })) => {
                    if model == "Concurrent Test A" {
                        started_a = true;
                    } else if model == "Concurrent Test B" {
                        started_b = true;
                    }
                    // Check if both started before any completed
                    if started_a && started_b && !complete_a && !complete_b {
                        both_started_before_any_complete = true;
                    }
                }
                Ok(Ok(DownloadEvent::Complete { ref model })) => {
                    if model == "Concurrent Test A" {
                        complete_a = true;
                    } else if model == "Concurrent Test B" {
                        complete_b = true;
                    }
                    if complete_a && complete_b {
                        break;
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Err(_timeout) => break,
                _ => {}
            }
        }
        (started_a, started_b, complete_a, complete_b, both_started_before_any_complete)
    });

    let download_result: anyhow::Result<()> = tokio::time::timeout(
        Duration::from_secs(120),
        async { download_handle.await.expect("download task should not panic") },
    )
    .await
    .expect("download timed out after 120s");

    assert!(
        download_result.is_ok(),
        "download should succeed: {download_result:?}"
    );

    let (started_a, started_b, complete_a, complete_b, both_started_before_any_complete) =
        event_handle.await.expect("event collector should not panic");

    assert!(started_a, "should have received Started event for model A");
    assert!(started_b, "should have received Started event for model B");
    assert!(complete_a, "should have received Complete event for model A");
    assert!(complete_b, "should have received Complete event for model B");
    assert!(
        both_started_before_any_complete,
        "both downloads should start before either completes (proving concurrency)"
    );
}

/// Simulate download failure (invalid URL), verify Failed event with error message.
/// DNS/HTTP errors should produce Failed, not VerificationFailed.
#[tokio::test]
async fn test_resume_after_failure() {
    let tmp_dir = TempDir::new().expect("create tempdir");
    let _guard = set_model_dir_override(tmp_dir.path().to_path_buf());

    let fake_model = models::ModelInfo {
        name: "Test Fake Model",
        filename: "test_fake_model.bin",
        url: "https://invalid.example.com/nonexistent-model.bin",
        sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        size_bytes: 100,
    };

    let downloader = ModelDownloader::new();
    let mut receiver = downloader.subscribe();

    // Download should fail (DNS/HTTP error, not checksum mismatch)
    let result: anyhow::Result<()> = tokio::time::timeout(
        Duration::from_secs(120),
        downloader.download_missing(&[&fake_model]),
    )
    .await
    .expect("download timed out after 120s");

    assert!(result.is_err(), "download with invalid URL should fail");

    // Check that we got a Failed event (no VerificationFailed — this is a network error)
    let mut saw_failed = false;
    let mut saw_verification_failed = false;
    loop {
        match receiver.try_recv() {
            Ok(DownloadEvent::Failed { model, error }) => {
                assert_eq!(model, "Test Fake Model");
                assert!(!error.is_empty(), "error message should not be empty");
                saw_failed = true;
            }
            Ok(DownloadEvent::VerificationFailed { .. }) => {
                saw_verification_failed = true;
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            _ => {}
        }
    }
    assert!(saw_failed, "should have received Failed event");
    assert!(
        !saw_verification_failed,
        "DNS/HTTP errors should not emit VerificationFailed"
    );
}
