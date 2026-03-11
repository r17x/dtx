//! Shared CLI argument parsers for service configuration.

use anyhow::{bail, Result};
use dtx_core::config::schema::RestartPolicy;
use dtx_core::model::HealthCheck;

/// Parse health check string.
/// Format: "exec:command" or "http:host:port/path"
pub fn parse_health_check(s: &str) -> Result<HealthCheck> {
    if let Some(cmd) = s.strip_prefix("exec:") {
        if cmd.is_empty() {
            bail!("Health check exec command cannot be empty");
        }
        Ok(HealthCheck::exec(cmd.to_string()))
    } else if let Some(http_spec) = s.strip_prefix("http:") {
        let parts: Vec<&str> = http_spec.splitn(2, '/').collect();
        let host_port = parts[0];
        let path = if parts.len() > 1 {
            format!("/{}", parts[1])
        } else {
            "/".to_string()
        };

        let hp_parts: Vec<&str> = host_port.rsplitn(2, ':').collect();
        if hp_parts.len() != 2 {
            bail!("Invalid HTTP health check format. Expected http:host:port/path");
        }

        let port: u16 = hp_parts[0]
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid port in health check: {}", hp_parts[0]))?;
        let host = hp_parts[1].to_string();

        Ok(HealthCheck::http(host, port, path))
    } else {
        bail!(
            "Invalid health check format: '{}'. Use 'exec:command' or 'http:host:port/path'",
            s
        )
    }
}

/// Parse restart policy string to RestartPolicy enum.
pub fn parse_restart_policy(s: &str) -> Result<RestartPolicy> {
    match s.to_lowercase().as_str() {
        "always" => Ok(RestartPolicy::Always),
        "on-failure" => Ok(RestartPolicy::OnFailure),
        "no" => Ok(RestartPolicy::No),
        _ => bail!(
            "Invalid restart policy '{}'. Use: always, on-failure, or no",
            s
        ),
    }
}
