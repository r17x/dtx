//! Plugin traits for backends and middleware.
//!
//! These traits define the interface that plugins must implement to provide
//! new resource backends or middleware.

use dtx_core::middleware::Middleware;
use dtx_core::resource::{Resource, ResourceKind};

use crate::error::PluginError;

/// Base trait for all plugins.
///
/// This trait provides common metadata and access to the plugin's components.
pub trait Plugin: Send + Sync {
    /// Unique name identifying this plugin.
    fn name(&self) -> &str;

    /// Plugin version string (semver).
    fn version(&self) -> &str;

    /// API version this plugin targets.
    fn api_version(&self) -> u32;

    /// Backend plugins provided by this plugin.
    fn backends(&self) -> Vec<Box<dyn BackendPlugin>>;

    /// Middleware plugins provided by this plugin.
    fn middleware(&self) -> Vec<Box<dyn MiddlewarePlugin>>;
}

/// A plugin that provides a new resource backend.
///
/// Backend plugins allow extending dtx with new resource types. For example,
/// a Docker backend plugin would provide `ResourceKind::Container` resources.
///
/// # Example
///
/// ```ignore
/// use dtx_core::resource::{Resource, ResourceKind};
/// use dtx_plugin::{BackendPlugin, PluginError};
///
/// struct DockerBackend;
///
/// impl BackendPlugin for DockerBackend {
///     fn name(&self) -> &str {
///         "docker"
///     }
///
///     fn resource_kind(&self) -> ResourceKind {
///         ResourceKind::Container
///     }
///
///     fn create_resource(
///         &self,
///         config: serde_json::Value,
///     ) -> Result<Box<dyn Resource>, PluginError> {
///         let container = DockerContainer::from_config(config)?;
///         Ok(Box::new(container))
///     }
/// }
/// ```
pub trait BackendPlugin: Send + Sync {
    /// Unique name identifying this backend.
    fn name(&self) -> &str;

    /// The kind of resource this backend provides.
    fn resource_kind(&self) -> ResourceKind;

    /// Create a new resource instance from configuration.
    ///
    /// The configuration is plugin-specific and should be documented
    /// by the plugin.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid or resource
    /// creation fails.
    fn create_resource(&self, config: serde_json::Value) -> Result<Box<dyn Resource>, PluginError>;
}

/// A plugin that provides middleware.
///
/// Middleware plugins add processing stages to the operation pipeline.
/// They can implement cross-cutting concerns like rate limiting, caching,
/// or custom logging.
///
/// # Example
///
/// ```ignore
/// use dtx_core::middleware::Middleware;
/// use dtx_plugin::MiddlewarePlugin;
///
/// struct RateLimiterPlugin {
///     requests_per_second: u32,
/// }
///
/// impl MiddlewarePlugin for RateLimiterPlugin {
///     fn name(&self) -> &str {
///         "rate-limiter"
///     }
///
///     fn create_middleware(&self) -> Box<dyn Middleware> {
///         Box::new(RateLimiterMiddleware::new(self.requests_per_second))
///     }
/// }
/// ```
pub trait MiddlewarePlugin: Send + Sync {
    /// Unique name identifying this middleware plugin.
    fn name(&self) -> &str;

    /// Create a new middleware instance.
    ///
    /// This is called when the middleware stack is being built.
    /// The returned middleware will be added to the operation pipeline.
    fn create_middleware(&self) -> Box<dyn Middleware>;
}

/// A plugin that provides resource translation.
///
/// Translator plugins enable converting resources from one type to another.
/// For example, translating a process definition to a container definition.
///
/// Note: This trait is a placeholder for Phase 6 translator support.
pub trait TranslatorPlugin: Send + Sync {
    /// Unique name identifying this translator.
    fn name(&self) -> &str;

    /// Source resource kind.
    fn source_kind(&self) -> ResourceKind;

    /// Target resource kind.
    fn target_kind(&self) -> ResourceKind;
}
