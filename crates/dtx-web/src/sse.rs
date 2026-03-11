//! Server-Sent Events (SSE) utilities.

use axum::response::sse::Event;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Create SSE event from data
pub fn event(event_type: &str, data: impl serde::Serialize) -> Result<Event, Infallible> {
    Ok(Event::default()
        .event(event_type)
        .data(serde_json::to_string(&data).unwrap_or_default()))
}

/// Keepalive interval for SSE connections
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);

/// Track active SSE connections
pub struct ConnectionTracker {
    count: AtomicUsize,
}

impl ConnectionTracker {
    pub fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
        }
    }

    pub fn connect(&self) -> ConnectionGuard<'_> {
        let prev = self.count.fetch_add(1, Ordering::SeqCst);
        tracing::debug!("SSE connection opened, total: {}", prev + 1);
        ConnectionGuard { tracker: self }
    }

    pub fn connection_count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

impl Default for ConnectionTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ConnectionGuard<'a> {
    tracker: &'a ConnectionTracker,
}

impl<'a> Drop for ConnectionGuard<'a> {
    fn drop(&mut self) {
        let prev = self.tracker.count.fetch_sub(1, Ordering::SeqCst);
        tracing::debug!("SSE connection closed, total: {}", prev - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
