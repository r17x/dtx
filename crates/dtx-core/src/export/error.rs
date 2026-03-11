//! Export error types.
//!
//! This module defines errors that can occur during export operations.

use std::io;

use thiserror::Error;

use crate::translation::TranslationError;

/// Errors that can occur during export.
#[derive(Debug, Error)]
pub enum ExportError {
    /// Translation error during export.
    #[error("translation error: {0}")]
    Translation(#[from] TranslationError),

    /// Serialization error (YAML, JSON, etc.).
    #[error("serialization error: {0}")]
    Serialization(String),

    /// I/O error during file operations.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// Export format not supported.
    #[error("unsupported export format: {0}")]
    Unsupported(String),

    /// Validation error in exported configuration.
    #[error("validation error: {0}")]
    Validation(String),

    /// Missing required field.
    #[error("missing required field: {0}")]
    Missing(String),

    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),
}

impl ExportError {
    /// Create a serialization error.
    pub fn serialization(msg: impl Into<String>) -> Self {
        Self::Serialization(msg.into())
    }

    /// Create an unsupported format error.
    pub fn unsupported(format: impl Into<String>) -> Self {
        Self::Unsupported(format.into())
    }

    /// Create a validation error.
    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Validation(msg.into())
    }

    /// Create a missing field error.
    pub fn missing(field: impl Into<String>) -> Self {
        Self::Missing(field.into())
    }

    /// Create a configuration error.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }
}

/// Result type for export operations.
pub type ExportResult<T> = Result<T, ExportError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translation_error_conversion() {
        let te = TranslationError::missing_field("image");
        let ee: ExportError = te.into();
        assert!(matches!(ee, ExportError::Translation(_)));
    }

    #[test]
    fn serialization_error() {
        let err = ExportError::serialization("invalid YAML");
        assert!(err.to_string().contains("serialization"));
    }

    #[test]
    fn io_error_conversion() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let ee: ExportError = io_err.into();
        assert!(matches!(ee, ExportError::Io(_)));
    }

    #[test]
    fn unsupported_error() {
        let err = ExportError::unsupported("custom-format");
        assert!(err.to_string().contains("unsupported"));
    }

    #[test]
    fn validation_error() {
        let err = ExportError::validation("port must be positive");
        assert!(err.to_string().contains("validation"));
    }

    #[test]
    fn missing_field_error() {
        let err = ExportError::missing("service_name");
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn config_error() {
        let err = ExportError::config("invalid configuration");
        assert!(err.to_string().contains("configuration"));
    }
}
