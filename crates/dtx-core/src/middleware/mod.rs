//! Middleware system for composable operation processing.
//!
//! This module provides Tower-style middleware composition for resource operations.
//! Middleware wraps operations, allowing cross-cutting concerns like logging,
//! metrics, timeout, and retry to be added without modifying core logic.
//!
//! # Architecture
//!
//! ```text
//! Request → [Logging] → [Metrics] → [Timeout] → [Retry] → Handler → Response
//!         ↑_________________________________________↓
//! ```
//!
//! # Example
//!
//! ```ignore
//! use dtx_core::middleware::{MiddlewareStack, Operation, Response};
//! use dtx_core::resource::Context;
//!
//! let chain = MiddlewareStack::new()
//!     .layer(LoggingMiddleware::new())
//!     .layer(MetricsMiddleware::new(registry))
//!     .layer(TimeoutMiddleware::new(Duration::from_secs(30)))
//!     .build(MyHandler);
//!
//! let response = chain.execute(Operation::StartAll, Context::new()).await?;
//! ```

mod operation;
mod stack;
mod traits;

pub use operation::{Operation, Response};
pub use stack::{MiddlewareChain, MiddlewareStack, MiddlewareStackBuilder};
pub use traits::{FnHandler, Handler, Middleware, Next, NoopHandler, PassthroughMiddleware};
