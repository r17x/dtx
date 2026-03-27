//! Web server for dtx.
//!
//! This crate provides the Axum-based web server for dtx:
//! - REST API endpoints for project and service management
//! - HTML handlers for web UI pages
//! - HTMX partial handlers for dynamic updates

pub mod config;
pub mod error;
pub mod handlers;
pub mod registry;
pub mod routes;
pub mod service;
pub mod sse;
pub mod state;
pub mod static_files;
pub mod types;

pub use registry::{ProjectRegistry, ProjectState};
pub use routes::create_router;
pub use state::AppState;
