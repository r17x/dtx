//! Logging middleware for operation tracing.
//!
//! Provides structured logging for all operations with timing information.

use async_trait::async_trait;
use std::time::Instant;
use tracing::{debug, error, info, trace, warn};

use dtx_core::middleware::{Middleware, Next, Operation, Response};
use dtx_core::resource::{Context, Result};

/// Log level for middleware operations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LogLevel {
    /// Trace level - most verbose.
    Trace,
    /// Debug level.
    Debug,
    /// Info level - default.
    #[default]
    Info,
    /// Warn level.
    Warn,
    /// Error level - least verbose.
    Error,
}

/// Middleware that logs all operations with structured data.
///
/// # Example
///
/// ```ignore
/// use dtx_middleware::LoggingMiddleware;
///
/// let chain = MiddlewareStack::new()
///     .layer(LoggingMiddleware::new())
///     .build(handler);
/// ```
pub struct LoggingMiddleware {
    /// Log level for successful operations.
    success_level: LogLevel,
    /// Log level for failed operations.
    error_level: LogLevel,
    /// Include timing information.
    include_timing: bool,
    /// Include context metadata in logs.
    include_metadata: bool,
}

impl LoggingMiddleware {
    /// Create a new logging middleware with default settings.
    pub fn new() -> Self {
        Self {
            success_level: LogLevel::Info,
            error_level: LogLevel::Error,
            include_timing: true,
            include_metadata: true,
        }
    }

    /// Set the log level for successful operations.
    pub fn success_level(mut self, level: LogLevel) -> Self {
        self.success_level = level;
        self
    }

    /// Set the log level for failed operations.
    pub fn error_level(mut self, level: LogLevel) -> Self {
        self.error_level = level;
        self
    }

    /// Enable or disable timing information.
    pub fn with_timing(mut self, enabled: bool) -> Self {
        self.include_timing = enabled;
        self
    }

    /// Enable or disable context metadata in logs.
    pub fn with_metadata(mut self, enabled: bool) -> Self {
        self.include_metadata = enabled;
        self
    }

    fn log_start(&self, op: &Operation, ctx: &Context) {
        let resource_id = op.resource_id().map(|id| id.as_str());

        info!(
            operation = %op.name(),
            resource = ?resource_id,
            request_id = %ctx.request_id,
            mutating = %op.is_mutating(),
            "Starting operation"
        );
    }

    fn log_success(&self, op: &Operation, ctx: &Context, elapsed_ms: u128) {
        let resource_id = op.resource_id().map(|id| id.as_str());

        match self.success_level {
            LogLevel::Trace => trace!(
                operation = %op.name(),
                resource = ?resource_id,
                elapsed_ms = %elapsed_ms,
                request_id = %ctx.request_id,
                "Operation completed"
            ),
            LogLevel::Debug => debug!(
                operation = %op.name(),
                resource = ?resource_id,
                elapsed_ms = %elapsed_ms,
                request_id = %ctx.request_id,
                "Operation completed"
            ),
            LogLevel::Info => info!(
                operation = %op.name(),
                resource = ?resource_id,
                elapsed_ms = %elapsed_ms,
                request_id = %ctx.request_id,
                "Operation completed"
            ),
            LogLevel::Warn => warn!(
                operation = %op.name(),
                resource = ?resource_id,
                elapsed_ms = %elapsed_ms,
                request_id = %ctx.request_id,
                "Operation completed"
            ),
            LogLevel::Error => error!(
                operation = %op.name(),
                resource = ?resource_id,
                elapsed_ms = %elapsed_ms,
                request_id = %ctx.request_id,
                "Operation completed"
            ),
        }
    }

    fn log_error(
        &self,
        op: &Operation,
        ctx: &Context,
        err: &dtx_core::resource::Error,
        elapsed_ms: u128,
    ) {
        let resource_id = op.resource_id().map(|id| id.as_str());

        error!(
            operation = %op.name(),
            resource = ?resource_id,
            error = %err,
            elapsed_ms = %elapsed_ms,
            request_id = %ctx.request_id,
            retryable = %err.is_retryable(),
            "Operation failed"
        );
    }
}

impl Default for LoggingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for LoggingMiddleware {
    fn name(&self) -> &'static str {
        "logging"
    }

    async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
        let start = Instant::now();

        self.log_start(&op, &ctx);

        let result = next.run(op.clone(), ctx.clone()).await;
        let elapsed_ms = start.elapsed().as_millis();

        match &result {
            Ok(_) => {
                if self.include_timing {
                    self.log_success(&op, &ctx, elapsed_ms);
                }
            }
            Err(err) => {
                self.log_error(&op, &ctx, err, elapsed_ms);
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtx_core::middleware::{Handler, MiddlewareStack, NoopHandler};

    #[tokio::test]
    async fn logging_middleware_success() {
        let chain = MiddlewareStack::new()
            .layer(LoggingMiddleware::new())
            .build(NoopHandler);

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn logging_middleware_with_config() {
        let middleware = LoggingMiddleware::new()
            .success_level(LogLevel::Debug)
            .error_level(LogLevel::Error)
            .with_timing(true)
            .with_metadata(false);

        let chain = MiddlewareStack::new().layer(middleware).build(NoopHandler);

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(result.is_ok());
    }

    struct FailingHandler;

    #[async_trait]
    impl Handler for FailingHandler {
        async fn handle(&self, _op: Operation, _ctx: Context) -> Result<Response> {
            Err(dtx_core::resource::Error::Cancelled)
        }
    }

    #[tokio::test]
    async fn logging_middleware_failure() {
        let chain = MiddlewareStack::new()
            .layer(LoggingMiddleware::new())
            .build(FailingHandler);

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(result.is_err());
    }
}
