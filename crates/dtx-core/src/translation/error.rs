//! Translation error types.
//!
//! This module defines the error types used throughout the translation system.

use thiserror::Error;

/// Errors that can occur during resource translation.
#[derive(Debug, Error)]
pub enum TranslationError {
    /// Source resource is incompatible with target type.
    #[error("incompatible resource: {0}")]
    Incompatible(String),

    /// Required field is missing for translation.
    #[error("missing required field: {field}")]
    MissingField { field: String },

    /// Field value cannot be translated.
    #[error("untranslatable value for {field}: {reason}")]
    UntranslatableValue { field: String, reason: String },

    /// No translator registered for this type pair.
    #[error("no translator registered from {from} to {to}")]
    NoTranslator { from: String, to: String },

    /// Translation logic failed.
    #[error("translation failed: {0}")]
    Failed(String),

    /// Async translation was cancelled.
    #[error("translation cancelled")]
    Cancelled,

    /// Wrapped error from underlying operation.
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl TranslationError {
    /// Create an incompatible error.
    pub fn incompatible(msg: impl Into<String>) -> Self {
        Self::Incompatible(msg.into())
    }

    /// Create a missing field error.
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingField {
            field: field.into(),
        }
    }

    /// Create an untranslatable value error.
    pub fn untranslatable(field: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::UntranslatableValue {
            field: field.into(),
            reason: reason.into(),
        }
    }

    /// Create a no translator error.
    pub fn no_translator(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self::NoTranslator {
            from: from.into(),
            to: to.into(),
        }
    }

    /// Create a failed error.
    pub fn failed(msg: impl Into<String>) -> Self {
        Self::Failed(msg.into())
    }
}

/// Result type for translation operations.
pub type TranslationResult<T> = Result<T, TranslationError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_incompatible() {
        let err = TranslationError::incompatible("cannot convert VM to process");
        assert_eq!(
            err.to_string(),
            "incompatible resource: cannot convert VM to process"
        );
    }

    #[test]
    fn error_display_missing_field() {
        let err = TranslationError::missing_field("command");
        assert_eq!(err.to_string(), "missing required field: command");
    }

    #[test]
    fn error_display_untranslatable() {
        let err = TranslationError::untranslatable("port", "negative value");
        assert_eq!(
            err.to_string(),
            "untranslatable value for port: negative value"
        );
    }

    #[test]
    fn error_display_no_translator() {
        let err = TranslationError::no_translator("Process", "VM");
        assert!(err.to_string().contains("Process"));
        assert!(err.to_string().contains("VM"));
    }

    #[test]
    fn error_display_failed() {
        let err = TranslationError::failed("unexpected error");
        assert_eq!(err.to_string(), "translation failed: unexpected error");
    }

    #[test]
    fn error_display_cancelled() {
        let err = TranslationError::Cancelled;
        assert_eq!(err.to_string(), "translation cancelled");
    }

    #[test]
    fn error_from_boxed() {
        let io_err: Box<dyn std::error::Error + Send + Sync> = Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file not found",
        ));
        let err: TranslationError = io_err.into();
        assert!(matches!(err, TranslationError::Other(_)));
    }
}
