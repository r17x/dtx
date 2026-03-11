//! Middleware implementations for dtx.
//!
//! This crate provides standard middleware for the dtx middleware stack:
//!
//! - [`LoggingMiddleware`] - Structured logging with tracing
//! - [`MetricsMiddleware`] - Prometheus-compatible metrics
//! - [`TimeoutMiddleware`] - Operation timeout enforcement
//! - [`RetryMiddleware`] - Automatic retry with backoff
//! - [`AIMiddleware`] - AI-powered suggestions and error explanations
//!
//! # Example
//!
//! ```ignore
//! use dtx_middleware::{
//!     LoggingMiddleware, MetricsMiddleware, TimeoutMiddleware, RetryMiddleware,
//!     MetricsRegistry,
//! };
//! use dtx_core::middleware::MiddlewareStack;
//! use std::sync::Arc;
//! use std::time::Duration;
//!
//! let registry = Arc::new(MetricsRegistry::new());
//!
//! let chain = MiddlewareStack::new()
//!     .layer(LoggingMiddleware::new())
//!     .layer(MetricsMiddleware::new(registry.clone()))
//!     .layer(TimeoutMiddleware::new(Duration::from_secs(30)))
//!     .layer(RetryMiddleware::new(3))
//!     .build(handler);
//! ```

pub mod ai;
mod logging;
mod metrics;
mod retry;
mod timeout;

// Logging middleware
pub use logging::{LogLevel, LoggingMiddleware};

// Metrics middleware
pub use metrics::{Counter, Gauge, Histogram, Labels, MetricsMiddleware, MetricsRegistry};

// Timeout middleware
pub use timeout::TimeoutMiddleware;

// Retry middleware
pub use retry::{
    BackoffStrategy, ExponentialBackoff, FixedBackoff, LinearBackoff, NoBackoff, RetryAlways,
    RetryCondition, RetryMiddleware, RetryOnRetryable,
};

// AI middleware
pub use ai::{AIConfig, AIError, AIMiddleware, AIProvider, AIRequest, AIResponse, Suggestions};

#[cfg(feature = "ai")]
pub use ai::{create_provider_from_env, ClaudeProvider, LocalProvider, OpenAIProvider};
