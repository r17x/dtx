//! Configuration schema for .dtx/config.yaml
//!
//! Defines the configuration format for dtx projects, supporting:
//! - Process, container, VM, and agent resources
//! - Health checks (readiness/liveness)
//! - Restart policies
//! - Nix integration
//! - Hierarchical configuration (system/global/project)

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Current schema version.
pub const SCHEMA_VERSION: &str = "2";

/// Root configuration structure for .dtx/config.yaml
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DtxConfig {
    /// Schema version (defaults to "2", usually omitted).
    #[serde(
        default = "default_version",
        skip_serializing_if = "is_default_version"
    )]
    pub version: String,

    /// Project metadata.
    #[serde(default)]
    pub project: ProjectMetadata,

    /// Global settings.
    #[serde(default)]
    pub settings: GlobalConfig,

    /// Resource definitions.
    #[serde(default)]
    pub resources: IndexMap<String, ResourceConfig>,

    /// Global defaults for settings (used in global config files).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<DefaultsConfig>,

    /// Nix configuration (global level).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nix: Option<GlobalNixConfig>,

    /// AI configuration (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<AiConfig>,

    /// MCP server configuration (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<McpConfig>,
}

fn default_version() -> String {
    SCHEMA_VERSION.to_string()
}

fn is_default_version(v: &String) -> bool {
    v == SCHEMA_VERSION
}

/// Project metadata.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectMetadata {
    /// Project name.
    #[serde(default)]
    pub name: String,

    /// Project description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Global settings that affect all resources.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GlobalConfig {
    /// Log level: trace, debug, info, warn, error.
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Default health check interval (e.g., "5s").
    #[serde(default = "default_health_interval")]
    pub health_check_interval: String,

    /// Default shutdown timeout (e.g., "30s").
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout: String,

    /// Directory for log files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_dir: Option<PathBuf>,

    /// Use Unix Domain Socket for communication.
    #[serde(default = "default_true")]
    pub use_uds: bool,

    /// Auto-resolve port conflicts.
    #[serde(default = "default_true")]
    pub auto_resolve_ports: bool,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_health_interval() -> String {
    "5s".to_string()
}

fn default_shutdown_timeout() -> String {
    "30s".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            health_check_interval: default_health_interval(),
            shutdown_timeout: default_shutdown_timeout(),
            log_dir: None,
            use_uds: true,
            auto_resolve_ports: true,
        }
    }
}

/// Defaults configuration (for global config files).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DefaultsConfig {
    /// Default log level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,

    /// Default health check interval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check_interval: Option<String>,

    /// Default shutdown timeout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shutdown_timeout: Option<String>,
}

/// Global Nix configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GlobalNixConfig {
    /// Default package mappings (command -> package).
    #[serde(default)]
    pub mappings: IndexMap<String, String>,
}

/// MCP server configuration.
// Future: transport config, workspace roots
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct McpConfig {}

/// AI provider configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AiConfig {
    /// AI provider (e.g., "anthropic", "openai").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Model to use (e.g., "claude-3-haiku").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Resource configuration supporting multiple resource kinds.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// Resource kind: process | container | vm | agent
    #[serde(default = "default_kind")]
    pub kind: ResourceKindConfig,

    /// Command to execute (process/container).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Primary port for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<PathBuf>,

    /// Environment variables.
    #[serde(default)]
    pub environment: IndexMap<String, String>,

    /// Dependencies on other resources.
    #[serde(default)]
    pub depends_on: Vec<DependencyConfig>,

    /// Readiness/health check configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthConfig>,

    /// Liveness probe (separate from readiness).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveness: Option<HealthConfig>,

    /// Restart policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<RestartConfig>,

    /// Shutdown configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shutdown: Option<ShutdownConfigSchema>,

    /// Nix configuration for this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nix: Option<NixConfig>,

    /// Container image (container kind).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Volume mounts (container kind).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<String>,

    /// VM configuration (vm kind).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vm: Option<VmConfig>,

    /// Agent runtime (agent kind).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,

    /// Agent model (agent kind).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Agent tools (agent kind).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,

    /// Whether the resource is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            kind: ResourceKindConfig::Process,
            command: None,
            port: None,
            working_dir: None,
            environment: IndexMap::new(),
            depends_on: Vec::new(),
            health: None,
            liveness: None,
            restart: None,
            shutdown: None,
            nix: None,
            image: None,
            volumes: Vec::new(),
            vm: None,
            runtime: None,
            model: None,
            tools: Vec::new(),
            enabled: true,
        }
    }
}

/// Resource kind enumeration.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKindConfig {
    /// Native OS process.
    #[default]
    Process,
    /// Docker/Podman container.
    Container,
    /// Virtual machine (QEMU, Firecracker).
    Vm,
    /// AI agent.
    Agent,
}

fn default_kind() -> ResourceKindConfig {
    ResourceKindConfig::Process
}

/// Dependency configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencyConfig {
    /// Simple dependency: just the name.
    Simple(String),
    /// Dependency with condition: { "postgres": "healthy" }
    WithCondition(IndexMap<String, DependencyConditionConfig>),
}

impl DependencyConfig {
    /// Get the dependency name.
    pub fn name(&self) -> &str {
        match self {
            Self::Simple(name) => name,
            Self::WithCondition(map) => map.keys().next().map(|s| s.as_str()).unwrap_or(""),
        }
    }

    /// Get the dependency condition.
    pub fn condition(&self) -> DependencyConditionConfig {
        match self {
            Self::Simple(_) => DependencyConditionConfig::Started,
            Self::WithCondition(map) => map
                .values()
                .next()
                .cloned()
                .unwrap_or(DependencyConditionConfig::Started),
        }
    }
}

/// Dependency condition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DependencyConditionConfig {
    /// Wait for process to start.
    #[default]
    Started,
    /// Wait for health check to pass.
    Healthy,
    /// Wait for process to complete successfully.
    Completed,
}

/// Health check configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthConfig {
    /// Exec command for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<String>,

    /// HTTP path for health check (uses resource port).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<String>,

    /// TCP address for health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp: Option<String>,

    /// Check interval (e.g., "5s").
    #[serde(default = "default_health_interval")]
    pub interval: String,

    /// Check timeout (e.g., "10s").
    #[serde(default = "default_health_timeout")]
    pub timeout: String,

    /// Number of retries before unhealthy.
    #[serde(default = "default_retries")]
    pub retries: u32,

    /// Initial delay before first check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_delay: Option<String>,
}

fn default_health_timeout() -> String {
    "10s".to_string()
}

fn default_retries() -> u32 {
    3
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            exec: None,
            http: None,
            tcp: None,
            interval: default_health_interval(),
            timeout: default_health_timeout(),
            retries: default_retries(),
            initial_delay: None,
        }
    }
}

/// Restart policy configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RestartConfig {
    /// Simple: "always" | "on-failure" | "no"
    Simple(RestartPolicy),
    /// Extended configuration.
    Extended {
        /// Restart policy.
        policy: RestartPolicy,
        /// Maximum restart attempts.
        #[serde(skip_serializing_if = "Option::is_none")]
        max_attempts: Option<u32>,
        /// Backoff duration (e.g., "1s").
        #[serde(skip_serializing_if = "Option::is_none")]
        backoff: Option<String>,
        /// Grace period between restarts.
        #[serde(skip_serializing_if = "Option::is_none")]
        grace_period: Option<String>,
    },
}

impl Default for RestartConfig {
    fn default() -> Self {
        Self::Simple(RestartPolicy::No)
    }
}

impl RestartConfig {
    /// Get the restart policy.
    pub fn policy(&self) -> &RestartPolicy {
        match self {
            Self::Simple(p) => p,
            Self::Extended { policy, .. } => policy,
        }
    }

    /// Get max attempts (if extended).
    pub fn max_attempts(&self) -> Option<u32> {
        match self {
            Self::Simple(_) => None,
            Self::Extended { max_attempts, .. } => *max_attempts,
        }
    }
}

/// Restart policy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    /// Always restart.
    Always,
    /// Restart only on failure.
    OnFailure,
    /// Never restart.
    #[default]
    No,
}

/// Shutdown configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ShutdownConfigSchema {
    /// Custom shutdown command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Signal to send (e.g., "SIGTERM", "SIGINT").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,

    /// Timeout for graceful shutdown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
}

/// Nix configuration for a resource.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NixConfig {
    /// Simple package list.
    #[serde(default)]
    pub packages: Vec<String>,

    /// Nix expression (for complex environments).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,

    /// Path to shell.nix or flake reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
}

/// VM-specific configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VmConfig {
    /// VM backend (qemu, firecracker).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,

    /// Memory allocation (e.g., "2G").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<String>,

    /// Number of CPUs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpus: Option<u32>,

    /// Disk image path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk: Option<String>,

    /// NixOS configuration (for Nix VMs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nixos: Option<String>,
}

/// Configuration errors.
#[derive(Debug, Error)]
pub enum SchemaError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(String),

    /// Parse error.
    #[error("parse error: {0}")]
    Parse(String),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialize(String),

    /// Validation error.
    #[error("validation error: {0}")]
    Validation(String),

    /// Unknown resource kind.
    #[error("unknown resource kind: {0}")]
    UnknownKind(String),
}

impl From<std::io::Error> for SchemaError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err.to_string())
    }
}

impl From<serde_yaml::Error> for SchemaError {
    fn from(err: serde_yaml::Error) -> Self {
        Self::Parse(err.to_string())
    }
}

impl DtxConfig {
    /// Create a new empty config.
    pub fn new() -> Self {
        Self {
            version: SCHEMA_VERSION.to_string(),
            ..Default::default()
        }
    }

    /// Create a config with project name.
    pub fn with_project_name(name: impl Into<String>) -> Self {
        Self {
            version: SCHEMA_VERSION.to_string(),
            project: ProjectMetadata {
                name: name.into(),
                description: None,
            },
            ..Default::default()
        }
    }

    /// Load from YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self, SchemaError> {
        serde_yaml::from_str(yaml).map_err(SchemaError::from)
    }

    /// Serialize to YAML string.
    pub fn to_yaml(&self) -> Result<String, SchemaError> {
        serde_yaml::to_string(self).map_err(|e| SchemaError::Serialize(e.to_string()))
    }

    /// Load from file path.
    pub fn load(path: &std::path::Path) -> Result<Self, SchemaError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_yaml(&content)
    }

    /// Save to file path.
    pub fn save(&self, path: &std::path::Path) -> Result<(), SchemaError> {
        let yaml = self.to_yaml()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, yaml)?;
        Ok(())
    }

    /// Add a resource to the configuration.
    pub fn add_resource(&mut self, name: impl Into<String>, config: ResourceConfig) {
        self.resources.insert(name.into(), config);
    }

    /// Get a resource by name.
    pub fn get_resource(&self, name: &str) -> Option<&ResourceConfig> {
        self.resources.get(name)
    }

    /// Get mutable resource by name.
    pub fn get_resource_mut(&mut self, name: &str) -> Option<&mut ResourceConfig> {
        self.resources.get_mut(name)
    }

    /// Remove a resource by name.
    pub fn remove_resource(&mut self, name: &str) -> Option<ResourceConfig> {
        self.resources.shift_remove(name)
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.version.is_empty() {
            return Err(SchemaError::Validation("missing version".to_string()));
        }

        for (name, resource) in &self.resources {
            self.validate_resource(name, resource)?;
        }

        for (name, resource) in &self.resources {
            for dep in &resource.depends_on {
                let dep_name = dep.name();
                if !dep_name.is_empty() && !self.resources.contains_key(dep_name) {
                    return Err(SchemaError::Validation(format!(
                        "resource '{}' depends on unknown resource '{}'",
                        name, dep_name
                    )));
                }
            }
        }

        Ok(())
    }

    /// Validate a single resource configuration.
    fn validate_resource(&self, name: &str, resource: &ResourceConfig) -> Result<(), SchemaError> {
        match resource.kind {
            ResourceKindConfig::Process => {
                if resource.command.is_none() {
                    return Err(SchemaError::Validation(format!(
                        "process resource '{}' requires a command",
                        name
                    )));
                }
            }
            ResourceKindConfig::Container => {
                if resource.image.is_none() && resource.command.is_none() {
                    return Err(SchemaError::Validation(format!(
                        "container resource '{}' requires an image or command",
                        name
                    )));
                }
            }
            ResourceKindConfig::Agent => {
                if resource.runtime.is_none() {
                    return Err(SchemaError::Validation(format!(
                        "agent resource '{}' requires a runtime",
                        name
                    )));
                }
            }
            ResourceKindConfig::Vm => {
                // VMs have flexible validation
            }
        }

        if let Some(port) = resource.port {
            if port == 0 {
                return Err(SchemaError::Validation(format!(
                    "resource '{}' has invalid port 0",
                    name
                )));
            }
        }

        Ok(())
    }

    /// Get all enabled resources.
    pub fn enabled_resources(&self) -> impl Iterator<Item = (&String, &ResourceConfig)> {
        self.resources.iter().filter(|(_, r)| r.enabled)
    }

    /// Get resources of a specific kind.
    pub fn resources_by_kind(
        &self,
        kind: ResourceKindConfig,
    ) -> impl Iterator<Item = (&String, &ResourceConfig)> {
        self.resources.iter().filter(move |(_, r)| r.kind == kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let yaml = r#"
project:
  name: myapp
resources:
  api:
    command: npm run dev
    port: 3000
"#;
        let config = DtxConfig::from_yaml(yaml).unwrap();
        assert_eq!(config.project.name, "myapp");
        assert!(config.resources.contains_key("api"));
        let api = config.resources.get("api").unwrap();
        assert_eq!(api.command, Some("npm run dev".to_string()));
        assert_eq!(api.port, Some(3000));
    }

    #[test]
    fn parse_dependency_conditions() {
        let yaml = r#"
resources:
  db:
    command: postgres
  api:
    command: npm start
    depends_on:
      - db
      - cache: healthy
"#;
        let config = DtxConfig::from_yaml(yaml).unwrap();
        let api = config.resources.get("api").unwrap();
        assert_eq!(api.depends_on.len(), 2);
        assert_eq!(api.depends_on[0].name(), "db");
        assert_eq!(
            api.depends_on[0].condition(),
            DependencyConditionConfig::Started
        );
    }

    #[test]
    fn validate_missing_command() {
        let yaml = r#"
resources:
  api:
    port: 3000
"#;
        let config = DtxConfig::from_yaml(yaml).unwrap();
        let result = config.validate();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("requires a command"));
    }

    #[test]
    fn roundtrip_yaml() {
        let mut config = DtxConfig::with_project_name("test");
        config.add_resource(
            "api",
            ResourceConfig {
                command: Some("npm run dev".to_string()),
                port: Some(3000),
                ..Default::default()
            },
        );

        let yaml = config.to_yaml().unwrap();
        let parsed = DtxConfig::from_yaml(&yaml).unwrap();

        assert_eq!(parsed.project.name, "test");
        assert!(parsed.resources.contains_key("api"));
    }

    #[test]
    fn parse_mcp_config() {
        let yaml = r#"
project:
  name: test
mcp: {}
"#;
        let config = DtxConfig::from_yaml(yaml).unwrap();
        assert!(config.mcp.is_some());
    }

    #[test]
    fn mcp_config_defaults_to_none() {
        let yaml = r#"
project:
  name: test
"#;
        let config = DtxConfig::from_yaml(yaml).unwrap();
        assert!(config.mcp.is_none());
    }
}
