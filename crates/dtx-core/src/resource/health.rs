//! Health status and log streaming types for resources.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Health status of a resource.
///
/// Resources can report their health status through health checks.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum HealthStatus {
    /// Resource is healthy and functioning normally.
    Healthy,

    /// Resource is unhealthy.
    Unhealthy {
        /// Reason for unhealthy status.
        reason: String,
    },

    /// Health status is unknown (no health check configured or not yet checked).
    #[default]
    Unknown,
}

impl HealthStatus {
    /// Check if the resource is healthy.
    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
    }

    /// Check if the resource is unhealthy.
    pub fn is_unhealthy(&self) -> bool {
        matches!(self, Self::Unhealthy { .. })
    }

    /// Check if the health status is unknown.
    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown)
    }

    /// Get the unhealthy reason if applicable.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Unhealthy { reason } => Some(reason),
            _ => None,
        }
    }
}

impl std::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Healthy => write!(f, "healthy"),
            Self::Unhealthy { reason } => write!(f, "unhealthy: {}", reason),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Stream type for log output.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum LogStreamKind {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

impl LogStreamKind {
    /// Get a string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

impl std::fmt::Display for LogStreamKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A log entry from a resource.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEntry {
    /// When the log was captured.
    pub timestamp: DateTime<Utc>,
    /// Which stream the log came from.
    pub stream: LogStreamKind,
    /// The log line content.
    pub line: String,
}

impl LogEntry {
    /// Create a new log entry with the current timestamp.
    pub fn new(stream: LogStreamKind, line: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            stream,
            line: line.into(),
        }
    }

    /// Create a stdout log entry.
    pub fn stdout(line: impl Into<String>) -> Self {
        Self::new(LogStreamKind::Stdout, line)
    }

    /// Create a stderr log entry.
    pub fn stderr(line: impl Into<String>) -> Self {
        Self::new(LogStreamKind::Stderr, line)
    }
}

/// Trait for streaming logs from a resource.
///
/// Implementations should provide non-blocking log retrieval.
pub trait LogStream: Send {
    /// Try to receive the next log entry without blocking.
    ///
    /// Returns `None` if no log is available or the stream is closed.
    fn try_recv(&mut self) -> Option<LogEntry>;

    /// Check if the log stream is still open.
    fn is_open(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_status_is_healthy() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Unhealthy {
            reason: "test".into()
        }
        .is_healthy());
        assert!(!HealthStatus::Unknown.is_healthy());
    }

    #[test]
    fn health_status_is_unhealthy() {
        assert!(HealthStatus::Unhealthy {
            reason: "test".into()
        }
        .is_unhealthy());
        assert!(!HealthStatus::Healthy.is_unhealthy());
    }

    #[test]
    fn health_status_reason() {
        let status = HealthStatus::Unhealthy {
            reason: "connection failed".into(),
        };
        assert_eq!(status.reason(), Some("connection failed"));
        assert_eq!(HealthStatus::Healthy.reason(), None);
    }

    #[test]
    fn health_status_default() {
        assert_eq!(HealthStatus::default(), HealthStatus::Unknown);
    }

    #[test]
    fn health_status_display() {
        assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
        assert_eq!(
            HealthStatus::Unhealthy {
                reason: "test".into()
            }
            .to_string(),
            "unhealthy: test"
        );
    }

    #[test]
    fn health_status_serde() {
        let healthy = HealthStatus::Healthy;
        let json = serde_json::to_value(&healthy).unwrap();
        assert_eq!(json["status"], "healthy");

        let unhealthy = HealthStatus::Unhealthy {
            reason: "timeout".into(),
        };
        let json = serde_json::to_value(&unhealthy).unwrap();
        assert_eq!(json["status"], "unhealthy");
        assert_eq!(json["reason"], "timeout");
    }

    #[test]
    fn log_stream_kind_as_str() {
        assert_eq!(LogStreamKind::Stdout.as_str(), "stdout");
        assert_eq!(LogStreamKind::Stderr.as_str(), "stderr");
    }

    #[test]
    fn log_stream_kind_display() {
        assert_eq!(LogStreamKind::Stdout.to_string(), "stdout");
        assert_eq!(LogStreamKind::Stderr.to_string(), "stderr");
    }

    #[test]
    fn log_entry_new() {
        let entry = LogEntry::new(LogStreamKind::Stdout, "hello");
        assert_eq!(entry.stream, LogStreamKind::Stdout);
        assert_eq!(entry.line, "hello");
    }

    #[test]
    fn log_entry_stdout() {
        let entry = LogEntry::stdout("test output");
        assert_eq!(entry.stream, LogStreamKind::Stdout);
        assert_eq!(entry.line, "test output");
    }

    #[test]
    fn log_entry_stderr() {
        let entry = LogEntry::stderr("error message");
        assert_eq!(entry.stream, LogStreamKind::Stderr);
        assert_eq!(entry.line, "error message");
    }

    #[test]
    fn log_entry_serde() {
        let entry = LogEntry::stdout("test");
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: LogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.stream, entry.stream);
        assert_eq!(parsed.line, entry.line);
    }
}
