//! Translation context for configuring translation behavior.
//!
//! This module provides types for controlling how translations are performed.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Context for translation operations.
///
/// Provides configuration, mappings, and defaults that influence
/// how translations are performed.
#[derive(Clone, Debug, Default)]
pub struct TranslationContext {
    /// Field name mappings (source field → target field).
    pub mappings: HashMap<String, String>,
    /// Default values for missing fields.
    pub defaults: HashMap<String, serde_json::Value>,
    /// Translation options.
    pub options: TranslationOptions,
    /// Custom metadata for translator-specific use.
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Options controlling translation behavior.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranslationOptions {
    /// Preserve fields that can't be translated (as metadata).
    #[serde(default)]
    pub preserve_unknown: bool,

    /// Fail on any untranslatable field (strict mode).
    #[serde(default)]
    pub strict: bool,

    /// Transform shell commands (e.g., expand variables).
    #[serde(default)]
    pub transform_commands: bool,

    /// Infer missing values where possible.
    #[serde(default = "default_true")]
    pub infer_values: bool,

    /// Target environment (affects command transformation).
    #[serde(default)]
    pub target_env: TargetEnvironment,
}

fn default_true() -> bool {
    true
}

impl Default for TranslationOptions {
    fn default() -> Self {
        Self {
            preserve_unknown: false,
            strict: false,
            transform_commands: true,
            infer_values: true,
            target_env: TargetEnvironment::default(),
        }
    }
}

/// Target environment for translation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetEnvironment {
    /// Local development.
    #[default]
    Local,
    /// Docker/container environment.
    Docker,
    /// Kubernetes.
    Kubernetes,
    /// Production.
    Production,
}

impl TranslationContext {
    /// Create a new empty context.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create context with options.
    pub fn with_options(options: TranslationOptions) -> Self {
        Self {
            options,
            ..Default::default()
        }
    }

    /// Create a strict context (fails on any issue).
    pub fn strict() -> Self {
        Self {
            options: TranslationOptions {
                strict: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Add a field mapping.
    pub fn map_field(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.mappings.insert(from.into(), to.into());
        self
    }

    /// Add a default value.
    pub fn default_value<T: Serialize>(mut self, field: impl Into<String>, value: T) -> Self {
        if let Ok(json) = serde_json::to_value(value) {
            self.defaults.insert(field.into(), json);
        }
        self
    }

    /// Get a mapped field name.
    pub fn get_mapping(&self, key: &str) -> Option<&str> {
        self.mappings.get(key).map(|s| s.as_str())
    }

    /// Get a default value.
    pub fn get_default<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.defaults
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Set metadata.
    pub fn set_metadata<T: Serialize>(mut self, key: impl Into<String>, value: T) -> Self {
        if let Ok(json) = serde_json::to_value(value) {
            self.metadata.insert(key.into(), json);
        }
        self
    }

    /// Get metadata.
    pub fn get_metadata<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.metadata
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    /// Set target environment.
    pub fn for_environment(mut self, env: TargetEnvironment) -> Self {
        self.options.target_env = env;
        self
    }

    /// Enable strict mode.
    pub fn enable_strict(mut self) -> Self {
        self.options.strict = true;
        self
    }

    /// Disable value inference.
    pub fn disable_inference(mut self) -> Self {
        self.options.infer_values = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_new() {
        let ctx = TranslationContext::new();
        assert!(ctx.mappings.is_empty());
        assert!(ctx.defaults.is_empty());
        assert!(!ctx.options.strict);
    }

    #[test]
    fn context_field_mapping() {
        let ctx = TranslationContext::new()
            .map_field("cmd", "command")
            .map_field("env", "environment");

        assert_eq!(ctx.get_mapping("cmd"), Some("command"));
        assert_eq!(ctx.get_mapping("env"), Some("environment"));
        assert_eq!(ctx.get_mapping("unknown"), None);
    }

    #[test]
    fn context_default_values() {
        let ctx = TranslationContext::new()
            .default_value("port", 8080u16)
            .default_value("replicas", 1u32)
            .default_value("image", "alpine:latest".to_string());

        assert_eq!(ctx.get_default::<u16>("port"), Some(8080));
        assert_eq!(ctx.get_default::<u32>("replicas"), Some(1));
        assert_eq!(
            ctx.get_default::<String>("image"),
            Some("alpine:latest".to_string())
        );
        assert_eq!(ctx.get_default::<u16>("missing"), None);
    }

    #[test]
    fn context_strict_mode() {
        let ctx = TranslationContext::strict();
        assert!(ctx.options.strict);
    }

    #[test]
    fn context_with_options() {
        let opts = TranslationOptions {
            strict: true,
            preserve_unknown: true,
            ..Default::default()
        };
        let ctx = TranslationContext::with_options(opts);
        assert!(ctx.options.strict);
        assert!(ctx.options.preserve_unknown);
    }

    #[test]
    fn context_metadata() {
        let ctx = TranslationContext::new()
            .set_metadata("custom_key", "custom_value".to_string())
            .set_metadata("count", 42i32);

        assert_eq!(
            ctx.get_metadata::<String>("custom_key"),
            Some("custom_value".to_string())
        );
        assert_eq!(ctx.get_metadata::<i32>("count"), Some(42));
    }

    #[test]
    fn context_target_environment() {
        let ctx = TranslationContext::new().for_environment(TargetEnvironment::Kubernetes);
        assert_eq!(ctx.options.target_env, TargetEnvironment::Kubernetes);
    }

    #[test]
    fn context_enable_strict() {
        let ctx = TranslationContext::new().enable_strict();
        assert!(ctx.options.strict);
    }

    #[test]
    fn context_disable_inference() {
        let ctx = TranslationContext::new().disable_inference();
        assert!(!ctx.options.infer_values);
    }

    #[test]
    fn options_default() {
        let opts = TranslationOptions::default();
        assert!(!opts.preserve_unknown);
        assert!(!opts.strict);
        assert!(opts.transform_commands);
        assert!(opts.infer_values);
        assert_eq!(opts.target_env, TargetEnvironment::Local);
    }

    #[test]
    fn target_environment_serialization() {
        let env = TargetEnvironment::Docker;
        let json = serde_json::to_string(&env).unwrap();
        assert_eq!(json, "\"docker\"");

        let parsed: TargetEnvironment = serde_json::from_str("\"kubernetes\"").unwrap();
        assert_eq!(parsed, TargetEnvironment::Kubernetes);
    }
}
