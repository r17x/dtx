//! Container configuration for translation purposes.
//!
//! This is a translation-focused subset of container configuration.
//! The full Container Resource will be in dtx-container (Phase 8.1).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::resource::ResourceId;

/// Container configuration for translation/export.
///
/// Represents the minimum viable container definition that can be
/// generated from a process and exported to docker-compose/k8s.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContainerConfig {
    /// Resource identifier (becomes container name).
    pub id: ResourceId,

    /// Container image (e.g., "node:20", "postgres:15").
    pub image: String,

    /// Command to run (overrides image CMD).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,

    /// Entrypoint override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<Vec<String>>,

    /// Working directory inside container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,

    /// Environment variables.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub environment: HashMap<String, String>,

    /// Port mappings (host:container).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<PortMapping>,

    /// Volume mounts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<VolumeMount>,

    /// Network name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Restart policy.
    #[serde(default)]
    pub restart: ContainerRestartPolicy,

    /// Health check configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_check: Option<ContainerHealthCheck>,

    /// Container dependencies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<ContainerDependency>,

    /// Labels for metadata.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,

    /// Resource limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceLimits>,
}

impl ContainerConfig {
    /// Create a new container config.
    pub fn new(id: impl Into<String>, image: impl Into<String>) -> Self {
        Self {
            id: ResourceId::new(id),
            image: image.into(),
            command: None,
            entrypoint: None,
            working_dir: None,
            environment: HashMap::new(),
            ports: Vec::new(),
            volumes: Vec::new(),
            network: None,
            restart: ContainerRestartPolicy::default(),
            health_check: None,
            depends_on: Vec::new(),
            labels: HashMap::new(),
            resources: None,
        }
    }

    /// Builder: set command.
    pub fn with_command(mut self, cmd: Vec<String>) -> Self {
        self.command = Some(cmd);
        self
    }

    /// Builder: set command from string (splits on whitespace).
    pub fn with_command_str(mut self, cmd: impl Into<String>) -> Self {
        let cmd_str: String = cmd.into();
        self.command = Some(cmd_str.split_whitespace().map(String::from).collect());
        self
    }

    /// Builder: set entrypoint.
    pub fn with_entrypoint(mut self, entrypoint: Vec<String>) -> Self {
        self.entrypoint = Some(entrypoint);
        self
    }

    /// Builder: set working directory.
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Builder: add environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    /// Builder: set all environment variables.
    pub fn with_environment(mut self, env: HashMap<String, String>) -> Self {
        self.environment = env;
        self
    }

    /// Builder: add port mapping.
    pub fn with_port(mut self, host: u16, container: u16) -> Self {
        self.ports.push(PortMapping {
            host,
            container,
            protocol: Protocol::Tcp,
        });
        self
    }

    /// Builder: add port mapping (same port for host and container).
    pub fn with_port_same(mut self, port: u16) -> Self {
        self.ports.push(PortMapping::tcp(port));
        self
    }

    /// Builder: add volume mount.
    pub fn with_volume(mut self, source: impl Into<PathBuf>, target: impl Into<String>) -> Self {
        self.volumes.push(VolumeMount {
            source: source.into(),
            target: target.into(),
            read_only: false,
        });
        self
    }

    /// Builder: set network.
    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.network = Some(network.into());
        self
    }

    /// Builder: set restart policy.
    pub fn with_restart(mut self, policy: ContainerRestartPolicy) -> Self {
        self.restart = policy;
        self
    }

    /// Builder: set health check.
    pub fn with_health_check(mut self, check: ContainerHealthCheck) -> Self {
        self.health_check = Some(check);
        self
    }

    /// Builder: add dependency.
    pub fn depends_on_service(mut self, service: impl Into<String>) -> Self {
        self.depends_on.push(ContainerDependency {
            service: service.into(),
            condition: DependencyCondition::ServiceStarted,
        });
        self
    }

    /// Builder: add dependency with condition.
    pub fn depends_on_healthy(mut self, service: impl Into<String>) -> Self {
        self.depends_on.push(ContainerDependency {
            service: service.into(),
            condition: DependencyCondition::ServiceHealthy,
        });
        self
    }

    /// Builder: add label.
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }

    /// Builder: set resource limits.
    pub fn with_resources(mut self, resources: ResourceLimits) -> Self {
        self.resources = Some(resources);
        self
    }
}

/// Port mapping configuration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PortMapping {
    /// Host port.
    pub host: u16,
    /// Container port.
    pub container: u16,
    /// Protocol (tcp/udp).
    #[serde(default)]
    pub protocol: Protocol,
}

impl PortMapping {
    /// Create a simple TCP port mapping (same host and container port).
    pub fn tcp(port: u16) -> Self {
        Self {
            host: port,
            container: port,
            protocol: Protocol::Tcp,
        }
    }

    /// Create a TCP port mapping with different host/container ports.
    pub fn tcp_mapped(host: u16, container: u16) -> Self {
        Self {
            host,
            container,
            protocol: Protocol::Tcp,
        }
    }

    /// Create a UDP port mapping.
    pub fn udp(port: u16) -> Self {
        Self {
            host: port,
            container: port,
            protocol: Protocol::Udp,
        }
    }
}

/// Network protocol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Tcp,
    Udp,
}

/// Volume mount configuration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct VolumeMount {
    /// Source path (host or named volume).
    pub source: PathBuf,
    /// Target path inside container.
    pub target: String,
    /// Read-only mount.
    #[serde(default)]
    pub read_only: bool,
}

impl VolumeMount {
    /// Create a read-write volume mount.
    pub fn new(source: impl Into<PathBuf>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            read_only: false,
        }
    }

    /// Create a read-only volume mount.
    pub fn read_only(source: impl Into<PathBuf>, target: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            target: target.into(),
            read_only: true,
        }
    }
}

/// Container restart policy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerRestartPolicy {
    /// Never restart.
    #[default]
    No,
    /// Always restart.
    Always,
    /// Restart on failure.
    OnFailure,
    /// Restart unless stopped.
    UnlessStopped,
}

/// Container health check configuration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerHealthCheck {
    /// Health check command.
    pub test: HealthCheckTest,
    /// Time between checks.
    #[serde(default = "default_interval")]
    pub interval: String,
    /// Timeout for each check.
    #[serde(default = "default_timeout")]
    pub timeout: String,
    /// Consecutive failures before unhealthy.
    #[serde(default = "default_retries")]
    pub retries: u32,
    /// Start period (grace time before checks).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_period: Option<String>,
}

fn default_interval() -> String {
    "30s".to_string()
}

fn default_timeout() -> String {
    "10s".to_string()
}

fn default_retries() -> u32 {
    3
}

impl ContainerHealthCheck {
    /// Create a shell command health check.
    pub fn shell(cmd: impl Into<String>) -> Self {
        Self {
            test: HealthCheckTest::shell(cmd),
            interval: default_interval(),
            timeout: default_timeout(),
            retries: default_retries(),
            start_period: None,
        }
    }

    /// Create an exec command health check.
    pub fn exec(args: Vec<String>) -> Self {
        Self {
            test: HealthCheckTest::cmd(args),
            interval: default_interval(),
            timeout: default_timeout(),
            retries: default_retries(),
            start_period: None,
        }
    }

    /// Builder: set interval.
    pub fn with_interval(mut self, interval: impl Into<String>) -> Self {
        self.interval = interval.into();
        self
    }

    /// Builder: set timeout.
    pub fn with_timeout(mut self, timeout: impl Into<String>) -> Self {
        self.timeout = timeout.into();
        self
    }

    /// Builder: set retries.
    pub fn with_retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }

    /// Builder: set start period.
    pub fn with_start_period(mut self, start_period: impl Into<String>) -> Self {
        self.start_period = Some(start_period.into());
        self
    }
}

/// Health check test command.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum HealthCheckTest {
    /// CMD form (exec).
    Cmd(Vec<String>),
    /// CMD-SHELL form.
    CmdShell(String),
}

impl HealthCheckTest {
    /// Create CMD-SHELL test.
    pub fn shell(cmd: impl Into<String>) -> Self {
        Self::CmdShell(cmd.into())
    }

    /// Create CMD (exec) test.
    pub fn cmd(args: Vec<String>) -> Self {
        Self::Cmd(args)
    }
}

/// Container dependency.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerDependency {
    /// Service name.
    pub service: String,
    /// Condition to wait for.
    #[serde(default)]
    pub condition: DependencyCondition,
}

/// Dependency condition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyCondition {
    /// Wait for service to start.
    #[default]
    ServiceStarted,
    /// Wait for service to be healthy.
    ServiceHealthy,
    /// Wait for service to complete successfully.
    ServiceCompletedSuccessfully,
}

/// Resource limits.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceLimits {
    /// CPU limit (e.g., "0.5" for half a core).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpus: Option<String>,
    /// Memory limit (e.g., "512m", "1g").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

impl ResourceLimits {
    /// Create new resource limits.
    pub fn new() -> Self {
        Self {
            cpus: None,
            memory: None,
        }
    }

    /// Set CPU limit.
    pub fn with_cpus(mut self, cpus: impl Into<String>) -> Self {
        self.cpus = Some(cpus.into());
        self
    }

    /// Set memory limit.
    pub fn with_memory(mut self, memory: impl Into<String>) -> Self {
        self.memory = Some(memory.into());
        self
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_config_new() {
        let config = ContainerConfig::new("api", "node:20");
        assert_eq!(config.id.as_str(), "api");
        assert_eq!(config.image, "node:20");
    }

    #[test]
    fn container_config_builder() {
        let config = ContainerConfig::new("api", "node:20")
            .with_command(vec!["node".into(), "server.js".into()])
            .with_port(3000, 3000)
            .with_env("NODE_ENV", "production");

        assert_eq!(config.id.as_str(), "api");
        assert_eq!(config.image, "node:20");
        assert_eq!(
            config.command,
            Some(vec!["node".into(), "server.js".into()])
        );
        assert_eq!(config.ports.len(), 1);
        assert_eq!(
            config.environment.get("NODE_ENV"),
            Some(&"production".to_string())
        );
    }

    #[test]
    fn container_config_command_str() {
        let config =
            ContainerConfig::new("api", "node:20").with_command_str("node server.js --port 3000");

        assert_eq!(
            config.command,
            Some(vec![
                "node".into(),
                "server.js".into(),
                "--port".into(),
                "3000".into()
            ])
        );
    }

    #[test]
    fn container_config_dependencies() {
        let config = ContainerConfig::new("api", "node:20")
            .depends_on_service("db")
            .depends_on_healthy("cache");

        assert_eq!(config.depends_on.len(), 2);
        assert_eq!(config.depends_on[0].service, "db");
        assert_eq!(
            config.depends_on[0].condition,
            DependencyCondition::ServiceStarted
        );
        assert_eq!(config.depends_on[1].service, "cache");
        assert_eq!(
            config.depends_on[1].condition,
            DependencyCondition::ServiceHealthy
        );
    }

    #[test]
    fn container_config_serialization() {
        let config = ContainerConfig::new("db", "postgres:16")
            .with_port(5432, 5432)
            .with_env("POSTGRES_PASSWORD", "secret");

        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("postgres:16"));
        assert!(yaml.contains("5432"));
    }

    #[test]
    fn port_mapping_tcp() {
        let port = PortMapping::tcp(8080);
        assert_eq!(port.host, 8080);
        assert_eq!(port.container, 8080);
        assert_eq!(port.protocol, Protocol::Tcp);
    }

    #[test]
    fn port_mapping_mapped() {
        let port = PortMapping::tcp_mapped(80, 8080);
        assert_eq!(port.host, 80);
        assert_eq!(port.container, 8080);
    }

    #[test]
    fn volume_mount_new() {
        let vol = VolumeMount::new("./data", "/app/data");
        assert_eq!(vol.source, PathBuf::from("./data"));
        assert_eq!(vol.target, "/app/data");
        assert!(!vol.read_only);
    }

    #[test]
    fn volume_mount_read_only() {
        let vol = VolumeMount::read_only("./config", "/app/config");
        assert!(vol.read_only);
    }

    #[test]
    fn restart_policy_default() {
        let policy = ContainerRestartPolicy::default();
        assert_eq!(policy, ContainerRestartPolicy::No);
    }

    #[test]
    fn health_check_shell() {
        let check = ContainerHealthCheck::shell("curl -f http://localhost/health");
        assert!(matches!(check.test, HealthCheckTest::CmdShell(_)));
        assert_eq!(check.interval, "30s");
        assert_eq!(check.timeout, "10s");
        assert_eq!(check.retries, 3);
    }

    #[test]
    fn health_check_builder() {
        let check = ContainerHealthCheck::shell("curl -f localhost")
            .with_interval("10s")
            .with_timeout("5s")
            .with_retries(5)
            .with_start_period("30s");

        assert_eq!(check.interval, "10s");
        assert_eq!(check.timeout, "5s");
        assert_eq!(check.retries, 5);
        assert_eq!(check.start_period, Some("30s".to_string()));
    }

    #[test]
    fn resource_limits_builder() {
        let limits = ResourceLimits::new().with_cpus("0.5").with_memory("512m");

        assert_eq!(limits.cpus, Some("0.5".to_string()));
        assert_eq!(limits.memory, Some("512m".to_string()));
    }

    #[test]
    fn dependency_condition_serialization() {
        let cond = DependencyCondition::ServiceHealthy;
        let json = serde_json::to_string(&cond).unwrap();
        assert_eq!(json, "\"service_healthy\"");
    }
}
