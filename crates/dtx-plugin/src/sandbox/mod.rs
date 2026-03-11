//! Plugin sandboxing via WebAssembly.
//!
//! This module provides secure execution of untrusted plugins using
//! WebAssembly isolation with capability-based permissions.
//!
//! # Trust Levels
//!
//! - **Trusted**: Signed by known keys, full capabilities
//! - **Signed**: Has signature but unknown key, standard capabilities
//! - **Unsigned**: No signature, minimal capabilities
//!
//! # Example
//!
//! ```ignore
//! use dtx_plugin::sandbox::{SandboxedPlugin, Capabilities, ResourceLimits};
//!
//! let plugin = SandboxedPlugin::from_file(
//!     Path::new("plugin.wasm"),
//!     Capabilities::standard(),
//!     ResourceLimits::default(),
//! )?;
//!
//! // Plugin runs in sandbox with limited capabilities
//! plugin.call_i32("init")?;
//! ```

mod capabilities;
mod host;
mod limits;
mod plugin;
mod runtime;
mod signing;

pub use capabilities::{
    Capabilities, EnvironmentCapabilities, EventCapabilities, FilesystemCapabilities,
    NetworkCapabilities, ProcessCapabilities, ResourceCapabilities,
};
pub use limits::{ResourceLimits, ResourceUsage};
pub use plugin::{SandboxWrapper, SandboxedPlugin};
pub use runtime::{SandboxState, WasmError, WasmRuntime};
pub use signing::{
    capabilities_for_trust_level, determine_trust_level, limits_for_trust_level, PluginSignature,
    TrustLevel, TrustedKeys,
};
