//! Protocol method definitions and parameter types.
//!
//! This module defines all dtx protocol methods and their associated
//! request/response types. Methods follow a namespace/action pattern.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// Resource lifecycle
pub const RESOURCE_START: &str = "resource/start";
pub const RESOURCE_STOP: &str = "resource/stop";
pub const RESOURCE_RESTART: &str = "resource/restart";
pub const RESOURCE_KILL: &str = "resource/kill";

// Resource query
pub const RESOURCE_STATUS: &str = "resource/status";
pub const RESOURCE_HEALTH: &str = "resource/health";
pub const RESOURCE_LOGS: &str = "resource/logs";
pub const RESOURCE_LIST: &str = "resource/list";

// Batch operations
pub const START_ALL: &str = "resource/startAll";
pub const STOP_ALL: &str = "resource/stopAll";

// Configuration
pub const RESOURCE_CONFIGURE: &str = "resource/configure";
pub const CONFIG_GET: &str = "config/get";
pub const CONFIG_SET: &str = "config/set";

// Events
pub const EVENTS_SUBSCRIBE: &str = "events/subscribe";
pub const EVENTS_UNSUBSCRIBE: &str = "events/unsubscribe";

// AI (optional)
pub const AI_EXECUTE: &str = "ai/execute";
pub const AI_SUGGEST: &str = "ai/suggest";

// MCP standard methods
pub const INITIALIZE: &str = "initialize";
pub const INITIALIZED: &str = "notifications/initialized";
pub const RESOURCES_LIST: &str = "resources/list";
pub const RESOURCES_READ: &str = "resources/read";
pub const RESOURCES_SUBSCRIBE: &str = "resources/subscribe";
pub const RESOURCES_UNSUBSCRIBE: &str = "resources/unsubscribe";
pub const TOOLS_LIST: &str = "tools/list";
pub const TOOLS_CALL: &str = "tools/call";
pub const PROMPTS_LIST: &str = "prompts/list";
pub const PROMPTS_GET: &str = "prompts/get";

// Parameter types

/// Parameters for single resource operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceParams {
    /// Resource identifier.
    pub id: String,
}

impl ResourceParams {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

/// Parameters for log retrieval.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogsParams {
    /// Resource identifier.
    pub id: String,

    /// Whether to follow logs in real-time.
    #[serde(default)]
    pub follow: bool,

    /// Number of recent lines to retrieve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,

    /// Filter by log level.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

impl LogsParams {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            follow: false,
            lines: None,
            level: None,
        }
    }

    pub fn follow(mut self) -> Self {
        self.follow = true;
        self
    }

    pub fn lines(mut self, n: u32) -> Self {
        self.lines = Some(n);
        self
    }
}

/// Parameters for resource configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigureParams {
    /// Resource identifier.
    pub id: String,

    /// Configuration values to set.
    pub config: Value,
}

/// Parameters for event subscription.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SubscribeParams {
    /// Event filter.
    #[serde(default)]
    pub filter: EventFilter,
}

/// Filter for event subscriptions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventFilter {
    /// Filter to specific resource IDs.
    #[serde(default)]
    pub resource_ids: Vec<String>,

    /// Filter to specific event types.
    #[serde(default)]
    pub event_types: Vec<String>,

    /// Include log events (default true).
    #[serde(default = "default_true")]
    pub include_logs: bool,

    /// Include health events (default true).
    #[serde(default = "default_true")]
    pub include_health: bool,
}

fn default_true() -> bool {
    true
}

impl Default for EventFilter {
    fn default() -> Self {
        Self {
            resource_ids: Vec::new(),
            event_types: Vec::new(),
            include_logs: true,
            include_health: true,
        }
    }
}

impl EventFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resource_ids(mut self, ids: Vec<String>) -> Self {
        self.resource_ids = ids;
        self
    }

    pub fn event_types(mut self, types: Vec<String>) -> Self {
        self.event_types = types;
        self
    }

    pub fn exclude_logs(mut self) -> Self {
        self.include_logs = false;
        self
    }
}

/// Parameters for AI execution.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AIExecuteParams {
    /// Natural language prompt.
    pub prompt: String,

    /// Additional context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

// Response types

/// Result for resource status query.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceStatusResult {
    /// Resource identifier.
    pub id: String,

    /// Resource kind (process, container, etc.).
    pub kind: String,

    /// Current state.
    pub state: String,

    /// Process ID (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,

    /// Health status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub healthy: Option<bool>,

    /// Start timestamp (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,

    /// Stop timestamp (ISO 8601).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<String>,

    /// Exit code (if stopped).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Result for resource list query.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceListResult {
    /// List of resources.
    pub resources: Vec<ResourceStatusResult>,
}

/// Log entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    /// Timestamp (ISO 8601).
    pub timestamp: String,

    /// Stream (stdout/stderr).
    pub stream: String,

    /// Log line content.
    pub line: String,

    /// Log level (if detected).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
}

/// Result for log query.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogsResult {
    /// Resource identifier.
    pub id: String,

    /// Log entries.
    pub entries: Vec<LogEntry>,

    /// Whether there are more entries.
    #[serde(default)]
    pub has_more: bool,
}

/// Result for event subscription.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SubscribeResult {
    /// Subscription identifier.
    pub subscription_id: String,
}

/// Result for batch operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchResult {
    /// Number of resources affected.
    pub count: usize,

    /// Resources that succeeded.
    #[serde(default)]
    pub succeeded: Vec<String>,

    /// Resources that failed.
    #[serde(default)]
    pub failed: Vec<BatchError>,
}

/// Error for a single resource in a batch operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchError {
    /// Resource identifier.
    pub id: String,

    /// Error message.
    pub error: String,
}

/// Result for health check.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthResult {
    /// Resource identifier.
    pub id: String,

    /// Health status (healthy, unhealthy, unknown).
    pub status: String,

    /// Optional message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Last check timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_check: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_params_serialization() {
        let params = ResourceParams::new("postgres");
        let json = serde_json::to_string(&params).unwrap();
        assert!(json.contains("\"id\":\"postgres\""));

        let parsed: ResourceParams = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "postgres");
    }

    #[test]
    fn logs_params_builder() {
        let params = LogsParams::new("api").follow().lines(100);
        assert!(params.follow);
        assert_eq!(params.lines, Some(100));
    }

    #[test]
    fn event_filter_defaults() {
        let filter = EventFilter::default();
        assert!(filter.resource_ids.is_empty());
        assert!(filter.include_logs);
        assert!(filter.include_health);
    }

    #[test]
    fn resource_status_result() {
        let result = ResourceStatusResult {
            id: "postgres".to_string(),
            kind: "process".to_string(),
            state: "running".to_string(),
            pid: Some(1234),
            healthy: Some(true),
            started_at: Some("2024-01-01T00:00:00Z".to_string()),
            stopped_at: None,
            exit_code: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"state\":\"running\""));
        assert!(json.contains("\"pid\":1234"));
        assert!(!json.contains("stopped_at"));
    }
}
