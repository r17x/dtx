//! Error types for the VM backend.

use std::path::PathBuf;
use std::time::Duration;

/// Result type for VM operations.
pub type Result<T> = std::result::Result<T, VmError>;

/// Errors that can occur during VM operations.
#[derive(Debug, thiserror::Error)]
pub enum VmError {
    /// VM runtime is not available.
    #[error("VM runtime '{0}' is not available")]
    RuntimeUnavailable(String),

    /// VM not found.
    #[error("VM '{0}' not found")]
    NotFound(String),

    /// VM already exists.
    #[error("VM '{0}' already exists")]
    AlreadyExists(String),

    /// VM is already running.
    #[error("VM '{0}' is already running")]
    AlreadyRunning(String),

    /// VM is not running.
    #[error("VM '{0}' is not running")]
    NotRunning(String),

    /// Invalid VM configuration.
    #[error("Invalid VM configuration: {0}")]
    InvalidConfig(String),

    /// VM image not found.
    #[error("VM image not found: {}", .0.display())]
    ImageNotFound(PathBuf),

    /// Failed to prepare VM image.
    #[error("Failed to prepare VM image: {0}")]
    ImagePreparation(String),

    /// QEMU error.
    #[error("QEMU error: {0}")]
    Qemu(String),

    /// QMP (QEMU Machine Protocol) error.
    #[error("QMP error: {0}")]
    Qmp(String),

    /// Firecracker error.
    #[error("Firecracker error: {0}")]
    Firecracker(String),

    /// NixOS VM error.
    #[error("NixOS VM error: {0}")]
    NixOS(String),

    /// SSH error.
    #[error("SSH error: {0}")]
    Ssh(String),

    /// Timeout waiting for VM.
    #[error("Timeout after {0:?} waiting for VM")]
    Timeout(Duration),

    /// Snapshot operation failed.
    #[error("Snapshot error: {0}")]
    Snapshot(String),

    /// Feature not supported by runtime.
    #[error("{0} is not supported by {1} runtime")]
    NotSupported(String, String),

    /// Process spawn error.
    #[error("Failed to spawn process: {0}")]
    Spawn(#[from] std::io::Error),

    /// JSON serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Generic backend error.
    #[error("Backend error: {0}")]
    Backend(String),
}

impl VmError {
    /// Create a configuration error.
    pub fn config(msg: impl Into<String>) -> Self {
        Self::InvalidConfig(msg.into())
    }

    /// Create a backend error.
    pub fn backend(msg: impl Into<String>) -> Self {
        Self::Backend(msg.into())
    }

    /// Create a timeout error.
    pub fn timeout(duration: Duration) -> Self {
        Self::Timeout(duration)
    }

    /// Create a not-supported error.
    pub fn not_supported(feature: impl Into<String>, runtime: impl Into<String>) -> Self {
        Self::NotSupported(feature.into(), runtime.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = VmError::RuntimeUnavailable("qemu".to_string());
        assert_eq!(err.to_string(), "VM runtime 'qemu' is not available");

        let err = VmError::NotFound("my-vm".to_string());
        assert_eq!(err.to_string(), "VM 'my-vm' not found");

        let err = VmError::Timeout(Duration::from_secs(30));
        assert_eq!(err.to_string(), "Timeout after 30s waiting for VM");
    }

    #[test]
    fn error_constructors() {
        let err = VmError::config("missing image");
        assert!(matches!(err, VmError::InvalidConfig(_)));

        let err = VmError::backend("connection failed");
        assert!(matches!(err, VmError::Backend(_)));

        let err = VmError::not_supported("snapshots", "nixos");
        assert!(matches!(err, VmError::NotSupported(_, _)));
    }
}
