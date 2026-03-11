//! Translator between ProcessResourceConfig and ContainerConfig.
//!
//! This module provides bidirectional translation between process and
//! container configurations.

use std::collections::HashMap;

use dtx_core::translation::{
    infer_image, Confidence, ContainerConfig, ContainerDependency, ContainerHealthCheck,
    ContainerRestartPolicy, ContextualTranslator, DependencyCondition, HealthCheckTest,
    PortMapping, TranslationContext, TranslationError, TranslationResult, TranslatorMetadata,
};
use dtx_core::ResourceId;

use crate::config::{ProbeConfig, ProcessResourceConfig, RestartPolicy};

/// Translator from ProcessResourceConfig to ContainerConfig.
pub struct ProcessToContainerTranslator;

impl ContextualTranslator<ProcessResourceConfig, ContainerConfig> for ProcessToContainerTranslator {
    fn translate(
        &self,
        process: &ProcessResourceConfig,
        ctx: &TranslationContext,
    ) -> TranslationResult<ContainerConfig> {
        // Determine image
        let image = ctx
            .get_default::<String>("image")
            .or_else(|| {
                let inferred = infer_image(&process.command);
                if ctx.options.strict && inferred.confidence == Confidence::Low {
                    None
                } else {
                    Some(inferred.image)
                }
            })
            .ok_or_else(|| TranslationError::missing_field("image"))?;

        // Build command - split into args for exec form
        let command = parse_command(&process.command);

        // Translate restart policy
        let restart = translate_restart_policy(&process.restart);

        // Translate health check
        let health_check = process
            .readiness_probe
            .as_ref()
            .or(process.liveness_probe.as_ref())
            .map(translate_health_check)
            .transpose()?;

        // Translate dependencies
        let depends_on: Vec<ContainerDependency> = process
            .depends_on
            .iter()
            .map(|dep| ContainerDependency {
                service: dep.as_str().to_string(),
                condition: DependencyCondition::ServiceStarted,
            })
            .collect();

        // Build port mappings
        let ports = process
            .port
            .map(|p| vec![PortMapping::tcp(p)])
            .unwrap_or_default();

        // Add dtx labels for reverse translation
        let mut labels = HashMap::new();
        labels.insert("dtx.source".to_string(), "process".to_string());
        labels.insert("dtx.original_command".to_string(), process.command.clone());
        if let Some(ref wd) = process.working_dir {
            labels.insert(
                "dtx.original_working_dir".to_string(),
                wd.to_string_lossy().to_string(),
            );
        }

        Ok(ContainerConfig {
            id: process.id.clone(),
            image,
            command: Some(command),
            entrypoint: None,
            working_dir: process
                .working_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            environment: process.environment.clone(),
            ports,
            volumes: Vec::new(), // Simplified: no volume mapping in basic translation
            network: ctx.get_default("network"),
            restart,
            health_check,
            depends_on,
            labels,
            resources: None,
        })
    }

    fn reverse(
        &self,
        container: &ContainerConfig,
        _ctx: &TranslationContext,
    ) -> TranslationResult<ProcessResourceConfig> {
        // Reconstruct command
        let command = container
            .command
            .as_ref()
            .map(|c| c.join(" "))
            .or_else(|| container.labels.get("dtx.original_command").cloned())
            .ok_or_else(|| TranslationError::missing_field("command"))?;

        // Translate restart policy back
        let restart = reverse_restart_policy(&container.restart);

        // Translate health check back
        let readiness_probe = container
            .health_check
            .as_ref()
            .map(reverse_health_check)
            .transpose()?;

        // Get port
        let port = container.ports.first().map(|p| p.host);

        // Translate dependencies
        let depends_on = container
            .depends_on
            .iter()
            .map(|d| ResourceId::new(&d.service))
            .collect();

        // Get working dir from container or labels
        let working_dir = container
            .working_dir
            .as_ref()
            .map(|s| s.into())
            .or_else(|| {
                container
                    .labels
                    .get("dtx.original_working_dir")
                    .map(|s| s.into())
            });

        Ok(ProcessResourceConfig {
            id: container.id.clone(),
            command,
            working_dir,
            environment: container.environment.clone(),
            port,
            shutdown: Default::default(),
            restart,
            readiness_probe,
            liveness_probe: None,
            depends_on,
        })
    }

    fn supports_reverse(&self) -> bool {
        true
    }

    fn metadata(&self) -> TranslatorMetadata {
        TranslatorMetadata::new("ProcessToContainer")
            .with_description("Translates native process to container configuration")
            .lossy() // Some process specifics may not map to containers
    }
}

/// Parse command string into exec form arguments.
fn parse_command(cmd: &str) -> Vec<String> {
    // Simple parsing - split on whitespace
    // TODO: Handle quoted strings properly
    cmd.split_whitespace().map(String::from).collect()
}

/// Translate process restart policy to container restart policy.
fn translate_restart_policy(policy: &RestartPolicy) -> ContainerRestartPolicy {
    match policy {
        RestartPolicy::Always { .. } => ContainerRestartPolicy::Always,
        RestartPolicy::OnFailure { .. } => ContainerRestartPolicy::OnFailure,
        RestartPolicy::No => ContainerRestartPolicy::No,
    }
}

/// Reverse translate container restart policy.
fn reverse_restart_policy(policy: &ContainerRestartPolicy) -> RestartPolicy {
    match policy {
        ContainerRestartPolicy::Always | ContainerRestartPolicy::UnlessStopped => {
            RestartPolicy::Always {
                max_retries: None,
                backoff: Default::default(),
            }
        }
        ContainerRestartPolicy::OnFailure => RestartPolicy::OnFailure {
            max_retries: Some(3),
            backoff: Default::default(),
        },
        ContainerRestartPolicy::No => RestartPolicy::No,
    }
}

/// Translate process probe to container health check.
fn translate_health_check(probe: &ProbeConfig) -> TranslationResult<ContainerHealthCheck> {
    let (test, interval, timeout, retries, start_period) = match probe {
        ProbeConfig::Exec { command, settings } => (
            HealthCheckTest::shell(command.clone()),
            format!("{}s", settings.period.as_secs()),
            format!("{}s", settings.timeout.as_secs()),
            settings.failure_threshold,
            if settings.initial_delay.as_secs() > 0 {
                Some(format!("{}s", settings.initial_delay.as_secs()))
            } else {
                None
            },
        ),
        ProbeConfig::HttpGet {
            host,
            port,
            path,
            settings,
        } => (
            HealthCheckTest::shell(format!(
                "wget --quiet --tries=1 --spider http://{}:{}{} || exit 1",
                host, port, path
            )),
            format!("{}s", settings.period.as_secs()),
            format!("{}s", settings.timeout.as_secs()),
            settings.failure_threshold,
            if settings.initial_delay.as_secs() > 0 {
                Some(format!("{}s", settings.initial_delay.as_secs()))
            } else {
                None
            },
        ),
        ProbeConfig::TcpSocket {
            host,
            port,
            settings,
        } => (
            HealthCheckTest::shell(format!("nc -z {} {}", host, port)),
            format!("{}s", settings.period.as_secs()),
            format!("{}s", settings.timeout.as_secs()),
            settings.failure_threshold,
            if settings.initial_delay.as_secs() > 0 {
                Some(format!("{}s", settings.initial_delay.as_secs()))
            } else {
                None
            },
        ),
    };

    Ok(ContainerHealthCheck {
        test,
        interval,
        timeout,
        retries,
        start_period,
    })
}

/// Reverse translate container health check to process probe.
fn reverse_health_check(check: &ContainerHealthCheck) -> TranslationResult<ProbeConfig> {
    // Extract command from health check
    let command = match &check.test {
        HealthCheckTest::CmdShell(cmd) => cmd.clone(),
        HealthCheckTest::Cmd(args) => args.join(" "),
    };

    // Parse duration strings (simplified)
    fn parse_duration(s: &str) -> std::time::Duration {
        let secs = s.trim_end_matches('s').parse().unwrap_or(10);
        std::time::Duration::from_secs(secs)
    }

    let settings = crate::config::ProbeSettings {
        initial_delay: check
            .start_period
            .as_ref()
            .map(|s| parse_duration(s))
            .unwrap_or_default(),
        period: parse_duration(&check.interval),
        timeout: parse_duration(&check.timeout),
        success_threshold: 1,
        failure_threshold: check.retries,
    };

    Ok(ProbeConfig::Exec { command, settings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProbeSettings;
    use std::time::Duration;

    #[test]
    fn translate_basic_process() {
        let process = ProcessResourceConfig::new("api", "node server.js").with_port(3000);

        let translator = ProcessToContainerTranslator;
        let ctx = TranslationContext::new();

        let container = translator.translate(&process, &ctx).unwrap();

        assert_eq!(container.id.as_str(), "api");
        assert!(container.image.starts_with("node:"));
        assert_eq!(container.ports.len(), 1);
        assert_eq!(container.ports[0].host, 3000);
    }

    #[test]
    fn translate_with_environment() {
        let process = ProcessResourceConfig::new("api", "node server.js")
            .with_env("NODE_ENV", "production")
            .with_env("PORT", "3000");

        let translator = ProcessToContainerTranslator;
        let container = translator
            .translate(&process, &TranslationContext::new())
            .unwrap();

        assert_eq!(
            container.environment.get("NODE_ENV"),
            Some(&"production".to_string())
        );
        assert_eq!(container.environment.get("PORT"), Some(&"3000".to_string()));
    }

    #[test]
    fn translate_with_dependencies() {
        let process = ProcessResourceConfig::new("api", "node server.js")
            .depends_on(ResourceId::new("db"))
            .depends_on(ResourceId::new("cache"));

        let translator = ProcessToContainerTranslator;
        let container = translator
            .translate(&process, &TranslationContext::new())
            .unwrap();

        assert_eq!(container.depends_on.len(), 2);
        assert_eq!(container.depends_on[0].service, "db");
        assert_eq!(container.depends_on[1].service, "cache");
    }

    #[test]
    fn translate_with_health_check() {
        let process = ProcessResourceConfig::new("api", "node server.js").with_readiness_probe(
            ProbeConfig::HttpGet {
                host: "localhost".into(),
                port: 3000,
                path: "/health".into(),
                settings: ProbeSettings {
                    period: Duration::from_secs(10),
                    timeout: Duration::from_secs(5),
                    failure_threshold: 3,
                    ..Default::default()
                },
            },
        );

        let translator = ProcessToContainerTranslator;
        let container = translator
            .translate(&process, &TranslationContext::new())
            .unwrap();

        assert!(container.health_check.is_some());
        let check = container.health_check.unwrap();
        match &check.test {
            HealthCheckTest::CmdShell(cmd) => assert!(cmd.contains("wget")),
            _ => panic!("Expected shell command"),
        }
    }

    #[test]
    fn translate_with_explicit_image() {
        let process = ProcessResourceConfig::new("api", "my-app");
        let ctx =
            TranslationContext::new().default_value("image", "myregistry/myapp:v1.0".to_string());

        let translator = ProcessToContainerTranslator;
        let container = translator.translate(&process, &ctx).unwrap();

        assert_eq!(container.image, "myregistry/myapp:v1.0");
    }

    #[test]
    fn translate_restart_always() {
        let process = ProcessResourceConfig::new("api", "node server.js").with_restart(
            RestartPolicy::Always {
                max_retries: None,
                backoff: Default::default(),
            },
        );

        let translator = ProcessToContainerTranslator;
        let container = translator
            .translate(&process, &TranslationContext::new())
            .unwrap();

        assert_eq!(container.restart, ContainerRestartPolicy::Always);
    }

    #[test]
    fn translate_restart_on_failure() {
        let process = ProcessResourceConfig::new("api", "node server.js").with_restart(
            RestartPolicy::OnFailure {
                max_retries: Some(5),
                backoff: Default::default(),
            },
        );

        let translator = ProcessToContainerTranslator;
        let container = translator
            .translate(&process, &TranslationContext::new())
            .unwrap();

        assert_eq!(container.restart, ContainerRestartPolicy::OnFailure);
    }

    #[test]
    fn roundtrip_basic() {
        let process = ProcessResourceConfig::new("api", "node server.js")
            .with_port(3000)
            .with_env("NODE_ENV", "production");

        let translator = ProcessToContainerTranslator;
        let ctx = TranslationContext::new();

        let container = translator.translate(&process, &ctx).unwrap();
        let recovered = translator.reverse(&container, &ctx).unwrap();

        assert_eq!(process.id, recovered.id);
        assert_eq!(process.command, recovered.command);
        assert_eq!(process.port, recovered.port);
        assert_eq!(process.environment, recovered.environment);
    }

    #[test]
    fn reverse_preserves_original_command() {
        let process = ProcessResourceConfig::new("api", "node server.js --port 3000");

        let translator = ProcessToContainerTranslator;
        let ctx = TranslationContext::new();

        let container = translator.translate(&process, &ctx).unwrap();
        let recovered = translator.reverse(&container, &ctx).unwrap();

        // Original command is preserved in labels
        assert_eq!(recovered.command, "node server.js --port 3000");
    }

    #[test]
    fn translator_metadata() {
        let translator = ProcessToContainerTranslator;
        let meta = translator.metadata();

        assert_eq!(meta.name, Some("ProcessToContainer".to_string()));
        assert!(meta.lossy);
    }

    #[test]
    fn translator_supports_reverse() {
        let translator = ProcessToContainerTranslator;
        assert!(translator.supports_reverse());
    }

    #[test]
    fn strict_mode_requires_image() {
        let process = ProcessResourceConfig::new("api", "./my-custom-binary");
        let ctx = TranslationContext::strict(); // Strict mode

        let translator = ProcessToContainerTranslator;
        let result = translator.translate(&process, &ctx);

        // Should fail because image can't be confidently inferred
        assert!(result.is_err());
    }
}
