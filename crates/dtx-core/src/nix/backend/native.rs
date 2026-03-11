//! Native Nix backend using nix-bindings (feature-gated).
//!
//! This module provides a native Nix backend using FFI bindings.
//! Requires the `native-nix` feature flag.
//!
//! Build with: `cargo build --features native-nix`

#![allow(dead_code)]

use super::NixBackend;
use crate::error::NixError;
use crate::nix::models::{Package, PackageInfo};
use async_trait::async_trait;

/// Native Nix backend using FFI bindings.
///
/// Provides faster evaluation by avoiding subprocess overhead.
/// Requires Nix development headers at build time.
///
/// This is currently a stub for future implementation.
#[derive(Default)]
pub struct NativeBackend {
    available: bool,
}

impl NativeBackend {
    /// Attempt to create a new native backend.
    ///
    /// Returns an error if native bindings are not available.
    #[cfg(feature = "native-nix")]
    pub fn new() -> Result<Self, NixError> {
        // TODO: Initialize native Nix bindings
        // nix_bindings::init()?;
        Err(NixError::NativeEvalNotImplemented)
    }

    /// Stub for when native-nix feature is disabled.
    #[cfg(not(feature = "native-nix"))]
    pub fn new() -> Result<Self, NixError> {
        Err(NixError::NativeBindingsNotAvailable)
    }

    /// Evaluate a Nix expression directly.
    fn eval_internal(&self, _expr: &str) -> Result<String, NixError> {
        Err(NixError::NativeEvalNotImplemented)
    }
}

#[async_trait]
impl NixBackend for NativeBackend {
    async fn search(
        &self,
        _query: &str,
        _flake_ref: Option<&str>,
    ) -> Result<Vec<Package>, NixError> {
        Err(NixError::NativeEvalNotImplemented)
    }

    async fn validate(&self, _package: &str, _flake_ref: Option<&str>) -> Result<bool, NixError> {
        Err(NixError::NativeEvalNotImplemented)
    }

    async fn eval(&self, _expr: &str) -> Result<String, NixError> {
        Err(NixError::NativeEvalNotImplemented)
    }

    async fn get_info(
        &self,
        _package: &str,
        _flake_ref: Option<&str>,
    ) -> Result<PackageInfo, NixError> {
        Err(NixError::NativeEvalNotImplemented)
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn name(&self) -> &'static str {
        "Native"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_native_backend_unavailable() {
        let result = NativeBackend::new();
        assert!(result.is_err());
    }

    #[test]
    fn test_native_backend_name() {
        let backend = NativeBackend::default();
        assert_eq!(backend.name(), "Native");
        assert!(!backend.is_available());
    }
}
