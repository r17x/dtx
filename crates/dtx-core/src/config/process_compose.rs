//! Typed process-compose configuration.
//!
//! This module provides a strongly-typed representation of process-compose.yaml.
//! Because all inputs are validated domain types, serialization is infallible.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level process-compose configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessComposeConfig {
    /// Version of process-compose format.
    #[serde(default = "default_version")]
    pub version: String,

    /// Global log level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,

    /// Process definitions.
    pub processes: HashMap<String, ProcessConfig>,
}

fn default_version() -> String {
    "0.5".to_string()
}

/// Configuration for a single process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    /// Command to execute.
    pub command: String,

    /// Working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,

    /// Environment variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<Vec<String>>,

    /// Dependencies on other processes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<DependsOn>,

    /// Readiness probe configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness_probe: Option<Probe>,

    /// Liveness probe configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liveness_probe: Option<Probe>,

    /// Shutdown configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shutdown: Option<ShutdownConfig>,

    /// Number of replicas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<u32>,

    /// Availability mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub availability: Option<Availability>,

    /// Whether the process is disabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled: Option<bool>,

    /// Log configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_location: Option<String>,
}

/// Dependency configuration using HashMap format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependsOn(pub HashMap<String, DependencyCondition>);

/// Condition for a dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyCondition {
    /// The condition type - field read via serde serialization.
    #[allow(dead_code)]
    pub condition: DependencyConditionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyConditionType {
    ProcessStarted,
    ProcessHealthy,
    ProcessCompleted,
    ProcessCompletedSuccessfully,
}

/// Health/readiness probe configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Probe {
    /// Exec probe command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<ExecProbe>,

    /// HTTP GET probe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_get: Option<HttpGetProbe>,

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecProbe {
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpGetProbe {
    pub host: String,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShutdownConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Availability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backoff_seconds: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_restarts: Option<u32>,
}

impl ProcessComposeConfig {
    /// Create a new configuration with default version.
    pub fn new() -> Self {
        ProcessComposeConfig {
            version: default_version(),
            log_level: None,
            processes: HashMap::new(),
        }
    }

    /// Set the version.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Add a process configuration.
    pub fn add_process(&mut self, name: impl Into<String>, config: ProcessConfig) {
        self.processes.insert(name.into(), config);
    }

    /// Serialize to YAML.
    ///
    /// This function cannot fail because all input types are validated.
    /// The serde serialization of valid types always succeeds.
    pub fn to_yaml(&self) -> String {
        serde_yaml::to_string(self)
            .expect("ProcessComposeConfig serialization is infallible for valid types")
    }
}

impl Default for ProcessComposeConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessConfig {
    /// Create a minimal process configuration.
    pub fn new(command: impl Into<String>) -> Self {
        ProcessConfig {
            command: command.into(),
            working_dir: None,
            environment: None,
            depends_on: None,
            readiness_probe: None,
            liveness_probe: None,
            shutdown: None,
            replicas: None,
            availability: None,
            disabled: None,
            log_location: None,
        }
    }

    /// Builder: set working directory.
    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Builder: add environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let env_str = format!("{}={}", key.into(), value.into());
        match &mut self.environment {
            Some(env) => env.push(env_str),
            None => self.environment = Some(vec![env_str]),
        }
        self
    }

    /// Builder: set environment from map.
    pub fn with_environment(mut self, env: HashMap<String, String>) -> Self {
        let env_list: Vec<String> = env
            .into_iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        if !env_list.is_empty() {
            self.environment = Some(env_list);
        }
        self
    }

    /// Builder: set dependencies.
    pub fn with_depends_on(mut self, deps: HashMap<String, DependencyCondition>) -> Self {
        if !deps.is_empty() {
            self.depends_on = Some(DependsOn(deps));
        }
        self
    }

    /// Builder: add a single dependency.
    pub fn with_dependency(
        mut self,
        name: impl Into<String>,
        condition: DependencyConditionType,
    ) -> Self {
        let dep = DependencyCondition { condition };
        match &mut self.depends_on {
            Some(DependsOn(map)) => {
                map.insert(name.into(), dep);
            }
            None => {
                let mut map = HashMap::new();
                map.insert(name.into(), dep);
                self.depends_on = Some(DependsOn(map));
            }
        }
        self
    }

    /// Builder: set readiness probe.
    pub fn with_readiness_probe(mut self, probe: Probe) -> Self {
        self.readiness_probe = Some(probe);
        self
    }

    /// Builder: set shutdown config.
    pub fn with_shutdown(mut self, shutdown: ShutdownConfig) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    /// Builder: set availability config.
    pub fn with_availability(mut self, availability: Availability) -> Self {
        self.availability = Some(availability);
        self
    }

    /// Builder: set log location.
    pub fn with_log_location(mut self, location: impl Into<String>) -> Self {
        self.log_location = Some(location.into());
        self
    }
}

impl Probe {
    /// Create an exec probe.
    pub fn exec(command: impl Into<String>) -> Self {
        Probe {
            exec: Some(ExecProbe {
                command: command.into(),
            }),
            http_get: None,
            initial_delay_seconds: None,
            period_seconds: None,
            timeout_seconds: None,
            success_threshold: None,
            failure_threshold: None,
        }
    }

    /// Create an HTTP GET probe.
    pub fn http(host: impl Into<String>, port: u16, path: impl Into<String>) -> Self {
        Probe {
            exec: None,
            http_get: Some(HttpGetProbe {
                host: host.into(),
                port,
                path: Some(path.into()),
                scheme: None,
            }),
            initial_delay_seconds: None,
            period_seconds: None,
            timeout_seconds: None,
            success_threshold: None,
            failure_threshold: None,
        }
    }

    /// Set initial delay.
    pub fn with_initial_delay(mut self, seconds: u32) -> Self {
        self.initial_delay_seconds = Some(seconds);
        self
    }

    /// Set period between probes.
    pub fn with_period(mut self, seconds: u32) -> Self {
        self.period_seconds = Some(seconds);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config() {
        let mut config = ProcessComposeConfig::new();
        config.add_process("api", ProcessConfig::new("npm start"));

        let yaml = config.to_yaml();
        assert!(yaml.contains("version:"));
        assert!(yaml.contains("api:"));
        assert!(yaml.contains("command: npm start"));
    }

    #[test]
    fn config_with_dependencies() {
        let mut config = ProcessComposeConfig::new();

        config.add_process("db", ProcessConfig::new("postgres"));

        config.add_process(
            "api",
            ProcessConfig::new("npm start")
                .with_dependency("db", DependencyConditionType::ProcessHealthy),
        );

        let yaml = config.to_yaml();
        assert!(yaml.contains("depends_on:"));
        assert!(yaml.contains("condition: process_healthy"));
    }

    #[test]
    fn config_with_environment() {
        let mut config = ProcessComposeConfig::new();

        config.add_process(
            "api",
            ProcessConfig::new("npm start")
                .with_env("NODE_ENV", "production")
                .with_env("PORT", "3000"),
        );

        let yaml = config.to_yaml();
        assert!(yaml.contains("environment:"));
    }

    #[test]
    fn config_with_readiness_probe() {
        let mut config = ProcessComposeConfig::new();

        config.add_process(
            "api",
            ProcessConfig::new("npm start").with_readiness_probe(
                Probe::http("localhost", 3000, "/health")
                    .with_initial_delay(5)
                    .with_period(10),
            ),
        );

        let yaml = config.to_yaml();
        assert!(yaml.contains("readiness_probe:"));
        assert!(yaml.contains("http_get:"));
        assert!(yaml.contains("initial_delay_seconds:"));
    }

    #[test]
    fn config_with_shutdown() {
        let mut config = ProcessComposeConfig::new();

        config.add_process(
            "postgres",
            ProcessConfig::new("postgres").with_shutdown(ShutdownConfig {
                command: Some("pg_ctl stop".to_string()),
                signal: None,
                timeout_seconds: Some(30),
            }),
        );

        let yaml = config.to_yaml();
        assert!(yaml.contains("shutdown:"));
        assert!(yaml.contains("command: pg_ctl stop"));
    }

    #[test]
    fn config_with_availability() {
        let mut config = ProcessComposeConfig::new();

        config.add_process(
            "init",
            ProcessConfig::new("echo init")
                .with_availability(Availability {
                    restart: Some("no".to_string()),
                    backoff_seconds: None,
                    max_restarts: None,
                })
                .with_log_location(".dtx/logs/init.log"),
        );

        let yaml = config.to_yaml();
        assert!(yaml.contains("availability:"));
        assert!(yaml.contains("restart: no"));
        assert!(yaml.contains("log_location:"));
    }

    #[test]
    fn to_yaml_is_infallible() {
        // This test demonstrates that to_yaml() never panics
        // for any valid ProcessComposeConfig
        let config = ProcessComposeConfig::new();
        let _ = config.to_yaml();

        let mut config = ProcessComposeConfig::new();
        for i in 0..100 {
            config.add_process(
                format!("service-{}", i),
                ProcessConfig::new(format!("command-{}", i)),
            );
        }
        let _ = config.to_yaml();
    }

    #[test]
    fn roundtrip_yaml() {
        let mut config = ProcessComposeConfig::new();
        config.add_process(
            "api",
            ProcessConfig::new("npm start")
                .with_working_dir("/app")
                .with_env("PORT", "3000"),
        );

        let yaml = config.to_yaml();
        let parsed: ProcessComposeConfig = serde_yaml::from_str(&yaml).unwrap();

        assert_eq!(parsed.version, "0.5");
        assert!(parsed.processes.contains_key("api"));
    }
}
