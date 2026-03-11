//! Native Nix dev environment extraction.
//!
//! Provides two extraction backends:
//! - CLI: Uses `nix print-dev-env --json` (always available)
//! - Native FFI: Uses nixops4/nix-bindings-rust (requires `native-nix` feature)
//!
//! The native backend avoids subprocess overhead by directly calling
//! the Nix C API through FFI bindings.

use crate::{CoreError, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;
use tracing::{debug, info};

#[cfg(feature = "native-nix")]
use super::native::{detect_system, NativeNixEvaluator};

/// Environment variable from nix print-dev-env.
#[derive(Debug, Clone, Deserialize)]
pub struct NixVariable {
    /// Variable type: "exported", "var", "array", etc.
    #[serde(rename = "type")]
    pub var_type: String,
    /// The value of the variable.
    pub value: serde_json::Value,
}

/// Output from `nix print-dev-env --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct DevEnvOutput {
    /// Environment variables.
    pub variables: HashMap<String, NixVariable>,
    /// Bash functions (not used but included for completeness).
    #[serde(rename = "bashFunctions")]
    #[serde(default)]
    pub bash_functions: HashMap<String, String>,
}

/// Extracted dev environment ready for process spawning.
#[derive(Debug, Clone)]
pub struct DevEnvironment {
    /// Environment variables to set.
    pub env_vars: HashMap<String, String>,
    /// Original flake path.
    pub flake_path: String,
}

impl DevEnvironment {
    /// Extracts the dev environment from a flake.
    ///
    /// Runs `nix print-dev-env --json <flake_path>` and parses the output.
    ///
    /// # Arguments
    ///
    /// * `flake_dir` - Path to directory containing flake.nix
    ///
    /// # Returns
    ///
    /// A `DevEnvironment` with all environment variables, or an error.
    pub async fn from_flake(flake_dir: &Path) -> Result<Self> {
        let flake_path = flake_dir
            .to_str()
            .ok_or_else(|| CoreError::ProcessCompose("invalid flake path".to_string()))?;

        // Check flake.nix exists
        if !flake_dir.join("flake.nix").exists() {
            return Err(CoreError::ProcessCompose(format!(
                "flake.nix not found in {}",
                flake_path
            )));
        }

        info!(flake_path = %flake_path, "Extracting dev environment from flake");

        let output = Command::new("nix")
            .args(["print-dev-env", "--json", flake_path])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    CoreError::ProcessCompose("nix not found in PATH".to_string())
                } else {
                    CoreError::Io(e)
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CoreError::ProcessCompose(format!(
                "nix print-dev-env failed: {}",
                stderr
            )));
        }

        let dev_env: DevEnvOutput = serde_json::from_slice(&output.stdout).map_err(|e| {
            CoreError::ProcessCompose(format!("Failed to parse dev-env JSON: {}", e))
        })?;

        // Extract exported variables
        let mut env_vars = HashMap::new();
        for (name, var) in dev_env.variables {
            // Only include exported variables
            if var.var_type == "exported" {
                if let Some(value) = var.value.as_str() {
                    env_vars.insert(name, value.to_string());
                }
            }
        }

        debug!(
            num_vars = env_vars.len(),
            "Extracted environment variables from flake"
        );

        Ok(Self {
            env_vars,
            flake_path: flake_path.to_string(),
        })
    }

    /// Gets the PATH variable from the environment.
    pub fn path(&self) -> Option<&str> {
        self.env_vars.get("PATH").map(|s| s.as_str())
    }

    /// Checks if process-compose is available in the environment's PATH.
    pub fn has_process_compose(&self) -> bool {
        self.path()
            .map(|p| p.contains("process-compose"))
            .unwrap_or(false)
    }

    /// Returns the number of environment variables.
    pub fn var_count(&self) -> usize {
        self.env_vars.len()
    }

    /// Extracts the dev environment using native Nix FFI bindings.
    ///
    /// This is faster than `from_flake()` as it avoids spawning a subprocess.
    /// Only available with the `native-nix` feature.
    ///
    /// # Arguments
    ///
    /// * `flake_dir` - Path to directory containing flake.nix
    ///
    /// # Returns
    ///
    /// A `DevEnvironment` with all environment variables, or an error.
    #[cfg(feature = "native-nix")]
    pub fn from_flake_native(flake_dir: &Path) -> Result<Self> {
        let flake_path = flake_dir
            .to_str()
            .ok_or_else(|| CoreError::ProcessCompose("invalid flake path".to_string()))?;

        // Check flake.nix exists
        if !flake_dir.join("flake.nix").exists() {
            return Err(CoreError::ProcessCompose(format!(
                "flake.nix not found in {}",
                flake_path
            )));
        }

        info!(flake_path = %flake_path, "Extracting dev environment using native Nix FFI");

        let mut evaluator = NativeNixEvaluator::new()?;
        let system = detect_system();

        let env_vars = evaluator.get_devshell_env(flake_dir, &system)?;

        debug!(
            num_vars = env_vars.len(),
            system = %system,
            "Extracted environment variables using native FFI"
        );

        Ok(Self {
            env_vars,
            flake_path: flake_path.to_string(),
        })
    }

    /// Extracts the dev environment, preferring native FFI if available.
    ///
    /// Uses native FFI with `native-nix` feature, falls back to CLI otherwise.
    ///
    /// # Arguments
    ///
    /// * `flake_dir` - Path to directory containing flake.nix
    ///
    /// # Returns
    ///
    /// A `DevEnvironment` with all environment variables, or an error.
    pub async fn from_flake_auto(flake_dir: &Path) -> Result<Self> {
        #[cfg(feature = "native-nix")]
        {
            // Try native first, fall back to CLI on error
            match Self::from_flake_native(flake_dir) {
                Ok(env) => return Ok(env),
                Err(e) => {
                    debug!(
                        error = %e,
                        "Native Nix extraction failed, falling back to CLI"
                    );
                }
            }
        }

        // CLI fallback (or only option without native-nix)
        Self::from_flake(flake_dir).await
    }
}

/// Cache for dev environments to avoid repeated extraction.
pub struct DevEnvCache {
    cache: tokio::sync::RwLock<HashMap<String, DevEnvironment>>,
}

impl Default for DevEnvCache {
    fn default() -> Self {
        Self::new()
    }
}

impl DevEnvCache {
    /// Creates a new empty cache.
    pub fn new() -> Self {
        Self {
            cache: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Gets or extracts the dev environment for a flake.
    ///
    /// Uses cached value if available, otherwise extracts and caches.
    pub async fn get_or_extract(&self, flake_dir: &Path) -> Result<DevEnvironment> {
        let key = flake_dir.to_string_lossy().to_string();

        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(env) = cache.get(&key) {
                debug!(flake_path = %key, "Using cached dev environment");
                return Ok(env.clone());
            }
        }

        // Extract and cache
        let env = DevEnvironment::from_flake(flake_dir).await?;
        {
            let mut cache = self.cache.write().await;
            cache.insert(key, env.clone());
        }

        Ok(env)
    }

    /// Invalidates the cache for a specific flake.
    pub async fn invalidate(&self, flake_dir: &Path) {
        let key = flake_dir.to_string_lossy().to_string();
        let mut cache = self.cache.write().await;
        cache.remove(&key);
    }

    /// Clears the entire cache.
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
    }
}

/// Global dev environment cache.
static DEV_ENV_CACHE: std::sync::OnceLock<DevEnvCache> = std::sync::OnceLock::new();

/// Gets the global dev environment cache.
pub fn dev_env_cache() -> &'static DevEnvCache {
    DEV_ENV_CACHE.get_or_init(DevEnvCache::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_dev_env_cache_new() {
        // Verify cache can be created (test passes if no panic)
        let _cache = DevEnvCache::new();
    }

    #[tokio::test]
    async fn test_from_flake_missing() {
        let dir = tempdir().unwrap();
        let result = DevEnvironment::from_flake(dir.path()).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("flake.nix not found"));
    }

    #[tokio::test]
    #[ignore] // Requires nix installation
    async fn test_from_flake_real() {
        let dir = tempdir().unwrap();

        // Create a minimal flake
        let flake = r#"{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = { nixpkgs, ... }: {
    devShells.aarch64-darwin.default = nixpkgs.legacyPackages.aarch64-darwin.mkShell {
      packages = [ nixpkgs.legacyPackages.aarch64-darwin.hello ];
    };
  };
}"#;
        fs::write(dir.path().join("flake.nix"), flake).unwrap();

        let env = DevEnvironment::from_flake(dir.path()).await.unwrap();
        assert!(env.path().is_some());
        assert!(env.var_count() > 0);
    }

    #[test]
    fn test_has_process_compose() {
        let mut env = DevEnvironment {
            env_vars: HashMap::new(),
            flake_path: "/test".to_string(),
        };

        // No PATH
        assert!(!env.has_process_compose());

        // PATH without process-compose
        env.env_vars
            .insert("PATH".to_string(), "/usr/bin:/bin".to_string());
        assert!(!env.has_process_compose());

        // PATH with process-compose
        env.env_vars.insert(
            "PATH".to_string(),
            "/nix/store/xxx-process-compose-1.0/bin:/usr/bin".to_string(),
        );
        assert!(env.has_process_compose());
    }
}
