//! Resource implementations for dtx.
//!
//! This crate provides implementations of the Resource trait for various
//! resource types, enabling native process management and orchestration
//! with the dtx architecture.
//!
//! # Features
//!
//! - **ProcessResource**: Native OS process implementing Resource trait
//! - **ContainerResource**: Docker/Podman container (feature: `container`)
//! - **VMResource**: Virtual machine resource (placeholder for Phase 8.2)
//! - **AgentResource**: AI agent resource (placeholder for Phase 8.3)
//! - **Probes**: Health check probes (exec, HTTP, TCP)
//! - **Restart**: Configurable restart policies with backoff
//! - **Orchestrator**: Dependency-aware resource orchestration
//!
//! # Example
//!
//! ```ignore
//! use dtx_process::{ProcessResource, ProcessResourceConfig};
//! use dtx_core::resource::{Resource, Context};
//! use dtx_core::events::ResourceEventBus;
//! use std::sync::Arc;
//!
//! let event_bus = Arc::new(ResourceEventBus::new());
//! let config = ProcessResourceConfig::new("api", "cargo run")?;
//!
//! let mut process = ProcessResource::new(config, event_bus);
//! process.start(&Context::new()).await?;
//! ```

mod agent;
mod config;
#[cfg(feature = "container")]
mod container;
mod orchestrator;
mod probe;
mod process;
mod translator;
mod vm;

pub use agent::{AgentResource, AgentResourceConfig};
pub use config::{
    BackoffConfig, ProbeConfig, ProbeSettings, ProcessResourceConfig, RestartPolicy,
    ShutdownConfig, Signal,
};
#[cfg(feature = "container")]
pub use container::ContainerResource;
pub use orchestrator::{Dependency, ResourceOrchestrator, StartAllResult};
pub use probe::ProbeRunner;
pub use process::ProcessResource;
pub use translator::ProcessToContainerTranslator;
pub use vm::{VMResource, VMResourceConfig};

use dtx_core::translation::TranslatorRegistry;

/// Create a translator registry with default translators registered.
///
/// Returns a registry pre-configured with:
/// - `ProcessToContainerTranslator` for Process → Container translation
///
/// # Example
///
/// ```
/// use dtx_process::{default_registry, ProcessResourceConfig};
/// use dtx_core::translation::{ContainerConfig, TranslationContext};
///
/// let registry = default_registry();
/// let process = ProcessResourceConfig::new("api", "node server.js").with_port(3000);
/// let ctx = TranslationContext::new();
///
/// let container: ContainerConfig = registry.translate_with_context(&process, &ctx).unwrap();
/// assert_eq!(container.id.as_str(), "api");
/// ```
pub fn default_registry() -> TranslatorRegistry {
    let mut registry = TranslatorRegistry::new();
    registry.register(ProcessToContainerTranslator);
    registry
}
