//! Plugin system for dtx.
//!
//! This crate provides the infrastructure for extending dtx with plugins.
//! Plugins can provide new resource backends, middleware, or translators.
//!
//! # Plugin Types
//!
//! - **Backend plugins** provide new resource types (e.g., Docker containers, VMs)
//! - **Middleware plugins** add processing stages to the operation pipeline
//! - **Translator plugins** convert resources between different types
//!
//! # Creating a Plugin
//!
//! Each plugin is a directory containing:
//! - `plugin.toml` - manifest describing the plugin
//! - Platform-specific shared library (for dynamic plugins)
//!
//! ## Example `plugin.toml`
//!
//! ```toml
//! name = "docker-backend"
//! version = "0.1.0"
//! description = "Docker container backend for dtx"
//! authors = ["Your Name <you@example.com>"]
//! plugin_type = "backend"
//! entry_point = "create_plugin"
//! ```
//!
//! # Static vs Dynamic Plugins
//!
//! Plugins can be loaded either statically or dynamically:
//!
//! - **Static plugins** are compiled directly into the application using
//!   `PluginLoader::register_backend` or `PluginLoader::register_middleware`.
//!   This is the default mode.
//!
//! - **Dynamic plugins** are loaded from shared libraries at runtime.
//!   This requires the `dynamic` feature flag. Use the [`dtx_plugin!`],
//!   [`dtx_backend_plugin!`], or [`dtx_middleware_plugin!`] macros to
//!   generate the required entry points.
//!
//! # Example Usage
//!
//! ## Static Plugin Registration
//!
//! ```ignore
//! use dtx_plugin::{PluginLoader, BackendPlugin};
//!
//! // Create a loader
//! let mut loader = PluginLoader::new("./plugins");
//!
//! // Register a static plugin
//! loader.register_backend(Box::new(MyBackend::new()));
//!
//! // Use the plugin
//! if let Some(backend) = loader.get_backend("my-backend") {
//!     let resource = backend.create_resource(config)?;
//! }
//! ```
//!
//! ## Dynamic Plugin (in plugin crate)
//!
//! ```ignore
//! use dtx_plugin::{dtx_backend_plugin, BackendPlugin, PluginError};
//! use dtx_core::resource::{Resource, ResourceKind};
//!
//! #[derive(Default)]
//! pub struct MyBackend;
//!
//! impl BackendPlugin for MyBackend {
//!     fn name(&self) -> &str { "my-backend" }
//!     fn resource_kind(&self) -> ResourceKind { ResourceKind::Custom(42) }
//!     fn create_resource(&self, config: serde_json::Value) -> Result<Box<dyn Resource>, PluginError> {
//!         // ...
//!     }
//! }
//!
//! // Generate the FFI entry point for dynamic loading
//! dtx_backend_plugin!(MyBackend);
//! ```
//!
//! # Features
//!
//! - `dynamic` - Enable dynamic plugin loading via `libloading`
//! - `sandbox` - Enable WASM sandboxing for untrusted plugins

mod error;
mod loader;
#[macro_use]
mod macros;
mod manifest;
mod traits;

#[cfg(feature = "sandbox")]
pub mod sandbox;

pub use error::{PluginError, Result};
pub use loader::{LoadedPlugin, PluginLoader};
pub use manifest::{PluginDependency, PluginManifest, PluginType};
pub use traits::{BackendPlugin, MiddlewarePlugin, Plugin, TranslatorPlugin};

// Macros are exported via #[macro_export] in macros.rs
// They are available at the crate root: dtx_plugin!, dtx_backend_plugin!, dtx_middleware_plugin!
