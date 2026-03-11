//! Resource abstraction for universal orchestration.
//!
//! This module provides the core `Resource` trait that all orchestratable
//! entities implement, along with supporting types for lifecycle management.
//!
//! # Overview
//!
//! A Resource represents anything with a lifecycle that dtx can manage:
//! - Processes (native OS processes)
//! - Containers (Docker, Podman)
//! - Virtual Machines (QEMU, Nix VMs)
//! - AI Agents (LLM workers)
//! - Custom plugin-defined types
//!
//! # Example
//!
//! ```ignore
//! use dtx_core::resource::{Resource, ResourceId, ResourceKind, Context};
//!
//! struct MyProcess {
//!     id: ResourceId,
//!     state: ResourceState,
//! }
//!
//! #[async_trait]
//! impl Resource for MyProcess {
//!     fn id(&self) -> &ResourceId { &self.id }
//!     fn kind(&self) -> ResourceKind { ResourceKind::Process }
//!     fn state(&self) -> &ResourceState { &self.state }
//!
//!     async fn start(&mut self, ctx: &Context) -> ResourceResult<()> {
//!         // Start the process...
//!         Ok(())
//!     }
//!
//!     async fn stop(&mut self, ctx: &Context) -> ResourceResult<()> {
//!         // Stop the process...
//!         Ok(())
//!     }
//!
//!     fn as_any(&self) -> &dyn Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn Any { self }
//! }
//! ```

mod config;
mod context;
mod error;
mod health;
mod id;
mod kind;
mod state;
mod traits;

// Re-export all public types
pub use config::{ConfigError, ResourceConfig};
pub use context::Context;
pub use error::{ConfigError as ErrorConfigError, Error, IoResultExt, Result, ResultExt};
pub use health::{HealthStatus, LogEntry, LogStream, LogStreamKind};
pub use id::ResourceId;
pub use kind::ResourceKind;
pub use state::ResourceState;
pub use traits::{Resource, ResourceError, ResourceExt, ResourceResult};
