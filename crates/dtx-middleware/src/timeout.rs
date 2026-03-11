//! Timeout middleware for enforcing operation deadlines.

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

use dtx_core::middleware::{Middleware, Next, Operation, Response};
use dtx_core::resource::{Context, Error, Result};

/// Middleware that enforces operation timeouts.
///
/// # Example
///
/// ```ignore
/// use dtx_middleware::TimeoutMiddleware;
/// use std::time::Duration;
///
/// let chain = MiddlewareStack::new()
///     .layer(TimeoutMiddleware::new(Duration::from_secs(30)))
///     .build(handler);
/// ```
pub struct TimeoutMiddleware {
    default_timeout: Duration,
    operation_timeouts: HashMap<String, Duration>,
}

impl TimeoutMiddleware {
    /// Create a new timeout middleware with the given default timeout.
    pub fn new(default_timeout: Duration) -> Self {
        Self {
            default_timeout,
            operation_timeouts: HashMap::new(),
        }
    }

    /// Set a custom timeout for a specific operation.
    pub fn operation_timeout(mut self, operation: &str, timeout: Duration) -> Self {
        self.operation_timeouts
            .insert(operation.to_string(), timeout);
        self
    }

    fn get_timeout(&self, op: &Operation) -> Duration {
        self.operation_timeouts
            .get(op.name())
            .copied()
            .unwrap_or(self.default_timeout)
    }
}

#[async_trait]
impl Middleware for TimeoutMiddleware {
    fn name(&self) -> &'static str {
        "timeout"
    }

    async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
        let timeout_duration = self.get_timeout(&op);

        match timeout(timeout_duration, next.run(op, ctx)).await {
            Ok(result) => result,
            Err(_) => Err(Error::Timeout {
                elapsed: timeout_duration,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtx_core::middleware::{Handler, MiddlewareStack, NoopHandler};
    use dtx_core::resource::ResourceId;
    use tokio::time::sleep;

    struct SlowHandler(Duration);

    #[async_trait]
    impl Handler for SlowHandler {
        async fn handle(&self, _op: Operation, _ctx: Context) -> Result<Response> {
            sleep(self.0).await;
            Ok(Response::ok())
        }
    }

    #[tokio::test]
    async fn timeout_succeeds_when_fast() {
        let chain = MiddlewareStack::new()
            .layer(TimeoutMiddleware::new(Duration::from_secs(1)))
            .build(SlowHandler(Duration::from_millis(10)));

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn timeout_fails_when_slow() {
        let chain = MiddlewareStack::new()
            .layer(TimeoutMiddleware::new(Duration::from_millis(50)))
            .build(SlowHandler(Duration::from_secs(1)));

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(matches!(result, Err(Error::Timeout { .. })));
    }

    #[tokio::test]
    async fn timeout_uses_operation_specific_timeout() {
        let chain = MiddlewareStack::new()
            .layer(
                TimeoutMiddleware::new(Duration::from_millis(50))
                    .operation_timeout("start", Duration::from_secs(2)),
            )
            .build(SlowHandler(Duration::from_millis(100)));

        // start_all uses default timeout (50ms) - should fail
        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(matches!(result, Err(Error::Timeout { .. })));

        // start uses custom timeout (2s) - should succeed
        let result = chain
            .execute(
                Operation::Start {
                    id: ResourceId::new("api"),
                },
                Context::new(),
            )
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn timeout_passes_through_fast() {
        let chain = MiddlewareStack::new()
            .layer(TimeoutMiddleware::new(Duration::from_secs(10)))
            .build(NoopHandler);

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(result.is_ok());
    }
}
