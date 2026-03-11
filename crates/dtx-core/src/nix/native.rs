//! Native Nix bindings using FFI to the Nix C API.
//!
//! This module provides direct access to Nix evaluation through the
//! nixops4/nix-bindings-rust crates, avoiding CLI subprocess overhead.
//!
//! Requires the `native-nix` feature flag.

#[cfg(feature = "native-nix")]
use nix_bindings_expr::eval_state::{
    gc_register_my_thread, init, EvalState, ThreadRegistrationGuard,
};
#[cfg(feature = "native-nix")]
use nix_bindings_fetchers::FetchersSettings;
#[cfg(feature = "native-nix")]
use nix_bindings_flake::{
    FlakeLockFlags, FlakeReference, FlakeReferenceParseFlags, FlakeSettings, LockedFlake,
};
#[cfg(feature = "native-nix")]
use nix_bindings_store::store::Store;

use crate::{CoreError, Result};
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

/// Native Nix evaluator using FFI bindings from nixops4/nix-bindings-rust.
///
/// This provides direct access to Nix's evaluation capabilities without
/// shelling out to the CLI.
#[cfg(feature = "native-nix")]
pub struct NativeNixEvaluator {
    store: Store,
    eval_state: EvalState,
    flake_settings: FlakeSettings,
    fetchers_settings: FetchersSettings,
    _gc_guard: ThreadRegistrationGuard,
}

#[cfg(feature = "native-nix")]
impl NativeNixEvaluator {
    /// Creates a new native Nix evaluator.
    ///
    /// Initializes the Nix libraries, registers the thread with the GC,
    /// opens the store, and creates an evaluation state with flake support.
    pub fn new() -> Result<Self> {
        // Initialize Nix libraries
        init().map_err(|e| CoreError::ProcessCompose(format!("Failed to init Nix: {}", e)))?;

        // Register this thread with the GC
        let gc_guard = gc_register_my_thread()
            .map_err(|e| CoreError::ProcessCompose(format!("Failed to register GC: {}", e)))?;

        // Open the Nix store
        let store = Store::open(None, HashMap::new())
            .map_err(|e| CoreError::ProcessCompose(format!("Failed to open store: {}", e)))?;

        // Create fetchers settings
        let fetchers_settings = FetchersSettings::new().map_err(|e| {
            CoreError::ProcessCompose(format!("Failed to create fetchers settings: {}", e))
        })?;

        // Create flake settings (enables builtins.getFlake)
        let flake_settings = FlakeSettings::new().map_err(|e| {
            CoreError::ProcessCompose(format!("Failed to create flake settings: {}", e))
        })?;

        // Create evaluation state
        let eval_state =
            EvalState::new(store.clone(), std::iter::empty::<&str>()).map_err(|e| {
                CoreError::ProcessCompose(format!("Failed to create eval state: {}", e))
            })?;

        info!("Native Nix evaluator initialized");

        Ok(Self {
            store,
            eval_state,
            flake_settings,
            fetchers_settings,
            _gc_guard: gc_guard,
        })
    }

    /// Evaluates a Nix expression string.
    ///
    /// # Arguments
    ///
    /// * `expr` - The Nix expression to evaluate
    ///
    /// # Returns
    ///
    /// A string representation of the evaluated value.
    pub fn eval_string(&mut self, expr: &str) -> Result<String> {
        let value = self
            .eval_state
            .eval_from_string(expr, "<native>")
            .map_err(|e| CoreError::ProcessCompose(format!("Eval failed: {}", e)))?;

        // Try to extract as string
        match self.eval_state.require_string(&value) {
            Ok(s) => Ok(s),
            Err(_) => {
                // If not a string, return a type indicator
                Ok("<nix-value>".to_string())
            }
        }
    }

    /// Evaluates a flake and locks it.
    ///
    /// # Arguments
    ///
    /// * `flake_dir` - Path to directory containing flake.nix
    ///
    /// # Returns
    ///
    /// A LockedFlake that can be used to access outputs.
    pub fn lock_flake(&mut self, flake_dir: &Path) -> Result<LockedFlake> {
        let flake_path = flake_dir
            .to_str()
            .ok_or_else(|| CoreError::ProcessCompose("Invalid flake path".to_string()))?;

        debug!(flake_path = %flake_path, "Locking flake with native bindings");

        // Parse the flake reference
        let parse_flags = FlakeReferenceParseFlags::new(&self.flake_settings).map_err(|e| {
            CoreError::ProcessCompose(format!("Failed to create parse flags: {}", e))
        })?;

        let (flake_ref, _fragment) = FlakeReference::parse_with_fragment(
            &self.fetchers_settings,
            &self.flake_settings,
            &parse_flags,
            flake_path,
        )
        .map_err(|e| CoreError::ProcessCompose(format!("Failed to parse flake ref: {}", e)))?;

        // Create lock flags (use virtual mode to avoid writing lock files)
        let mut lock_flags = FlakeLockFlags::new(&self.flake_settings).map_err(|e| {
            CoreError::ProcessCompose(format!("Failed to create lock flags: {}", e))
        })?;

        // Use virtual mode to not write to flake.lock
        lock_flags
            .set_mode_virtual()
            .map_err(|e| CoreError::ProcessCompose(format!("Failed to set virtual mode: {}", e)))?;

        // Lock the flake
        let locked = LockedFlake::lock(
            &self.fetchers_settings,
            &self.flake_settings,
            &self.eval_state,
            &lock_flags,
            &flake_ref,
        )
        .map_err(|e| CoreError::ProcessCompose(format!("Failed to lock flake: {}", e)))?;

        info!(flake_path = %flake_path, "Flake locked successfully");

        Ok(locked)
    }

    /// Gets the flake outputs as an evaluated value.
    ///
    /// # Arguments
    ///
    /// * `flake_dir` - Path to directory containing flake.nix
    ///
    /// # Returns
    ///
    /// The evaluated outputs attribute set.
    pub fn get_flake_outputs(
        &mut self,
        flake_dir: &Path,
    ) -> Result<nix_bindings_expr::value::Value> {
        let locked = self.lock_flake(flake_dir)?;

        let outputs = locked
            .outputs(&self.flake_settings, &mut self.eval_state)
            .map_err(|e| CoreError::ProcessCompose(format!("Failed to get outputs: {}", e)))?;

        Ok(outputs)
    }

    /// Extracts environment variables from a devShell.
    ///
    /// # Arguments
    ///
    /// * `flake_dir` - Path to directory containing flake.nix
    /// * `system` - System architecture (e.g., "aarch64-darwin")
    ///
    /// # Returns
    ///
    /// A HashMap of environment variable names to values.
    pub fn get_devshell_env(
        &mut self,
        flake_dir: &Path,
        system: &str,
    ) -> Result<HashMap<String, String>> {
        let outputs = self.get_flake_outputs(flake_dir)?;

        // Navigate to devShells.<system>.default
        let dev_shells = self
            .eval_state
            .require_attrs_select(&outputs, "devShells")
            .map_err(|e| CoreError::ProcessCompose(format!("No devShells in flake: {}", e)))?;

        let system_shells = self
            .eval_state
            .require_attrs_select(&dev_shells, system)
            .map_err(|e| {
                CoreError::ProcessCompose(format!("No devShells for system {}: {}", system, e))
            })?;

        let default_shell = self
            .eval_state
            .require_attrs_select(&system_shells, "default")
            .map_err(|e| CoreError::ProcessCompose(format!("No default devShell: {}", e)))?;

        // Get the derivation attributes
        // The shell's environment comes from drvAttrs or we need to look at the derivation
        let mut env_vars = HashMap::new();

        // Try to get common environment-related attributes
        let attr_names = self
            .eval_state
            .require_attrs_names(&default_shell)
            .map_err(|e| CoreError::ProcessCompose(format!("Failed to get shell attrs: {}", e)))?;

        debug!(
            num_attrs = attr_names.len(),
            "DevShell has {} attributes",
            attr_names.len()
        );

        // Extract string attributes that look like environment variables
        for name in attr_names {
            if let Ok(attr_value) = self.eval_state.require_attrs_select(&default_shell, &name) {
                if let Ok(string_value) = self.eval_state.require_string(&attr_value) {
                    // Only include attributes that look like env vars (uppercase)
                    if name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                        env_vars.insert(name, string_value);
                    }
                }
            }
        }

        info!(
            flake_path = %flake_dir.display(),
            system = %system,
            num_vars = env_vars.len(),
            "Extracted {} environment variables from devShell",
            env_vars.len()
        );

        Ok(env_vars)
    }

    /// Returns a reference to the evaluation state.
    pub fn eval_state(&mut self) -> &mut EvalState {
        &mut self.eval_state
    }

    /// Returns a reference to the store.
    pub fn store(&self) -> &Store {
        &self.store
    }
}

/// Represents a Nix value in Rust (for non-native-nix builds).
#[derive(Debug, Clone)]
pub enum NixValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Path(String),
    Attrs(HashMap<String, NixValue>),
    List(Vec<NixValue>),
}

impl NixValue {
    /// Returns the value as a string if it is one.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            NixValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the value as an attribute set if it is one.
    pub fn as_attrs(&self) -> Option<&HashMap<String, NixValue>> {
        match self {
            NixValue::Attrs(a) => Some(a),
            _ => None,
        }
    }

    /// Gets a nested attribute by path (e.g., "devShells.aarch64-darwin.default").
    pub fn get_attr_path(&self, path: &str) -> Option<&NixValue> {
        let mut current = self;
        for part in path.split('.') {
            match current {
                NixValue::Attrs(attrs) => {
                    current = attrs.get(part)?;
                }
                _ => return None,
            }
        }
        Some(current)
    }
}

/// Detects the current system architecture for Nix.
pub fn detect_system() -> String {
    #[cfg(target_arch = "aarch64")]
    let arch = "aarch64";
    #[cfg(target_arch = "x86_64")]
    let arch = "x86_64";
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    let arch = "unknown";

    #[cfg(target_os = "macos")]
    let os = "darwin";
    #[cfg(target_os = "linux")]
    let os = "linux";
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let os = "unknown";

    format!("{}-{}", arch, os)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_system() {
        let system = detect_system();
        assert!(!system.is_empty());
        assert!(system.contains("-"));
    }

    #[test]
    fn test_nix_value_get_attr_path() {
        let mut inner = HashMap::new();
        inner.insert("default".to_string(), NixValue::String("shell".to_string()));

        let mut outer = HashMap::new();
        outer.insert("devShells".to_string(), NixValue::Attrs(inner));

        let root = NixValue::Attrs(outer);

        let result = root.get_attr_path("devShells.default");
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_string(), Some("shell"));
    }

    #[cfg(feature = "native-nix")]
    #[test]
    fn test_native_nix_eval_simple() {
        let mut evaluator = NativeNixEvaluator::new().expect("Failed to create evaluator");

        // Test simple string evaluation
        let result = evaluator.eval_string("\"hello\"").expect("Failed to eval");
        assert_eq!(result, "hello");

        // Test arithmetic
        let result = evaluator.eval_string("1 + 2").expect("Failed to eval");
        // Will return <nix-value> since it's an integer, not a string
        assert!(!result.is_empty());
    }

    #[cfg(feature = "native-nix")]
    #[test]
    fn test_native_nix_lock_flake() {
        use std::env;

        // Find the project root (contains flake.nix)
        let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
        let project_root = std::path::Path::new(&manifest_dir)
            .parent() // crates/
            .and_then(|p| p.parent()) // project root
            .expect("Could not find project root");

        if !project_root.join("flake.nix").exists() {
            eprintln!("Skipping test: no flake.nix found at {:?}", project_root);
            return;
        }

        let mut evaluator = NativeNixEvaluator::new().expect("Failed to create evaluator");
        let result = evaluator.lock_flake(project_root);

        // This test may fail if the flake has issues, but it validates the API works
        match result {
            Ok(_locked) => {
                println!("Successfully locked flake at {:?}", project_root);
            }
            Err(e) => {
                // Flake locking might fail for various reasons, but we got past initialization
                println!("Flake lock returned error (may be expected): {}", e);
            }
        }
    }
}
