//! Request context for resource operations.
//!
//! Context carries metadata through the system including request IDs,
//! distributed tracing information, deadlines, and cancellation tokens.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Request context carrying metadata through the system.
///
/// Context provides request-scoped information like deadlines,
/// tracing IDs, and metadata for resource operations.
///
/// # Cancellation
///
/// Context supports cooperative cancellation via a notify token.
/// Use `cancel()` to signal cancellation and `cancelled()` to wait for it.
///
/// # Example
///
/// ```
/// use dtx_core::resource::Context;
/// use std::time::Duration;
///
/// let ctx = Context::with_timeout(Duration::from_secs(30))
///     .with_trace("trace-123", "span-456")
///     .with_metadata("user", "alice");
///
/// assert!(!ctx.is_expired());
/// ```
#[derive(Clone, Debug)]
pub struct Context {
    /// Unique request/operation ID.
    pub request_id: String,
    /// Trace ID for distributed tracing.
    pub trace_id: Option<String>,
    /// Span ID for distributed tracing.
    pub span_id: Option<String>,
    /// Operation start time.
    started_at: Instant,
    /// Operation deadline.
    deadline: Option<Instant>,
    /// Custom metadata.
    metadata: HashMap<String, String>,
    /// Cancellation token.
    cancellation: Arc<tokio::sync::Notify>,
}

impl Context {
    /// Create a new context with a generated request ID.
    pub fn new() -> Self {
        Self {
            request_id: Self::generate_id(),
            trace_id: None,
            span_id: None,
            started_at: Instant::now(),
            deadline: None,
            metadata: HashMap::new(),
            cancellation: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Create a context with a timeout.
    pub fn with_timeout(timeout: Duration) -> Self {
        let mut ctx = Self::new();
        ctx.deadline = Some(Instant::now() + timeout);
        ctx
    }

    /// Set a custom request ID.
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = id.into();
        self
    }

    /// Set trace and span IDs for distributed tracing.
    pub fn with_trace(mut self, trace_id: impl Into<String>, span_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self.span_id = Some(span_id.into());
        self
    }

    /// Add metadata to the context.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Check if context has timed out.
    pub fn is_expired(&self) -> bool {
        self.deadline.map(|d| Instant::now() > d).unwrap_or(false)
    }

    /// Get remaining time until deadline.
    pub fn remaining(&self) -> Option<Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
    }

    /// Elapsed time since context creation.
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Get metadata value.
    pub fn get_metadata(&self, key: &str) -> Option<&str> {
        self.metadata.get(key).map(|s| s.as_str())
    }

    /// Get all metadata as a reference.
    pub fn metadata(&self) -> &HashMap<String, String> {
        &self.metadata
    }

    /// Cancel this context.
    ///
    /// Signals any waiters on `cancelled()` to wake up.
    pub fn cancel(&self) {
        self.cancellation.notify_waiters();
    }

    /// Wait for cancellation.
    ///
    /// Returns when `cancel()` is called on this context or any clone.
    pub async fn cancelled(&self) {
        self.cancellation.notified().await
    }

    fn generate_id() -> String {
        use std::time::SystemTime;
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{:x}", ts)
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn context_new() {
        let ctx = Context::new();
        assert!(!ctx.request_id.is_empty());
        assert!(!ctx.is_expired());
    }

    #[test]
    fn context_with_timeout() {
        let ctx = Context::with_timeout(Duration::from_millis(100));
        assert!(!ctx.is_expired());
        thread::sleep(Duration::from_millis(150));
        assert!(ctx.is_expired());
    }

    #[test]
    fn context_remaining() {
        let ctx = Context::with_timeout(Duration::from_secs(10));
        let remaining = ctx.remaining().unwrap();
        assert!(remaining > Duration::from_secs(9));
        assert!(remaining <= Duration::from_secs(10));
    }

    #[test]
    fn context_metadata() {
        let ctx = Context::new()
            .with_metadata("user", "alice")
            .with_metadata("tenant", "acme");
        assert_eq!(ctx.get_metadata("user"), Some("alice"));
        assert_eq!(ctx.get_metadata("tenant"), Some("acme"));
        assert_eq!(ctx.get_metadata("missing"), None);
    }

    #[test]
    fn context_elapsed() {
        let ctx = Context::new();
        thread::sleep(Duration::from_millis(10));
        assert!(ctx.elapsed() >= Duration::from_millis(10));
    }

    #[test]
    fn context_custom_request_id() {
        let ctx = Context::new().with_request_id("custom-123");
        assert_eq!(ctx.request_id, "custom-123");
    }

    #[test]
    fn context_with_trace() {
        let ctx = Context::new().with_trace("trace-abc", "span-xyz");
        assert_eq!(ctx.trace_id.as_deref(), Some("trace-abc"));
        assert_eq!(ctx.span_id.as_deref(), Some("span-xyz"));
    }

    #[tokio::test]
    async fn context_cancellation() {
        let ctx = Context::new();
        let ctx_clone = ctx.clone();

        let handle = tokio::spawn(async move {
            ctx_clone.cancelled().await;
            true
        });

        // Small delay to ensure the spawn has started
        tokio::time::sleep(Duration::from_millis(10)).await;

        ctx.cancel();
        assert!(handle.await.unwrap());
    }

    #[tokio::test]
    async fn context_cancellation_multiple_waiters() {
        let ctx = Context::new();
        let ctx1 = ctx.clone();
        let ctx2 = ctx.clone();

        let h1 = tokio::spawn(async move {
            ctx1.cancelled().await;
            1
        });
        let h2 = tokio::spawn(async move {
            ctx2.cancelled().await;
            2
        });

        tokio::time::sleep(Duration::from_millis(10)).await;
        ctx.cancel();

        assert_eq!(h1.await.unwrap(), 1);
        assert_eq!(h2.await.unwrap(), 2);
    }
}
