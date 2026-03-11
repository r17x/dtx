//! Process utilities module.
//!
//! This module provides utilities for process management:
//!
//! - **Preflight checks**: Validate service dependencies before starting
//! - **Port utilities**: Port availability checking and conflict resolution
//!
//! ## Note
//!
//! The main process orchestration is now in the `dtx-process` crate:
//!
//! ```ignore
//! use dtx_process::{ProcessResource, ResourceOrchestrator, ProcessResourceConfig};
//! use dtx_core::events::ResourceEventBus;
//!
//! let event_bus = Arc::new(ResourceEventBus::new());
//! let mut orchestrator = ResourceOrchestrator::new(event_bus);
//!
//! let config = ProcessResourceConfig::new("api", "cargo run");
//! orchestrator.add_resource(config);
//! orchestrator.start_all().await?;
//! ```

pub mod port;
pub mod preflight;

// Re-exports
pub use port::{
    check_ports_availability, extract_service_ports, find_available_port, find_available_port_near,
    is_port_available, resolve_port_conflicts, validate_ports, validate_ports_sync,
    validate_service_ports, PortReassignment, ServicePort,
};
pub use preflight::{
    analyze_services, run_preflight, run_preflight_with_path, CheckType, PreflightCheck,
    PreflightResult,
};
