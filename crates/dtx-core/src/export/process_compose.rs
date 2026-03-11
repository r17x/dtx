//! Process-compose v1.x compatible exporter.
//!
//! This module exports dtx projects to process-compose.yaml format,
//! maintaining full compatibility with process-compose v1.x.
//!
//! # process-compose Compatibility Matrix
//!
//! dtx is designed as a **superset of process-compose v1.x**. All process-compose
//! fields are supported, with some renamed for clarity.
//!
//! ## Core Fields Mapping
//!
//! | process-compose       | dtx                    | Status    | Notes                       |
//! |-----------------------|------------------------|-----------|------------------------------|
//! | `command`             | `command`              | Identical | Shell command to run         |
//! | `working_dir`         | `working_dir`          | Identical | Working directory            |
//! | `environment`         | `environment`          | Identical | Environment variables (map)  |
//! | `log_location`        | `settings.log_dir`     | Moved     | Global or per-resource       |
//! | `namespace`           | `project.name`         | Renamed   | Project-level setting        |
//! | `disabled`            | `enabled: false`       | Inverted  | Clearer semantics            |
//! | `is_daemon`           | (inferred)             | Auto      | Detected from behavior       |
//! | `shutdown.command`    | `shutdown.command`     | Identical | Custom shutdown command      |
//! | `shutdown.signal`     | `shutdown.signal`      | Identical | Signal to send (SIGTERM)     |
//! | `shutdown.timeout_seconds` | `shutdown.timeout` | Renamed   | Duration format in dtx       |
//!
//! ## Dependency Fields Mapping
//!
//! | process-compose                                    | dtx                        | Status     |
//! |----------------------------------------------------|----------------------------|------------|
//! | `depends_on` (list)                                | `depends_on` (list)        | Identical  |
//! | `depends_on` (map)                                 | `depends_on` (map)         | Identical  |
//! | `depends_on.*.condition: process_healthy`          | `healthy`                  | Simplified |
//! | `depends_on.*.condition: process_started`          | `started`                  | Simplified |
//! | `depends_on.*.condition: process_completed_successfully` | `completed`          | Simplified |
//! | `availability.restart`                             | `restart`                  | Flattened  |
//! | `availability.backoff_seconds`                     | `restart.backoff`          | Extended   |
//! | `availability.max_restarts`                        | `restart.max_attempts`     | Renamed    |
//!
//! ## Health Check Fields Mapping
//!
//! | process-compose                         | dtx                  | Status     | Notes                    |
//! |-----------------------------------------|----------------------|------------|--------------------------|
//! | `readiness_probe`                       | `health`             | Renamed    | Clearer for most uses    |
//! | `liveness_probe`                        | `liveness`           | Separate   | dtx adds distinct liveness |
//! | `readiness_probe.exec`                  | `health.exec`        | Identical  | Command to run           |
//! | `readiness_probe.http_get`              | `health.http`        | Simplified | Path, host/port inferred |
//! | `readiness_probe.initial_delay_seconds` | `health.initial_delay` | Renamed  | Duration format          |
//! | `readiness_probe.period_seconds`        | `health.interval`    | Renamed    | Duration format          |
//! | `readiness_probe.timeout_seconds`       | `health.timeout`     | Renamed    | Duration format          |
//! | `readiness_probe.failure_threshold`     | `health.retries`     | Renamed    | Clearer name             |
//! | `readiness_probe.success_threshold`     | `health.success_threshold` | Identical |                     |
//!
//! ## dtx Extensions (not in process-compose)
//!
//! | Field           | Purpose                | Example                        |
//! |-----------------|------------------------|--------------------------------|
//! | `kind`          | Resource type          | `process`, `container`, `agent`|
//! | `port`          | Primary port           | Used for health checks, display|
//! | `nix.packages`  | Nix packages (list)    | `[nodejs_20, postgresql_16]`   |
//! | `nix.expr`      | Nix expression (string)| Complex Nix environments       |
//! | `nix.shell`     | Nix shell file path    | `./shell.nix`                  |
//! | `runtime`       | Agent runtime          | `openai`, `anthropic`, `local` |
//! | `model`         | Agent model            | `gpt-4`, `claude-3`            |
//! | `tools`         | Agent tools            | List of available tools        |
//! | `liveness`      | Liveness probe         | Separate from readiness        |

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::error::{ExportError, ExportResult};
use super::types::{ExportFormat, ExportableProject, ExportableService, Exporter};
use crate::translation::{ContainerHealthCheck, HealthCheckTest};

/// process-compose format version.
pub const PROCESS_COMPOSE_VERSION: &str = "0.5";

/// Condition values for process-compose dependencies.
#[allow(dead_code)]
pub mod conditions {
    /// Process has started (default).
    pub const PROCESS_STARTED: &str = "process_started";
    /// Process is healthy (readiness probe passed).
    pub const PROCESS_HEALTHY: &str = "process_healthy";
    /// Process completed (exited with any code).
    pub const PROCESS_COMPLETED: &str = "process_completed";
    /// Process completed successfully (exit code 0).
    pub const PROCESS_COMPLETED_SUCCESSFULLY: &str = "process_completed_successfully";
}

/// Restart policy values for process-compose.
pub mod restart_policies {
    /// Always restart the process.
    pub const ALWAYS: &str = "always";
    /// Restart on failure (non-zero exit).
    pub const ON_FAILURE: &str = "on_failure";
    /// Never restart.
    pub const NO: &str = "no";
}

/// Default values for process-compose fields.
#[allow(dead_code)]
pub mod defaults {
    /// Default period between health probes (seconds).
    pub const PROBE_PERIOD_SECONDS: u32 = 10;
    /// Default timeout for health probes (seconds).
    pub const PROBE_TIMEOUT_SECONDS: u32 = 5;
    /// Default failure threshold.
    pub const PROBE_FAILURE_THRESHOLD: u32 = 3;
    /// Default success threshold.
    pub const PROBE_SUCCESS_THRESHOLD: u32 = 1;
    /// Default shutdown timeout (seconds).
    pub const SHUTDOWN_TIMEOUT_SECONDS: u32 = 10;
    /// Default backoff delay (seconds).
    pub const BACKOFF_SECONDS: u32 = 1;
}

/// Process-compose exporter (v1.x compatible).
#[derive(Debug, Clone, Default)]
pub struct ProcessComposeExporter {
    /// Include dtx metadata as comments in output.
    include_comments: bool,
    /// Custom version string (defaults to PROCESS_COMPOSE_VERSION).
    version: Option<String>,
    /// Global log level.
    log_level: Option<String>,
    /// Log location template.
    log_location: Option<String>,
}

impl ProcessComposeExporter {
    /// Create a new process-compose exporter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Include comments with dtx extension info.
    pub fn with_comments(mut self, include: bool) -> Self {
        self.include_comments = include;
        self
    }

    /// Set custom version string.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set global log level.
    pub fn with_log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = Some(level.into());
        self
    }

    /// Set log location template.
    pub fn with_log_location(mut self, location: impl Into<String>) -> Self {
        self.log_location = Some(location.into());
        self
    }

    /// Build process-compose file structure.
    fn build_process_compose(
        &self,
        project: &ExportableProject,
    ) -> ExportResult<ProcessComposeFile> {
        let mut processes = HashMap::new();

        for service in project.services.iter().filter(|s| s.enabled) {
            let process = self.build_process(service)?;
            processes.insert(service.id.as_str().to_string(), process);
        }

        Ok(ProcessComposeFile {
            version: self
                .version
                .clone()
                .unwrap_or_else(|| PROCESS_COMPOSE_VERSION.to_string()),
            log_level: self.log_level.clone(),
            processes,
        })
    }

    /// Build a single process configuration.
    fn build_process(&self, service: &ExportableService) -> ExportResult<ProcessComposeProcess> {
        // Get command from service or container
        let command = service
            .command
            .clone()
            .or_else(|| {
                service
                    .container
                    .as_ref()
                    .and_then(|c| c.command.as_ref().map(|args| args.join(" ")))
            })
            .ok_or_else(|| ExportError::missing("command"))?;

        // Get working directory
        let working_dir = service
            .working_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| {
                service
                    .container
                    .as_ref()
                    .and_then(|c| c.working_dir.clone())
            });

        // Get environment variables
        let environment = if service.environment.is_empty() {
            service
                .container
                .as_ref()
                .filter(|c| !c.environment.is_empty())
                .map(|c| env_map_to_list(&c.environment))
        } else {
            Some(env_map_to_list(&service.environment))
        };

        // Build depends_on
        let depends_on = self.build_depends_on(service);

        // Build probes from container config
        let (readiness_probe, liveness_probe) = self.build_probes(service);

        // Build shutdown config
        let shutdown = self.build_shutdown(service);

        // Build availability (restart policy)
        let availability = self.build_availability(service);

        // Disabled state (inverted from enabled)
        let disabled = if service.enabled { None } else { Some(true) };

        // Log location
        let log_location = self.log_location.clone();

        Ok(ProcessComposeProcess {
            command,
            working_dir,
            environment,
            depends_on,
            readiness_probe,
            liveness_probe,
            shutdown,
            replicas: None,
            availability,
            disabled,
            log_location,
        })
    }

    /// Build depends_on configuration.
    fn build_depends_on(
        &self,
        service: &ExportableService,
    ) -> Option<HashMap<String, ProcessComposeDependency>> {
        // First try to get from container with conditions
        if let Some(container) = &service.container {
            if !container.depends_on.is_empty() {
                let deps: HashMap<_, _> = container
                    .depends_on
                    .iter()
                    .map(|d| {
                        let condition = match d.condition {
                            crate::translation::DependencyCondition::ServiceStarted => {
                                conditions::PROCESS_STARTED
                            }
                            crate::translation::DependencyCondition::ServiceHealthy => {
                                conditions::PROCESS_HEALTHY
                            }
                            crate::translation::DependencyCondition::ServiceCompletedSuccessfully => {
                                conditions::PROCESS_COMPLETED_SUCCESSFULLY
                            }
                        };
                        (
                            d.service.clone(),
                            ProcessComposeDependency {
                                condition: condition.to_string(),
                            },
                        )
                    })
                    .collect();
                return Some(deps);
            }
        }

        // Fall back to simple depends_on list (default to started)
        if !service.depends_on.is_empty() {
            let deps: HashMap<_, _> = service
                .depends_on
                .iter()
                .map(|d| {
                    (
                        d.as_str().to_string(),
                        ProcessComposeDependency {
                            condition: conditions::PROCESS_STARTED.to_string(),
                        },
                    )
                })
                .collect();
            return Some(deps);
        }

        None
    }

    /// Build readiness and liveness probes.
    fn build_probes(
        &self,
        service: &ExportableService,
    ) -> (Option<ProcessComposeProbe>, Option<ProcessComposeProbe>) {
        let container = match &service.container {
            Some(c) => c,
            None => return (None, None),
        };

        let readiness = container
            .health_check
            .as_ref()
            .map(|hc| health_check_to_probe(hc, service.ports.first().copied()));

        // For liveness, we could use a separate liveness config if available
        let liveness = None;

        (readiness, liveness)
    }

    /// Build shutdown configuration.
    fn build_shutdown(&self, _service: &ExportableService) -> Option<ProcessComposeShutdown> {
        // Container config doesn't have shutdown details
        None
    }

    /// Build availability (restart) configuration.
    fn build_availability(
        &self,
        service: &ExportableService,
    ) -> Option<ProcessComposeAvailability> {
        let container = match &service.container {
            Some(c) => c,
            None => return None,
        };

        let restart = match container.restart {
            crate::translation::ContainerRestartPolicy::Always => {
                Some(restart_policies::ALWAYS.to_string())
            }
            crate::translation::ContainerRestartPolicy::OnFailure => {
                Some(restart_policies::ON_FAILURE.to_string())
            }
            crate::translation::ContainerRestartPolicy::No => {
                Some(restart_policies::NO.to_string())
            }
            crate::translation::ContainerRestartPolicy::UnlessStopped => {
                Some(restart_policies::ALWAYS.to_string())
            }
        };

        restart.map(|r| ProcessComposeAvailability {
            restart: Some(r),
            backoff_seconds: None,
            max_restarts: None,
        })
    }
}

impl Exporter for ProcessComposeExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::ProcessCompose
    }

    fn export(&self, project: &ExportableProject) -> ExportResult<String> {
        let pc = self.build_process_compose(project)?;
        serde_yaml::to_string(&pc).map_err(|e| ExportError::serialization(e.to_string()))
    }
}

/// Convert environment map to list format (KEY=VALUE).
fn env_map_to_list(env: &HashMap<String, String>) -> Vec<String> {
    env.iter().map(|(k, v)| format!("{}={}", k, v)).collect()
}

/// Convert health check to process-compose probe format.
fn health_check_to_probe(hc: &ContainerHealthCheck, _port: Option<u16>) -> ProcessComposeProbe {
    let (exec, http_get) = match &hc.test {
        HealthCheckTest::CmdShell(cmd) => (
            Some(ProcessComposeExec {
                command: cmd.clone(),
            }),
            None,
        ),
        HealthCheckTest::Cmd(args) => (
            Some(ProcessComposeExec {
                command: args.join(" "),
            }),
            None,
        ),
    };

    let initial_delay_seconds = hc
        .start_period
        .as_ref()
        .and_then(|s| parse_duration_seconds(s));
    let period_seconds =
        Some(parse_duration_seconds(&hc.interval).unwrap_or(defaults::PROBE_PERIOD_SECONDS));
    let timeout_seconds =
        Some(parse_duration_seconds(&hc.timeout).unwrap_or(defaults::PROBE_TIMEOUT_SECONDS));
    let failure_threshold = Some(hc.retries);
    let success_threshold = Some(defaults::PROBE_SUCCESS_THRESHOLD);

    ProcessComposeProbe {
        exec,
        http_get,
        initial_delay_seconds,
        period_seconds,
        timeout_seconds,
        success_threshold,
        failure_threshold,
    }
}

/// Parse a duration string like "30s" to seconds.
fn parse_duration_seconds(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.ends_with('s') {
        s.trim_end_matches('s').parse().ok()
    } else if s.ends_with('m') {
        s.trim_end_matches('m').parse::<u32>().ok().map(|m| m * 60)
    } else {
        s.parse().ok()
    }
}

// ============================================================================
// Process-compose YAML structures (matching v1.x schema)
// ============================================================================

/// Process-compose file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeFile {
    /// Format version.
    pub version: String,

    /// Global log level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,

    /// Process definitions.
    pub processes: HashMap<String, ProcessComposeProcess>,
}

/// Process configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeProcess {
    /// Command to execute.
    pub command: String,

    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,

    /// Environment variables (KEY=VALUE format).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Vec<String>>,

    /// Dependencies on other processes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<HashMap<String, ProcessComposeDependency>>,

    /// Readiness probe configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness_probe: Option<ProcessComposeProbe>,

    /// Liveness probe configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveness_probe: Option<ProcessComposeProbe>,

    /// Shutdown configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shutdown: Option<ProcessComposeShutdown>,

    /// Number of replicas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<u32>,

    /// Availability mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<ProcessComposeAvailability>,

    /// Whether the process is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    /// Log file location.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_location: Option<String>,
}

/// Dependency with condition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeDependency {
    /// Condition to wait for - field read via serde serialization.
    #[allow(dead_code)]
    pub condition: String,
}

/// Health/readiness probe configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeProbe {
    /// Exec probe command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<ProcessComposeExec>,

    /// HTTP GET probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_get: Option<ProcessComposeHttpGet>,

    /// Initial delay before starting probes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_delay_seconds: Option<u32>,

    /// Period between probes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_seconds: Option<u32>,

    /// Timeout for each probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,

    /// Number of successes to be healthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_threshold: Option<u32>,

    /// Number of failures to be unhealthy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_threshold: Option<u32>,
}

/// Exec probe command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeExec {
    /// Command to execute.
    pub command: String,
}

/// HTTP GET probe configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeHttpGet {
    /// Host to connect to.
    pub host: String,

    /// Port to connect to.
    pub port: u16,

    /// Path to request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Scheme (http or https).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
}

/// Shutdown configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeShutdown {
    /// Custom shutdown command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,

    /// Signal to send (as integer, e.g., 15 for SIGTERM).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,

    /// Timeout before SIGKILL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
}

/// Availability (restart policy) configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeAvailability {
    /// Restart policy: "always", "on_failure", "no".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,

    /// Backoff delay between restarts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backoff_seconds: Option<u32>,

    /// Maximum restart attempts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_restarts: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_seconds() {
        assert_eq!(parse_duration_seconds("30s"), Some(30));
        assert_eq!(parse_duration_seconds("5s"), Some(5));
        assert_eq!(parse_duration_seconds("2m"), Some(120));
        assert_eq!(parse_duration_seconds("10"), Some(10));
        assert_eq!(parse_duration_seconds("invalid"), None);
    }

    #[test]
    fn test_dependency_condition_constants() {
        assert_eq!(conditions::PROCESS_STARTED, "process_started");
        assert_eq!(conditions::PROCESS_HEALTHY, "process_healthy");
        assert_eq!(conditions::PROCESS_COMPLETED, "process_completed");
        assert_eq!(
            conditions::PROCESS_COMPLETED_SUCCESSFULLY,
            "process_completed_successfully"
        );
    }

    #[test]
    fn test_restart_policy_constants() {
        assert_eq!(restart_policies::ALWAYS, "always");
        assert_eq!(restart_policies::ON_FAILURE, "on_failure");
        assert_eq!(restart_policies::NO, "no");
    }

    #[test]
    fn test_env_map_to_list() {
        let mut env = HashMap::new();
        env.insert("KEY1".to_string(), "value1".to_string());
        env.insert("KEY2".to_string(), "value2".to_string());

        let list = env_map_to_list(&env);
        assert!(list.contains(&"KEY1=value1".to_string()));
        assert!(list.contains(&"KEY2=value2".to_string()));
    }
}
