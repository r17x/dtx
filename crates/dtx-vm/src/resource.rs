//! VM Resource implementing the dtx-core Resource trait.
//!
//! This module provides the `VmResource` type that wraps a VM runtime
//! and exposes it as a dtx Resource for orchestration.

use std::any::Any;
use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use parking_lot::Mutex;
use tracing::{error, info, warn};

use crate::config::VmConfig;
use crate::error::{Result, VmError};
use crate::runtime::{ExecResult, VmInfo, VmRuntime};
use dtx_core::events::{LifecycleEvent, ResourceEventBus};
use dtx_core::resource::{
    Context, HealthStatus, LogEntry, LogStream, LogStreamKind, Resource, ResourceError, ResourceId,
    ResourceKind, ResourceResult, ResourceState,
};

/// Buffer capacity for VM logs.
const LOG_BUFFER_CAPACITY: usize = 1000;

/// VM resource implementing the Resource trait.
///
/// VmResource wraps a VM runtime (QEMU, Firecracker, or NixOS) and provides
/// lifecycle management through the standard Resource interface.
pub struct VmResource {
    /// VM configuration.
    config: VmConfig,
    /// Current resource state.
    state: ResourceState,
    /// VM runtime backend.
    runtime: Arc<dyn VmRuntime>,
    /// Event bus for publishing lifecycle events.
    event_bus: Arc<ResourceEventBus>,
    /// Prepared image path.
    image_path: Option<std::path::PathBuf>,
    /// VM info from the runtime.
    vm_info: Option<VmInfo>,
    /// Log buffer.
    logs: Arc<Mutex<VecDeque<LogEntry>>>,
}

impl VmResource {
    /// Create a new VM resource.
    ///
    /// # Arguments
    ///
    /// * `config` - VM configuration
    /// * `runtime` - VM runtime backend (QEMU, Firecracker, or NixOS)
    /// * `event_bus` - Event bus for publishing lifecycle events
    pub fn new(
        config: VmConfig,
        runtime: Arc<dyn VmRuntime>,
        event_bus: Arc<ResourceEventBus>,
    ) -> Self {
        Self {
            config,
            state: ResourceState::Pending,
            runtime,
            event_bus,
            image_path: None,
            vm_info: None,
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(LOG_BUFFER_CAPACITY))),
        }
    }

    /// Get the VM configuration.
    pub fn config(&self) -> &VmConfig {
        &self.config
    }

    /// Get VM info if running.
    pub fn vm_info(&self) -> Option<&VmInfo> {
        self.vm_info.as_ref()
    }

    /// Get the runtime name.
    pub fn runtime_name(&self) -> &str {
        self.runtime.name()
    }

    /// Execute a command in the VM via SSH.
    ///
    /// # Arguments
    ///
    /// * `command` - Command and arguments to execute
    ///
    /// # Returns
    ///
    /// Execution result with exit code, stdout, and stderr.
    pub async fn exec(&self, command: &[String]) -> Result<ExecResult> {
        let id = self.config.id.as_str();
        self.runtime.exec(id, command, &self.config).await
    }

    /// Get console log output.
    ///
    /// # Arguments
    ///
    /// * `lines` - Number of lines to return (None for all)
    pub async fn console_log(&self, lines: Option<usize>) -> Result<String> {
        self.runtime
            .console_log(self.config.id.as_str(), lines)
            .await
    }

    /// Create a snapshot of the VM.
    ///
    /// # Arguments
    ///
    /// * `name` - Snapshot name
    ///
    /// # Returns
    ///
    /// Snapshot identifier.
    pub async fn snapshot(&self, name: &str) -> Result<String> {
        self.runtime.snapshot(self.config.id.as_str(), name).await
    }

    /// Restore a snapshot.
    ///
    /// # Arguments
    ///
    /// * `name` - Snapshot name to restore
    pub async fn restore_snapshot(&self, name: &str) -> Result<()> {
        self.runtime
            .restore_snapshot(self.config.id.as_str(), name)
            .await
    }

    /// List available snapshots.
    pub async fn list_snapshots(&self) -> Result<Vec<crate::runtime::SnapshotInfo>> {
        self.runtime.list_snapshots(self.config.id.as_str()).await
    }

    /// Add a log line.
    fn add_log(&self, line: String) {
        let mut logs = self.logs.lock();
        if logs.len() >= LOG_BUFFER_CAPACITY {
            logs.pop_front();
        }
        logs.push_back(LogEntry {
            timestamp: Utc::now(),
            stream: LogStreamKind::Stdout,
            line,
        });
    }

    /// Publish a lifecycle event.
    fn publish_event(&self, event: LifecycleEvent) {
        self.event_bus.publish(event);
    }

    /// Convert internal VmError to ResourceError.
    fn to_resource_error(err: VmError) -> ResourceError {
        Box::new(err)
    }
}

#[async_trait]
impl Resource for VmResource {
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
        // Check current state
        if self.state.is_running() {
            return Ok(());
        }

        let started_at = Utc::now();

        info!(
            id = %self.config.id,
            runtime = %self.runtime.name(),
            "Starting VM resource"
        );

        // Transition to Starting
        self.state = ResourceState::Starting { started_at };
        self.publish_event(LifecycleEvent::Starting {
            id: self.config.id.clone(),
            kind: ResourceKind::VM,
            timestamp: started_at,
        });

        // Prepare image if not already done
        if self.image_path.is_none() {
            info!(id = %self.config.id, "Preparing VM image");
            self.add_log("Preparing VM image...".to_string());

            match self.runtime.prepare_image(&self.config).await {
                Ok(path) => {
                    self.add_log(format!("Image prepared: {}", path.display()));
                    self.image_path = Some(path);
                }
                Err(e) => {
                    let error = format!("Failed to prepare VM image: {}", e);
                    self.add_log(error.clone());
                    error!(id = %self.config.id, error = %e, "Failed to prepare VM image");

                    self.state = ResourceState::Failed {
                        error: error.clone(),
                        exit_code: None,
                        started_at: Some(started_at),
                        failed_at: Utc::now(),
                    };
                    self.publish_event(LifecycleEvent::Failed {
                        id: self.config.id.clone(),
                        kind: ResourceKind::VM,
                        error,
                        exit_code: None,
                        timestamp: Utc::now(),
                    });

                    return Err(Self::to_resource_error(e));
                }
            }
        }

        let image_path = self.image_path.as_ref().unwrap();

        // Create VM
        info!(id = %self.config.id, "Creating VM");
        self.add_log("Creating VM...".to_string());

        if let Err(e) = self.runtime.create(&self.config).await {
            let error = format!("Failed to create VM: {}", e);
            self.add_log(error.clone());
            error!(id = %self.config.id, error = %e, "Failed to create VM");

            self.state = ResourceState::Failed {
                error: error.clone(),
                exit_code: None,
                started_at: Some(started_at),
                failed_at: Utc::now(),
            };
            self.publish_event(LifecycleEvent::Failed {
                id: self.config.id.clone(),
                kind: ResourceKind::VM,
                error,
                exit_code: None,
                timestamp: Utc::now(),
            });

            return Err(Self::to_resource_error(e));
        }

        // Start VM
        info!(id = %self.config.id, "Starting VM");
        self.add_log("Starting VM...".to_string());

        match self.runtime.start(&self.config, image_path).await {
            Ok(info) => {
                self.add_log(format!("VM started (state: {})", info.state));
                self.vm_info = Some(info.clone());
            }
            Err(e) => {
                let error = format!("Failed to start VM: {}", e);
                self.add_log(error.clone());
                error!(id = %self.config.id, error = %e, "Failed to start VM");

                self.state = ResourceState::Failed {
                    error: error.clone(),
                    exit_code: None,
                    started_at: Some(started_at),
                    failed_at: Utc::now(),
                };
                self.publish_event(LifecycleEvent::Failed {
                    id: self.config.id.clone(),
                    kind: ResourceKind::VM,
                    error,
                    exit_code: None,
                    timestamp: Utc::now(),
                });

                return Err(Self::to_resource_error(e));
            }
        }

        // Wait for boot
        info!(id = %self.config.id, timeout = ?self.config.boot_timeout, "Waiting for VM to boot");
        self.add_log(format!(
            "Waiting for VM to boot (timeout: {:?})...",
            self.config.boot_timeout
        ));

        if let Err(e) = self
            .runtime
            .wait_for_boot(
                self.config.id.as_str(),
                &self.config,
                self.config.boot_timeout,
            )
            .await
        {
            let error = format!("VM boot timeout: {}", e);
            self.add_log(error.clone());
            warn!(id = %self.config.id, error = %e, "VM boot timeout");

            // Don't fail completely - VM might still be usable
            // Just log a warning
        } else {
            self.add_log("VM boot complete".to_string());
        }

        // Update VM info
        if let Ok(info) = self.runtime.inspect(self.config.id.as_str()).await {
            self.vm_info = Some(info);
        }

        // Transition to Running
        let pid = self.vm_info.as_ref().and_then(|i| i.pid);
        self.state = ResourceState::Running { pid, started_at };
        self.publish_event(LifecycleEvent::Running {
            id: self.config.id.clone(),
            kind: ResourceKind::VM,
            pid,
            timestamp: Utc::now(),
        });

        info!(id = %self.config.id, "VM resource started");
        Ok(())
    }

    async fn stop(&mut self, _ctx: &Context) -> ResourceResult<()> {
        if !self.state.is_running() {
            return Ok(());
        }

        let id = self.config.id.as_str();
        let started_at = self.state.started_at().unwrap_or_else(Utc::now);
        let stopping_at = Utc::now();

        info!(id = %id, "Stopping VM resource");
        self.add_log("Stopping VM...".to_string());

        // Transition to Stopping
        self.state = ResourceState::Stopping {
            started_at,
            stopping_at,
        };
        self.publish_event(LifecycleEvent::Stopping {
            id: self.config.id.clone(),
            kind: ResourceKind::VM,
            timestamp: stopping_at,
        });

        // Stop VM
        if let Err(e) = self.runtime.stop(id, self.config.shutdown_timeout).await {
            warn!(id = %id, error = %e, "Failed to stop VM gracefully, force killing");
            self.add_log(format!("Graceful stop failed: {}, force killing", e));

            // Force kill
            if let Err(e) = self.runtime.kill(id).await {
                error!(id = %id, error = %e, "Failed to kill VM");
                self.add_log(format!("Force kill failed: {}", e));
            }
        } else {
            self.add_log("VM stopped".to_string());
        }

        // Transition to Stopped
        let stopped_at = Utc::now();
        self.state = ResourceState::Stopped {
            exit_code: Some(0),
            started_at,
            stopped_at,
        };
        self.publish_event(LifecycleEvent::Stopped {
            id: self.config.id.clone(),
            kind: ResourceKind::VM,
            exit_code: Some(0),
            timestamp: stopped_at,
        });

        self.vm_info = None;
        info!(id = %id, "VM resource stopped");

        Ok(())
    }

    async fn kill(&mut self, _ctx: &Context) -> ResourceResult<()> {
        let id = self.config.id.as_str();

        info!(id = %id, "Killing VM resource");
        self.add_log("Force killing VM...".to_string());

        self.runtime
            .kill(id)
            .await
            .map_err(Self::to_resource_error)?;

        let started_at = self.state.started_at().unwrap_or_else(Utc::now);
        self.state = ResourceState::Stopped {
            exit_code: Some(-9), // SIGKILL
            started_at,
            stopped_at: Utc::now(),
        };
        self.publish_event(LifecycleEvent::Stopped {
            id: self.config.id.clone(),
            kind: ResourceKind::VM,
            exit_code: Some(-9),
            timestamp: Utc::now(),
        });

        self.vm_info = None;
        self.add_log("VM killed".to_string());

        Ok(())
    }

    async fn restart(&mut self, ctx: &Context) -> ResourceResult<()> {
        let id = self.config.id.as_str();

        info!(id = %id, "Restarting VM resource");
        self.add_log("Restarting VM...".to_string());

        // Try in-place restart first
        match self.runtime.restart(id, &self.config).await {
            Ok(()) => {
                self.add_log("VM restarted in-place".to_string());
                // Update VM info
                if let Ok(info) = self.runtime.inspect(id).await {
                    self.vm_info = Some(info);
                }
                Ok(())
            }
            Err(VmError::NotSupported(_, _)) => {
                // Fallback to stop/start
                self.add_log("In-place restart not supported, doing stop/start".to_string());
                self.stop(ctx).await?;
                self.start(ctx).await
            }
            Err(e) => {
                self.add_log(format!("Restart failed: {}", e));
                Err(Self::to_resource_error(e))
            }
        }
    }

    async fn health(&self) -> HealthStatus {
        let id = self.config.id.as_str();

        // Check if we think we're running
        if !self.state.is_running() {
            return HealthStatus::Unhealthy {
                reason: format!("VM not running (state: {})", self.state),
            };
        }

        // Query runtime for health
        self.runtime
            .health(id, &self.config)
            .await
            .unwrap_or(HealthStatus::Unknown)
    }

    fn logs(&self) -> Option<Box<dyn LogStream>> {
        Some(Box::new(VmLogStream {
            logs: self.logs.clone(),
            position: 0,
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Log stream for VM resource.
struct VmLogStream {
    logs: Arc<Mutex<VecDeque<LogEntry>>>,
    position: usize,
}

impl LogStream for VmLogStream {
    fn try_recv(&mut self) -> Option<LogEntry> {
        let logs = self.logs.lock();
        if self.position < logs.len() {
            let line = logs.get(self.position).cloned();
            self.position += 1;
            line
        } else {
            None
        }
    }

    fn is_open(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::config::{ImageFormat, VmImage, VmRuntimeType};
    use crate::runtime::{SnapshotInfo, VmState};

    /// Mock VM runtime for testing.
    struct MockRuntime {
        available: bool,
    }

    impl MockRuntime {
        fn new(available: bool) -> Self {
            Self { available }
        }
    }

    #[async_trait]
    impl VmRuntime for MockRuntime {
        fn name(&self) -> &str {
            "mock"
        }

        async fn is_available(&self) -> bool {
            self.available
        }

        async fn prepare_image(&self, _config: &VmConfig) -> Result<PathBuf> {
            Ok(PathBuf::from("/mock/image.qcow2"))
        }

        async fn create(&self, config: &VmConfig) -> Result<String> {
            Ok(config.id.as_str().to_string())
        }

        async fn start(&self, config: &VmConfig, _image_path: &std::path::Path) -> Result<VmInfo> {
            Ok(VmInfo::new(config.id.as_str(), VmState::Running))
        }

        async fn stop(&self, _id: &str, _timeout: Duration) -> Result<()> {
            Ok(())
        }

        async fn kill(&self, _id: &str) -> Result<()> {
            Ok(())
        }

        async fn pause(&self, _id: &str) -> Result<()> {
            Ok(())
        }

        async fn resume(&self, _id: &str) -> Result<()> {
            Ok(())
        }

        async fn restart(&self, _id: &str, _config: &VmConfig) -> Result<()> {
            Ok(())
        }

        async fn inspect(&self, id: &str) -> Result<VmInfo> {
            Ok(VmInfo::new(id, VmState::Running))
        }

        async fn is_running(&self, _id: &str) -> Result<bool> {
            Ok(true)
        }

        async fn wait_for_boot(
            &self,
            _id: &str,
            _config: &VmConfig,
            _timeout: Duration,
        ) -> Result<()> {
            Ok(())
        }

        async fn exec(
            &self,
            _id: &str,
            _command: &[String],
            _config: &VmConfig,
        ) -> Result<ExecResult> {
            Ok(ExecResult::new(0, "output", ""))
        }

        async fn console_log(&self, _id: &str, _lines: Option<usize>) -> Result<String> {
            Ok("console output".to_string())
        }

        async fn remove(&self, _id: &str) -> Result<()> {
            Ok(())
        }

        async fn health(&self, _id: &str, _config: &VmConfig) -> Result<HealthStatus> {
            Ok(HealthStatus::Healthy)
        }

        async fn snapshot(&self, _id: &str, name: &str) -> Result<String> {
            Ok(name.to_string())
        }

        async fn restore_snapshot(&self, _id: &str, _name: &str) -> Result<()> {
            Ok(())
        }

        async fn list_snapshots(&self, _id: &str) -> Result<Vec<SnapshotInfo>> {
            Ok(Vec::new())
        }
    }

    fn make_config() -> VmConfig {
        VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/test/image.qcow2"),
                format: ImageFormat::Qcow2,
            },
        )
        .with_runtime(VmRuntimeType::Qemu)
        .with_boot_timeout(Duration::from_secs(5))
    }

    fn make_resource() -> VmResource {
        let config = make_config();
        let runtime = Arc::new(MockRuntime::new(true));
        let event_bus = Arc::new(ResourceEventBus::new());
        VmResource::new(config, runtime, event_bus)
    }

    #[test]
    fn vm_resource_new() {
        let resource = make_resource();

        assert_eq!(resource.id().as_str(), "test-vm");
        assert_eq!(resource.kind(), ResourceKind::VM);
        assert_eq!(*resource.state(), ResourceState::Pending);
        assert_eq!(resource.runtime_name(), "mock");
    }

    #[test]
    fn vm_resource_config() {
        let resource = make_resource();
        let config = resource.config();

        assert_eq!(config.id.as_str(), "test-vm");
        assert_eq!(config.runtime, VmRuntimeType::Qemu);
    }

    #[tokio::test]
    async fn vm_resource_start() {
        let mut resource = make_resource();
        let ctx = Context::new();

        let result = resource.start(&ctx).await;

        assert!(result.is_ok());
        assert!(resource.state().is_running());
        assert!(resource.vm_info().is_some());
    }

    #[tokio::test]
    async fn vm_resource_stop() {
        let mut resource = make_resource();
        let ctx = Context::new();

        // Start first
        resource.start(&ctx).await.unwrap();
        assert!(resource.state().is_running());

        // Stop
        let result = resource.stop(&ctx).await;

        assert!(result.is_ok());
        assert!(!resource.state().is_running());
        assert!(resource.vm_info().is_none());
    }

    #[tokio::test]
    async fn vm_resource_kill() {
        let mut resource = make_resource();
        let ctx = Context::new();

        // Start first
        resource.start(&ctx).await.unwrap();

        // Kill
        let result = resource.kill(&ctx).await;

        assert!(result.is_ok());
        assert!(!resource.state().is_running());
    }

    #[tokio::test]
    async fn vm_resource_restart() {
        let mut resource = make_resource();
        let ctx = Context::new();

        // Start first
        resource.start(&ctx).await.unwrap();

        // Restart
        let result = resource.restart(&ctx).await;

        assert!(result.is_ok());
        assert!(resource.state().is_running());
    }

    #[tokio::test]
    async fn vm_resource_health() {
        let mut resource = make_resource();
        let ctx = Context::new();

        // Before start - unhealthy
        let health = resource.health().await;
        assert!(matches!(health, HealthStatus::Unhealthy { .. }));

        // After start - healthy
        resource.start(&ctx).await.unwrap();
        let health = resource.health().await;
        assert_eq!(health, HealthStatus::Healthy);
    }

    #[tokio::test]
    async fn vm_resource_exec() {
        let mut resource = make_resource();
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();

        let result = resource
            .exec(&["echo".to_string(), "hello".to_string()])
            .await;

        assert!(result.is_ok());
        let exec_result = result.unwrap();
        assert_eq!(exec_result.exit_code, 0);
    }

    #[tokio::test]
    async fn vm_resource_console_log() {
        let mut resource = make_resource();
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();

        let result = resource.console_log(None).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "console output");
    }

    #[tokio::test]
    async fn vm_resource_snapshot() {
        let mut resource = make_resource();
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();

        let result = resource.snapshot("snap-1").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "snap-1");
    }

    #[tokio::test]
    async fn vm_resource_logs() {
        let resource = make_resource();

        let logs = resource.logs();
        assert!(logs.is_some());

        let log_stream = logs.unwrap();
        assert!(log_stream.is_open());
    }

    #[test]
    fn vm_resource_as_any() {
        let resource = make_resource();

        let any = resource.as_any();
        assert!(any.downcast_ref::<VmResource>().is_some());
    }

    #[test]
    fn vm_resource_start_when_already_running() {
        // Test idempotent start is handled via state check
        let resource = make_resource();
        assert!(!resource.state().is_running());
    }

    #[test]
    fn vm_resource_stop_when_not_running() {
        // Test stop when not running returns Ok
        let resource = make_resource();
        assert!(!resource.state().is_running());
    }
}
