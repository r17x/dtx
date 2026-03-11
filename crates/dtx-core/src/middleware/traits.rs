//! Middleware trait and chain execution types.

use async_trait::async_trait;
use std::sync::Arc;

use super::operation::{Operation, Response};
use crate::resource::{Context, Result};

/// Middleware processes operations before/after core execution.
///
/// Middleware follows the onion model: each layer wraps the next.
/// The first middleware added is the outermost layer.
///
/// # Example
///
/// ```ignore
/// use dtx_core::middleware::{Middleware, Next, Operation, Response};
/// use dtx_core::resource::{Context, Result};
///
/// struct LoggingMiddleware;
///
/// #[async_trait]
/// impl Middleware for LoggingMiddleware {
///     fn name(&self) -> &'static str { "logging" }
///
///     async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
///         println!("Before: {}", op.name());
///         let result = next.run(op, ctx).await;
///         println!("After");
///         result
///     }
/// }
/// ```
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Unique name for this middleware.
    fn name(&self) -> &'static str;

    /// Process an operation.
    ///
    /// Call `next.run(op, ctx).await` to continue the chain.
    /// Can modify operation, context, or response.
    async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response>;
}

/// The next middleware or handler in the chain.
pub struct Next<'a> {
    middleware: &'a [Arc<dyn Middleware>],
    handler: &'a dyn Handler,
}

impl<'a> Next<'a> {
    /// Create a new Next from middleware slice and handler.
    pub(crate) fn new(middleware: &'a [Arc<dyn Middleware>], handler: &'a dyn Handler) -> Self {
        Self {
            middleware,
            handler,
        }
    }

    /// Execute the next middleware or the final handler.
    pub async fn run(self, op: Operation, ctx: Context) -> Result<Response> {
        if let Some((first, rest)) = self.middleware.split_first() {
            let next = Next::new(rest, self.handler);
            first.handle(op, ctx, next).await
        } else {
            self.handler.handle(op, ctx).await
        }
    }
}

/// The final handler that processes operations.
///
/// This is the core logic that actually executes operations.
/// Middleware wraps the handler.
#[async_trait]
pub trait Handler: Send + Sync {
    /// Handle an operation and return a response.
    async fn handle(&self, op: Operation, ctx: Context) -> Result<Response>;
}

/// A middleware that does nothing (passthrough).
///
/// Useful for testing or as a placeholder.
pub struct PassthroughMiddleware;

#[async_trait]
impl Middleware for PassthroughMiddleware {
    fn name(&self) -> &'static str {
        "passthrough"
    }

    async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
        next.run(op, ctx).await
    }
}

/// A handler that always returns Ok.
///
/// Useful for testing middleware in isolation.
pub struct NoopHandler;

#[async_trait]
impl Handler for NoopHandler {
    async fn handle(&self, _op: Operation, _ctx: Context) -> Result<Response> {
        Ok(Response::ok())
    }
}

/// A handler that wraps a function.
pub struct FnHandler<F>(pub F);

#[async_trait]
impl<F, Fut> Handler for FnHandler<F>
where
    F: Fn(Operation, Context) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<Response>> + Send,
{
    async fn handle(&self, op: Operation, ctx: Context) -> Result<Response> {
        (self.0)(op, ctx).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingMiddleware {
        counter: Arc<AtomicU32>,
    }

    #[async_trait]
    impl Middleware for CountingMiddleware {
        fn name(&self) -> &'static str {
            "counting"
        }

        async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            next.run(op, ctx).await
        }
    }

    #[tokio::test]
    async fn passthrough_middleware() {
        let middleware: Vec<Arc<dyn Middleware>> = vec![Arc::new(PassthroughMiddleware)];
        let handler = NoopHandler;

        let next = Next::new(&middleware, &handler);
        let result = next.run(Operation::StartAll, Context::new()).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn middleware_chain() {
        let counter = Arc::new(AtomicU32::new(0));
        let middleware: Vec<Arc<dyn Middleware>> = vec![
            Arc::new(CountingMiddleware {
                counter: counter.clone(),
            }),
            Arc::new(CountingMiddleware {
                counter: counter.clone(),
            }),
        ];
        let handler = NoopHandler;

        let next = Next::new(&middleware, &handler);
        let result = next.run(Operation::StartAll, Context::new()).await;

        assert!(result.is_ok());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn fn_handler() {
        let handler =
            FnHandler(|op: Operation, _ctx: Context| async move { Ok(Response::data(op.name())) });

        let result = handler.handle(Operation::StartAll, Context::new()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn empty_middleware_chain() {
        let middleware: Vec<Arc<dyn Middleware>> = vec![];
        let handler = NoopHandler;

        let next = Next::new(&middleware, &handler);
        let result = next.run(Operation::StartAll, Context::new()).await;

        assert!(result.is_ok());
    }
}
