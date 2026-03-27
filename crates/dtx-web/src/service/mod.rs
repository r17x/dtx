//! Service layer for dtx-web.
//!
//! Transport-agnostic business logic extracted from handlers.
//! Handlers parse requests and format responses; this module does the work.

pub mod ops;
pub mod orchestration;

pub use ops::ServiceOps;
pub use orchestration::OrchestratorHandle;
