//! Plugin error types.

use std::path::PathBuf;

/// Errors that can occur during plugin operations.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// Plugin manifest file not found.
    #[error("manifest not found at {0}")]
    ManifestNotFound(PathBuf),

    /// Failed to parse plugin manifest.
    #[error("failed to parse manifest: {0}")]
    ManifestParse(#[from] toml::de::Error),

    /// Failed to read manifest file.
    #[error("failed to read manifest: {0}")]
    ManifestRead(std::io::Error),

    /// Plugin not found.
    #[error("plugin not found: {0}")]
    PluginNotFound(String),

    /// Plugin already loaded.
    #[error("plugin already loaded: {0}")]
    AlreadyLoaded(String),

    /// Plugin dependency not satisfied.
    #[error("dependency not satisfied: {name} requires {dependency} {version}")]
    DependencyNotSatisfied {
        name: String,
        dependency: String,
        version: String,
    },

    /// Plugin type mismatch.
    #[error("plugin {name} is a {actual:?}, expected {expected:?}")]
    TypeMismatch {
        name: String,
        expected: crate::PluginType,
        actual: crate::PluginType,
    },

    /// Failed to create resource.
    #[error("failed to create resource: {0}")]
    ResourceCreation(String),

    /// Failed to create middleware.
    #[error("failed to create middleware: {0}")]
    MiddlewareCreation(String),

    /// Plugin directory does not exist.
    #[error("plugin directory does not exist: {0}")]
    DirectoryNotFound(PathBuf),

    /// Dynamic loading not available.
    #[error("dynamic loading not available: compile with 'dynamic' feature")]
    DynamicNotAvailable,

    /// Failed to load dynamic library.
    #[cfg(feature = "dynamic")]
    #[error("failed to load library: {0}")]
    LibraryLoad(#[from] libloading::Error),

    /// Symbol not found in dynamic library.
    #[error("symbol not found: {0}")]
    SymbolNotFound(String),

    /// Invalid entry point.
    #[error("invalid entry point: {0}")]
    InvalidEntryPoint(String),
}

/// Result type for plugin operations.
pub type Result<T> = std::result::Result<T, PluginError>;
