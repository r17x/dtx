//! Configuration trait and validation for resources.

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

/// Configuration for a resource.
///
/// Implementations should validate their configuration when constructed
/// or when `validate()` is called.
pub trait ResourceConfig: Clone + Send + Sync + Serialize + DeserializeOwned {
    /// Validate the configuration.
    ///
    /// Returns `Ok(())` if the configuration is valid, or an error describing
    /// what is invalid.
    fn validate(&self) -> Result<(), ConfigError>;
}

/// Errors that can occur during configuration validation.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ConfigError {
    /// A required field is missing.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// A field has an invalid value.
    #[error("invalid value for {field}: {reason}")]
    InvalidValue {
        /// The field name.
        field: String,
        /// Why the value is invalid.
        reason: String,
    },

    /// Custom validation error.
    #[error("validation failed: {0}")]
    Custom(String),
}

impl ConfigError {
    /// Create a missing field error.
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField(field.into())
    }

    /// Create an invalid value error.
    pub fn invalid_value(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidValue {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Create a custom validation error.
    pub fn custom(message: impl Into<String>) -> Self {
        Self::Custom(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_missing_field() {
        let err = ConfigError::missing_field("command");
        assert_eq!(err.to_string(), "missing required field: command");
    }

    #[test]
    fn config_error_invalid_value() {
        let err = ConfigError::invalid_value("port", "must be between 1 and 65535");
        assert_eq!(
            err.to_string(),
            "invalid value for port: must be between 1 and 65535"
        );
    }

    #[test]
    fn config_error_custom() {
        let err = ConfigError::custom("dependency cycle detected");
        assert_eq!(
            err.to_string(),
            "validation failed: dependency cycle detected"
        );
    }

    #[test]
    fn config_error_equality() {
        let a = ConfigError::missing_field("command");
        let b = ConfigError::missing_field("command");
        let c = ConfigError::missing_field("port");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
