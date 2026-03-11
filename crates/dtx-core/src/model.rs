//! Lightweight service model types.
//!
//! These types serve as an intermediate representation between
//! `ResourceConfig` (the config.yaml schema) and `ProcessResourceConfig`
//! (the runtime process config). They are used by graph validation,
//! flake generation, port conflict resolution, and preflight checks.

use crate::config::schema::ResourceConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A service (process) that can be managed by dtx.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Service {
    pub name: String,
    pub command: String,
    pub package: Option<String>,
    pub port: Option<u16>,
    pub working_dir: Option<String>,
    pub environment: Option<HashMap<String, String>>,
    pub depends_on: Option<Vec<Dependency>>,
    pub health_check: Option<HealthCheck>,
    pub shutdown_command: Option<String>,
    pub enabled: bool,
}

/// A dependency on another service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Dependency {
    pub service: String,
    pub condition: DependencyCondition,
}

/// Conditions for service dependencies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DependencyCondition {
    ProcessHealthy,
    ProcessCompletedSuccessfully,
    ProcessStarted,
}

/// Health check configuration for a service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HealthCheck {
    #[serde(rename = "type")]
    pub check_type: HealthCheckType,
    pub command: Option<String>,
    pub http_get: Option<HttpHealthCheck>,
    #[serde(default)]
    pub initial_delay_seconds: u32,
    #[serde(default = "default_period")]
    pub period_seconds: u32,
}

fn default_period() -> u32 {
    10
}

/// Type of health check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckType {
    Exec,
    HttpGet,
}

/// HTTP health check configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpHealthCheck {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Service {
    pub fn new(name: String, command: String) -> Self {
        Self {
            name,
            command,
            package: None,
            port: None,
            working_dir: None,
            environment: None,
            depends_on: None,
            health_check: None,
            shutdown_command: None,
            enabled: true,
        }
    }

    pub fn with_package(mut self, package: String) -> Self {
        self.package = Some(package);
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    pub fn with_working_dir(mut self, working_dir: String) -> Self {
        self.working_dir = Some(working_dir);
        self
    }

    pub fn with_environment(mut self, env: HashMap<String, String>) -> Self {
        self.environment = Some(env);
        self
    }

    pub fn with_dependency(mut self, service: String, condition: DependencyCondition) -> Self {
        let dep = Dependency { service, condition };
        match &mut self.depends_on {
            Some(deps) => deps.push(dep),
            None => self.depends_on = Some(vec![dep]),
        }
        self
    }

    pub fn with_health_check(mut self, health_check: HealthCheck) -> Self {
        self.health_check = Some(health_check);
        self
    }

    pub fn with_shutdown_command(mut self, command: String) -> Self {
        self.shutdown_command = Some(command);
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.is_empty() {
            return Err("name cannot be empty".to_string());
        }
        if self.command.is_empty() {
            return Err("command cannot be empty".to_string());
        }
        Ok(())
    }

    /// Create a Service from a ResourceConfig entry.
    pub fn from_resource_config(name: &str, rc: &ResourceConfig) -> Self {
        use crate::config::schema::DependencyConfig;

        let package = rc.nix.as_ref().and_then(|n| n.packages.first().cloned());

        let depends_on = if rc.depends_on.is_empty() {
            None
        } else {
            Some(
                rc.depends_on
                    .iter()
                    .map(|dep| {
                        let condition = match dep {
                            DependencyConfig::Simple(_) => DependencyCondition::ProcessStarted,
                            DependencyConfig::WithCondition(map) => {
                                use crate::config::schema::DependencyConditionConfig;
                                match map.values().next() {
                                    Some(DependencyConditionConfig::Healthy) => {
                                        DependencyCondition::ProcessHealthy
                                    }
                                    Some(DependencyConditionConfig::Completed) => {
                                        DependencyCondition::ProcessCompletedSuccessfully
                                    }
                                    _ => DependencyCondition::ProcessStarted,
                                }
                            }
                        };
                        Dependency {
                            service: dep.name().to_string(),
                            condition,
                        }
                    })
                    .collect(),
            )
        };

        let health_check = rc.health.as_ref().map(|h| {
            if h.exec.is_some() {
                HealthCheck {
                    check_type: HealthCheckType::Exec,
                    command: h.exec.clone(),
                    http_get: None,
                    initial_delay_seconds: parse_duration_secs(h.initial_delay.as_deref()),
                    period_seconds: parse_duration_secs(Some(&h.interval)).max(1),
                }
            } else if let Some(ref http) = h.http {
                // Parse http spec: "host:port/path"
                let parts: Vec<&str> = http.splitn(2, '/').collect();
                let host_port = parts[0];
                let path = if parts.len() > 1 {
                    format!("/{}", parts[1])
                } else {
                    "/".to_string()
                };
                let hp_parts: Vec<&str> = host_port.rsplitn(2, ':').collect();
                let (host, port) = if hp_parts.len() == 2 {
                    (
                        hp_parts[1].to_string(),
                        hp_parts[0].parse::<u16>().unwrap_or(80),
                    )
                } else {
                    ("127.0.0.1".to_string(), 80)
                };
                HealthCheck {
                    check_type: HealthCheckType::HttpGet,
                    command: None,
                    http_get: Some(HttpHealthCheck { host, port, path }),
                    initial_delay_seconds: parse_duration_secs(h.initial_delay.as_deref()),
                    period_seconds: parse_duration_secs(Some(&h.interval)).max(1),
                }
            } else {
                HealthCheck {
                    check_type: HealthCheckType::Exec,
                    command: None,
                    http_get: None,
                    initial_delay_seconds: 0,
                    period_seconds: 10,
                }
            }
        });

        let environment = if rc.environment.is_empty() {
            None
        } else {
            Some(
                rc.environment
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        };

        Self {
            name: name.to_string(),
            command: rc.command.clone().unwrap_or_default(),
            package,
            port: rc.port,
            working_dir: rc
                .working_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            environment,
            depends_on,
            health_check,
            shutdown_command: rc.shutdown.as_ref().and_then(|s| s.command.clone()),
            enabled: rc.enabled,
        }
    }
}

impl HealthCheck {
    pub fn exec(command: String) -> Self {
        Self {
            check_type: HealthCheckType::Exec,
            command: Some(command),
            http_get: None,
            initial_delay_seconds: 0,
            period_seconds: default_period(),
        }
    }

    pub fn http(host: String, port: u16, path: String) -> Self {
        Self {
            check_type: HealthCheckType::HttpGet,
            command: None,
            http_get: Some(HttpHealthCheck { host, port, path }),
            initial_delay_seconds: 0,
            period_seconds: default_period(),
        }
    }

    pub fn with_initial_delay(mut self, seconds: u32) -> Self {
        self.initial_delay_seconds = seconds;
        self
    }

    pub fn with_period(mut self, seconds: u32) -> Self {
        self.period_seconds = seconds;
        self
    }
}

/// Parse a duration string like "5s" to seconds.
fn parse_duration_secs(s: Option<&str>) -> u32 {
    match s {
        Some(s) => {
            let s = s.trim();
            if let Some(n) = s.strip_suffix('s') {
                n.parse().unwrap_or(0)
            } else {
                s.parse().unwrap_or(0)
            }
        }
        None => 0,
    }
}

/// Convert a DtxConfig's resources to a Vec<Service>.
pub fn services_from_config(config: &crate::config::schema::DtxConfig) -> Vec<Service> {
    config
        .resources
        .iter()
        .map(|(name, rc)| Service::from_resource_config(name, rc))
        .collect()
}

/// Convert a DtxConfig's enabled resources to a Vec<Service>.
pub fn enabled_services_from_config(config: &crate::config::schema::DtxConfig) -> Vec<Service> {
    config
        .resources
        .iter()
        .filter(|(_, rc)| rc.enabled)
        .map(|(name, rc)| Service::from_resource_config(name, rc))
        .collect()
}
