//! Middleware stack builder and chain execution.

use std::sync::Arc;

use super::operation::{Operation, Response};
use super::traits::{Handler, Middleware, Next};
use crate::resource::{Context, Result};

/// A composable stack of middleware.
///
/// Middleware is added in order, with the first added being the outermost layer.
///
/// # Example
///
/// ```ignore
/// use dtx_core::middleware::{MiddlewareStack, PassthroughMiddleware};
///
/// let chain = MiddlewareStack::new()
///     .layer(LoggingMiddleware)
///     .layer(MetricsMiddleware)
///     .build(MyHandler);
///
/// let response = chain.execute(Operation::StartAll, Context::new()).await?;
/// ```
pub struct MiddlewareStack {
    middleware: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareStack {
    /// Create a new empty middleware stack.
    pub fn new() -> Self {
        Self {
            middleware: Vec::new(),
        }
    }

    /// Add a middleware layer (first added = outermost).
    pub fn layer<M: Middleware + 'static>(mut self, middleware: M) -> Self {
        self.middleware.push(Arc::new(middleware));
        self
    }

    /// Add a middleware layer from Arc.
    pub fn layer_arc(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    /// Add a middleware layer conditionally.
    pub fn layer_if<M: Middleware + 'static>(self, condition: bool, middleware: M) -> Self {
        if condition {
            self.layer(middleware)
        } else {
            self
        }
    }

    /// Add a middleware layer from Option.
    pub fn layer_option<M: Middleware + 'static>(self, middleware: Option<M>) -> Self {
        match middleware {
            Some(m) => self.layer(m),
            None => self,
        }
    }

    /// Build the stack with a handler.
    pub fn build<H: Handler + 'static>(self, handler: H) -> MiddlewareChain {
        MiddlewareChain {
            middleware: self.middleware,
            handler: Arc::new(handler),
        }
    }

    /// Get the middleware list.
    pub fn middleware(&self) -> &[Arc<dyn Middleware>] {
        &self.middleware
    }

    /// Number of middleware layers.
    pub fn len(&self) -> usize {
        self.middleware.len()
    }

    /// Check if the stack is empty.
    pub fn is_empty(&self) -> bool {
        self.middleware.is_empty()
    }
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}

/// A complete middleware chain with handler.
///
/// This is the result of calling `MiddlewareStack::build()`.
pub struct MiddlewareChain {
    middleware: Vec<Arc<dyn Middleware>>,
    handler: Arc<dyn Handler>,
}

impl MiddlewareChain {
    /// Execute an operation through the middleware chain.
    pub async fn execute(&self, op: Operation, ctx: Context) -> Result<Response> {
        let next = Next::new(&self.middleware, self.handler.as_ref());
        next.run(op, ctx).await
    }

    /// Get the number of middleware layers.
    pub fn middleware_count(&self) -> usize {
        self.middleware.len()
    }
}

/// Builder pattern alias for MiddlewareStack.
pub type MiddlewareStackBuilder = MiddlewareStack;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::traits::{NoopHandler, PassthroughMiddleware};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn middleware_stack_builder() {
        let chain = MiddlewareStack::new()
            .layer(PassthroughMiddleware)
            .layer(PassthroughMiddleware)
            .build(NoopHandler);

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(result.is_ok());
        assert_eq!(chain.middleware_count(), 2);
    }

    #[tokio::test]
    async fn middleware_order() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);

        struct OrderedMiddleware(usize);

        #[async_trait::async_trait]
        impl Middleware for OrderedMiddleware {
            fn name(&self) -> &'static str {
                "ordered"
            }

            async fn handle(
                &self,
                op: Operation,
                ctx: Context,
                next: Next<'_>,
            ) -> Result<Response> {
                let before = COUNTER.fetch_add(1, Ordering::SeqCst);
                assert_eq!(before, self.0, "Middleware executed out of order");
                next.run(op, ctx).await
            }
        }

        COUNTER.store(0, Ordering::SeqCst);

        let chain = MiddlewareStack::new()
            .layer(OrderedMiddleware(0))
            .layer(OrderedMiddleware(1))
            .layer(OrderedMiddleware(2))
            .build(NoopHandler);

        chain
            .execute(Operation::StartAll, Context::new())
            .await
            .unwrap();

        assert_eq!(COUNTER.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn conditional_layer() {
        let chain = MiddlewareStack::new()
            .layer_if(true, PassthroughMiddleware)
            .layer_if(false, PassthroughMiddleware)
            .build(NoopHandler);

        assert_eq!(chain.middleware_count(), 1);

        let result = chain.execute(Operation::StartAll, Context::new()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn option_layer() {
        let some_middleware: Option<PassthroughMiddleware> = Some(PassthroughMiddleware);
        let none_middleware: Option<PassthroughMiddleware> = None;

        let chain = MiddlewareStack::new()
            .layer_option(some_middleware)
            .layer_option(none_middleware)
            .build(NoopHandler);

        assert_eq!(chain.middleware_count(), 1);
    }

    #[test]
    fn stack_default() {
        let stack = MiddlewareStack::default();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
    }
}
