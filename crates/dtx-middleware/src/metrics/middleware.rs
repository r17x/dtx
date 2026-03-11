//! Metrics collection middleware.

use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

use dtx_core::middleware::{Middleware, Next, Operation, Response};
use dtx_core::resource::{Context, Result};

use super::registry::MetricsRegistry;
use super::types::Labels;

/// Middleware that collects metrics for all operations.
///
/// Records:
/// - `dtx_operations_total` - counter of operations by type and success
/// - `dtx_operation_duration_seconds` - histogram of operation duration
/// - `dtx_operations_in_flight` - gauge of currently executing operations
///
/// # Example
///
/// ```ignore
/// use dtx_middleware::{MetricsMiddleware, MetricsRegistry};
///
/// let registry = Arc::new(MetricsRegistry::new());
/// let chain = MiddlewareStack::new()
///     .layer(MetricsMiddleware::new(registry.clone()))
///     .build(handler);
///
/// // Later, export metrics
/// let prometheus = registry.export_prometheus();
/// ```
pub struct MetricsMiddleware {
    registry: Arc<MetricsRegistry>,
}

impl MetricsMiddleware {
    /// Create a new metrics middleware with the given registry.
    pub fn new(registry: Arc<MetricsRegistry>) -> Self {
        Self { registry }
    }

    fn labels_for_op(&self, op: &Operation, success: bool) -> Labels {
        Labels::new()
            .add("operation", op.name())
            .add("success", if success { "true" } else { "false" })
    }

    fn record_operation(&self, op: &Operation, elapsed: std::time::Duration, success: bool) {
        let labels = self.labels_for_op(op, success);

        // Increment operation counter
        self.registry
            .counter("dtx_operations_total", labels.clone())
            .inc();

        // Record duration
        self.registry
            .histogram("dtx_operation_duration_seconds", labels)
            .observe(elapsed.as_secs_f64());
    }
}

#[async_trait]
impl Middleware for MetricsMiddleware {
    fn name(&self) -> &'static str {
        "metrics"
    }

    async fn handle(&self, op: Operation, ctx: Context, next: Next<'_>) -> Result<Response> {
        // Track in-flight operations
        let in_flight = self.registry.gauge(
            "dtx_operations_in_flight",
            Labels::new().add("operation", op.name()),
        );
        in_flight.inc();

        let start = Instant::now();
        let result = next.run(op.clone(), ctx).await;
        let elapsed = start.elapsed();

        in_flight.dec();

        self.record_operation(&op, elapsed, result.is_ok());

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtx_core::middleware::{Handler, MiddlewareStack, NoopHandler};

    #[tokio::test]
    async fn metrics_middleware_records() {
        let registry = Arc::new(MetricsRegistry::new());
        let chain = MiddlewareStack::new()
            .layer(MetricsMiddleware::new(registry.clone()))
            .build(NoopHandler);

        chain
            .execute(Operation::StartAll, Context::new())
            .await
            .unwrap();

        let prometheus = registry.export_prometheus();
        assert!(prometheus.contains("dtx_operations_total"));
        assert!(prometheus.contains("operation=\"start_all\""));
        assert!(prometheus.contains("success=\"true\""));
    }

    struct FailingHandler;

    #[async_trait]
    impl Handler for FailingHandler {
        async fn handle(&self, _op: Operation, _ctx: Context) -> Result<Response> {
            Err(dtx_core::resource::Error::Cancelled)
        }
    }

    #[tokio::test]
    async fn metrics_middleware_records_failures() {
        let registry = Arc::new(MetricsRegistry::new());
        let chain = MiddlewareStack::new()
            .layer(MetricsMiddleware::new(registry.clone()))
            .build(FailingHandler);

        let _ = chain.execute(Operation::StartAll, Context::new()).await;

        let prometheus = registry.export_prometheus();
        assert!(prometheus.contains("success=\"false\""));
    }

    #[tokio::test]
    async fn metrics_middleware_in_flight() {
        let registry = Arc::new(MetricsRegistry::new());
        let chain = MiddlewareStack::new()
            .layer(MetricsMiddleware::new(registry.clone()))
            .build(NoopHandler);

        // Before execution
        assert!(registry.export_prometheus().is_empty());

        // After execution, in_flight should be 0
        chain
            .execute(Operation::StartAll, Context::new())
            .await
            .unwrap();

        let prometheus = registry.export_prometheus();
        assert!(prometheus.contains("dtx_operations_in_flight{operation=\"start_all\"} 0"));
    }
}
