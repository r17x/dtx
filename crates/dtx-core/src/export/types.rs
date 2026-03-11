//! Export types and traits.
//!
//! This module defines the core abstractions for the export system.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::error::{ExportError, ExportResult};
use crate::translation::ContainerConfig;
use crate::ResourceId;

/// Supported export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportFormat {
    /// Docker Compose YAML format.
    DockerCompose,
    /// Kubernetes manifest YAML format.
    Kubernetes,
    /// process-compose YAML format.
    ProcessCompose,
    /// dtx native format.
    Dtx,
}

impl ExportFormat {
    /// Get the file extension for this format.
    pub fn extension(&self) -> &'static str {
        match self {
            Self::DockerCompose => "yaml",
            Self::Kubernetes => "yaml",
            Self::ProcessCompose => "yaml",
            Self::Dtx => "yaml",
        }
    }

    /// Get the default filename for this format.
    pub fn default_filename(&self) -> &'static str {
        match self {
            Self::DockerCompose => "docker-compose.yaml",
            Self::Kubernetes => "k8s-manifest.yaml",
            Self::ProcessCompose => "process-compose.yaml",
            Self::Dtx => "config.yaml",
        }
    }

    /// Get all supported formats.
    pub fn all() -> &'static [ExportFormat] {
        &[
            Self::DockerCompose,
            Self::Kubernetes,
            Self::ProcessCompose,
            Self::Dtx,
        ]
    }

    /// Get format name for display.
    pub fn name(&self) -> &'static str {
        match self {
            Self::DockerCompose => "docker-compose",
            Self::Kubernetes => "kubernetes",
            Self::ProcessCompose => "process-compose",
            Self::Dtx => "dtx",
        }
    }
}

impl FromStr for ExportFormat {
    type Err = ExportError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "docker-compose" | "docker" | "compose" => Ok(Self::DockerCompose),
            "kubernetes" | "k8s" | "kube" => Ok(Self::Kubernetes),
            "process-compose" | "pc" => Ok(Self::ProcessCompose),
            "dtx" | "native" => Ok(Self::Dtx),
            _ => Err(ExportError::unsupported(s)),
        }
    }
}

impl std::fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Trait for exporters that can convert resources to specific formats.
pub trait Exporter: Send + Sync {
    /// Get the export format this exporter handles.
    fn format(&self) -> ExportFormat;

    /// Export a project to a string.
    fn export(&self, project: &ExportableProject) -> ExportResult<String>;

    /// Export a project to a file.
    fn export_to_file(&self, project: &ExportableProject, path: &Path) -> ExportResult<()> {
        let content = self.export(project)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Export to the default file in the given directory.
    fn export_to_dir(&self, project: &ExportableProject, dir: &Path) -> ExportResult<PathBuf> {
        let path = dir.join(self.format().default_filename());
        self.export_to_file(project, &path)?;
        Ok(path)
    }
}

/// A project that can be exported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportableProject {
    /// Project name.
    pub name: String,
    /// Project description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Services in the project.
    pub services: Vec<ExportableService>,
    /// Global environment variables.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub environment: HashMap<String, String>,
    /// Volumes defined at project level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<String>,
    /// Networks defined at project level.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub networks: Vec<String>,
}

impl ExportableProject {
    /// Create a new exportable project.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            services: Vec::new(),
            environment: HashMap::new(),
            volumes: Vec::new(),
            networks: Vec::new(),
        }
    }

    /// Add a description.
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Add a service.
    pub fn with_service(mut self, service: ExportableService) -> Self {
        self.services.push(service);
        self
    }

    /// Add multiple services.
    pub fn with_services(mut self, services: impl IntoIterator<Item = ExportableService>) -> Self {
        self.services.extend(services);
        self
    }

    /// Add an environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    /// Add a volume.
    pub fn with_volume(mut self, volume: impl Into<String>) -> Self {
        self.volumes.push(volume.into());
        self
    }

    /// Add a network.
    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.networks.push(network.into());
        self
    }
}

/// A service that can be exported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportableService {
    /// Service identifier.
    pub id: ResourceId,
    /// Service name for display.
    pub name: String,
    /// Container configuration (for container-based exports).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerConfig>,
    /// Original command (for process-based exports).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,
    /// Environment variables.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub environment: HashMap<String, String>,
    /// Port mappings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
    /// Dependencies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<ResourceId>,
    /// Whether service is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl ExportableService {
    /// Create a new exportable service.
    pub fn new(id: ResourceId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            container: None,
            command: None,
            working_dir: None,
            environment: HashMap::new(),
            ports: Vec::new(),
            depends_on: Vec::new(),
            enabled: true,
        }
    }

    /// Create from a container configuration.
    pub fn from_container(container: ContainerConfig) -> Self {
        let name = container.id.as_str().to_string();
        Self {
            id: container.id.clone(),
            name,
            ports: container.ports.iter().map(|p| p.host).collect(),
            environment: container.environment.clone(),
            depends_on: container
                .depends_on
                .iter()
                .map(|d| ResourceId::new(&d.service))
                .collect(),
            working_dir: container.working_dir.as_ref().map(PathBuf::from),
            command: container.command.as_ref().map(|c| c.join(" ")),
            container: Some(container),
            enabled: true,
        }
    }

    /// Set container configuration.
    pub fn with_container(mut self, container: ContainerConfig) -> Self {
        self.container = Some(container);
        self
    }

    /// Set command.
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set working directory.
    pub fn with_working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Add environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    /// Add a port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.ports.push(port);
        self
    }

    /// Add a dependency.
    pub fn depends_on(mut self, dep: ResourceId) -> Self {
        self.depends_on.push(dep);
        self
    }

    /// Set enabled state.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_format_extension() {
        assert_eq!(ExportFormat::DockerCompose.extension(), "yaml");
        assert_eq!(ExportFormat::Kubernetes.extension(), "yaml");
    }

    #[test]
    fn export_format_default_filename() {
        assert_eq!(
            ExportFormat::DockerCompose.default_filename(),
            "docker-compose.yaml"
        );
        assert_eq!(
            ExportFormat::Kubernetes.default_filename(),
            "k8s-manifest.yaml"
        );
    }

    #[test]
    fn export_format_from_str() {
        assert_eq!(
            "docker-compose".parse::<ExportFormat>().unwrap(),
            ExportFormat::DockerCompose
        );
        assert_eq!(
            "docker".parse::<ExportFormat>().unwrap(),
            ExportFormat::DockerCompose
        );
        assert_eq!(
            "kubernetes".parse::<ExportFormat>().unwrap(),
            ExportFormat::Kubernetes
        );
        assert_eq!(
            "k8s".parse::<ExportFormat>().unwrap(),
            ExportFormat::Kubernetes
        );
        assert_eq!(
            "process-compose".parse::<ExportFormat>().unwrap(),
            ExportFormat::ProcessCompose
        );
        assert_eq!("dtx".parse::<ExportFormat>().unwrap(), ExportFormat::Dtx);
    }

    #[test]
    fn export_format_from_str_case_insensitive() {
        assert_eq!(
            "DOCKER-COMPOSE".parse::<ExportFormat>().unwrap(),
            ExportFormat::DockerCompose
        );
        assert_eq!(
            "Kubernetes".parse::<ExportFormat>().unwrap(),
            ExportFormat::Kubernetes
        );
    }

    #[test]
    fn export_format_from_str_error() {
        let err = "unknown-format".parse::<ExportFormat>().unwrap_err();
        assert!(matches!(err, ExportError::Unsupported(_)));
    }

    #[test]
    fn export_format_display() {
        assert_eq!(ExportFormat::DockerCompose.to_string(), "docker-compose");
        assert_eq!(ExportFormat::Kubernetes.to_string(), "kubernetes");
    }

    #[test]
    fn exportable_project_new() {
        let project = ExportableProject::new("my-app");
        assert_eq!(project.name, "my-app");
        assert!(project.services.is_empty());
    }

    #[test]
    fn exportable_project_builder() {
        let project = ExportableProject::new("my-app")
            .with_description("Test project")
            .with_env("DEBUG", "true")
            .with_volume("data")
            .with_network("app-net");

        assert_eq!(project.description.as_deref(), Some("Test project"));
        assert_eq!(project.environment.get("DEBUG"), Some(&"true".to_string()));
        assert!(project.volumes.contains(&"data".to_string()));
        assert!(project.networks.contains(&"app-net".to_string()));
    }

    #[test]
    fn exportable_service_new() {
        let service = ExportableService::new(ResourceId::new("api"), "API Service");
        assert_eq!(service.id.as_str(), "api");
        assert_eq!(service.name, "API Service");
        assert!(service.enabled);
    }

    #[test]
    fn exportable_service_builder() {
        let service = ExportableService::new(ResourceId::new("api"), "API")
            .with_command("node server.js")
            .with_port(3000)
            .with_env("NODE_ENV", "production")
            .depends_on(ResourceId::new("db"));

        assert_eq!(service.command.as_deref(), Some("node server.js"));
        assert!(service.ports.contains(&3000));
        assert_eq!(
            service.environment.get("NODE_ENV"),
            Some(&"production".to_string())
        );
        assert!(service.depends_on.iter().any(|d| d.as_str() == "db"));
    }

    #[test]
    fn exportable_service_from_container() {
        use crate::translation::ContainerConfig;

        let container = ContainerConfig::new("web", "nginx:alpine").with_port_same(80);

        let service = ExportableService::from_container(container);
        assert_eq!(service.id.as_str(), "web");
        assert!(service.ports.contains(&80));
        assert!(service.container.is_some());
    }

    #[test]
    fn export_format_all() {
        let all = ExportFormat::all();
        assert!(all.contains(&ExportFormat::DockerCompose));
        assert!(all.contains(&ExportFormat::Kubernetes));
        assert!(all.contains(&ExportFormat::ProcessCompose));
        assert!(all.contains(&ExportFormat::Dtx));
    }
}
