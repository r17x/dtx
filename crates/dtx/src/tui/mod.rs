//! Terminal UI for native process backend.
//!
//! Provides a real-time view of running services with:
//! - Service list with status indicators
//! - Live log streaming
//! - Keyboard controls for service management

mod app;
mod ui;

pub use app::run_tui;
