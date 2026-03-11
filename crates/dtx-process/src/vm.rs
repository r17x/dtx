//! VM resource implementation (placeholder).
//!
//! This module provides a stub VMResource implementation that returns
//! "not implemented" errors. It defines the type structure for future
//! implementation in Phase 8.2.

use std::any::Any;

use async_trait::async_trait;
use dtx_core::resource::{
    Context, HealthStatus, LogStream, Resource, ResourceError, ResourceId, ResourceKind,
    ResourceResult, ResourceState,
};
use serde::{Deserialize, Serialize};

/// Configuration for a VM resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VMResourceConfig {
    /// Unique identifier for this VM.
    pub id: ResourceId,
    /// VM image path or identifier.
    pub image: String,
    /// Memory allocation in MB.
    pub memory_mb: u64,
    /// Number of virtual CPUs.
    pub cpus: u32,
    /// Disk size in GB (optional).
    pub disk_size_gb: Option<u64>,
    /// Network configuration (e.g., "bridge", "host").
    pub network: Option<String>,
    /// SSH port for connecting to the VM.
    pub ssh_port: Option<u16>,
    /// Additional VM-specific options.
    pub extra_options: Option<serde_json::Value>,
}

impl VMResourceConfig {
    /// Create a new VM resource configuration.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for the VM
    /// * `image` - VM image path or identifier
    ///
    /// # Example
    ///
    /// ```
    /// use dtx_process::VMResourceConfig;
    /// use dtx_core::resource::ResourceId;
    ///
    /// let config = VMResourceConfig::new(
    ///     ResourceId::new("my-vm"),
    ///     "nixos-22.11",
    /// );
    /// ```
    pub fn new(id: ResourceId, image: impl Into<String>) -> Self {
        Self {
            id,
            image: image.into(),
            memory_mb: 1024,
            cpus: 1,
            disk_size_gb: None,
            network: None,
            ssh_port: None,
            extra_options: None,
        }
    }

    /// Set memory allocation.
    #[must_use]
    pub fn with_memory_mb(mut self, memory_mb: u64) -> Self {
        self.memory_mb = memory_mb;
        self
    }

    /// Set number of CPUs.
    #[must_use]
    pub fn with_cpus(mut self, cpus: u32) -> Self {
        self.cpus = cpus;
        self
    }

    /// Set disk size.
    #[must_use]
    pub fn with_disk_size_gb(mut self, disk_size_gb: u64) -> Self {
        self.disk_size_gb = Some(disk_size_gb);
        self
    }

    /// Set network configuration.
    #[must_use]
    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.network = Some(network.into());
        self
    }

    /// Set SSH port.
    #[must_use]
    pub fn with_ssh_port(mut self, port: u16) -> Self {
        self.ssh_port = Some(port);
        self
    }
}

/// A VM resource (placeholder implementation).
///
/// This is a stub that returns "not implemented" errors for all operations.
/// Full implementation is planned for Phase 8.2.
///
/// # Future Backends
///
/// The full implementation will support:
/// - QEMU/KVM for local development
/// - NixOS VM tests for reproducible testing
/// - Firecracker for lightweight microVMs
pub struct VMResource {
    config: VMResourceConfig,
    state: ResourceState,
}

impl VMResource {
    /// Create a new VM resource with the given configuration.
    pub fn new(config: VMResourceConfig) -> Self {
        Self {
            config,
            state: ResourceState::Pending,
        }
    }

    /// Get the VM configuration.
    pub fn config(&self) -> &VMResourceConfig {
        &self.config
    }

    fn not_implemented() -> ResourceError {
        Box::new(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "VM backend not yet implemented (planned for Phase 8.2)",
        ))
    }
}

#[async_trait]
impl Resource for VMResource {
    fn id(&self) -> &ResourceId {
        &self.config.id
    }

    fn kind(&self) -> ResourceKind {
        ResourceKind::VM
    }

    fn state(&self) -> &ResourceState {
        &self.state
    }

    async fn start(&mut self, _ctx: &Context) -> ResourceResult<()> {
        Err(Self::not_implemented())
    }

    async fn stop(&mut self, _ctx: &Context) -> ResourceResult<()> {
        Err(Self::not_implemented())
    }

    async fn kill(&mut self, _ctx: &Context) -> ResourceResult<()> {
        Err(Self::not_implemented())
    }

    async fn restart(&mut self, _ctx: &Context) -> ResourceResult<()> {
        Err(Self::not_implemented())
    }

    async fn health(&self) -> HealthStatus {
        HealthStatus::Unknown
    }

    fn logs(&self) -> Option<Box<dyn LogStream>> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_resource_new() {
        let config = VMResourceConfig::new(ResourceId::new("test-vm"), "nixos-22.11")
            .with_memory_mb(2048)
            .with_cpus(2)
            .with_disk_size_gb(20)
            .with_network("bridge")
            .with_ssh_port(2222);

        let resource = VMResource::new(config);

        assert_eq!(resource.id().as_str(), "test-vm");
        assert_eq!(resource.kind(), ResourceKind::VM);
        assert_eq!(resource.state(), &ResourceState::Pending);
        assert_eq!(resource.config().memory_mb, 2048);
        assert_eq!(resource.config().cpus, 2);
        assert_eq!(resource.config().disk_size_gb, Some(20));
        assert_eq!(resource.config().network, Some("bridge".to_string()));
        assert_eq!(resource.config().ssh_port, Some(2222));
    }

    #[tokio::test]
    async fn vm_resource_start_returns_error() {
        let config = VMResourceConfig::new(ResourceId::new("test-vm"), "nixos-22.11");
        let mut resource = VMResource::new(config);
        let ctx = Context::new();

        let result = resource.start(&ctx).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[tokio::test]
    async fn vm_resource_stop_returns_error() {
        let config = VMResourceConfig::new(ResourceId::new("test-vm"), "nixos-22.11");
        let mut resource = VMResource::new(config);
        let ctx = Context::new();

        let result = resource.stop(&ctx).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn vm_resource_health_returns_unknown() {
        let config = VMResourceConfig::new(ResourceId::new("test-vm"), "nixos-22.11");
        let resource = VMResource::new(config);

        let health = resource.health().await;

        assert_eq!(health, HealthStatus::Unknown);
    }
}
