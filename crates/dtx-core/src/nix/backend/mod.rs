//! Nix backend trait and implementations.
//!
//! This module provides the `NixBackend` trait for abstracting Nix operations,
//! allowing different implementations (CLI, native bindings) to be swapped.

mod cli;
mod native;

pub use cli::CliBackend;
pub use native::NativeBackend;

use crate::error::NixError;
use crate::nix::models::{Package, PackageInfo};
use async_trait::async_trait;

/// Backend trait for Nix operations.
///
/// Allows swapping between CLI and native implementations.
/// All implementations must be `Send + Sync` for async compatibility.
#[async_trait]
pub trait NixBackend: Send + Sync {
    /// Search packages with optional flake reference.
    ///
    /// # Arguments
    /// * `query` - Search term (package name or description)
    /// * `flake_ref` - Optional flake reference (e.g., ".#" or "github:NixOS/nixpkgs/<rev>#")
    async fn search(&self, query: &str, flake_ref: Option<&str>) -> Result<Vec<Package>, NixError>;

    /// Validate that a package exists.
    ///
    /// # Arguments
    /// * `package` - Package name to validate
    /// * `flake_ref` - Optional flake reference for validation context
    async fn validate(&self, package: &str, flake_ref: Option<&str>) -> Result<bool, NixError>;

    /// Evaluate a Nix expression.
    ///
    /// # Arguments
    /// * `expr` - Nix expression to evaluate
    async fn eval(&self, expr: &str) -> Result<String, NixError>;

    /// Get detailed package information.
    ///
    /// # Arguments
    /// * `package` - Package name
    /// * `flake_ref` - Optional flake reference
    async fn get_info(
        &self,
        package: &str,
        flake_ref: Option<&str>,
    ) -> Result<PackageInfo, NixError>;

    /// Check if this backend is available.
    fn is_available(&self) -> bool;

    /// Get the backend name for logging.
    fn name(&self) -> &'static str;
}
