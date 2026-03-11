//! Metrics collection for observability.
//!
//! Provides Prometheus-compatible metrics collection.

mod middleware;
mod registry;
mod types;

pub use middleware::MetricsMiddleware;
pub use registry::MetricsRegistry;
pub use types::{Counter, Gauge, Histogram, Labels};
