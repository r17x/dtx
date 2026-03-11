//! Docker Compose exporter.
//!
//! Exports dtx projects to docker-compose.yaml format.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::error::{ExportError, ExportResult};
use super::types::{ExportFormat, ExportableProject, ExportableService, Exporter};
use crate::translation::{
    ContainerDependency, ContainerHealthCheck, ContainerRestartPolicy, DependencyCondition,
    HealthCheckTest, PortMapping, ResourceLimits, VolumeMount,
};

/// Docker Compose file exporter.
#[derive(Debug, Clone, Default)]
pub struct DockerComposeExporter {
    /// Compose file version (default: "3.8").
    version: Option<String>,
    /// Default network name.
    default_network: Option<String>,
}

impl DockerComposeExporter {
    /// Create a new Docker Compose exporter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set compose file version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set default network.
    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.default_network = Some(network.into());
        self
    }

    /// Convert project to compose file structure.
    fn to_compose_file(&self, project: &ExportableProject) -> ExportResult<ComposeFile> {
        let services: HashMap<String, ComposeService> = project
            .services
            .iter()
            .filter(|s| s.enabled)
            .map(|s| {
                let name = s.id.as_str().to_string();
                let service = self.service_to_compose(s)?;
                Ok((name, service))
            })
            .collect::<ExportResult<_>>()?;

        let mut networks = HashMap::new();
        if let Some(ref net) = self.default_network {
            networks.insert(net.clone(), ComposeNetwork::default());
        }
        for net in &project.networks {
            networks.insert(net.clone(), ComposeNetwork::default());
        }

        let mut volumes = HashMap::new();
        for vol in &project.volumes {
            volumes.insert(vol.clone(), ComposeVolume::default());
        }

        Ok(ComposeFile {
            version: self.version.clone(),
            services,
            networks: if networks.is_empty() {
                None
            } else {
                Some(networks)
            },
            volumes: if volumes.is_empty() {
                None
            } else {
                Some(volumes)
            },
        })
    }

    /// Convert service to compose service.
    fn service_to_compose(&self, service: &ExportableService) -> ExportResult<ComposeService> {
        let container = service.container.as_ref();

        // Get image from container or infer
        let image = container
            .map(|c| c.image.clone())
            .ok_or_else(|| ExportError::missing("image"))?;

        // Build ports
        let ports: Vec<String> = container
            .map(|c| c.ports.iter().map(format_port).collect())
            .unwrap_or_else(|| service.ports.iter().map(|p| p.to_string()).collect());

        // Build volumes
        let volumes: Vec<String> = container
            .map(|c| c.volumes.iter().map(format_volume).collect())
            .unwrap_or_default();

        // Build environment
        let environment: HashMap<String, String> = container
            .map(|c| c.environment.clone())
            .unwrap_or_else(|| service.environment.clone());

        // Build depends_on
        let depends_on = container
            .map(|c| {
                if c.depends_on.is_empty() {
                    None
                } else {
                    Some(ComposeDependsOn::Long(
                        c.depends_on
                            .iter()
                            .map(|d| (d.service.clone(), dependency_to_compose(d)))
                            .collect(),
                    ))
                }
            })
            .unwrap_or_else(|| {
                if service.depends_on.is_empty() {
                    None
                } else {
                    Some(ComposeDependsOn::Short(
                        service
                            .depends_on
                            .iter()
                            .map(|d| d.as_str().to_string())
                            .collect(),
                    ))
                }
            });

        // Build health check
        let healthcheck = container
            .and_then(|c| c.health_check.as_ref())
            .map(health_to_compose);

        // Build restart policy
        let restart = container.map(|c| format_restart(&c.restart));

        // Build command
        let command = container
            .and_then(|c| c.command.clone())
            .map(ComposeCommand::List);

        // Build entrypoint
        let entrypoint = container
            .and_then(|c| c.entrypoint.clone())
            .map(ComposeCommand::List);

        // Build working_dir
        let working_dir = container.and_then(|c| c.working_dir.clone());

        // Build deploy (resources)
        let deploy = container
            .and_then(|c| c.resources.as_ref())
            .map(resources_to_deploy);

        // Build labels
        let labels = container
            .map(|c| {
                if c.labels.is_empty() {
                    None
                } else {
                    Some(c.labels.clone())
                }
            })
            .unwrap_or(None);

        // Build networks
        let networks = container.and_then(|c| {
            c.network.as_ref().map(|n| {
                let mut map = HashMap::new();
                map.insert(n.clone(), ComposeServiceNetwork::default());
                map
            })
        });

        Ok(ComposeService {
            image: Some(image),
            command,
            entrypoint,
            working_dir,
            environment: if environment.is_empty() {
                None
            } else {
                Some(environment)
            },
            ports: if ports.is_empty() { None } else { Some(ports) },
            volumes: if volumes.is_empty() {
                None
            } else {
                Some(volumes)
            },
            depends_on,
            restart,
            healthcheck,
            deploy,
            labels,
            networks,
        })
    }
}

impl Exporter for DockerComposeExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::DockerCompose
    }

    fn export(&self, project: &ExportableProject) -> ExportResult<String> {
        let compose = self.to_compose_file(project)?;
        serde_yaml::to_string(&compose).map_err(|e| ExportError::serialization(e.to_string()))
    }
}

/// Docker Compose file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeFile {
    /// Compose file version (deprecated but still used).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Service definitions.
    pub services: HashMap<String, ComposeService>,

    /// Network definitions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub networks: Option<HashMap<String, ComposeNetwork>>,

    /// Volume definitions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<HashMap<String, ComposeVolume>>,
}

/// Compose service definition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposeService {
    /// Container image.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Command override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<ComposeCommand>,

    /// Entrypoint override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<ComposeCommand>,

    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,

    /// Environment variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<HashMap<String, String>>,

    /// Port mappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<String>>,

    /// Volume mounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<Vec<String>>,

    /// Service dependencies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<ComposeDependsOn>,

    /// Restart policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,

    /// Health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthcheck: Option<ComposeHealthCheck>,

    /// Deploy configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy: Option<ComposeDeploy>,

    /// Labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Networks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub networks: Option<HashMap<String, ComposeServiceNetwork>>,
}

/// Command can be string or list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComposeCommand {
    /// Single string command.
    String(String),
    /// List of command args.
    List(Vec<String>),
}

/// Dependencies can be short or long form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ComposeDependsOn {
    /// Short form: list of service names.
    Short(Vec<String>),
    /// Long form: service name to condition map.
    Long(HashMap<String, ComposeDependency>),
}

/// Long form dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeDependency {
    /// Condition to wait for - field read via serde serialization.
    #[allow(dead_code)]
    pub condition: String,
}

/// Health check configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeHealthCheck {
    /// Test command.
    pub test: Vec<String>,

    /// Interval between checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,

    /// Timeout for each check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,

    /// Number of retries.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<u32>,

    /// Start period.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_period: Option<String>,
}

/// Deploy configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposeDeploy {
    /// Resource limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ComposeResources>,
}

/// Resource configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposeResources {
    /// Resource limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<ComposeResourceLimits>,
}

/// Resource limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposeResourceLimits {
    /// CPU limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<String>,

    /// Memory limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,
}

/// Network definition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposeNetwork {
    /// Network driver.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,

    /// External network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<bool>,
}

/// Service network configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposeServiceNetwork {
    /// Aliases for this service on the network.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
}

/// Volume definition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ComposeVolume {
    /// Volume driver.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,

    /// External volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<bool>,
}

// Helper functions

/// Format port mapping.
fn format_port(port: &PortMapping) -> String {
    if port.host == port.container {
        format!("{}", port.host)
    } else {
        format!("{}:{}", port.host, port.container)
    }
}

/// Format volume mount.
fn format_volume(vol: &VolumeMount) -> String {
    let mut s = format!("{}:{}", vol.source.display(), vol.target);
    if vol.read_only {
        s.push_str(":ro");
    }
    s
}

/// Format restart policy.
fn format_restart(policy: &ContainerRestartPolicy) -> String {
    match policy {
        ContainerRestartPolicy::No => "no".to_string(),
        ContainerRestartPolicy::Always => "always".to_string(),
        ContainerRestartPolicy::OnFailure => "on-failure".to_string(),
        ContainerRestartPolicy::UnlessStopped => "unless-stopped".to_string(),
    }
}

/// Convert dependency to compose format.
fn dependency_to_compose(dep: &ContainerDependency) -> ComposeDependency {
    let condition = match dep.condition {
        DependencyCondition::ServiceStarted => "service_started",
        DependencyCondition::ServiceHealthy => "service_healthy",
        DependencyCondition::ServiceCompletedSuccessfully => "service_completed_successfully",
    };
    ComposeDependency {
        condition: condition.to_string(),
    }
}

/// Convert health check to compose format.
fn health_to_compose(check: &ContainerHealthCheck) -> ComposeHealthCheck {
    let test = match &check.test {
        HealthCheckTest::CmdShell(cmd) => vec!["CMD-SHELL".to_string(), cmd.clone()],
        HealthCheckTest::Cmd(args) => {
            let mut v = vec!["CMD".to_string()];
            v.extend(args.iter().cloned());
            v
        }
    };

    ComposeHealthCheck {
        test,
        interval: Some(check.interval.clone()),
        timeout: Some(check.timeout.clone()),
        retries: Some(check.retries),
        start_period: check.start_period.clone(),
    }
}

/// Convert resources to deploy format.
fn resources_to_deploy(limits: &ResourceLimits) -> ComposeDeploy {
    ComposeDeploy {
        resources: Some(ComposeResources {
            limits: Some(ComposeResourceLimits {
                cpus: limits.cpus.clone(),
                memory: limits.memory.clone(),
            }),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::ExportableService;
    use crate::translation::ContainerConfig;

    fn make_test_project() -> ExportableProject {
        let container = ContainerConfig::new("api", "node:20-alpine")
            .with_port_same(3000)
            .with_env("NODE_ENV", "production");

        ExportableProject::new("test-app")
            .with_service(ExportableService::from_container(container))
    }

    #[test]
    fn export_basic() {
        let exporter = DockerComposeExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("services:"));
        assert!(yaml.contains("api:"));
        assert!(yaml.contains("image: node:20-alpine"));
    }

    #[test]
    fn export_with_version() {
        let exporter = DockerComposeExporter::new().with_version("3.8");
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("version: '3.8'") || yaml.contains("version: \"3.8\""));
    }

    #[test]
    fn export_ports() {
        let exporter = DockerComposeExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("ports:"));
        assert!(yaml.contains("3000") || yaml.contains("'3000'"));
    }

    #[test]
    fn export_environment() {
        let exporter = DockerComposeExporter::new();
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("environment:"));
        assert!(yaml.contains("NODE_ENV"));
        assert!(yaml.contains("production"));
    }

    #[test]
    fn export_with_network() {
        let exporter = DockerComposeExporter::new().with_network("app-net");
        let project = make_test_project();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("networks:"));
        assert!(yaml.contains("app-net"));
    }

    #[test]
    fn export_with_depends_on() {
        let db_container = ContainerConfig::new("db", "postgres:16-alpine").with_port_same(5432);

        let api_container = ContainerConfig::new("api", "node:20-alpine")
            .with_port_same(3000)
            .depends_on_healthy("db");

        let project = ExportableProject::new("test")
            .with_service(ExportableService::from_container(db_container))
            .with_service(ExportableService::from_container(api_container));

        let exporter = DockerComposeExporter::new();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("depends_on:"));
        assert!(yaml.contains("db:"));
        assert!(yaml.contains("service_healthy"));
    }

    #[test]
    fn export_with_healthcheck() {
        let container =
            ContainerConfig::new("api", "node:20-alpine").with_health_check(ContainerHealthCheck {
                test: HealthCheckTest::shell("curl -f http://localhost:3000/health || exit 1"),
                interval: "30s".to_string(),
                timeout: "10s".to_string(),
                retries: 3,
                start_period: Some("10s".to_string()),
            });

        let project = ExportableProject::new("test")
            .with_service(ExportableService::from_container(container));

        let exporter = DockerComposeExporter::new();
        let yaml = exporter.export(&project).unwrap();

        assert!(yaml.contains("healthcheck:"));
        assert!(yaml.contains("CMD-SHELL"));
        assert!(yaml.contains("curl"));
    }

    #[test]
    fn export_disabled_services_skipped() {
        let container = ContainerConfig::new("api", "node:20-alpine");
        let service = ExportableService::from_container(container).with_enabled(false);

        let project = ExportableProject::new("test").with_service(service);

        let exporter = DockerComposeExporter::new();
        let yaml = exporter.export(&project).unwrap();

        assert!(!yaml.contains("api:"));
    }

    #[test]
    fn format_port_same() {
        let port = PortMapping::tcp(3000);
        assert_eq!(format_port(&port), "3000");
    }

    #[test]
    fn format_port_mapped() {
        let port = PortMapping::tcp_mapped(8080, 80);
        assert_eq!(format_port(&port), "8080:80");
    }

    #[test]
    fn format_volume_basic() {
        let vol = VolumeMount::new("./data", "/app/data");
        assert_eq!(format_volume(&vol), "./data:/app/data");
    }

    #[test]
    fn format_volume_readonly() {
        let vol = VolumeMount::read_only("./config", "/app/config");
        assert_eq!(format_volume(&vol), "./config:/app/config:ro");
    }

    #[test]
    fn format_restart_policies() {
        assert_eq!(format_restart(&ContainerRestartPolicy::No), "no");
        assert_eq!(format_restart(&ContainerRestartPolicy::Always), "always");
        assert_eq!(
            format_restart(&ContainerRestartPolicy::OnFailure),
            "on-failure"
        );
        assert_eq!(
            format_restart(&ContainerRestartPolicy::UnlessStopped),
            "unless-stopped"
        );
    }

    #[test]
    fn exporter_format() {
        let exporter = DockerComposeExporter::new();
        assert_eq!(exporter.format(), ExportFormat::DockerCompose);
    }

    #[test]
    fn compose_file_serializes() {
        let mut services = HashMap::new();
        services.insert(
            "web".to_string(),
            ComposeService {
                image: Some("nginx:alpine".to_string()),
                ..Default::default()
            },
        );

        let compose = ComposeFile {
            version: Some("3.8".to_string()),
            services,
            networks: None,
            volumes: None,
        };

        let yaml = serde_yaml::to_string(&compose).unwrap();
        assert!(yaml.contains("version:"));
        assert!(yaml.contains("services:"));
        assert!(yaml.contains("web:"));
        assert!(yaml.contains("nginx:alpine"));
    }
}
