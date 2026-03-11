//! Typed error hierarchy for resource operations.
//!
//! This module provides comprehensive error types for resource lifecycle,
//! dependency management, and configuration.

use std::fmt::Write;
use std::time::Duration;
use thiserror::Error;

use super::id::ResourceId;
use super::kind::ResourceKind;

/// Core error type for resource operations.
#[derive(Debug, Error)]
pub enum Error {
    // Resource errors
    /// Resource not found.
    #[error("resource not found: {id}")]
    ResourceNotFound { id: ResourceId },

    /// Resource already exists.
    #[error("resource already exists: {id}")]
    ResourceExists { id: ResourceId },

    /// Resource is in invalid state for the requested operation.
    #[error("resource {id} is in invalid state {current} for operation {operation}")]
    InvalidState {
        id: ResourceId,
        current: String,
        operation: String,
    },

    // Lifecycle errors
    /// Failed to start resource.
    #[error("failed to start resource {id}: {reason}")]
    StartFailed { id: ResourceId, reason: String },

    /// Failed to stop resource.
    #[error("failed to stop resource {id}: {reason}")]
    StopFailed { id: ResourceId, reason: String },

    /// Health check failed.
    #[error("health check failed for {id}: {reason}")]
    HealthCheckFailed { id: ResourceId, reason: String },

    // Dependency errors
    /// Dependency cycle detected.
    #[error("dependency cycle detected: {cycle:?}")]
    DependencyCycle { cycle: Vec<ResourceId> },

    /// Missing dependency.
    #[error("missing dependency {dependency} for resource {id}")]
    MissingDependency {
        id: ResourceId,
        dependency: ResourceId,
    },

    /// Dependency failed.
    #[error("dependency {dependency} failed, cannot start {id}")]
    DependencyFailed {
        id: ResourceId,
        dependency: ResourceId,
    },

    // Configuration errors
    /// Configuration error.
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    // Context errors
    /// Operation timed out.
    #[error("operation timed out after {elapsed:?}")]
    Timeout { elapsed: Duration },

    /// Operation cancelled.
    #[error("operation cancelled")]
    Cancelled,

    // Backend errors
    /// Backend-specific error.
    #[error("backend error ({kind}): {message}")]
    Backend {
        kind: ResourceKind,
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    // IO errors
    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    // Serialization errors
    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),

    // Plugin errors
    /// Plugin error.
    #[error("plugin error ({plugin}): {message}")]
    Plugin { plugin: String, message: String },

    // Generic with context
    /// Error with additional context.
    #[error("{context}: {source}")]
    WithContext {
        context: String,
        #[source]
        source: Box<Error>,
    },
}

/// Configuration-specific errors.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Missing required field.
    #[error("missing required field: {0}")]
    MissingField(String),

    /// Invalid value for field.
    #[error("invalid value for {field}: {reason}")]
    InvalidValue { field: String, reason: String },

    /// Parse error.
    #[error("parse error: {0}")]
    Parse(String),

    /// Validation failed.
    #[error("validation failed: {0}")]
    Validation(String),
}

/// Result type alias for resource operations.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Add context to an error.
    pub fn context(self, ctx: impl Into<String>) -> Self {
        Error::WithContext {
            context: ctx.into(),
            source: Box::new(self),
        }
    }

    /// Create a backend error.
    pub fn backend(kind: ResourceKind, message: impl Into<String>) -> Self {
        Error::Backend {
            kind,
            message: message.into(),
            source: None,
        }
    }

    /// Create a backend error with source.
    pub fn backend_with_source(
        kind: ResourceKind,
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Error::Backend {
            kind,
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Check if error is retryable.
    ///
    /// Retryable errors are transient failures that may succeed on retry:
    /// - Timeouts
    /// - IO errors (network, filesystem)
    /// - Backend errors (may be transient)
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::Timeout { .. } | Error::Io(_) | Error::Backend { .. }
        )
    }

    /// Get the resource ID if applicable.
    pub fn resource_id(&self) -> Option<&ResourceId> {
        match self {
            Error::ResourceNotFound { id }
            | Error::ResourceExists { id }
            | Error::InvalidState { id, .. }
            | Error::StartFailed { id, .. }
            | Error::StopFailed { id, .. }
            | Error::HealthCheckFailed { id, .. }
            | Error::MissingDependency { id, .. }
            | Error::DependencyFailed { id, .. } => Some(id),
            _ => None,
        }
    }

    /// Format error with full chain for debugging.
    pub fn debug_chain(&self) -> String {
        let mut output = String::new();
        let mut current: &dyn std::error::Error = self;
        let mut depth = 0;

        loop {
            if depth > 0 {
                write!(output, "\n  caused by: ").unwrap();
            }
            write!(output, "{}", current).unwrap();

            match current.source() {
                Some(source) => {
                    current = source;
                    depth += 1;
                }
                None => break,
            }
        }

        output
    }

    /// Format error as JSON for API responses.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "error": {
                "type": self.error_type(),
                "message": self.to_string(),
                "resource_id": self.resource_id().map(|id| id.as_str()),
                "retryable": self.is_retryable(),
            }
        })
    }

    /// Get the error type as a string.
    pub fn error_type(&self) -> &'static str {
        match self {
            Error::ResourceNotFound { .. } => "resource_not_found",
            Error::ResourceExists { .. } => "resource_exists",
            Error::InvalidState { .. } => "invalid_state",
            Error::StartFailed { .. } => "start_failed",
            Error::StopFailed { .. } => "stop_failed",
            Error::HealthCheckFailed { .. } => "health_check_failed",
            Error::DependencyCycle { .. } => "dependency_cycle",
            Error::MissingDependency { .. } => "missing_dependency",
            Error::DependencyFailed { .. } => "dependency_failed",
            Error::Config(_) => "config_error",
            Error::Timeout { .. } => "timeout",
            Error::Cancelled => "cancelled",
            Error::Backend { .. } => "backend_error",
            Error::Io(_) => "io_error",
            Error::Serialization(_) => "serialization_error",
            Error::Plugin { .. } => "plugin_error",
            Error::WithContext { .. } => "internal_error",
        }
    }
}

/// Extension trait for adding context to Results.
pub trait ResultExt<T> {
    /// Add context to an error.
    fn context(self, ctx: impl Into<String>) -> Result<T>;

    /// Add context lazily.
    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

impl<T> ResultExt<T> for Result<T> {
    fn context(self, ctx: impl Into<String>) -> Result<T> {
        self.map_err(|e| e.context(ctx))
    }

    fn with_context<F: FnOnce() -> String>(self, f: F) -> Result<T> {
        self.map_err(|e| e.context(f()))
    }
}

/// Extension trait for wrapping std::io::Result with context.
pub trait IoResultExt<T> {
    /// Wrap IO error with context.
    fn io_context(self, ctx: impl Into<String>) -> Result<T>;
}

impl<T> IoResultExt<T> for std::io::Result<T> {
    fn io_context(self, ctx: impl Into<String>) -> Result<T> {
        self.map_err(|e| {
            let context = ctx.into();
            Error::WithContext {
                context,
                source: Box::new(Error::Io(e)),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_context() {
        let err = Error::ResourceNotFound {
            id: ResourceId::new("api"),
        };
        let wrapped = err.context("while starting services");
        assert!(wrapped.to_string().contains("while starting services"));
        assert!(wrapped.to_string().contains("resource not found"));
    }

    #[test]
    fn error_retryable() {
        let timeout = Error::Timeout {
            elapsed: Duration::from_secs(30),
        };
        assert!(timeout.is_retryable());

        let not_found = Error::ResourceNotFound {
            id: ResourceId::new("api"),
        };
        assert!(!not_found.is_retryable());

        let io_error = Error::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "connection refused",
        ));
        assert!(io_error.is_retryable());
    }

    #[test]
    fn error_resource_id() {
        let err = Error::StartFailed {
            id: ResourceId::new("api"),
            reason: "port in use".into(),
        };
        assert_eq!(err.resource_id().map(|id| id.as_str()), Some("api"));

        let err = Error::Cancelled;
        assert!(err.resource_id().is_none());
    }

    #[test]
    fn error_type() {
        let err = Error::ResourceNotFound {
            id: ResourceId::new("api"),
        };
        assert_eq!(err.error_type(), "resource_not_found");

        let err = Error::Timeout {
            elapsed: Duration::from_secs(30),
        };
        assert_eq!(err.error_type(), "timeout");
    }

    #[test]
    fn error_to_json() {
        let err = Error::ResourceNotFound {
            id: ResourceId::new("api"),
        };
        let json = err.to_json();
        assert_eq!(json["error"]["type"], "resource_not_found");
        assert_eq!(json["error"]["resource_id"], "api");
        assert_eq!(json["error"]["retryable"], false);
    }

    #[test]
    fn error_debug_chain() {
        let inner = Error::ResourceNotFound {
            id: ResourceId::new("postgres"),
        };
        let outer = inner.context("while resolving dependencies");
        let chain = outer.debug_chain();
        assert!(chain.contains("while resolving dependencies"));
        assert!(chain.contains("caused by"));
        assert!(chain.contains("resource not found: postgres"));
    }

    #[test]
    fn result_ext_context() {
        let result: Result<()> = Err(Error::Cancelled);
        let wrapped = result.context("during startup");
        assert!(wrapped.unwrap_err().to_string().contains("during startup"));
    }

    #[test]
    fn result_ext_with_context() {
        let result: Result<()> = Err(Error::Cancelled);
        let wrapped = result.with_context(|| format!("at step {}", 5));
        assert!(wrapped.unwrap_err().to_string().contains("at step 5"));
    }

    #[test]
    fn config_error() {
        let err: Error = ConfigError::MissingField("port".into()).into();
        assert!(err.to_string().contains("missing required field: port"));
    }

    #[test]
    fn backend_error() {
        let err = Error::backend(ResourceKind::Container, "container not found");
        assert!(err.to_string().contains("backend error"));
        assert!(err.to_string().contains("container"));
    }

    #[test]
    fn dependency_cycle() {
        let err = Error::DependencyCycle {
            cycle: vec![
                ResourceId::new("a"),
                ResourceId::new("b"),
                ResourceId::new("c"),
            ],
        };
        assert!(err.to_string().contains("dependency cycle detected"));
    }
}
