//! Container resource implementation.
//!
//! This module provides the `ContainerResource` type that implements the
//! `Resource` trait for Docker/Podman containers.
//!
//! # Features
//!
//! - Docker API integration via bollard
//! - Container lifecycle management (create, start, stop, kill)
//! - Log streaming from container stdout/stderr
//! - Health check support
//!
//! # Example
//!
//! ```ignore
//! use dtx_process::ContainerResource;
//! use dtx_core::translation::ContainerConfig;
//! use dtx_core::resource::{Resource, Context, ResourceId};
//! use dtx_core::events::ResourceEventBus;
//! use std::sync::Arc;
//!
//! let event_bus = Arc::new(ResourceEventBus::new());
//! let config = ContainerConfig::new(
//!     ResourceId::new("redis"),
//!     "redis:7-alpine",
//! );
//!
//! let mut container = ContainerResource::new(config, event_bus);
//! container.start(&Context::new()).await?;
//! ```

use std::any::Any;
use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use bollard::container::{
    Config, CreateContainerOptions, KillContainerOptions, LogOutput, LogsOptions,
    RemoveContainerOptions, StartContainerOptions, StopContainerOptions, WaitContainerOptions,
};
use bollard::models::{HostConfig, PortBinding, RestartPolicy, RestartPolicyNameEnum};
use bollard::Docker;
use chrono::Utc;
use futures::StreamExt;
use tracing::{debug, error, info, warn};

use dtx_core::events::{LifecycleEvent, ResourceEventBus};
use dtx_core::resource::{
    Context, HealthStatus, LogEntry, LogStream, LogStreamKind, Resource, ResourceError, ResourceId,
    ResourceKind, ResourceResult, ResourceState,
};
use dtx_core::translation::{
    ContainerConfig, ContainerHealthCheck, ContainerRestartPolicy, HealthCheckTest, PortMapping,
    Protocol, VolumeMount,
};

/// Buffer capacity for container logs.
const LOG_BUFFER_CAPACITY: usize = 1000;

/// A container resource implementing the Resource trait.
///
/// `ContainerResource` manages a Docker/Podman container with support for:
/// - Environment variables and working directory
/// - Port mappings
/// - Volume mounts
/// - Configurable restart policies
/// - Health checks
/// - Log capture
pub struct ContainerResource {
    /// Configuration.
    config: ContainerConfig,
    /// Current state.
    state: ResourceState,
    /// Docker container ID (assigned after creation).
    container_id: Option<String>,
    /// Event bus for publishing lifecycle events.
    event_bus: Arc<ResourceEventBus>,
    /// Captured logs.
    logs: VecDeque<LogEntry>,
    /// Docker client (lazily initialized).
    docker: Option<Docker>,
}

impl ContainerResource {
    /// Create a new container resource.
    ///
    /// # Arguments
    ///
    /// * `config` - Container configuration
    /// * `event_bus` - Event bus for lifecycle events
    pub fn new(config: ContainerConfig, event_bus: Arc<ResourceEventBus>) -> Self {
        Self {
            config,
            state: ResourceState::Pending,
            container_id: None,
            event_bus,
            logs: VecDeque::with_capacity(LOG_BUFFER_CAPACITY),
            docker: None,
        }
    }

    /// Get the container configuration.
    pub fn config(&self) -> &ContainerConfig {
        &self.config
    }

    /// Get the Docker container ID if the container has been created.
    pub fn container_id(&self) -> Option<&str> {
        self.container_id.as_deref()
    }

    /// Ensure Docker client is initialized.
    async fn ensure_docker(&mut self) -> ResourceResult<()> {
        if self.docker.is_none() {
            let docker = Docker::connect_with_socket_defaults().map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    format!("Failed to connect to Docker: {}", e),
                )) as ResourceError
            })?;
            self.docker = Some(docker);
        }
        Ok(())
    }

    /// Get Docker client reference (must be initialized first).
    fn docker(&self) -> &Docker {
        self.docker.as_ref().expect("docker client not initialized")
    }

    /// Create the container.
    async fn create_container(&mut self) -> ResourceResult<String> {
        // Ensure docker is initialized first
        self.ensure_docker().await?;

        let container_name = self.config.id.as_str();

        // Build port bindings
        let mut port_bindings = std::collections::HashMap::new();
        let mut exposed_ports = std::collections::HashMap::new();

        for port_mapping in &self.config.ports {
            let container_port = format_port_key(port_mapping);
            exposed_ports.insert(container_port.clone(), std::collections::HashMap::new());
            port_bindings.insert(
                container_port,
                Some(vec![PortBinding {
                    host_ip: Some("0.0.0.0".to_string()),
                    host_port: Some(port_mapping.host.to_string()),
                }]),
            );
        }

        // Build volume bindings
        let binds: Vec<String> = self.config.volumes.iter().map(format_volume_bind).collect();

        // Build environment
        let env: Vec<String> = self
            .config
            .environment
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        // Build labels
        let labels: std::collections::HashMap<String, String> = self.config.labels.clone();

        // Build restart policy
        let restart_policy = convert_restart_policy(&self.config.restart);

        // Build health check
        let healthcheck = self.config.health_check.as_ref().map(convert_health_check);

        // Build host config
        let host_config = HostConfig {
            port_bindings: if port_bindings.is_empty() {
                None
            } else {
                Some(port_bindings)
            },
            binds: if binds.is_empty() { None } else { Some(binds) },
            restart_policy: Some(restart_policy),
            network_mode: self.config.network.clone(),
            memory: self
                .config
                .resources
                .as_ref()
                .and_then(|r| r.memory.as_ref().and_then(|m| parse_memory_limit(m))),
            nano_cpus: self
                .config
                .resources
                .as_ref()
                .and_then(|r| r.cpus.as_ref().and_then(|c| parse_cpu_limit(c))),
            ..Default::default()
        };

        // Build container config
        let container_config = Config {
            image: Some(self.config.image.clone()),
            cmd: self.config.command.clone(),
            entrypoint: self.config.entrypoint.clone(),
            working_dir: self.config.working_dir.clone(),
            env: if env.is_empty() { None } else { Some(env) },
            exposed_ports: if exposed_ports.is_empty() {
                None
            } else {
                Some(exposed_ports)
            },
            labels: if labels.is_empty() {
                None
            } else {
                Some(labels)
            },
            host_config: Some(host_config),
            healthcheck,
            ..Default::default()
        };

        // Create container
        let options = CreateContainerOptions {
            name: container_name,
            platform: None,
        };

        let response = self
            .docker()
            .create_container(Some(options), container_config)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to create container: {}", e),
                )) as ResourceError
            })?;

        debug!(
            id = %self.config.id,
            container_id = %response.id,
            "Container created"
        );

        Ok(response.id)
    }

    /// Start the container.
    async fn start_container(&self) -> ResourceResult<()> {
        let Some(ref container_id) = self.container_id else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Container not created",
            )) as ResourceError);
        };

        self.docker()
            .start_container(container_id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to start container: {}", e),
                )) as ResourceError
            })?;

        debug!(id = %self.config.id, container_id, "Container started");
        Ok(())
    }

    /// Stop the container gracefully.
    async fn stop_container(&self, timeout_secs: i64) -> ResourceResult<()> {
        let Some(ref container_id) = self.container_id else {
            return Ok(());
        };

        let options = StopContainerOptions { t: timeout_secs };

        self.docker()
            .stop_container(container_id, Some(options))
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to stop container: {}", e),
                )) as ResourceError
            })?;

        debug!(id = %self.config.id, container_id, "Container stopped");
        Ok(())
    }

    /// Kill the container.
    async fn kill_container(&self, signal: &str) -> ResourceResult<()> {
        let Some(ref container_id) = self.container_id else {
            return Ok(());
        };

        let options = KillContainerOptions { signal };

        self.docker()
            .kill_container(container_id, Some(options))
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to kill container: {}", e),
                )) as ResourceError
            })?;

        debug!(id = %self.config.id, container_id, signal, "Container killed");
        Ok(())
    }

    /// Remove the container.
    async fn remove_container(&mut self) -> ResourceResult<()> {
        let Some(ref container_id) = self.container_id else {
            return Ok(());
        };

        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };

        self.docker()
            .remove_container(container_id, Some(options))
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to remove container: {}", e),
                )) as ResourceError
            })?;

        debug!(id = %self.config.id, container_id, "Container removed");
        self.container_id = None;
        Ok(())
    }

    /// Wait for the container to exit.
    async fn wait_for_exit(&self) -> ResourceResult<Option<i32>> {
        let Some(ref container_id) = self.container_id else {
            return Ok(None);
        };

        let options = WaitContainerOptions {
            condition: "not-running",
        };

        let mut stream = self.docker().wait_container(container_id, Some(options));

        if let Some(result) = stream.next().await {
            match result {
                Ok(response) => {
                    let exit_code = response.status_code as i32;
                    Ok(Some(exit_code))
                }
                Err(e) => Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to wait for container: {}", e),
                )) as ResourceError),
            }
        } else {
            Ok(None)
        }
    }

    /// Fetch recent logs from the container.
    pub async fn fetch_logs(&mut self) -> ResourceResult<()> {
        let Some(ref container_id) = self.container_id else {
            return Ok(());
        };

        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: "100".to_string(),
            ..Default::default()
        };

        let mut stream = self.docker().logs(container_id, Some(options));

        while let Some(result) = stream.next().await {
            match result {
                Ok(log_output) => {
                    let (stream_kind, line) = match log_output {
                        LogOutput::StdOut { message } => (
                            LogStreamKind::Stdout,
                            String::from_utf8_lossy(&message).into_owned(),
                        ),
                        LogOutput::StdErr { message } => (
                            LogStreamKind::Stderr,
                            String::from_utf8_lossy(&message).into_owned(),
                        ),
                        LogOutput::Console { message } => (
                            LogStreamKind::Stdout,
                            String::from_utf8_lossy(&message).into_owned(),
                        ),
                        LogOutput::StdIn { message: _ } => continue,
                    };

                    let entry = LogEntry {
                        timestamp: Utc::now(),
                        stream: stream_kind,
                        line: line.trim_end().to_string(),
                    };

                    self.add_log(entry);
                }
                Err(e) => {
                    warn!(error = %e, "Error fetching container logs");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Add a log entry to the buffer.
    fn add_log(&mut self, entry: LogEntry) {
        // Publish log event
        self.event_bus.publish(LifecycleEvent::Log {
            id: self.config.id.clone(),
            stream: entry.stream,
            line: entry.line.clone(),
            timestamp: entry.timestamp,
        });

        // Store in buffer (evict oldest if at capacity)
        if self.logs.len() >= LOG_BUFFER_CAPACITY {
            self.logs.pop_front();
        }
        self.logs.push_back(entry);
    }
}

#[async_trait]
impl Resource for ContainerResource {
    fn id(&self) -> &ResourceId {
        &self.config.id
    }

    fn kind(&self) -> ResourceKind {
        ResourceKind::Container
    }

    fn state(&self) -> &ResourceState {
        &self.state
    }

    async fn start(&mut self, _ctx: &Context) -> ResourceResult<()> {
        // Check current state
        if self.state.is_running() {
            return Ok(());
        }

        info!(id = %self.config.id, image = %self.config.image, "Starting container");

        // Transition to Starting
        self.state = ResourceState::Starting {
            started_at: Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Starting {
            id: self.config.id.clone(),
            kind: ResourceKind::Container,
            timestamp: Utc::now(),
        });

        // Create container if not exists
        if self.container_id.is_none() {
            match self.create_container().await {
                Ok(id) => {
                    self.container_id = Some(id);
                }
                Err(e) => {
                    let error = format!("Failed to create container: {}", e);
                    self.state = ResourceState::Failed {
                        error: error.clone(),
                        exit_code: None,
                        started_at: None,
                        failed_at: Utc::now(),
                    };
                    self.event_bus.publish(LifecycleEvent::Failed {
                        id: self.config.id.clone(),
                        kind: ResourceKind::Container,
                        error,
                        exit_code: None,
                        timestamp: Utc::now(),
                    });
                    return Err(e);
                }
            }
        }

        // Start container
        if let Err(e) = self.start_container().await {
            let error = format!("Failed to start container: {}", e);
            self.state = ResourceState::Failed {
                error: error.clone(),
                exit_code: None,
                started_at: None,
                failed_at: Utc::now(),
            };
            self.event_bus.publish(LifecycleEvent::Failed {
                id: self.config.id.clone(),
                kind: ResourceKind::Container,
                error,
                exit_code: None,
                timestamp: Utc::now(),
            });
            return Err(e);
        }

        // Transition to Running
        let started_at = Utc::now();
        self.state = ResourceState::Running {
            pid: None, // Containers don't expose PID directly
            started_at,
        };
        self.event_bus.publish(LifecycleEvent::Running {
            id: self.config.id.clone(),
            kind: ResourceKind::Container,
            pid: None,
            timestamp: started_at,
        });

        Ok(())
    }

    async fn stop(&mut self, _ctx: &Context) -> ResourceResult<()> {
        if !self.state.is_running() {
            return Ok(());
        }

        info!(id = %self.config.id, "Stopping container");

        // Transition to Stopping
        let started_at = self.state.started_at().unwrap_or_else(Utc::now);
        self.state = ResourceState::Stopping {
            started_at,
            stopping_at: Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Stopping {
            id: self.config.id.clone(),
            kind: ResourceKind::Container,
            timestamp: Utc::now(),
        });

        // Stop container with 10 second timeout
        if let Err(e) = self.stop_container(10).await {
            warn!(id = %self.config.id, error = %e, "Failed to stop container gracefully");
            // Try to kill if stop fails
            if let Err(e) = self.kill_container("SIGKILL").await {
                error!(id = %self.config.id, error = %e, "Failed to kill container");
            }
        }

        // Wait for exit and get exit code
        let exit_code = self.wait_for_exit().await.ok().flatten();

        // Transition to Stopped
        self.state = ResourceState::Stopped {
            exit_code,
            started_at,
            stopped_at: Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Stopped {
            id: self.config.id.clone(),
            kind: ResourceKind::Container,
            exit_code,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    async fn kill(&mut self, _ctx: &Context) -> ResourceResult<()> {
        self.kill_container("SIGKILL").await?;

        let started_at = self.state.started_at().unwrap_or_else(Utc::now);
        self.state = ResourceState::Stopped {
            exit_code: None,
            started_at,
            stopped_at: Utc::now(),
        };
        self.event_bus.publish(LifecycleEvent::Stopped {
            id: self.config.id.clone(),
            kind: ResourceKind::Container,
            exit_code: None,
            timestamp: Utc::now(),
        });

        Ok(())
    }

    async fn health(&self) -> HealthStatus {
        // If not running, unhealthy
        if !self.state.is_running() {
            return HealthStatus::Unhealthy {
                reason: format!("Container not running (state: {})", self.state),
            };
        }

        // If no health check configured, assume healthy if running
        if self.config.health_check.is_none() {
            return HealthStatus::Healthy;
        }

        // Query Docker for health status
        let Some(ref container_id) = self.container_id else {
            return HealthStatus::Unknown;
        };

        let Some(ref docker) = self.docker else {
            return HealthStatus::Unknown;
        };

        match docker.inspect_container(container_id, None).await {
            Ok(info) => {
                if let Some(state) = info.state {
                    if let Some(health) = state.health {
                        use bollard::models::HealthStatusEnum;
                        match health.status {
                            Some(HealthStatusEnum::HEALTHY) => HealthStatus::Healthy,
                            Some(HealthStatusEnum::UNHEALTHY) => HealthStatus::Unhealthy {
                                reason: health
                                    .log
                                    .and_then(|logs| logs.last().and_then(|l| l.output.clone()))
                                    .unwrap_or_else(|| "Health check failed".to_string()),
                            },
                            Some(HealthStatusEnum::STARTING) => HealthStatus::Unknown,
                            Some(HealthStatusEnum::NONE) | Some(HealthStatusEnum::EMPTY) | None => {
                                HealthStatus::Unknown
                            }
                        }
                    } else {
                        // No health check configured in Docker
                        HealthStatus::Healthy
                    }
                } else {
                    HealthStatus::Unknown
                }
            }
            Err(e) => HealthStatus::Unhealthy {
                reason: format!("Failed to inspect container: {}", e),
            },
        }
    }

    fn logs(&self) -> Option<Box<dyn LogStream>> {
        Some(Box::new(ContainerLogStream {
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

/// Log stream for container logs.
pub struct ContainerLogStream {
    logs: VecDeque<LogEntry>,
    position: usize,
}

impl LogStream for ContainerLogStream {
    fn try_recv(&mut self) -> Option<LogEntry> {
        if self.position < self.logs.len() {
            let entry = self.logs[self.position].clone();
            self.position += 1;
            Some(entry)
        } else {
            None
        }
    }

    fn is_open(&self) -> bool {
        self.position < self.logs.len()
    }
}

// === Helper Functions ===

/// Format port key for Docker API (e.g., "80/tcp").
fn format_port_key(port: &PortMapping) -> String {
    let protocol = match port.protocol {
        Protocol::Tcp => "tcp",
        Protocol::Udp => "udp",
    };
    format!("{}/{}", port.container, protocol)
}

/// Format volume bind string (e.g., "/host/path:/container/path:ro").
fn format_volume_bind(volume: &VolumeMount) -> String {
    if volume.read_only {
        format!("{}:{}:ro", volume.source.display(), volume.target)
    } else {
        format!("{}:{}", volume.source.display(), volume.target)
    }
}

/// Convert ContainerRestartPolicy to bollard RestartPolicy.
fn convert_restart_policy(policy: &ContainerRestartPolicy) -> RestartPolicy {
    let name = match policy {
        ContainerRestartPolicy::No => RestartPolicyNameEnum::NO,
        ContainerRestartPolicy::Always => RestartPolicyNameEnum::ALWAYS,
        ContainerRestartPolicy::OnFailure => RestartPolicyNameEnum::ON_FAILURE,
        ContainerRestartPolicy::UnlessStopped => RestartPolicyNameEnum::UNLESS_STOPPED,
    };
    RestartPolicy {
        name: Some(name),
        maximum_retry_count: None,
    }
}

/// Convert ContainerHealthCheck to bollard HealthConfig.
fn convert_health_check(health_check: &ContainerHealthCheck) -> bollard::models::HealthConfig {
    let test = match &health_check.test {
        HealthCheckTest::CmdShell(cmd) => vec!["CMD-SHELL".to_string(), cmd.clone()],
        HealthCheckTest::Cmd(args) => {
            let mut test = vec!["CMD".to_string()];
            test.extend(args.clone());
            test
        }
    };

    bollard::models::HealthConfig {
        test: Some(test),
        interval: parse_duration_to_nanos(&health_check.interval),
        timeout: parse_duration_to_nanos(&health_check.timeout),
        retries: Some(health_check.retries as i64),
        start_period: health_check
            .start_period
            .as_ref()
            .and_then(|s| parse_duration_to_nanos(s)),
        start_interval: None,
    }
}

/// Parse a duration string (e.g., "10s", "1m", "500ms") to nanoseconds.
fn parse_duration_to_nanos(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    // Find where the number ends and unit begins
    let num_end = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .count();
    if num_end == 0 {
        return None;
    }

    let (num_str, unit) = s.split_at(num_end);
    let num: f64 = num_str.parse().ok()?;
    let unit = unit.trim();

    let nanos = match unit {
        "ns" | "" => num,
        "us" | "µs" => num * 1_000.0,
        "ms" => num * 1_000_000.0,
        "s" => num * 1_000_000_000.0,
        "m" => num * 60_000_000_000.0,
        "h" => num * 3_600_000_000_000.0,
        _ => return None,
    };

    Some(nanos as i64)
}

/// Parse a memory limit string (e.g., "512m", "1g", "1024k") to bytes.
fn parse_memory_limit(s: &str) -> Option<i64> {
    let s = s.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }

    let num_end = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if num_end == 0 {
        return None;
    }

    let (num_str, unit) = s.split_at(num_end);
    let num: i64 = num_str.parse().ok()?;
    let unit = unit.trim();

    let bytes = match unit {
        "" | "b" => num,
        "k" | "kb" => num * 1024,
        "m" | "mb" => num * 1024 * 1024,
        "g" | "gb" => num * 1024 * 1024 * 1024,
        _ => return None,
    };

    Some(bytes)
}

/// Parse a CPU limit string (e.g., "0.5", "2") to nanocpus.
fn parse_cpu_limit(s: &str) -> Option<i64> {
    let num: f64 = s.trim().parse().ok()?;
    Some((num * 1_000_000_000.0) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(id: &str, image: &str) -> ContainerConfig {
        ContainerConfig::new(id, image)
    }

    fn make_resource(config: ContainerConfig) -> ContainerResource {
        let event_bus = Arc::new(ResourceEventBus::new());
        ContainerResource::new(config, event_bus)
    }

    #[test]
    fn container_resource_new() {
        let config = make_config("redis", "redis:7-alpine");
        let resource = make_resource(config);

        assert_eq!(resource.id().as_str(), "redis");
        assert_eq!(resource.kind(), ResourceKind::Container);
        assert_eq!(resource.state(), &ResourceState::Pending);
        assert!(resource.container_id().is_none());
    }

    #[test]
    fn container_resource_state_transitions() {
        let config = make_config("api", "node:20");
        let resource = make_resource(config);

        // Initial state
        assert!(!resource.state().is_running());

        // Verify correct kind
        assert_eq!(resource.kind(), ResourceKind::Container);
    }

    #[test]
    fn container_config_to_bollard() {
        // Test port formatting
        let port = PortMapping::tcp_mapped(8080, 80);
        assert_eq!(format_port_key(&port), "80/tcp");

        let port_udp = PortMapping::udp(53);
        assert_eq!(format_port_key(&port_udp), "53/udp");

        // Test volume formatting
        let vol = VolumeMount::new("/data", "/app/data");
        assert_eq!(format_volume_bind(&vol), "/data:/app/data");

        let vol_ro = VolumeMount::read_only("/config", "/app/config");
        assert_eq!(format_volume_bind(&vol_ro), "/config:/app/config:ro");

        // Test restart policy conversion
        let policy = convert_restart_policy(&ContainerRestartPolicy::Always);
        assert_eq!(policy.name, Some(RestartPolicyNameEnum::ALWAYS));

        let policy = convert_restart_policy(&ContainerRestartPolicy::OnFailure);
        assert_eq!(policy.name, Some(RestartPolicyNameEnum::ON_FAILURE));

        let policy = convert_restart_policy(&ContainerRestartPolicy::No);
        assert_eq!(policy.name, Some(RestartPolicyNameEnum::NO));

        let policy = convert_restart_policy(&ContainerRestartPolicy::UnlessStopped);
        assert_eq!(policy.name, Some(RestartPolicyNameEnum::UNLESS_STOPPED));
    }

    #[test]
    fn container_health_check_conversion() {
        let health = ContainerHealthCheck::shell("curl -f http://localhost:8080/health")
            .with_interval("10s")
            .with_timeout("5s")
            .with_retries(3);

        let bollard_health = convert_health_check(&health);
        assert!(bollard_health.test.is_some());

        let test = bollard_health.test.unwrap();
        assert_eq!(test[0], "CMD-SHELL");
        assert_eq!(test[1], "curl -f http://localhost:8080/health");

        assert_eq!(bollard_health.retries, Some(3));
        assert_eq!(bollard_health.interval, Some(10_000_000_000)); // 10s in ns
        assert_eq!(bollard_health.timeout, Some(5_000_000_000)); // 5s in ns
    }

    #[test]
    fn parse_duration_tests() {
        assert_eq!(parse_duration_to_nanos("10s"), Some(10_000_000_000));
        assert_eq!(parse_duration_to_nanos("500ms"), Some(500_000_000));
        assert_eq!(parse_duration_to_nanos("1m"), Some(60_000_000_000));
        assert_eq!(parse_duration_to_nanos("1h"), Some(3_600_000_000_000));
        assert_eq!(parse_duration_to_nanos("100us"), Some(100_000));
        assert_eq!(parse_duration_to_nanos(""), None);
        assert_eq!(parse_duration_to_nanos("invalid"), None);
    }

    #[test]
    fn parse_memory_tests() {
        assert_eq!(parse_memory_limit("512m"), Some(512 * 1024 * 1024));
        assert_eq!(parse_memory_limit("1g"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory_limit("1024k"), Some(1024 * 1024));
        assert_eq!(parse_memory_limit("1024"), Some(1024));
        assert_eq!(parse_memory_limit(""), None);
    }

    #[test]
    fn parse_cpu_tests() {
        assert_eq!(parse_cpu_limit("0.5"), Some(500_000_000));
        assert_eq!(parse_cpu_limit("1"), Some(1_000_000_000));
        assert_eq!(parse_cpu_limit("2"), Some(2_000_000_000));
    }

    #[test]
    fn container_log_stream() {
        let mut logs = VecDeque::new();
        logs.push_back(LogEntry {
            timestamp: Utc::now(),
            stream: LogStreamKind::Stdout,
            line: "Line 1".to_string(),
        });
        logs.push_back(LogEntry {
            timestamp: Utc::now(),
            stream: LogStreamKind::Stderr,
            line: "Error 1".to_string(),
        });

        let mut stream = ContainerLogStream { logs, position: 0 };

        assert!(stream.is_open());

        let entry1 = stream.try_recv().unwrap();
        assert_eq!(entry1.line, "Line 1");
        assert_eq!(entry1.stream, LogStreamKind::Stdout);

        let entry2 = stream.try_recv().unwrap();
        assert_eq!(entry2.line, "Error 1");
        assert_eq!(entry2.stream, LogStreamKind::Stderr);

        assert!(stream.try_recv().is_none());
        assert!(!stream.is_open());
    }

    #[test]
    fn container_resource_downcast() {
        let config = make_config("test", "alpine:latest");
        let resource = make_resource(config);

        // Test as_any
        let any_ref = resource.as_any();
        assert!(any_ref.downcast_ref::<ContainerResource>().is_some());
    }
}
