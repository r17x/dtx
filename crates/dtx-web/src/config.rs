use std::time::Duration;

/// Centralized configuration for dtx-web tunables.
///
/// Replaces hardcoded `Duration` and limit values scattered across handlers.
/// All fields have sensible defaults matching the original hardcoded values.
/// Each field can be overridden via `DTX_WEB_*` environment variables.
#[derive(Debug, Clone)]
pub struct WebConfig {
    /// Interval between status polling ticks in the SSE status stream.
    pub status_poll_interval: Duration,

    /// Interval between orchestrator state polling ticks.
    pub orchestrator_poll_interval: Duration,

    /// Keepalive interval for SSE connections (comment-only pings).
    pub sse_keepalive_interval: Duration,

    /// Idle timeout for SSE event/log streams before yielding a keepalive.
    pub sse_stream_timeout: Duration,

    /// Default result limit for Nix package search endpoints.
    pub default_search_limit: usize,

    /// Delay after stopping orchestrator before restarting services.
    pub restart_drain_delay: Duration,

    /// Maximum number of replay events sent to late SSE subscribers.
    pub max_log_replay: usize,

    /// Grace period after cancellation before stopping the orchestrator.
    pub shutdown_grace_period: Duration,

    /// Timeout for `orchestrator.stop_all()` during shutdown.
    pub orchestrator_stop_timeout: Duration,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            status_poll_interval: Duration::from_secs(2),
            orchestrator_poll_interval: Duration::from_millis(100),
            sse_keepalive_interval: Duration::from_secs(15),
            sse_stream_timeout: Duration::from_secs(30),
            default_search_limit: 20,
            restart_drain_delay: Duration::from_millis(500),
            max_log_replay: 1000,
            shutdown_grace_period: Duration::from_millis(200),
            orchestrator_stop_timeout: Duration::from_secs(3),
        }
    }
}

impl WebConfig {
    /// Default search limit for serde `#[serde(default)]` on query params.
    pub fn default_search_limit() -> usize {
        Self::default().default_search_limit
    }

    /// Default import format for serde `#[serde(default)]`.
    pub fn default_import_format() -> String {
        "auto".to_string()
    }

    /// Build a `WebConfig` from `DTX_WEB_*` environment variables, falling back
    /// to [`Default`] for any variable that is absent or unparseable.
    pub fn from_env() -> Self {
        let defaults = Self::default();

        Self {
            status_poll_interval: parse_millis_env(
                "DTX_WEB_STATUS_POLL_MS",
                defaults.status_poll_interval,
            ),
            orchestrator_poll_interval: parse_millis_env(
                "DTX_WEB_ORCHESTRATOR_POLL_MS",
                defaults.orchestrator_poll_interval,
            ),
            sse_keepalive_interval: parse_secs_env(
                "DTX_WEB_SSE_KEEPALIVE_SECS",
                defaults.sse_keepalive_interval,
            ),
            sse_stream_timeout: parse_secs_env(
                "DTX_WEB_SSE_STREAM_TIMEOUT_SECS",
                defaults.sse_stream_timeout,
            ),
            default_search_limit: parse_usize_env(
                "DTX_WEB_DEFAULT_SEARCH_LIMIT",
                defaults.default_search_limit,
            ),
            restart_drain_delay: parse_millis_env(
                "DTX_WEB_RESTART_DRAIN_MS",
                defaults.restart_drain_delay,
            ),
            max_log_replay: parse_usize_env("DTX_WEB_MAX_LOG_REPLAY", defaults.max_log_replay),
            shutdown_grace_period: parse_millis_env(
                "DTX_WEB_SHUTDOWN_GRACE_MS",
                defaults.shutdown_grace_period,
            ),
            orchestrator_stop_timeout: parse_secs_env(
                "DTX_WEB_ORCHESTRATOR_STOP_TIMEOUT_SECS",
                defaults.orchestrator_stop_timeout,
            ),
        }
    }
}

fn parse_millis_env(key: &str, fallback: Duration) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(fallback)
}

fn parse_secs_env(key: &str, fallback: Duration) -> Duration {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(fallback)
}

fn parse_usize_env(key: &str, fallback: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_original_hardcoded_values() {
        let cfg = WebConfig::default();
        assert_eq!(cfg.status_poll_interval, Duration::from_secs(2));
        assert_eq!(cfg.orchestrator_poll_interval, Duration::from_millis(100));
        assert_eq!(cfg.sse_keepalive_interval, Duration::from_secs(15));
        assert_eq!(cfg.sse_stream_timeout, Duration::from_secs(30));
        assert_eq!(cfg.default_search_limit, 20);
        assert_eq!(cfg.restart_drain_delay, Duration::from_millis(500));
        assert_eq!(cfg.max_log_replay, 1000);
        assert_eq!(cfg.shutdown_grace_period, Duration::from_millis(200));
        assert_eq!(cfg.orchestrator_stop_timeout, Duration::from_secs(3));
    }

    #[test]
    fn from_env_reads_overrides() {
        unsafe {
            std::env::set_var("DTX_WEB_STATUS_POLL_MS", "5000");
            std::env::set_var("DTX_WEB_DEFAULT_SEARCH_LIMIT", "50");
        }

        let cfg = WebConfig::from_env();
        assert_eq!(cfg.status_poll_interval, Duration::from_millis(5000));
        assert_eq!(cfg.default_search_limit, 50);

        unsafe {
            std::env::remove_var("DTX_WEB_STATUS_POLL_MS");
            std::env::remove_var("DTX_WEB_DEFAULT_SEARCH_LIMIT");
        }
    }

    #[test]
    fn from_env_ignores_invalid_values() {
        unsafe {
            std::env::set_var("DTX_WEB_SSE_KEEPALIVE_SECS", "not_a_number");
        }

        let cfg = WebConfig::from_env();
        assert_eq!(cfg.sse_keepalive_interval, Duration::from_secs(15));

        unsafe {
            std::env::remove_var("DTX_WEB_SSE_KEEPALIVE_SECS");
        }
    }
}
