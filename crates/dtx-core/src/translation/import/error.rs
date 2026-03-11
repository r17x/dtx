//! Import error types.

use thiserror::Error;

/// Import-specific errors.
#[derive(Debug, Error)]
pub enum ImportError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Parse error with location.
    #[error("parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    /// Unsupported feature.
    #[error("unsupported feature: {0}")]
    Unsupported(String),

    /// Validation error.
    #[error("validation error: {0}")]
    Validation(String),

    /// Unknown format.
    #[error("unable to detect configuration format")]
    UnknownFormat,

    /// YAML parse error.
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    /// Merge conflict.
    #[error("merge conflict for '{name}': {reason}")]
    MergeConflict { name: String, reason: String },
}

impl ImportError {
    /// Create a parse error.
    pub fn parse(line: usize, message: impl Into<String>) -> Self {
        Self::Parse {
            line,
            message: message.into(),
        }
    }

    /// Create an unsupported feature error.
    pub fn unsupported(feature: impl Into<String>) -> Self {
        Self::Unsupported(feature.into())
    }

    /// Create a validation error.
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// Create a merge conflict error.
    pub fn merge_conflict(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::MergeConflict {
            name: name.into(),
            reason: reason.into(),
        }
    }
}

/// Result type for import operations.
pub type ImportResult<T> = Result<T, ImportError>;
