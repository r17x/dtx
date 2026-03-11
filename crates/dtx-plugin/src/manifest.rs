//! Plugin manifest types.
//!
//! A plugin manifest is a TOML file (`plugin.toml`) that describes a plugin's
//! metadata, type, entry point, and dependencies.
//!
//! # Example
//!
//! ```toml
//! name = "docker-backend"
//! version = "0.1.0"
//! description = "Docker container backend for dtx"
//! authors = ["Your Name <you@example.com>"]
//! plugin_type = "backend"
//! entry_point = "create_plugin"
//!
//! [[dependencies]]
//! name = "dtx-plugin"
//! version = "0.1.0"
//! ```

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::str::FromStr;

use crate::error::{PluginError, Result};

/// Plugin manifest loaded from `plugin.toml`.
///
/// Contains all metadata needed to identify, load, and validate a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique name of the plugin.
    pub name: String,

    /// Version string (semver recommended).
    pub version: String,

    /// Optional description of what the plugin does.
    #[serde(default)]
    pub description: Option<String>,

    /// List of authors.
    #[serde(default)]
    pub authors: Vec<String>,

    /// The type of plugin (backend, middleware, or translator).
    pub plugin_type: PluginType,

    /// Entry point symbol name for dynamic loading.
    ///
    /// For cdylib plugins, this is the symbol name that returns the plugin instance.
    /// The function signature should be `extern "C" fn() -> *mut dyn BackendPlugin`
    /// (or appropriate plugin trait).
    pub entry_point: String,

    /// Dependencies on other plugins.
    #[serde(default)]
    pub dependencies: Vec<PluginDependency>,
}

impl PluginManifest {
    /// Load a manifest from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path).map_err(PluginError::ManifestRead)?;
        content.parse()
    }

    /// Serialize the manifest to a TOML string.
    pub fn to_toml(&self) -> String {
        // toml::to_string_pretty won't fail for valid PluginManifest
        toml::to_string_pretty(self).expect("valid manifest serialization")
    }
}

impl FromStr for PluginManifest {
    type Err = PluginError;

    /// Parse a manifest from a TOML string.
    ///
    /// # Errors
    ///
    /// Returns an error if the TOML is invalid.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        toml::from_str(s).map_err(PluginError::ManifestParse)
    }
}

/// The type of plugin.
///
/// Determines what trait the plugin implements and how it's used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    /// Provides a new `ResourceKind` and backend implementation.
    Backend,

    /// Provides middleware for the operation stack.
    Middleware,

    /// Provides resource translation between types.
    Translator,
}

/// A dependency on another plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDependency {
    /// Name of the required plugin.
    pub name: String,

    /// Required version (semver constraint).
    pub version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_backend_manifest() {
        let toml = r#"
            name = "docker-backend"
            version = "0.1.0"
            description = "Docker container backend"
            authors = ["Test Author <test@example.com>"]
            plugin_type = "backend"
            entry_point = "create_docker_plugin"

            [[dependencies]]
            name = "dtx-plugin"
            version = "0.1.0"
        "#;

        let manifest = PluginManifest::from_str(toml).unwrap();

        assert_eq!(manifest.name, "docker-backend");
        assert_eq!(manifest.version, "0.1.0");
        assert_eq!(
            manifest.description,
            Some("Docker container backend".into())
        );
        assert_eq!(manifest.authors.len(), 1);
        assert_eq!(manifest.plugin_type, PluginType::Backend);
        assert_eq!(manifest.entry_point, "create_docker_plugin");
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].name, "dtx-plugin");
    }

    #[test]
    fn test_parse_middleware_manifest() {
        let toml = r#"
            name = "rate-limiter"
            version = "1.0.0"
            plugin_type = "middleware"
            entry_point = "create_rate_limiter"
        "#;

        let manifest = PluginManifest::from_str(toml).unwrap();

        assert_eq!(manifest.name, "rate-limiter");
        assert_eq!(manifest.plugin_type, PluginType::Middleware);
        assert!(manifest.description.is_none());
        assert!(manifest.authors.is_empty());
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn test_parse_translator_manifest() {
        let toml = r#"
            name = "process-to-container"
            version = "0.2.0"
            plugin_type = "translator"
            entry_point = "create_translator"
        "#;

        let manifest = PluginManifest::from_str(toml).unwrap();

        assert_eq!(manifest.name, "process-to-container");
        assert_eq!(manifest.plugin_type, PluginType::Translator);
    }

    #[test]
    fn test_manifest_roundtrip() {
        let manifest = PluginManifest {
            name: "test-plugin".into(),
            version: "1.2.3".into(),
            description: Some("A test plugin".into()),
            authors: vec!["Author One".into(), "Author Two".into()],
            plugin_type: PluginType::Backend,
            entry_point: "create_test_plugin".into(),
            dependencies: vec![PluginDependency {
                name: "other-plugin".into(),
                version: ">=1.0.0".into(),
            }],
        };

        let toml = manifest.to_toml();
        let parsed = PluginManifest::from_str(&toml).unwrap();

        assert_eq!(parsed.name, manifest.name);
        assert_eq!(parsed.version, manifest.version);
        assert_eq!(parsed.description, manifest.description);
        assert_eq!(parsed.authors.len(), manifest.authors.len());
        assert_eq!(parsed.plugin_type, manifest.plugin_type);
        assert_eq!(parsed.entry_point, manifest.entry_point);
        assert_eq!(parsed.dependencies.len(), manifest.dependencies.len());
    }

    #[test]
    fn test_invalid_manifest() {
        let toml = r#"
            name = "incomplete"
            # missing required fields
        "#;

        assert!(PluginManifest::from_str(toml).is_err());
    }
}
