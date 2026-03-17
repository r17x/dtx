//! Server-Sent Events (SSE) utilities.

use axum::response::sse::Event;
use dtx_core::events::LifecycleEvent;
use dtx_core::resource::LogStreamKind;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Create SSE event from data
pub fn event(event_type: &str, data: impl serde::Serialize) -> Result<Event, Infallible> {
    Ok(Event::default()
        .event(event_type)
        .data(serde_json::to_string(&data).unwrap_or_default()))
}

/// Track active SSE connections.
///
/// Always stored behind an `Arc` so that `ConnectionGuard` can own a clone
/// and live independently of the handler that created it (fixing the lifetime
/// bug where guards dropped when the handler returned, not when the SSE
/// stream ended).
pub struct ConnectionTracker {
    count: AtomicUsize,
}

impl ConnectionTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            count: AtomicUsize::new(0),
        })
    }

    pub fn connect(self: &Arc<Self>) -> ConnectionGuard {
        let prev = self.count.fetch_add(1, Ordering::SeqCst);
        tracing::debug!("SSE connection opened, total: {}", prev + 1);
        ConnectionGuard {
            tracker: Arc::clone(self),
        }
    }

    pub fn connection_count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

impl Default for ConnectionTracker {
    fn default() -> Self {
        Self {
            count: AtomicUsize::new(0),
        }
    }
}

pub struct ConnectionGuard {
    tracker: Arc<ConnectionTracker>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let prev = self.tracker.count.fetch_sub(1, Ordering::SeqCst);
        tracing::debug!("SSE connection closed, total: {}", prev - 1);
    }
}

/// Log entry for SSE streams, representing a single loggable event.
///
/// Produced by [`lifecycle_to_log_entry`] from lifecycle events.
#[derive(Clone, Debug, serde::Serialize)]
pub struct SseLogEntry {
    pub service: String,
    pub message: String,
    pub level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
}

/// Convert a lifecycle event into a log entry suitable for SSE streaming.
///
/// Returns `None` for events that don't represent loggable activity
/// (health checks, dependency events, config/memory changes).
pub fn lifecycle_to_log_entry(event: &LifecycleEvent) -> Option<SseLogEntry> {
    match event {
        LifecycleEvent::Starting { id, .. } => Some(SseLogEntry {
            service: id.to_string(),
            message: "Service starting".to_string(),
            level: "info".to_string(),
            stream: None,
        }),
        LifecycleEvent::Running { id, .. } => Some(SseLogEntry {
            service: id.to_string(),
            message: "Service running".to_string(),
            level: "info".to_string(),
            stream: None,
        }),
        LifecycleEvent::Stopping { id, .. } => Some(SseLogEntry {
            service: id.to_string(),
            message: "Service stopping".to_string(),
            level: "info".to_string(),
            stream: None,
        }),
        LifecycleEvent::Stopped { id, exit_code, .. } => {
            let message = match exit_code {
                Some(code) => format!("Service stopped (exit code: {})", code),
                None => "Service stopped".to_string(),
            };
            Some(SseLogEntry {
                service: id.to_string(),
                message,
                level: "info".to_string(),
                stream: None,
            })
        }
        LifecycleEvent::Failed { id, error, .. } => Some(SseLogEntry {
            service: id.to_string(),
            message: format!("Service failed: {}", error),
            level: "error".to_string(),
            stream: None,
        }),
        LifecycleEvent::Restarting {
            id,
            attempt,
            max_attempts,
            ..
        } => {
            let max = max_attempts
                .map(|m| m.to_string())
                .unwrap_or_else(|| "unlimited".to_string());
            Some(SseLogEntry {
                service: id.to_string(),
                message: format!("Restarting ({}/{})", attempt, max),
                level: "warn".to_string(),
                stream: None,
            })
        }
        LifecycleEvent::Log {
            id, line, stream, ..
        } => {
            let level = match stream {
                LogStreamKind::Stderr => "error",
                _ => "info",
            };
            Some(SseLogEntry {
                service: id.to_string(),
                message: line.clone(),
                level: level.to_string(),
                stream: Some(stream.as_str().to_string()),
            })
        }
        // Health checks, dependency events, config/memory changes are not log entries
        LifecycleEvent::HealthCheckPassed { .. }
        | LifecycleEvent::HealthCheckFailed { .. }
        | LifecycleEvent::DependencyWaiting { .. }
        | LifecycleEvent::DependencyResolved { .. }
        | LifecycleEvent::ConfigChanged { .. }
        | LifecycleEvent::MemoryChanged { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use dtx_core::resource::{ResourceId, ResourceKind};

    #[test]
    fn test_connection_tracker() {
        let tracker = ConnectionTracker::new();
        assert_eq!(tracker.connection_count(), 0);

        let _guard1 = tracker.connect();
        assert_eq!(tracker.connection_count(), 1);

        let _guard2 = tracker.connect();
        assert_eq!(tracker.connection_count(), 2);

        drop(_guard1);
        assert_eq!(tracker.connection_count(), 1);

        drop(_guard2);
        assert_eq!(tracker.connection_count(), 0);
    }

    #[test]
    fn test_connection_guard_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<ConnectionGuard>();
    }

    #[test]
    fn test_event_creation() {
        #[derive(serde::Serialize)]
        struct TestData {
            message: String,
        }

        let data = TestData {
            message: "test".to_string(),
        };

        let result = event("test-event", &data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_lifecycle_to_log_entry_starting() {
        let event = LifecycleEvent::Starting {
            id: ResourceId::new("api"),
            kind: ResourceKind::Process,
            timestamp: Utc::now(),
        };
        let entry = lifecycle_to_log_entry(&event).unwrap();
        assert_eq!(entry.service, "api");
        assert_eq!(entry.level, "info");
        assert_eq!(entry.message, "Service starting");
        assert!(entry.stream.is_none());
    }

    #[test]
    fn test_lifecycle_to_log_entry_failed() {
        let event = LifecycleEvent::Failed {
            id: ResourceId::new("db"),
            kind: ResourceKind::Process,
            error: "connection refused".to_string(),
            exit_code: Some(1),
            timestamp: Utc::now(),
        };
        let entry = lifecycle_to_log_entry(&event).unwrap();
        assert_eq!(entry.service, "db");
        assert_eq!(entry.level, "error");
        assert!(entry.message.contains("connection refused"));
    }

    #[test]
    fn test_lifecycle_to_log_entry_log_stderr() {
        let event = LifecycleEvent::Log {
            id: ResourceId::new("api"),
            stream: LogStreamKind::Stderr,
            line: "error output".to_string(),
            timestamp: Utc::now(),
        };
        let entry = lifecycle_to_log_entry(&event).unwrap();
        assert_eq!(entry.level, "error");
        assert_eq!(entry.stream.as_deref(), Some("stderr"));
    }

    #[test]
    fn test_lifecycle_to_log_entry_log_stdout() {
        let event = LifecycleEvent::Log {
            id: ResourceId::new("api"),
            stream: LogStreamKind::Stdout,
            line: "normal output".to_string(),
            timestamp: Utc::now(),
        };
        let entry = lifecycle_to_log_entry(&event).unwrap();
        assert_eq!(entry.level, "info");
        assert_eq!(entry.stream.as_deref(), Some("stdout"));
    }

    #[test]
    fn test_lifecycle_to_log_entry_restarting() {
        let event = LifecycleEvent::Restarting {
            id: ResourceId::new("api"),
            kind: ResourceKind::Process,
            attempt: 2,
            max_attempts: Some(5),
            timestamp: Utc::now(),
        };
        let entry = lifecycle_to_log_entry(&event).unwrap();
        assert_eq!(entry.level, "warn");
        assert!(entry.message.contains("2/5"));
    }

    #[test]
    fn test_lifecycle_to_log_entry_stopped_with_code() {
        let event = LifecycleEvent::Stopped {
            id: ResourceId::new("api"),
            kind: ResourceKind::Process,
            exit_code: Some(137),
            timestamp: Utc::now(),
        };
        let entry = lifecycle_to_log_entry(&event).unwrap();
        assert!(entry.message.contains("137"));
    }

    #[test]
    fn test_lifecycle_to_log_entry_none_for_health() {
        let event = LifecycleEvent::HealthCheckPassed {
            id: ResourceId::new("api"),
            timestamp: Utc::now(),
        };
        assert!(lifecycle_to_log_entry(&event).is_none());
    }

    #[test]
    fn test_lifecycle_to_log_entry_none_for_config() {
        let event = LifecycleEvent::ConfigChanged {
            project_id: "proj".to_string(),
            timestamp: Utc::now(),
        };
        assert!(lifecycle_to_log_entry(&event).is_none());
    }

    #[test]
    fn test_lifecycle_to_log_entry_none_for_memory() {
        let event = LifecycleEvent::MemoryChanged {
            project_id: "proj".to_string(),
            memory_name: "test".to_string(),
            timestamp: Utc::now(),
        };
        assert!(lifecycle_to_log_entry(&event).is_none());
    }
}
