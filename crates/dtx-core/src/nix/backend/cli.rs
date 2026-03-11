//! CLI backend for Nix commands.
//!
//! This backend executes Nix operations via subprocess calls to the `nix` CLI.
//! It serves as the reliable fallback when native bindings are unavailable.

use super::NixBackend;
use crate::error::NixError;
use crate::nix::models::{NixSearchResult, Package, PackageInfo};
use async_trait::async_trait;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, warn};

/// CLI-based Nix backend using subprocess calls.
///
/// Always available when Nix is installed on the system.
pub struct CliBackend {
    nix_available: bool,
}

impl CliBackend {
    /// Creates a new CLI backend, checking for Nix availability.
    pub fn new() -> Self {
        let nix_available = std::process::Command::new("nix")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if nix_available {
            debug!("Nix CLI backend initialized");
        } else {
            warn!("Nix CLI not available");
        }

        Self { nix_available }
    }
}

impl Default for CliBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NixBackend for CliBackend {
    async fn search(&self, query: &str, flake_ref: Option<&str>) -> Result<Vec<Package>, NixError> {
        if !self.nix_available {
            return Err(NixError::NixNotInstalled);
        }

        let flake = flake_ref.unwrap_or("nixpkgs");
        debug!(query = %query, flake = %flake, "Searching packages via CLI");

        let output = Command::new("nix")
            .args(["search", "--json", flake, query])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| NixError::CommandFailed(e.to_string()))?;

        // Handle non-zero exit (could mean no results or actual error)
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Empty search result is not an error
            if stderr.contains("no results")
                || stderr.contains("does not provide")
                || output.stdout.is_empty()
            {
                return Ok(vec![]);
            }
            debug!(stderr = %stderr, "nix search returned error");
            return Ok(vec![]);
        }

        // Handle empty output
        if output.stdout.is_empty() || output.stdout == b"{}" || output.stdout == b"{}\n" {
            return Ok(vec![]);
        }

        let results: HashMap<String, NixSearchResult> = serde_json::from_slice(&output.stdout)
            .map_err(|e| NixError::ParseError(e.to_string()))?;

        Ok(results
            .into_iter()
            .map(|(attr, result)| Package {
                attr_path: attr.clone(),
                name: attr,
                pname: result.pname,
                version: result.version,
                description: result.description,
            })
            .collect())
    }

    async fn validate(&self, package: &str, flake_ref: Option<&str>) -> Result<bool, NixError> {
        if !self.nix_available {
            return Err(NixError::NixNotInstalled);
        }

        let flake = flake_ref.unwrap_or("nixpkgs");
        let attr = format!("{}#{}.name", flake, package);

        debug!(package = %package, attr = %attr, "Validating package");

        let output = Command::new("nix")
            .args(["eval", "--json", &attr])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| NixError::CommandFailed(e.to_string()))?;

        Ok(output.status.success())
    }

    async fn eval(&self, expr: &str) -> Result<String, NixError> {
        if !self.nix_available {
            return Err(NixError::NixNotInstalled);
        }

        debug!(expr = %expr, "Evaluating Nix expression");

        let output = Command::new("nix")
            .args(["eval", "--expr", expr, "--json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| NixError::CommandFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NixError::EvalFailed(stderr.to_string()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    async fn get_info(
        &self,
        package: &str,
        flake_ref: Option<&str>,
    ) -> Result<PackageInfo, NixError> {
        if !self.nix_available {
            return Err(NixError::NixNotInstalled);
        }

        let flake = flake_ref.unwrap_or("nixpkgs");
        debug!(package = %package, flake = %flake, "Getting package info");

        // Get meta information
        let meta_attr = format!("{}#{}.meta", flake, package);
        let output = Command::new("nix")
            .args(["eval", "--json", &meta_attr])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| NixError::CommandFailed(e.to_string()))?;

        if !output.status.success() {
            return Err(NixError::PackageNotFound(package.to_string()));
        }

        let meta: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| NixError::ParseError(e.to_string()))?;

        // Get version separately
        let version_attr = format!("{}#{}.version", flake, package);
        let version_output = Command::new("nix")
            .args(["eval", "--raw", &version_attr])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| NixError::CommandFailed(e.to_string()))?;

        let version = if version_output.status.success() {
            String::from_utf8_lossy(&version_output.stdout).to_string()
        } else {
            "unknown".to_string()
        };

        Ok(PackageInfo {
            name: package.to_string(),
            version,
            description: meta
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            homepage: meta
                .get("homepage")
                .and_then(|v| v.as_str())
                .map(String::from),
            license: meta.get("license").and_then(|v| {
                if let Some(s) = v.as_str() {
                    Some(s.to_string())
                } else {
                    v.get("spdxId").and_then(|id| id.as_str()).map(String::from)
                }
            }),
        })
    }

    fn is_available(&self) -> bool {
        self.nix_available
    }

    fn name(&self) -> &'static str {
        "CLI"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_backend_creation() {
        let backend = CliBackend::new();
        // May or may not be available depending on environment
        println!("CLI backend available: {}", backend.is_available());
        assert_eq!(backend.name(), "CLI");
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_search() {
        let backend = CliBackend::new();
        let results = backend.search("hello", None).await.unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_validate() {
        let backend = CliBackend::new();
        assert!(backend.validate("hello", None).await.unwrap());
        assert!(!backend.validate("zzz-nonexistent-123", None).await.unwrap());
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_eval() {
        let backend = CliBackend::new();
        let result = backend.eval("1 + 1").await.unwrap();
        assert_eq!(result, "2");
    }

    #[tokio::test]
    #[ignore = "requires nix"]
    async fn test_get_info() {
        let backend = CliBackend::new();
        let info = backend.get_info("hello", None).await.unwrap();
        assert_eq!(info.name, "hello");
        assert!(!info.version.is_empty());
    }
}
