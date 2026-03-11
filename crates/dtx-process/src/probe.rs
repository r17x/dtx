//! Health check probes for process resources.
//!
//! This module provides implementations for:
//! - Exec probes (run a command)
//! - HTTP probes (HTTP GET request)
//! - TCP probes (TCP connection check)

use chrono::Utc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::process::Command;
use tracing::{debug, warn};

use dtx_core::resource::HealthStatus;

use crate::config::ProbeConfig;
#[cfg(test)]
use crate::config::ProbeSettings;

/// Runs health check probes.
///
/// ProbeRunner manages readiness and liveness probes, tracking
/// consecutive successes/failures against thresholds.
pub struct ProbeRunner {
    /// Readiness probe configuration.
    readiness_probe: Option<ProbeConfig>,
    /// Liveness probe configuration.
    liveness_probe: Option<ProbeConfig>,
    /// Consecutive readiness successes.
    readiness_successes: u32,
    /// Consecutive readiness failures.
    readiness_failures: u32,
    /// Consecutive liveness successes.
    liveness_successes: u32,
    /// Consecutive liveness failures.
    liveness_failures: u32,
    /// Whether readiness has been achieved.
    ready: bool,
    /// Last check time.
    last_check: Option<chrono::DateTime<Utc>>,
}

impl ProbeRunner {
    /// Create a new probe runner.
    pub fn new(readiness_probe: Option<ProbeConfig>, liveness_probe: Option<ProbeConfig>) -> Self {
        Self {
            readiness_probe,
            liveness_probe,
            readiness_successes: 0,
            readiness_failures: 0,
            liveness_successes: 0,
            liveness_failures: 0,
            ready: false,
            last_check: None,
        }
    }

    /// Check health status.
    ///
    /// Returns Healthy if both probes pass (or are not configured).
    pub async fn check_health(&self) -> HealthStatus {
        // Check readiness first
        if let Some(ref probe) = self.readiness_probe {
            if !self.ready {
                let result = Self::run_probe(probe).await;
                if !result {
                    return HealthStatus::Unhealthy {
                        reason: "Readiness probe failed".to_string(),
                    };
                }
            }
        }

        // Check liveness
        if let Some(ref probe) = self.liveness_probe {
            let result = Self::run_probe(probe).await;
            if !result {
                return HealthStatus::Unhealthy {
                    reason: "Liveness probe failed".to_string(),
                };
            }
        }

        HealthStatus::Healthy
    }

    /// Run readiness probe and update counters.
    pub async fn check_readiness(&mut self) -> bool {
        let Some(ref probe) = self.readiness_probe else {
            self.ready = true;
            return true;
        };

        let settings = probe.settings();

        // Check if initial delay has passed
        if let Some(last) = self.last_check {
            let since_last = Utc::now().signed_duration_since(last);
            let period = chrono::Duration::from_std(settings.period).unwrap_or_default();
            if since_last < period {
                return self.ready;
            }
        }

        self.last_check = Some(Utc::now());

        if Self::run_probe(probe).await {
            self.readiness_successes += 1;
            self.readiness_failures = 0;

            if self.readiness_successes >= settings.success_threshold {
                self.ready = true;
            }
        } else {
            self.readiness_failures += 1;
            self.readiness_successes = 0;

            if self.readiness_failures >= settings.failure_threshold {
                self.ready = false;
            }
        }

        self.ready
    }

    /// Run liveness probe and update counters.
    pub async fn check_liveness(&mut self) -> bool {
        let Some(ref probe) = self.liveness_probe else {
            return true;
        };

        let settings = probe.settings();

        if Self::run_probe(probe).await {
            self.liveness_successes += 1;
            self.liveness_failures = 0;
            true
        } else {
            self.liveness_failures += 1;
            self.liveness_successes = 0;

            // Return false only if we've exceeded failure threshold
            self.liveness_failures < settings.failure_threshold
        }
    }

    /// Run a single probe.
    async fn run_probe(probe: &ProbeConfig) -> bool {
        let settings = probe.settings();
        let timeout = settings.timeout;

        match probe {
            ProbeConfig::Exec { command, .. } => Self::run_exec_probe(command, timeout).await,
            ProbeConfig::HttpGet {
                host, port, path, ..
            } => Self::run_http_probe(host, *port, path, timeout).await,
            ProbeConfig::TcpSocket { host, port, .. } => {
                Self::run_tcp_probe(host, *port, timeout).await
            }
        }
    }

    /// Run an exec probe.
    async fn run_exec_probe(command: &str, timeout: Duration) -> bool {
        debug!(command = %command, "Running exec probe");

        let result = tokio::time::timeout(timeout, async {
            Command::new("sh").arg("-c").arg(command).output().await
        })
        .await;

        match result {
            Ok(Ok(output)) => {
                let success = output.status.success();
                if !success {
                    debug!(
                        command = %command,
                        exit_code = ?output.status.code(),
                        "Exec probe failed"
                    );
                }
                success
            }
            Ok(Err(e)) => {
                warn!(command = %command, error = %e, "Exec probe error");
                false
            }
            Err(_) => {
                warn!(command = %command, "Exec probe timed out");
                false
            }
        }
    }

    /// Run an HTTP probe.
    #[cfg(feature = "http-probe")]
    async fn run_http_probe(host: &str, port: u16, path: &str, timeout: Duration) -> bool {
        let url = format!("http://{}:{}{}", host, port, path);
        debug!(url = %url, "Running HTTP probe");

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        match client.get(&url).send().await {
            Ok(response) => {
                let success = response.status().is_success();
                if !success {
                    debug!(url = %url, status = %response.status(), "HTTP probe failed");
                }
                success
            }
            Err(e) => {
                debug!(url = %url, error = %e, "HTTP probe error");
                false
            }
        }
    }

    /// Run an HTTP probe (fallback when http-probe feature is disabled).
    #[cfg(not(feature = "http-probe"))]
    async fn run_http_probe(host: &str, port: u16, path: &str, timeout: Duration) -> bool {
        // Fall back to TCP check when HTTP probe is not available
        warn!(
            host = %host,
            port = port,
            "HTTP probe not available, falling back to TCP check"
        );
        Self::run_tcp_probe(host, port, timeout).await
    }

    /// Run a TCP probe.
    async fn run_tcp_probe(host: &str, port: u16, timeout: Duration) -> bool {
        let addr = format!("{}:{}", host, port);
        debug!(addr = %addr, "Running TCP probe");

        let result = tokio::time::timeout(timeout, TcpStream::connect(&addr)).await;

        match result {
            Ok(Ok(_)) => true,
            Ok(Err(e)) => {
                debug!(addr = %addr, error = %e, "TCP probe failed");
                false
            }
            Err(_) => {
                debug!(addr = %addr, "TCP probe timed out");
                false
            }
        }
    }

    /// Check if the resource is ready.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    /// Reset the probe state.
    pub fn reset(&mut self) {
        self.readiness_successes = 0;
        self.readiness_failures = 0;
        self.liveness_successes = 0;
        self.liveness_failures = 0;
        self.ready = false;
        self.last_check = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_exec_probe(cmd: &str) -> ProbeConfig {
        ProbeConfig::Exec {
            command: cmd.to_string(),
            settings: ProbeSettings::default(),
        }
    }

    #[allow(dead_code)]
    fn make_tcp_probe(port: u16) -> ProbeConfig {
        ProbeConfig::TcpSocket {
            host: "127.0.0.1".to_string(),
            port,
            settings: ProbeSettings::default(),
        }
    }

    #[test]
    fn probe_runner_new() {
        let runner = ProbeRunner::new(None, None);
        assert!(!runner.is_ready());
    }

    #[tokio::test]
    async fn probe_runner_no_probes() {
        let runner = ProbeRunner::new(None, None);
        let health = runner.check_health().await;
        assert!(health.is_healthy());
    }

    #[tokio::test]
    async fn exec_probe_success() {
        let result = ProbeRunner::run_exec_probe("true", Duration::from_secs(1)).await;
        assert!(result);
    }

    #[tokio::test]
    async fn exec_probe_failure() {
        let result = ProbeRunner::run_exec_probe("false", Duration::from_secs(1)).await;
        assert!(!result);
    }

    #[tokio::test]
    async fn exec_probe_timeout() {
        let result = ProbeRunner::run_exec_probe("sleep 10", Duration::from_millis(100)).await;
        assert!(!result);
    }

    #[tokio::test]
    async fn tcp_probe_failure() {
        // Use a port that is unlikely to be in use
        let result =
            ProbeRunner::run_tcp_probe("127.0.0.1", 59999, Duration::from_millis(100)).await;
        assert!(!result);
    }

    #[tokio::test]
    async fn probe_runner_readiness() {
        let probe = make_exec_probe("true");
        let mut runner = ProbeRunner::new(Some(probe), None);

        // First check should succeed and set ready
        let ready = runner.check_readiness().await;
        assert!(ready);
        assert!(runner.is_ready());
    }

    #[tokio::test]
    async fn probe_runner_liveness() {
        let probe = make_exec_probe("true");
        let mut runner = ProbeRunner::new(None, Some(probe));

        let live = runner.check_liveness().await;
        assert!(live);
    }

    #[tokio::test]
    async fn probe_runner_readiness_failure() {
        let probe = make_exec_probe("false");
        let mut runner = ProbeRunner::new(Some(probe), None);

        let ready = runner.check_readiness().await;
        assert!(!ready);
        assert!(!runner.is_ready());
    }

    #[tokio::test]
    async fn probe_runner_health_with_readiness() {
        let probe = make_exec_probe("true");
        let runner = ProbeRunner::new(Some(probe), None);

        // Without being ready, health check should fail
        let health = runner.check_health().await;
        // Actually it runs the probe, so it should pass
        assert!(health.is_healthy());
    }

    #[test]
    fn probe_runner_reset() {
        let mut runner = ProbeRunner::new(None, None);
        runner.ready = true;
        runner.readiness_successes = 5;

        runner.reset();
        assert!(!runner.is_ready());
        assert_eq!(runner.readiness_successes, 0);
    }
}
