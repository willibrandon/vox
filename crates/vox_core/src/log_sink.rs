//! Log capture layer for routing tracing events to the UI.
//!
//! Provides [`LogSink`], a [`tracing_subscriber::Layer`] that formats events
//! into [`LogEntry`] structs and sends them over an unbounded mpsc channel.
//! The UI receives entries via [`LogReceiver`] to display in the log panel.

use std::fmt;

use tokio::sync::mpsc;
use tracing::field::{Field, Visit};

/// A single captured log event for display in the log panel.
#[derive(Clone, Debug)]
pub struct LogEntry {
    /// ISO 8601 timestamp when the event was captured.
    pub timestamp: String,
    /// Severity level of the log event.
    pub level: LogLevel,
    /// Module path or target that emitted the event.
    pub target: String,
    /// Human-readable log message.
    pub message: String,
}

/// Log severity levels ordered from most to least severe.
///
/// Ordering: Error < Warn < Info < Debug < Trace. A filter level of `Info`
/// shows Error, Warn, and Info events (everything at or above Info severity).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    /// Critical errors requiring attention.
    Error = 0,
    /// Warning conditions that may need investigation.
    Warn = 1,
    /// Informational messages about normal operation.
    Info = 2,
    /// Detailed debugging information.
    Debug = 3,
    /// Very verbose trace-level information.
    Trace = 4,
}

impl PartialOrd for LogLevel {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LogLevel {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Error => write!(f, "ERROR"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Trace => write!(f, "TRACE"),
        }
    }
}

impl LogLevel {
    /// Convert from tracing's Level type.
    pub fn from_tracing(level: &tracing::Level) -> Self {
        match *level {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warn,
            tracing::Level::INFO => LogLevel::Info,
            tracing::Level::DEBUG => LogLevel::Debug,
            tracing::Level::TRACE => LogLevel::Trace,
        }
    }
}

/// Visitor that extracts the message field from a tracing event.
struct MessageVisitor {
    message: String,
}

impl MessageVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
        }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else if self.message.is_empty() {
            self.message = format!("{} = {value:?}", field.name());
        } else {
            self.message
                .push_str(&format!(", {} = {value:?}", field.name()));
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else if self.message.is_empty() {
            self.message = format!("{} = {value}", field.name());
        } else {
            self.message
                .push_str(&format!(", {} = {value}", field.name()));
        }
    }
}

/// Tracing layer that captures events and sends them to the UI.
///
/// Created via [`LogSink::new`] which returns both the layer and a
/// [`LogReceiver`] for the UI to consume.
pub struct LogSink {
    sender: mpsc::UnboundedSender<LogEntry>,
}

/// Receiver end of the log channel for UI consumption.
///
/// Wraps a `tokio::sync::mpsc::UnboundedReceiver<LogEntry>`.
/// Passed to the log panel which polls it for new entries.
pub struct LogReceiver {
    /// The receiving end of the log channel.
    pub rx: mpsc::UnboundedReceiver<LogEntry>,
}

impl LogSink {
    /// Create a new log sink and its paired receiver.
    ///
    /// The sink implements `tracing_subscriber::Layer` and should be added
    /// to the subscriber. The receiver should be passed to `VoxState` for
    /// the log panel to consume.
    pub fn new() -> (Self, LogReceiver) {
        let (sender, rx) = mpsc::unbounded_channel();
        (Self { sender }, LogReceiver { rx })
    }
}

impl<S> tracing_subscriber::Layer<S> for LogSink
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: chrono_now(),
            level: LogLevel::from_tracing(metadata.level()),
            target: metadata.target().to_string(),
            message: visitor.message,
        };

        // Ignore send errors — receiver may have been dropped if the
        // settings window was closed.
        let _ = self.sender.send(entry);
    }
}

/// Get current UTC timestamp as ISO 8601 string without external chrono dependency.
fn chrono_now() -> String {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Simple UTC timestamp formatting
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Days since epoch to date (simplified Gregorian calendar)
    let mut y = 1970i64;
    let mut remaining_days = days as i64;

    loop {
        let year_days = if is_leap(y) { 366 } else { 365 };
        if remaining_days < year_days {
            break;
        }
        remaining_days -= year_days;
        y += 1;
    }

    let leap = is_leap(y);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut m = 0usize;
    for (i, &md) in month_days.iter().enumerate() {
        if remaining_days < md {
            m = i;
            break;
        }
        remaining_days -= md;
    }

    format!(
        "{y:04}-{:02}-{:02}T{hours:02}:{minutes:02}:{seconds:02}Z",
        m + 1,
        remaining_days + 1
    )
}

/// Check if a year is a leap year.
fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_level_ordering() {
        assert!(LogLevel::Error < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Debug);
        assert!(LogLevel::Debug < LogLevel::Trace);
    }

    #[test]
    fn test_log_level_display() {
        assert_eq!(LogLevel::Error.to_string(), "ERROR");
        assert_eq!(LogLevel::Warn.to_string(), "WARN");
        assert_eq!(LogLevel::Info.to_string(), "INFO");
        assert_eq!(LogLevel::Debug.to_string(), "DEBUG");
        assert_eq!(LogLevel::Trace.to_string(), "TRACE");
    }

    #[test]
    fn test_log_level_filter() {
        let filter = LogLevel::Info;
        assert!(LogLevel::Error <= filter, "Error should pass Info filter");
        assert!(LogLevel::Warn <= filter, "Warn should pass Info filter");
        assert!(LogLevel::Info <= filter, "Info should pass Info filter");
        assert!(LogLevel::Debug > filter, "Debug should NOT pass Info filter");
        assert!(LogLevel::Trace > filter, "Trace should NOT pass Info filter");
    }

    #[test]
    fn test_log_sink_sends_entries() {
        let (sink, mut receiver) = LogSink::new();
        let entry = LogEntry {
            timestamp: "2026-02-24T10:00:00Z".into(),
            level: LogLevel::Info,
            target: "test".into(),
            message: "hello".into(),
        };
        let _ = sink.sender.send(entry);
        let received = receiver.rx.try_recv().expect("should receive entry");
        assert_eq!(received.message, "hello");
        assert_eq!(received.level, LogLevel::Info);
    }

    #[test]
    fn test_chrono_now_format() {
        let ts = chrono_now();
        assert!(ts.ends_with('Z'), "timestamp should end with Z: {ts}");
        assert!(ts.contains('T'), "timestamp should contain T separator: {ts}");
        assert_eq!(ts.len(), 20, "timestamp should be 20 chars: {ts}");
    }
}
