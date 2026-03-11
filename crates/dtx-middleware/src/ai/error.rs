//! AI provider error types.

use thiserror::Error;

/// Error type for AI operations.
#[derive(Debug, Error)]
pub enum AIError {
    /// HTTP request failed.
    #[error("HTTP request failed: {0}")]
    Http(String),

    /// API returned an error.
    #[error("API error: {message} (code: {code})")]
    Api { code: String, message: String },

    /// Rate limited by provider.
    #[error("Rate limited, retry after {retry_after:?} seconds")]
    RateLimited { retry_after: Option<u64> },

    /// Invalid response from provider.
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// Serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Provider not configured.
    #[error("AI provider not configured")]
    NotConfigured,

    /// Provider not available (feature not enabled).
    #[error("AI provider not available: {0}")]
    NotAvailable(String),

    /// Timeout waiting for response.
    #[error("Request timed out")]
    Timeout,

    /// Other error.
    #[error("{0}")]
    Other(String),
}

impl AIError {
    /// Check if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Http(_) | Self::RateLimited { .. } | Self::Timeout
        )
    }

    /// Get retry delay if rate limited.
    pub fn retry_after(&self) -> Option<std::time::Duration> {
        match self {
            Self::RateLimited {
                retry_after: Some(secs),
            } => Some(std::time::Duration::from_secs(*secs)),
            _ => None,
        }
    }
}

#[cfg(feature = "ai")]
impl From<reqwest::Error> for AIError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Self::Timeout
        } else {
            Self::Http(err.to_string())
        }
    }
}

impl From<serde_json::Error> for AIError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = AIError::Api {
            code: "invalid_api_key".to_string(),
            message: "Invalid API key".to_string(),
        };
        assert!(err.to_string().contains("Invalid API key"));
    }

    #[test]
    fn error_retryable() {
        assert!(AIError::Timeout.is_retryable());
        assert!(AIError::RateLimited { retry_after: None }.is_retryable());
        assert!(!AIError::NotConfigured.is_retryable());
    }

    #[test]
    fn error_retry_after() {
        let err = AIError::RateLimited {
            retry_after: Some(30),
        };
        assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(30)));
    }
}
