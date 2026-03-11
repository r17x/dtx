//! Error types for dtx-core.

use thiserror::Error;

/// Result type alias using CoreError.
pub type Result<T> = std::result::Result<T, CoreError>;

/// Errors that can occur in dtx-core.
#[derive(Debug, Error)]
pub enum CoreError {
    /// YAML generation failed.
    #[error("YAML generation failed: {0}")]
    YamlGeneration(String),

    /// Process-compose related error.
    #[error("process-compose error: {0}")]
    ProcessCompose(String),

    /// Process-compose API error.
    #[error("process-compose API error: {0}")]
    Api(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_yaml::Error),

    /// HTTP request error.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Timeout error.
    #[error("timeout: {0}")]
    Timeout(String),

    /// Validation error.
    #[error("validation error: {0}")]
    Validation(String),

    /// Nix error.
    #[error(transparent)]
    Nix(#[from] NixError),

    /// Port conflict error - one or more ports are already in use.
    #[error("port conflict: {0}")]
    PortConflict(PortConflictError),
}

/// Details about port conflicts detected before starting services.
#[derive(Debug, Clone)]
pub struct PortConflictError {
    /// Ports that are already in use with details.
    pub conflicts: Vec<PortConflictDetail>,
}

impl std::fmt::Display for PortConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.conflicts.len() == 1 {
            write!(f, "{}", self.conflicts[0])
        } else {
            writeln!(f, "{} port conflicts detected:", self.conflicts.len())?;
            for (i, conflict) in self.conflicts.iter().enumerate() {
                write!(f, "  {}. {}", i + 1, conflict)?;
                if i < self.conflicts.len() - 1 {
                    writeln!(f)?;
                }
            }
            Ok(())
        }
    }
}

/// Detail about a single port conflict.
#[derive(Debug, Clone)]
pub struct PortConflictDetail {
    /// The conflicting port number.
    pub port: u16,
    /// Name of the service that wants this port.
    pub service_name: String,
    /// Process using the port (if known).
    pub used_by: Option<String>,
}

impl std::fmt::Display for PortConflictDetail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.used_by {
            Some(process) => write!(
                f,
                "port {} (wanted by '{}') is in use by {}",
                self.port, self.service_name, process
            ),
            None => write!(
                f,
                "port {} (wanted by '{}') is already in use",
                self.port, self.service_name
            ),
        }
    }
}

/// Errors specific to Nix operations.
#[derive(Debug, Error)]
pub enum NixError {
    /// Nix is not installed on the system.
    #[error("Nix is not installed. Install from https://nixos.org/download")]
    NixNotInstalled,

    /// Nix command execution failed.
    #[error("Nix command failed: {0}")]
    CommandFailed(String),

    /// Nix evaluation failed.
    #[error("Nix evaluation failed: {0}")]
    EvalFailed(String),

    /// Failed to parse Nix output or file.
    #[error("Failed to parse Nix output: {0}")]
    ParseError(String),

    /// Package not found in nixpkgs.
    #[error("Package not found: {0}")]
    PackageNotFound(String),

    /// IO error during Nix operations.
    #[error("IO error: {0}")]
    IoError(String),

    /// Invalid flake.lock format.
    #[error("Invalid flake.lock format: {0}")]
    InvalidLockFile(String),

    /// Native Nix bindings not available.
    #[error("Native Nix bindings not available (compile with --features native-nix)")]
    NativeBindingsNotAvailable,

    /// Native evaluation not yet implemented.
    #[error("Native evaluation not implemented")]
    NativeEvalNotImplemented,

    /// No flake.nix found in project.
    #[error("Flake not found in project")]
    FlakeNotFound,

    /// All search tiers exhausted without results.
    #[error("Search failed: {0}")]
    SearchFailed(String),

    /// Backend is not available.
    #[error("Nix backend unavailable: {0}")]
    BackendUnavailable(String),
}

impl From<std::io::Error> for NixError {
    fn from(err: std::io::Error) -> Self {
        NixError::IoError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = CoreError::YamlGeneration("invalid service".to_string());
        assert_eq!(err.to_string(), "YAML generation failed: invalid service");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: CoreError = io_err.into();
        assert!(matches!(err, CoreError::Io(_)));
    }
}
