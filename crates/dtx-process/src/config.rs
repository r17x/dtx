//! Configuration types for process resources.

use dtx_core::resource::ResourceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Configuration for a process resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessResourceConfig {
    /// Resource identifier.
    pub id: ResourceId,

    /// Command to execute.
    pub command: String,

    /// Working directory.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,

    /// Environment variables.
    #[serde(default)]
    pub environment: HashMap<String, String>,

    /// Port the process listens on.
    #[serde(default)]
    pub port: Option<u16>,

    /// Shutdown configuration.
    #[serde(default)]
    pub shutdown: ShutdownConfig,

    /// Restart policy.
    #[serde(default)]
    pub restart: RestartPolicy,

    /// Readiness probe.
    #[serde(default)]
    pub readiness_probe: Option<ProbeConfig>,

    /// Liveness probe.
    #[serde(default)]
    pub liveness_probe: Option<ProbeConfig>,

    /// Dependencies.
    #[serde(default)]
    pub depends_on: Vec<ResourceId>,
}

impl ProcessResourceConfig {
    /// Create a new process config.
    pub fn new(id: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: ResourceId::new(id),
            command: command.into(),
            working_dir: None,
            environment: HashMap::new(),
            port: None,
            shutdown: ShutdownConfig::default(),
            restart: RestartPolicy::default(),
            readiness_probe: None,
            liveness_probe: None,
            depends_on: Vec::new(),
        }
    }

    /// Set working directory.
    pub fn with_working_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    /// Set environment variables.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.environment.insert(key.into(), value.into());
        self
    }

    /// Set environment from map.
    pub fn with_environment(mut self, env: HashMap<String, String>) -> Self {
        self.environment = env;
        self
    }

    /// Set port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// Set shutdown config.
    pub fn with_shutdown(mut self, config: ShutdownConfig) -> Self {
        self.shutdown = config;
        self
    }

    /// Set restart policy.
    pub fn with_restart(mut self, policy: RestartPolicy) -> Self {
        self.restart = policy;
        self
    }

    /// Set readiness probe.
    pub fn with_readiness_probe(mut self, probe: ProbeConfig) -> Self {
        self.readiness_probe = Some(probe);
        self
    }

    /// Set liveness probe.
    pub fn with_liveness_probe(mut self, probe: ProbeConfig) -> Self {
        self.liveness_probe = Some(probe);
        self
    }

    /// Add dependency.
    pub fn depends_on(mut self, id: impl Into<ResourceId>) -> Self {
        self.depends_on.push(id.into());
        self
    }

    /// Create a ProcessResourceConfig from a ResourceConfig.
    ///
    /// This is the preferred conversion path from config.yaml because it preserves
    /// all fields (restart policy, liveness probes, shutdown signal/timeout) that
    /// would be lost going through the DB Service model.
    pub fn from_resource_config(
        name: &str,
        resource: &dtx_core::config::schema::ResourceConfig,
        project_root: &std::path::Path,
    ) -> Self {
        let mut config = Self::new(name, resource.command.as_deref().unwrap_or(""));

        config.working_dir = resource
            .working_dir
            .clone()
            .or_else(|| Some(project_root.to_path_buf()));

        config.port = resource.port;

        config.environment = resource
            .environment
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if let Some(ref health) = resource.health {
            config.readiness_probe = Some(health_config_to_probe(health, resource.port));
        }

        if let Some(ref liveness) = resource.liveness {
            config.liveness_probe = Some(health_config_to_probe(liveness, resource.port));
        }

        if let Some(ref restart) = resource.restart {
            config.restart = restart_config_to_policy(restart);
        }

        if let Some(ref shutdown) = resource.shutdown {
            config.shutdown = shutdown_schema_to_config(shutdown);
        }

        for dep in &resource.depends_on {
            config.depends_on.push(ResourceId::new(dep.name()));
        }

        config
    }

    /// Get the effective working directory.
    pub fn effective_working_dir(&self) -> PathBuf {
        self.working_dir
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }
}

/// Shutdown configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShutdownConfig {
    /// Custom shutdown command.
    #[serde(default)]
    pub command: Option<String>,

    /// Signal to send.
    #[serde(default)]
    pub signal: Signal,

    /// Timeout before SIGKILL.
    #[serde(default = "default_shutdown_timeout")]
    #[serde(with = "duration_secs")]
    pub timeout: Duration,
}

fn default_shutdown_timeout() -> Duration {
    Duration::from_secs(10)
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            command: None,
            signal: Signal::SIGTERM,
            timeout: default_shutdown_timeout(),
        }
    }
}

/// Unix signals for process control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum Signal {
    #[default]
    SIGTERM,
    SIGINT,
    SIGKILL,
    SIGHUP,
}

impl Signal {
    /// Get the libc signal number.
    #[cfg(unix)]
    pub fn as_libc(&self) -> i32 {
        match self {
            Signal::SIGTERM => libc::SIGTERM,
            Signal::SIGINT => libc::SIGINT,
            Signal::SIGKILL => libc::SIGKILL,
            Signal::SIGHUP => libc::SIGHUP,
        }
    }
}

/// Restart policy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum RestartPolicy {
    /// Always restart.
    Always {
        #[serde(default)]
        max_retries: Option<u32>,
        #[serde(default)]
        backoff: BackoffConfig,
    },
    /// Restart on non-zero exit.
    OnFailure {
        #[serde(default)]
        max_retries: Option<u32>,
        #[serde(default)]
        backoff: BackoffConfig,
    },
    /// Never restart.
    No,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self::No
    }
}

impl RestartPolicy {
    /// Get the backoff config if applicable.
    pub fn backoff(&self) -> Option<&BackoffConfig> {
        match self {
            Self::Always { backoff, .. } | Self::OnFailure { backoff, .. } => Some(backoff),
            Self::No => None,
        }
    }

    /// Get max retries if applicable.
    pub fn max_retries(&self) -> Option<u32> {
        match self {
            Self::Always { max_retries, .. } | Self::OnFailure { max_retries, .. } => *max_retries,
            Self::No => None,
        }
    }

    /// Check if restart is disabled.
    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::No)
    }
}

/// Backoff configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BackoffConfig {
    /// Initial delay.
    #[serde(default = "default_initial_delay")]
    #[serde(with = "duration_secs")]
    pub initial_delay: Duration,

    /// Maximum delay.
    #[serde(default = "default_max_delay")]
    #[serde(with = "duration_secs")]
    pub max_delay: Duration,

    /// Multiplier for exponential backoff.
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,
}

fn default_initial_delay() -> Duration {
    Duration::from_secs(1)
}

fn default_max_delay() -> Duration {
    Duration::from_secs(60)
}

fn default_multiplier() -> f64 {
    2.0
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_delay: default_initial_delay(),
            max_delay: default_max_delay(),
            multiplier: default_multiplier(),
        }
    }
}

impl BackoffConfig {
    /// Calculate delay for the given attempt (0-indexed).
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return self.initial_delay;
        }
        let delay_secs = self.initial_delay.as_secs_f64() * self.multiplier.powi(attempt as i32);
        Duration::from_secs_f64(delay_secs.min(self.max_delay.as_secs_f64()))
    }
}

/// Health check probe configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProbeConfig {
    /// Execute command and check exit code.
    Exec {
        command: String,
        #[serde(default)]
        settings: ProbeSettings,
    },
    /// HTTP GET and check status code.
    HttpGet {
        #[serde(default = "default_host")]
        host: String,
        port: u16,
        #[serde(default = "default_path")]
        path: String,
        #[serde(default)]
        settings: ProbeSettings,
    },
    /// TCP connection check.
    TcpSocket {
        #[serde(default = "default_host")]
        host: String,
        port: u16,
        #[serde(default)]
        settings: ProbeSettings,
    },
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_path() -> String {
    "/".to_string()
}

impl ProbeConfig {
    /// Get probe settings.
    pub fn settings(&self) -> &ProbeSettings {
        match self {
            Self::Exec { settings, .. }
            | Self::HttpGet { settings, .. }
            | Self::TcpSocket { settings, .. } => settings,
        }
    }
}

/// Common probe settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeSettings {
    /// Initial delay before probing.
    #[serde(default)]
    #[serde(with = "duration_secs")]
    pub initial_delay: Duration,

    /// Period between probes.
    #[serde(default = "default_period")]
    #[serde(with = "duration_secs")]
    pub period: Duration,

    /// Timeout per probe.
    #[serde(default = "default_probe_timeout")]
    #[serde(with = "duration_secs")]
    pub timeout: Duration,

    /// Consecutive successes to be healthy.
    #[serde(default = "default_success_threshold")]
    pub success_threshold: u32,

    /// Consecutive failures to be unhealthy.
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
}

fn default_period() -> Duration {
    Duration::from_secs(10)
}

fn default_probe_timeout() -> Duration {
    Duration::from_secs(1)
}

fn default_success_threshold() -> u32 {
    1
}

fn default_failure_threshold() -> u32 {
    3
}

impl Default for ProbeSettings {
    fn default() -> Self {
        Self {
            initial_delay: Duration::ZERO,
            period: default_period(),
            timeout: default_probe_timeout(),
            success_threshold: default_success_threshold(),
            failure_threshold: default_failure_threshold(),
        }
    }
}

/// Duration serialization as seconds.
mod duration_secs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

/// Parse a human-readable duration string (e.g., "5s", "2m", "100ms") into a Duration.
fn parse_duration_string(s: &str) -> Duration {
    let s = s.trim();
    if let Some(ms) = s.strip_suffix("ms") {
        return Duration::from_millis(ms.parse().unwrap_or(0));
    }
    if let Some(m) = s.strip_suffix('m') {
        return Duration::from_secs(m.parse::<u64>().unwrap_or(0) * 60);
    }
    let secs: u64 = s.trim_end_matches('s').parse().unwrap_or(10);
    Duration::from_secs(secs)
}

/// Convert a schema HealthConfig to a process ProbeConfig.
fn health_config_to_probe(
    health: &dtx_core::config::schema::HealthConfig,
    resource_port: Option<u16>,
) -> ProbeConfig {
    let settings = ProbeSettings {
        initial_delay: health
            .initial_delay
            .as_deref()
            .map(parse_duration_string)
            .unwrap_or_default(),
        period: parse_duration_string(&health.interval),
        timeout: parse_duration_string(&health.timeout),
        success_threshold: 1,
        failure_threshold: health.retries,
    };

    if let Some(ref exec_cmd) = health.exec {
        return ProbeConfig::Exec {
            command: exec_cmd.clone(),
            settings,
        };
    }

    if let Some(ref http_path) = health.http {
        let port = resource_port.unwrap_or(80);
        return ProbeConfig::HttpGet {
            host: "127.0.0.1".to_string(),
            port,
            path: http_path.clone(),
            settings,
        };
    }

    if let Some(ref tcp_addr) = health.tcp {
        // tcp field may be "host:port" or just "port"
        let (host, port) = if let Some((h, p)) = tcp_addr.rsplit_once(':') {
            (
                h.to_string(),
                p.parse().unwrap_or(resource_port.unwrap_or(80)),
            )
        } else {
            (
                "127.0.0.1".to_string(),
                tcp_addr.parse().unwrap_or(resource_port.unwrap_or(80)),
            )
        };
        return ProbeConfig::TcpSocket {
            host,
            port,
            settings,
        };
    }

    // Fallback: if port is known, use TCP; otherwise exec with "true"
    if let Some(port) = resource_port {
        ProbeConfig::TcpSocket {
            host: "127.0.0.1".to_string(),
            port,
            settings,
        }
    } else {
        ProbeConfig::Exec {
            command: "true".to_string(),
            settings,
        }
    }
}

/// Convert a schema RestartConfig to a process RestartPolicy.
fn restart_config_to_policy(
    restart: &dtx_core::config::schema::RestartConfig,
) -> RestartPolicy {
    use dtx_core::config::schema::RestartPolicy as SchemaPolicy;

    let (policy, max_retries, backoff_str) = match restart {
        dtx_core::config::schema::RestartConfig::Simple(p) => (p, None, None),
        dtx_core::config::schema::RestartConfig::Extended {
            policy,
            max_attempts,
            backoff,
            ..
        } => (policy, *max_attempts, backoff.as_deref()),
    };

    let backoff = if let Some(b) = backoff_str {
        BackoffConfig {
            initial_delay: parse_duration_string(b),
            ..BackoffConfig::default()
        }
    } else {
        BackoffConfig::default()
    };

    match policy {
        SchemaPolicy::Always => RestartPolicy::Always {
            max_retries,
            backoff,
        },
        SchemaPolicy::OnFailure => RestartPolicy::OnFailure {
            max_retries,
            backoff,
        },
        SchemaPolicy::No => RestartPolicy::No,
    }
}

/// Convert a schema ShutdownConfigSchema to a process ShutdownConfig.
fn shutdown_schema_to_config(
    shutdown: &dtx_core::config::schema::ShutdownConfigSchema,
) -> ShutdownConfig {
    let signal = shutdown
        .signal
        .as_deref()
        .map(|s| match s.to_uppercase().as_str() {
            "SIGINT" => Signal::SIGINT,
            "SIGKILL" => Signal::SIGKILL,
            "SIGHUP" => Signal::SIGHUP,
            _ => Signal::SIGTERM,
        })
        .unwrap_or(Signal::SIGTERM);

    let timeout = shutdown
        .timeout
        .as_deref()
        .map(parse_duration_string)
        .unwrap_or_else(default_shutdown_timeout);

    ShutdownConfig {
        command: shutdown.command.clone(),
        signal,
        timeout,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtx_core::config::schema::{
        DependencyConfig, HealthConfig, ResourceConfig, ShutdownConfigSchema,
    };

    #[test]
    fn from_resource_config_full() {
        use dtx_core::config::schema::RestartConfig as SchemaRestart;
        use dtx_core::config::schema::RestartPolicy as SchemaRestartPolicy;

        let env: indexmap::IndexMap<String, String> =
            [("NODE_ENV".to_string(), "production".to_string())]
                .into_iter()
                .collect();

        let resource = ResourceConfig {
            command: Some("npm start".to_string()),
            port: Some(3000),
            working_dir: Some(PathBuf::from("./api")),
            environment: env,
            depends_on: vec![DependencyConfig::Simple("db".to_string())],
            health: Some(HealthConfig {
                http: Some("/health".to_string()),
                interval: "10s".to_string(),
                timeout: "5s".to_string(),
                retries: 5,
                initial_delay: Some("2s".to_string()),
                ..Default::default()
            }),
            liveness: Some(HealthConfig {
                exec: Some("curl localhost:3000".to_string()),
                ..Default::default()
            }),
            restart: Some(SchemaRestart::Extended {
                policy: SchemaRestartPolicy::OnFailure,
                max_attempts: Some(3),
                backoff: Some("2s".to_string()),
                grace_period: None,
            }),
            shutdown: Some(ShutdownConfigSchema {
                command: Some("npm stop".to_string()),
                signal: Some("SIGINT".to_string()),
                timeout: Some("15s".to_string()),
            }),
            ..Default::default()
        };

        let config =
            ProcessResourceConfig::from_resource_config("api", &resource, std::path::Path::new("/project"));

        assert_eq!(config.id.as_str(), "api");
        assert_eq!(config.command, "npm start");
        assert_eq!(config.port, Some(3000));
        assert_eq!(config.working_dir, Some(PathBuf::from("./api")));
        assert_eq!(
            config.environment.get("NODE_ENV"),
            Some(&"production".to_string())
        );
        assert_eq!(config.depends_on.len(), 1);
        assert_eq!(config.depends_on[0].as_str(), "db");

        // Readiness probe (HTTP)
        let probe = config.readiness_probe.unwrap();
        match probe {
            ProbeConfig::HttpGet { port, path, settings, .. } => {
                assert_eq!(port, 3000);
                assert_eq!(path, "/health");
                assert_eq!(settings.period, Duration::from_secs(10));
                assert_eq!(settings.timeout, Duration::from_secs(5));
                assert_eq!(settings.failure_threshold, 5);
                assert_eq!(settings.initial_delay, Duration::from_secs(2));
            }
            _ => panic!("expected HttpGet probe"),
        }

        // Liveness probe (Exec)
        let liveness = config.liveness_probe.unwrap();
        match liveness {
            ProbeConfig::Exec { command, .. } => {
                assert_eq!(command, "curl localhost:3000");
            }
            _ => panic!("expected Exec probe"),
        }

        // Restart policy
        match config.restart {
            RestartPolicy::OnFailure { max_retries, backoff } => {
                assert_eq!(max_retries, Some(3));
                assert_eq!(backoff.initial_delay, Duration::from_secs(2));
            }
            _ => panic!("expected OnFailure restart policy"),
        }

        // Shutdown
        assert_eq!(config.shutdown.command, Some("npm stop".to_string()));
        assert_eq!(config.shutdown.signal, Signal::SIGINT);
        assert_eq!(config.shutdown.timeout, Duration::from_secs(15));
    }

    #[test]
    fn from_resource_config_minimal() {
        let resource = ResourceConfig {
            command: Some("echo hello".to_string()),
            ..Default::default()
        };

        let config = ProcessResourceConfig::from_resource_config(
            "worker",
            &resource,
            std::path::Path::new("/root"),
        );

        assert_eq!(config.id.as_str(), "worker");
        assert_eq!(config.command, "echo hello");
        assert_eq!(config.working_dir, Some(PathBuf::from("/root")));
        assert!(config.readiness_probe.is_none());
        assert!(config.liveness_probe.is_none());
        assert!(config.restart.is_disabled());
    }

    #[test]
    fn parse_duration_string_variants() {
        assert_eq!(parse_duration_string("5s"), Duration::from_secs(5));
        assert_eq!(parse_duration_string("2m"), Duration::from_secs(120));
        assert_eq!(parse_duration_string("500ms"), Duration::from_millis(500));
        assert_eq!(parse_duration_string("30"), Duration::from_secs(30));
    }

    #[test]
    fn process_config_new() {
        let config = ProcessResourceConfig::new("api", "cargo run");
        assert_eq!(config.id.as_str(), "api");
        assert_eq!(config.command, "cargo run");
    }

    #[test]
    fn process_config_builder() {
        let config = ProcessResourceConfig::new("api", "cargo run")
            .with_port(3000)
            .with_env("PORT", "3000")
            .with_working_dir("/app")
            .depends_on("db");

        assert_eq!(config.port, Some(3000));
        assert_eq!(config.environment.get("PORT"), Some(&"3000".to_string()));
        assert_eq!(config.working_dir, Some(PathBuf::from("/app")));
        assert_eq!(config.depends_on.len(), 1);
    }

    #[test]
    fn shutdown_config_default() {
        let config = ShutdownConfig::default();
        assert!(config.command.is_none());
        assert_eq!(config.signal, Signal::SIGTERM);
        assert_eq!(config.timeout, Duration::from_secs(10));
    }

    #[test]
    fn restart_policy_default() {
        let policy = RestartPolicy::default();
        assert_eq!(policy.max_retries(), None);
        assert!(policy.is_disabled());
    }

    #[test]
    fn backoff_delay_calculation() {
        let config = BackoffConfig::default();
        assert_eq!(config.delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(config.delay_for_attempt(1), Duration::from_secs(2));
        assert_eq!(config.delay_for_attempt(2), Duration::from_secs(4));
    }

    #[test]
    fn backoff_respects_max() {
        let config = BackoffConfig {
            initial_delay: Duration::from_secs(10),
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
        };
        assert_eq!(config.delay_for_attempt(2), Duration::from_secs(30));
        assert_eq!(config.delay_for_attempt(3), Duration::from_secs(30));
    }

    #[test]
    fn probe_config_settings() {
        let probe = ProbeConfig::HttpGet {
            host: "localhost".to_string(),
            port: 8080,
            path: "/health".to_string(),
            settings: ProbeSettings::default(),
        };
        assert_eq!(probe.settings().success_threshold, 1);
    }

    #[test]
    fn config_serialization() {
        let config = ProcessResourceConfig::new("api", "cargo run")
            .with_port(3000)
            .with_restart(RestartPolicy::Always {
                max_retries: None,
                backoff: BackoffConfig::default(),
            });

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"id\":\"api\""));
        assert!(json.contains("\"port\":3000"));

        let parsed: ProcessResourceConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id.as_str(), "api");
    }
}
