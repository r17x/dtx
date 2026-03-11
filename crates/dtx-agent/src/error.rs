//! Error types for the agent crate.

use std::time::Duration;
use thiserror::Error;

/// Result type alias for agent operations.
pub type Result<T> = std::result::Result<T, AgentError>;

/// Errors that can occur in agent operations.
#[derive(Debug, Error)]
pub enum AgentError {
    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(String),

    /// Backend/runtime error.
    #[error("backend error: {0}")]
    Backend(String),

    /// API error from provider.
    #[error("API error: {status} - {message}")]
    Api { status: u16, message: String },

    /// Rate limited by provider.
    #[error("rate limited: retry after {retry_after:?}")]
    RateLimited { retry_after: Option<Duration> },

    /// Operation timed out.
    #[error("operation timed out after {0:?}")]
    Timeout(Duration),

    /// Tool execution error.
    #[error("tool execution failed: {tool} - {reason}")]
    ToolExecution { tool: String, reason: String },

    /// Invalid message format.
    #[error("invalid message: {0}")]
    InvalidMessage(String),

    /// Agent not running.
    #[error("agent not running")]
    NotRunning,

    /// Runtime not available.
    #[error("runtime not available: {0}")]
    RuntimeUnavailable(String),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// HTTP request error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl AgentError {
    /// Create a configuration error.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Create a backend error.
    pub fn backend(msg: impl Into<String>) -> Self {
        Self::Backend(msg.into())
    }

    /// Create an API error.
    pub fn api(status: u16, message: impl Into<String>) -> Self {
        Self::Api {
            status,
            message: message.into(),
        }
    }

    /// Create a timeout error.
    pub fn timeout(duration: Duration) -> Self {
        Self::Timeout(duration)
    }

    /// Create a tool execution error.
    pub fn tool_execution(tool: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::ToolExecution {
            tool: tool.into(),
            reason: reason.into(),
        }
    }

    /// Check if this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. } | Self::Timeout(_) | Self::Http(_)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_config() {
        let err = AgentError::config("missing API key");
        assert!(err.to_string().contains("missing API key"));
    }

    #[test]
    fn error_backend() {
        let err = AgentError::backend("connection failed");
        assert!(err.to_string().contains("connection failed"));
    }

    #[test]
    fn error_api() {
        let err = AgentError::api(401, "Unauthorized");
        assert!(err.to_string().contains("401"));
        assert!(err.to_string().contains("Unauthorized"));
    }

    #[test]
    fn error_timeout() {
        let err = AgentError::timeout(Duration::from_secs(30));
        assert!(err.to_string().contains("30"));
    }

    #[test]
    fn error_tool_execution() {
        let err = AgentError::tool_execution("read_file", "file not found");
        assert!(err.to_string().contains("read_file"));
        assert!(err.to_string().contains("file not found"));
    }

    #[test]
    fn error_retryable() {
        let rate_limited = AgentError::RateLimited {
            retry_after: Some(Duration::from_secs(60)),
        };
        assert!(rate_limited.is_retryable());

        let timeout = AgentError::timeout(Duration::from_secs(30));
        assert!(timeout.is_retryable());

        let config = AgentError::config("bad config");
        assert!(!config.is_retryable());
    }
}
