//! Event system for resource lifecycle events.
//!
//! This module provides:
//! - `LifecycleEvent` — Resource lifecycle events using `ResourceId`
//! - `EventFilter` — Filter events by resource, kind, type
//! - `ResourceEventBus` — Pub-sub with filtering and replay
//! - Unix socket IPC for cross-process notification (CLI → Web)
//!
//! # Example
//!
//! ```ignore
//! use dtx_core::events::{ResourceEventBus, LifecycleEvent, EventFilter};
//! use dtx_core::resource::ResourceKind;
//!
//! let bus = ResourceEventBus::new();
//!
//! // Subscribe with filter
//! let filter = EventFilter::new().resource("api").without_logs();
//! let mut sub = bus.subscribe_filtered(filter);
//!
//! // Publish events
//! bus.publish(LifecycleEvent::starting("api", ResourceKind::Process));
//! ```

mod filter;
mod helpers;
mod lifecycle;
mod resource_bus;
pub mod socket;

pub use filter::EventFilter;
pub use lifecycle::{DependencyCondition, LifecycleEvent};
pub use resource_bus::{ResourceEventBus, ResourceEventSubscriber};
pub use socket::{
    event_socket_path, notify_config_changed, notify_config_changed_sync, read_web_port,
    start_event_listener, PortGuard, SocketGuard,
};
