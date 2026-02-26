//! Structured logging with daily file rotation and retention cleanup.
//!
//! Provides [`init_logging`] to set up tracing with daily rotating log files
//! and [`cleanup_old_logs`] to delete log files older than a specified number
//! of days. Log directory is platform-specific via [`log_dir`].

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::log_sink::{LogReceiver, LogSink};

/// Guard that flushes pending log entries when dropped.
///
/// Must be held for the application lifetime. Store in a `let _guard`
/// binding in `main()`.
pub struct LoggingGuard {
    _guard: WorkerGuard,
}

/// Shared state for tracking cumulative bytes written and daily reset.
struct SizeCappedState {
    /// Total bytes written to the file since the last daily reset.
    bytes_written: AtomicU64,
    /// Maximum bytes allowed per day before silent discard.
    max_bytes: u64,
    /// Day-of-epoch when `bytes_written` was last reset.
    current_date: AtomicU32,
}

/// A writer wrapper that enforces a per-day byte cap on log output.
///
/// Once `max_bytes` are written in a single calendar day, subsequent writes
/// are silently discarded (the caller sees `Ok(buf.len())` so tracing never
/// errors). The counter resets automatically at the start of each new day
/// (aligned with `tracing-appender`'s daily rotation).
///
/// This prevents runaway logging from consuming unbounded disk space while
/// preserving all events in the in-memory [`LogSink`] (UI log panel).
#[derive(Clone)]
pub struct SizeCappedWriter<W> {
    inner: W,
    state: Arc<SizeCappedState>,
}

impl<W> SizeCappedWriter<W> {
    /// Create a new size-capped writer wrapping `inner` with the given byte limit.
    pub fn new(inner: W, max_bytes: u64) -> Self {
        Self {
            inner,
            state: Arc::new(SizeCappedState {
                bytes_written: AtomicU64::new(0),
                max_bytes,
                current_date: AtomicU32::new(day_of_epoch()),
            }),
        }
    }

    /// Total bytes written since the last daily reset.
    #[cfg(test)]
    fn bytes_written(&self) -> u64 {
        self.state.bytes_written.load(Ordering::Relaxed)
    }
}

impl<W: Write> Write for SizeCappedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Daily reset: if the calendar day changed, reset the counter.
        let today = day_of_epoch();
        let stored = self.state.current_date.load(Ordering::Relaxed);
        if today != stored {
            self.state.current_date.store(today, Ordering::Relaxed);
            self.state.bytes_written.store(0, Ordering::Relaxed);
        }

        let current = self.state.bytes_written.load(Ordering::Relaxed);
        if current >= self.state.max_bytes {
            return Ok(buf.len()); // Silently discard
        }

        let written = self.inner.write(buf)?;
        self.state
            .bytes_written
            .fetch_add(written as u64, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<'a, W: Write + Clone + 'static> MakeWriter<'a> for SizeCappedWriter<W> {
    type Writer = SizeCappedWriter<W>;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Current day number since Unix epoch (for daily counter reset).
fn day_of_epoch() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86400) as u32
}

/// Initialize structured logging with daily file rotation and UI log capture.
///
/// Creates the log directory if needed, sets up a daily rotating file appender,
/// configures env-filter (`VOX_LOG` > `RUST_LOG` > default), adds a [`LogSink`]
/// layer for routing events to the log panel, and cleans up logs older than 7
/// days. Returns a guard (must be held for the application lifetime) and a
/// [`LogReceiver`] to pass to `VoxState` for the log panel.
pub fn init_logging() -> (LoggingGuard, LogReceiver) {
    let dir = log_dir();
    fs::create_dir_all(&dir).ok();

    let file_appender = tracing_appender::rolling::daily(&dir, "vox");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // 10 MB daily cap — silently discards file writes past the limit.
    // The in-memory LogSink (UI panel) is unaffected and receives all events.
    let size_capped = SizeCappedWriter::new(non_blocking, 10 * 1024 * 1024);

    let filter = std::env::var("VOX_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .map(|val| EnvFilter::new(val))
        .unwrap_or_else(|_| {
            EnvFilter::new("info,vox=info,vox_core=info,vox_ui=info")
        });

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(size_capped);

    let (log_sink, log_receiver) = LogSink::new();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(log_sink)
        .init();

    cleanup_old_logs(&dir, 7);

    (LoggingGuard { _guard: guard }, log_receiver)
}

/// Platform-specific log directory path.
///
/// Windows: `%LOCALAPPDATA%/com.vox.app/logs/`
/// macOS: `~/Library/Logs/com.vox.app/`
pub fn log_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir()
            .expect("LOCALAPPDATA not available")
            .join("com.vox.app")
            .join("logs")
    }
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .expect("HOME not available")
            .join("Library/Logs/com.vox.app")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        dirs::data_local_dir()
            .expect("data directory not available")
            .join("com.vox.app")
            .join("logs")
    }
}

/// Delete log files older than `retention_days` from the given directory.
///
/// Scans for files matching the `vox.YYYY-MM-DD` naming pattern (created by
/// tracing-appender daily rotation) and removes those whose date stamp is
/// older than `retention_days` from today. Non-log files are left untouched.
pub fn cleanup_old_logs(dir: &Path, retention_days: u32) {
    let Some(cutoff) = cutoff_date_string(retention_days) else {
        return;
    };

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) => {
            tracing::warn!(dir = %dir.display(), %err, "failed to read log directory for cleanup");
            return;
        }
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();

        // tracing-appender daily creates files named "vox.YYYY-MM-DD"
        if let Some(date_str) = name.strip_prefix("vox.") {
            if date_str.len() == 10 && date_str < cutoff.as_str() {
                if let Err(err) = fs::remove_file(entry.path()) {
                    tracing::warn!(
                        path = %entry.path().display(), %err,
                        "failed to remove old log file"
                    );
                }
            }
        }
    }
}

/// Compute the YYYY-MM-DD string for `days` days ago without a chrono dependency.
///
/// Uses Howard Hinnant's civil date algorithm to convert epoch days to calendar date.
fn cutoff_date_string(days: u32) -> Option<String> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let target_secs = now.as_secs().checked_sub(u64::from(days) * 86400)?;
    let total_days = (target_secs / 86400) as i64;

    let z = total_days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    Some(format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::sync::Mutex;

    /// A shared in-memory buffer that implements Write + Clone for testing.
    #[derive(Clone)]
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(Vec::new())))
        }

        fn len(&self) -> usize {
            self.0.lock().expect("lock").len()
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_log_size_cap() {
        let buf = SharedBuf::new();
        let cap = 1024; // 1 KB cap for test speed
        let mut writer = SizeCappedWriter::new(buf.clone(), cap);

        // Write 512 bytes — should succeed
        let chunk = vec![b'A'; 512];
        let written = writer.write(&chunk).expect("write");
        assert_eq!(written, 512);
        assert_eq!(buf.len(), 512);
        assert_eq!(writer.bytes_written(), 512);

        // Write another 512 bytes — should succeed (exactly at cap)
        let written = writer.write(&chunk).expect("write");
        assert_eq!(written, 512);
        assert_eq!(buf.len(), 1024);

        // Write another 512 bytes — should be silently discarded
        let written = writer.write(&chunk).expect("write");
        assert_eq!(written, 512, "should report success even when discarding");
        assert_eq!(buf.len(), 1024, "buffer should not grow past cap");

        // Write many more times — all silently discarded
        for _ in 0..100 {
            let written = writer.write(&chunk).expect("write");
            assert_eq!(written, 512);
        }
        assert_eq!(buf.len(), 1024, "buffer should still be at cap after many writes");
    }

    #[test]
    fn test_log_dir_platform() {
        let dir = log_dir();
        let path_str = dir.to_string_lossy();
        assert!(
            path_str.contains("com.vox.app"),
            "log dir should contain com.vox.app, got: {path_str}"
        );
        #[cfg(target_os = "windows")]
        assert!(
            path_str.ends_with("logs"),
            "Windows log dir should end with 'logs', got: {path_str}"
        );
    }

    #[test]
    fn test_cleanup_old_logs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path();

        // Create files matching tracing-appender daily pattern: vox.YYYY-MM-DD
        let test_files = [
            "vox.2020-01-01",        // very old — should be deleted
            "vox.2020-06-15",        // very old — should be deleted
            "vox.9999-12-30",        // far future — should be retained
            "vox.9999-12-31",        // far future — should be retained
            "not-a-log-file.txt",    // non-log — should be untouched
            "vox.short",             // wrong date length — should be untouched
        ];

        for name in &test_files {
            File::create(path.join(name)).expect("create test file");
        }

        cleanup_old_logs(path, 7);

        assert!(!path.join("vox.2020-01-01").exists(), "old file should be deleted");
        assert!(!path.join("vox.2020-06-15").exists(), "old file should be deleted");
        assert!(path.join("vox.9999-12-30").exists(), "future file should be retained");
        assert!(path.join("vox.9999-12-31").exists(), "future file should be retained");
        assert!(path.join("not-a-log-file.txt").exists(), "non-log file should be untouched");
        assert!(path.join("vox.short").exists(), "wrong-length date should be untouched");
    }
}
