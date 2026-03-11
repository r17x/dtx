//! YAML generator for process-compose configuration.

use crate::config::process_compose::{
    self as pc, ProcessComposeConfig, ProcessConfig, ShutdownConfig,
};
use crate::error::CoreError;
use crate::Result;
use crate::model::{Dependency, DependencyCondition, HealthCheck, HealthCheckType, Service};
use serde_yaml::{Mapping, Value};
use std::collections::HashMap;

/// Generator for process-compose YAML configuration.
pub struct YamlGenerator {
    /// Version of the process-compose format.
    version: String,
}

impl Default for YamlGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl YamlGenerator {
    /// Creates a new YAML generator with the default version.
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::YamlGenerator;
    ///
    /// let generator = YamlGenerator::new();
    /// ```
    pub fn new() -> Self {
        Self {
            version: "0.5".to_string(),
        }
    }

    /// Sets the process-compose version.
    pub fn with_version(mut self, version: String) -> Self {
        self.version = version;
        self
    }

    /// Generates process-compose YAML from a list of services.
    ///
    /// # Arguments
    ///
    /// * `services` - The services to include in the configuration
    ///
    /// # Returns
    ///
    /// A string containing valid process-compose YAML.
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::YamlGenerator;
    /// use dtx_core::model::Service;
    ///
    /// let generator = YamlGenerator::new();
    /// let service = Service::new("postgres".to_string(), "postgres -D /data".to_string());
    /// let yaml = generator.generate(vec![service]).unwrap();
    /// assert!(yaml.contains("postgres:"));
    /// ```
    pub fn generate(&self, services: Vec<Service>) -> Result<String> {
        let mut processes = Mapping::new();

        for service in services {
            if !service.enabled {
                continue;
            }

            // Validate service before generating
            service.validate().map_err(CoreError::Validation)?;

            let process = self.serialize_service(&service)?;
            processes.insert(Value::String(service.name), process);
        }

        let mut root = Mapping::new();
        root.insert(
            Value::String("version".to_string()),
            Value::String(self.version.clone()),
        );
        root.insert(
            Value::String("processes".to_string()),
            Value::Mapping(processes),
        );

        let yaml = serde_yaml::to_string(&root)?;
        Ok(yaml)
    }

    /// Generates a typed `ProcessComposeConfig` from a list of services.
    ///
    /// This is the Phase 6.3 typed generation method. The returned config
    /// can be serialized to YAML via `.to_yaml()` (infallible).
    ///
    /// # Examples
    ///
    /// ```
    /// use dtx_core::YamlGenerator;
    /// use dtx_core::model::Service;
    ///
    /// let generator = YamlGenerator::new();
    /// let service = Service::new("postgres".to_string(), "postgres -D /data".to_string());
    /// let config = generator.generate_config(vec![service]).unwrap();
    /// let yaml = config.to_yaml(); // infallible
    /// assert!(yaml.contains("postgres:"));
    /// ```
    pub fn generate_config(&self, services: Vec<Service>) -> Result<ProcessComposeConfig> {
        let mut config = ProcessComposeConfig::new().with_version(&self.version);

        for service in services {
            if !service.enabled {
                continue;
            }

            // Validate service before generating
            service.validate().map_err(CoreError::Validation)?;

            // Build the main process config
            let mut process = ProcessConfig::new(&service.command);

            // Optional working directory
            if let Some(ref working_dir) = service.working_dir {
                process = process.with_working_dir(working_dir.as_str());
            }

            // Optional environment variables
            if let Some(ref env) = service.environment {
                process = process.with_environment(env.clone());
            }

            // Optional health check (as readiness_probe)
            if let Some(ref health_check) = service.health_check {
                let probe = self.build_probe(health_check);
                process = process.with_readiness_probe(probe);
            }

            // Dependencies
            let all_deps: Vec<Dependency> = service.depends_on.clone().unwrap_or_default();
            if !all_deps.is_empty() {
                let mut dep_map = HashMap::new();
                for dep in &all_deps {
                    let condition = match dep.condition {
                        DependencyCondition::ProcessHealthy => {
                            pc::DependencyConditionType::ProcessHealthy
                        }
                        DependencyCondition::ProcessCompletedSuccessfully => {
                            pc::DependencyConditionType::ProcessCompletedSuccessfully
                        }
                        DependencyCondition::ProcessStarted => {
                            pc::DependencyConditionType::ProcessStarted
                        }
                    };
                    dep_map.insert(dep.service.clone(), pc::DependencyCondition { condition });
                }
                process = process.with_depends_on(dep_map);
            }

            // Optional shutdown command
            if let Some(ref shutdown_cmd) = service.shutdown_command {
                process = process.with_shutdown(ShutdownConfig {
                    command: Some(shutdown_cmd.clone()),
                    signal: None,
                    timeout_seconds: None,
                });
            }

            config.add_process(&service.name, process);
        }

        Ok(config)
    }

    /// Builds a typed `Probe` from a `HealthCheck`.
    fn build_probe(&self, hc: &HealthCheck) -> pc::Probe {
        let mut probe = match hc.check_type {
            HealthCheckType::Exec => pc::Probe::exec(hc.command.as_deref().unwrap_or_default()),
            HealthCheckType::HttpGet => {
                if let Some(ref http) = hc.http_get {
                    pc::Probe::http(&http.host, http.port, &http.path)
                } else {
                    pc::Probe::exec("true") // fallback
                }
            }
        };

        if hc.initial_delay_seconds > 0 {
            probe = probe.with_initial_delay(hc.initial_delay_seconds);
        }
        if hc.period_seconds > 0 {
            probe = probe.with_period(hc.period_seconds);
        }

        probe
    }

    /// Serializes a single service to a YAML value.
    fn serialize_service(&self, service: &Service) -> Result<Value> {
        let mut process = Mapping::new();

        // Command is required
        process.insert(
            Value::String("command".to_string()),
            Value::String(service.command.clone()),
        );

        // Optional working directory
        if let Some(ref working_dir) = service.working_dir {
            process.insert(
                Value::String("working_dir".to_string()),
                Value::String(working_dir.clone()),
            );
        }

        // Optional environment variables
        if let Some(ref env) = service.environment {
            let env_list: Vec<Value> = env
                .iter()
                .map(|(k, v)| Value::String(format!("{}={}", k, v)))
                .collect();
            process.insert(
                Value::String("environment".to_string()),
                Value::Sequence(env_list),
            );
        }

        // Optional health check (as readiness_probe)
        if let Some(ref health_check) = service.health_check {
            let probe = self.serialize_health_check(health_check)?;
            process.insert(Value::String("readiness_probe".to_string()), probe);
        }

        // Dependencies
        if let Some(ref deps) = service.depends_on {
            if !deps.is_empty() {
                let depends = self.serialize_dependencies(deps)?;
                process.insert(Value::String("depends_on".to_string()), depends);
            }
        }

        // Optional shutdown command
        if let Some(ref shutdown_cmd) = service.shutdown_command {
            process.insert(
                Value::String("shutdown".to_string()),
                Value::Mapping({
                    let mut m = Mapping::new();
                    m.insert(
                        Value::String("command".to_string()),
                        Value::String(shutdown_cmd.clone()),
                    );
                    m
                }),
            );
        }

        Ok(Value::Mapping(process))
    }

    /// Serializes a health check to a YAML value.
    fn serialize_health_check(&self, hc: &HealthCheck) -> Result<Value> {
        let mut probe = Mapping::new();

        match hc.check_type {
            HealthCheckType::Exec => {
                if let Some(ref cmd) = hc.command {
                    probe.insert(
                        Value::String("exec".to_string()),
                        Value::Mapping({
                            let mut m = Mapping::new();
                            m.insert(
                                Value::String("command".to_string()),
                                Value::String(cmd.clone()),
                            );
                            m
                        }),
                    );
                }
            }
            HealthCheckType::HttpGet => {
                if let Some(ref http) = hc.http_get {
                    probe.insert(
                        Value::String("http_get".to_string()),
                        Value::Mapping({
                            let mut m = Mapping::new();
                            m.insert(
                                Value::String("host".to_string()),
                                Value::String(http.host.clone()),
                            );
                            m.insert(
                                Value::String("port".to_string()),
                                Value::Number(http.port.into()),
                            );
                            m.insert(
                                Value::String("path".to_string()),
                                Value::String(http.path.clone()),
                            );
                            m
                        }),
                    );
                }
            }
        }

        probe.insert(
            Value::String("initial_delay_seconds".to_string()),
            Value::Number(hc.initial_delay_seconds.into()),
        );
        probe.insert(
            Value::String("period_seconds".to_string()),
            Value::Number(hc.period_seconds.into()),
        );

        Ok(Value::Mapping(probe))
    }

    /// Serializes dependencies to a YAML value.
    fn serialize_dependencies(&self, deps: &[Dependency]) -> Result<Value> {
        let mut depends_on = Mapping::new();

        for dep in deps {
            let condition_str = match dep.condition {
                DependencyCondition::ProcessHealthy => "process_healthy",
                DependencyCondition::ProcessCompletedSuccessfully => {
                    "process_completed_successfully"
                }
                DependencyCondition::ProcessStarted => "process_started",
            };

            depends_on.insert(
                Value::String(dep.service.clone()),
                Value::Mapping({
                    let mut m = Mapping::new();
                    m.insert(
                        Value::String("condition".to_string()),
                        Value::String(condition_str.to_string()),
                    );
                    m
                }),
            );
        }

        Ok(Value::Mapping(depends_on))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_basic_yaml_generation() {
        let generator = YamlGenerator::new();
        let service = Service::new("postgres".to_string(), "postgres -D /data".to_string());

        let yaml = generator.generate(vec![service]).unwrap();

        assert!(yaml.contains("version: '0.5'") || yaml.contains("version: \"0.5\""));
        assert!(yaml.contains("processes:"));
        assert!(yaml.contains("postgres:"));
        assert!(yaml.contains("command: postgres -D /data"));
    }

    #[test]
    fn test_disabled_service_excluded() {
        let generator = YamlGenerator::new();
        let service = Service::new("disabled".to_string(), "echo test".to_string()).disabled();

        let yaml = generator.generate(vec![service]).unwrap();
        assert!(!yaml.contains("disabled:"));
    }

    #[test]
    fn test_service_with_environment() {
        let generator = YamlGenerator::new();
        let mut env = HashMap::new();
        env.insert(
            "DATABASE_URL".to_string(),
            "postgres://localhost".to_string(),
        );
        env.insert("PORT".to_string(), "3000".to_string());

        let service =
            Service::new("api".to_string(), "node server.js".to_string()).with_environment(env);

        let yaml = generator.generate(vec![service]).unwrap();
        assert!(yaml.contains("environment:"));
        // Environment variables are formatted as KEY=VALUE
        assert!(yaml.contains("DATABASE_URL=postgres://localhost") || yaml.contains("PORT=3000"));
    }

    #[test]
    fn test_service_with_working_dir() {
        let generator = YamlGenerator::new();
        let service = Service::new("app".to_string(), "./run.sh".to_string())
            .with_working_dir("/app".to_string());

        let yaml = generator.generate(vec![service]).unwrap();
        assert!(yaml.contains("working_dir: /app"));
    }

    #[test]
    fn test_service_with_health_check_exec() {
        let generator = YamlGenerator::new();
        let service = Service::new("postgres".to_string(), "postgres -D /data".to_string())
            .with_health_check(HealthCheck::exec("pg_isready".to_string()).with_initial_delay(5));

        let yaml = generator.generate(vec![service]).unwrap();
        assert!(yaml.contains("readiness_probe:"));
        assert!(yaml.contains("exec:"));
        assert!(yaml.contains("command: pg_isready"));
        assert!(yaml.contains("initial_delay_seconds: 5"));
    }

    #[test]
    fn test_service_with_health_check_http() {
        let generator = YamlGenerator::new();
        let service =
            Service::new("api".to_string(), "node server.js".to_string()).with_health_check(
                HealthCheck::http("localhost".to_string(), 3000, "/health".to_string()),
            );

        let yaml = generator.generate(vec![service]).unwrap();
        assert!(yaml.contains("readiness_probe:"));
        assert!(yaml.contains("http_get:"));
        assert!(yaml.contains("host: localhost"));
        assert!(yaml.contains("port: 3000"));
        assert!(yaml.contains("path: /health"));
    }

    #[test]
    fn test_service_with_dependencies() {
        let generator = YamlGenerator::new();
        let service = Service::new("api".to_string(), "node server.js".to_string())
            .with_dependency("postgres".to_string(), DependencyCondition::ProcessHealthy)
            .with_dependency("redis".to_string(), DependencyCondition::ProcessStarted);

        let yaml = generator.generate(vec![service]).unwrap();
        assert!(yaml.contains("depends_on:"));
        assert!(yaml.contains("postgres:"));
        assert!(yaml.contains("condition: process_healthy"));
        assert!(yaml.contains("redis:"));
        assert!(yaml.contains("condition: process_started"));
    }

    #[test]
    fn test_service_with_shutdown_command() {
        let generator = YamlGenerator::new();
        let service = Service::new("postgres".to_string(), "postgres -D /data".to_string())
            .with_shutdown_command("pg_ctl stop -D /data".to_string());

        let yaml = generator.generate(vec![service]).unwrap();
        assert!(yaml.contains("shutdown:"));
        assert!(yaml.contains("command: pg_ctl stop -D /data"));
    }

    #[test]
    fn test_multiple_services() {
        let generator = YamlGenerator::new();
        let postgres = Service::new("postgres".to_string(), "postgres -D /data".to_string());
        let redis = Service::new("redis".to_string(), "redis-server".to_string());
        let api = Service::new("api".to_string(), "node server.js".to_string())
            .with_dependency("postgres".to_string(), DependencyCondition::ProcessHealthy)
            .with_dependency("redis".to_string(), DependencyCondition::ProcessStarted);

        let yaml = generator.generate(vec![postgres, redis, api]).unwrap();
        assert!(yaml.contains("postgres:"));
        assert!(yaml.contains("redis:"));
        assert!(yaml.contains("api:"));
    }

    #[test]
    fn test_custom_version() {
        let generator = YamlGenerator::new().with_version("0.6".to_string());
        let service = Service::new("test".to_string(), "echo test".to_string());

        let yaml = generator.generate(vec![service]).unwrap();
        assert!(yaml.contains("version: '0.6'") || yaml.contains("version: \"0.6\""));
    }

    #[test]
    fn test_empty_services() {
        let generator = YamlGenerator::new();
        let yaml = generator.generate(vec![]).unwrap();
        assert!(yaml.contains("processes:"));
    }

    #[test]
    fn test_invalid_service_rejected() {
        let generator = YamlGenerator::new();
        let mut service = Service::new("test".to_string(), "echo test".to_string());
        service.name = String::new(); // Invalid: empty name

        let result = generator.generate(vec![service]);
        assert!(result.is_err());
    }

}
