//! Lifecycle events for the Resource abstraction.
//!
//! These events represent state transitions and observability data
//! for any resource managed by dtx.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::resource::{LogStreamKind, ResourceId, ResourceKind};

/// Events emitted during resource lifecycle.
///
/// All events carry a timestamp and most carry a resource ID.
/// Events are designed for pub-sub distribution via EventBus.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum LifecycleEvent {
    // === State Transitions ===
    /// Resource is starting up.
    Starting {
        id: ResourceId,
        kind: ResourceKind,
        timestamp: DateTime<Utc>,
    },

    /// Resource is now running.
    Running {
        id: ResourceId,
        kind: ResourceKind,
        /// Process ID (for process resources).
        pid: Option<u32>,
        timestamp: DateTime<Utc>,
    },

    /// Resource is stopping.
    Stopping {
        id: ResourceId,
        kind: ResourceKind,
        timestamp: DateTime<Utc>,
    },

    /// Resource has stopped.
    Stopped {
        id: ResourceId,
        kind: ResourceKind,
        exit_code: Option<i32>,
        timestamp: DateTime<Utc>,
    },

    /// Resource has failed.
    Failed {
        id: ResourceId,
        kind: ResourceKind,
        error: String,
        exit_code: Option<i32>,
        timestamp: DateTime<Utc>,
    },

    /// Resource is restarting.
    Restarting {
        id: ResourceId,
        kind: ResourceKind,
        /// Current attempt number (1-indexed).
        attempt: u32,
        /// Maximum attempts allowed (None = unlimited).
        max_attempts: Option<u32>,
        timestamp: DateTime<Utc>,
    },

    // === Health ===
    /// Health check passed.
    HealthCheckPassed {
        id: ResourceId,
        timestamp: DateTime<Utc>,
    },

    /// Health check failed.
    HealthCheckFailed {
        id: ResourceId,
        reason: String,
        timestamp: DateTime<Utc>,
    },

    // === Logs ===
    /// Log output from a resource.
    Log {
        id: ResourceId,
        stream: LogStreamKind,
        line: String,
        timestamp: DateTime<Utc>,
    },

    // === Dependencies ===
    /// Waiting for a dependency to be ready.
    DependencyWaiting {
        id: ResourceId,
        dependency: ResourceId,
        condition: DependencyCondition,
        timestamp: DateTime<Utc>,
    },

    /// Dependency is now ready.
    DependencyResolved {
        id: ResourceId,
        dependency: ResourceId,
        timestamp: DateTime<Utc>,
    },

    // === Configuration ===
    /// Configuration has changed.
    ConfigChanged {
        project_id: String,
        timestamp: DateTime<Utc>,
    },

    // === Memory ===
    /// A code memory has changed.
    MemoryChanged {
        project_id: String,
        memory_name: String,
        timestamp: DateTime<Utc>,
    },
}

/// Condition for dependency readiness.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DependencyCondition {
    /// Dependency has started.
    Started,
    /// Dependency is healthy.
    Healthy,
    /// Dependency has completed successfully.
    Completed,
}

impl LifecycleEvent {
    /// Get the resource ID associated with this event.
    ///
    /// Returns `None` for project-level events like `ConfigChanged`.
    pub fn resource_id(&self) -> Option<&ResourceId> {
        match self {
            Self::Starting { id, .. }
            | Self::Running { id, .. }
            | Self::Stopping { id, .. }
            | Self::Stopped { id, .. }
            | Self::Failed { id, .. }
            | Self::Restarting { id, .. }
            | Self::HealthCheckPassed { id, .. }
            | Self::HealthCheckFailed { id, .. }
            | Self::Log { id, .. }
            | Self::DependencyWaiting { id, .. }
            | Self::DependencyResolved { id, .. } => Some(id),
            Self::ConfigChanged { .. } | Self::MemoryChanged { .. } => None,
        }
    }

    /// Get the timestamp of this event.
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::Starting { timestamp, .. }
            | Self::Running { timestamp, .. }
            | Self::Stopping { timestamp, .. }
            | Self::Stopped { timestamp, .. }
            | Self::Failed { timestamp, .. }
            | Self::Restarting { timestamp, .. }
            | Self::HealthCheckPassed { timestamp, .. }
            | Self::HealthCheckFailed { timestamp, .. }
            | Self::Log { timestamp, .. }
            | Self::DependencyWaiting { timestamp, .. }
            | Self::DependencyResolved { timestamp, .. }
            | Self::ConfigChanged { timestamp, .. }
            | Self::MemoryChanged { timestamp, .. } => *timestamp,
        }
    }

    /// Get the event type as a string.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Starting { .. } => "starting",
            Self::Running { .. } => "running",
            Self::Stopping { .. } => "stopping",
            Self::Stopped { .. } => "stopped",
            Self::Failed { .. } => "failed",
            Self::Restarting { .. } => "restarting",
            Self::HealthCheckPassed { .. } => "health_check_passed",
            Self::HealthCheckFailed { .. } => "health_check_failed",
            Self::Log { .. } => "log",
            Self::DependencyWaiting { .. } => "dependency_waiting",
            Self::DependencyResolved { .. } => "dependency_resolved",
            Self::ConfigChanged { .. } => "config_changed",
            Self::MemoryChanged { .. } => "memory_changed",
        }
    }

    /// Get the resource kind if this is a state transition event.
    pub fn kind(&self) -> Option<ResourceKind> {
        match self {
            Self::Starting { kind, .. }
            | Self::Running { kind, .. }
            | Self::Stopping { kind, .. }
            | Self::Stopped { kind, .. }
            | Self::Failed { kind, .. }
            | Self::Restarting { kind, .. } => Some(*kind),
            _ => None,
        }
    }

    /// Check if this is a state transition event.
    pub fn is_state_transition(&self) -> bool {
        matches!(
            self,
            Self::Starting { .. }
                | Self::Running { .. }
                | Self::Stopping { .. }
                | Self::Stopped { .. }
                | Self::Failed { .. }
                | Self::Restarting { .. }
        )
    }

    /// Check if this is a log event.
    pub fn is_log(&self) -> bool {
        matches!(self, Self::Log { .. })
    }

    /// Check if this is a health event.
    pub fn is_health(&self) -> bool {
        matches!(
            self,
            Self::HealthCheckPassed { .. } | Self::HealthCheckFailed { .. }
        )
    }
}

impl std::fmt::Display for DependencyCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Started => write!(f, "started"),
            Self::Healthy => write!(f, "healthy"),
            Self::Completed => write!(f, "completed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_event_resource_id() {
        let event = LifecycleEvent::Running {
            id: ResourceId::new("api"),
            kind: ResourceKind::Process,
            pid: Some(1234),
            timestamp: Utc::now(),
        };
        assert_eq!(event.resource_id(), Some(&ResourceId::new("api")));

        let config_event = LifecycleEvent::ConfigChanged {
            project_id: "proj".to_string(),
            timestamp: Utc::now(),
        };
        assert_eq!(config_event.resource_id(), None);
    }

    #[test]
    fn lifecycle_event_type() {
        let event = LifecycleEvent::Starting {
            id: ResourceId::new("api"),
            kind: ResourceKind::Process,
            timestamp: Utc::now(),
        };
        assert_eq!(event.event_type(), "starting");
    }

    #[test]
    fn lifecycle_event_kind() {
        let event = LifecycleEvent::Running {
            id: ResourceId::new("api"),
            kind: ResourceKind::Container,
            pid: None,
            timestamp: Utc::now(),
        };
        assert_eq!(event.kind(), Some(ResourceKind::Container));

        let log_event = LifecycleEvent::Log {
            id: ResourceId::new("api"),
            stream: LogStreamKind::Stdout,
            line: "test".to_string(),
            timestamp: Utc::now(),
        };
        assert_eq!(log_event.kind(), None);
    }

    #[test]
    fn lifecycle_event_is_state_transition() {
        let starting = LifecycleEvent::Starting {
            id: ResourceId::new("api"),
            kind: ResourceKind::Process,
            timestamp: Utc::now(),
        };
        assert!(starting.is_state_transition());

        let log = LifecycleEvent::Log {
            id: ResourceId::new("api"),
            stream: LogStreamKind::Stdout,
            line: "test".to_string(),
            timestamp: Utc::now(),
        };
        assert!(!log.is_state_transition());
    }

    #[test]
    fn lifecycle_event_serde() {
        let event = LifecycleEvent::Running {
            id: ResourceId::new("api"),
            kind: ResourceKind::Process,
            pid: Some(1234),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "running");
        assert_eq!(json["id"], "api");
        assert_eq!(json["pid"], 1234);

        let parsed: LifecycleEvent = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.resource_id(), Some(&ResourceId::new("api")));
    }

    #[test]
    fn lifecycle_event_failed_serde() {
        let event = LifecycleEvent::Failed {
            id: ResourceId::new("db"),
            kind: ResourceKind::Process,
            error: "connection refused".to_string(),
            exit_code: Some(1),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "failed");
        assert_eq!(json["error"], "connection refused");
    }

    #[test]
    fn lifecycle_event_memory_changed_serde() {
        let event = LifecycleEvent::MemoryChanged {
            project_id: "proj".to_string(),
            memory_name: "test-mem".to_string(),
            timestamp: Utc::now(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "memory_changed");
        assert_eq!(json["memory_name"], "test-mem");
        assert_eq!(event.resource_id(), None);
        assert_eq!(event.event_type(), "memory_changed");

        let parsed: LifecycleEvent = serde_json::from_value(json).unwrap();
        assert_eq!(parsed.event_type(), "memory_changed");
    }

    #[test]
    fn dependency_condition_display() {
        assert_eq!(DependencyCondition::Started.to_string(), "started");
        assert_eq!(DependencyCondition::Healthy.to_string(), "healthy");
        assert_eq!(DependencyCondition::Completed.to_string(), "completed");
    }

    #[test]
    fn dependency_condition_serde() {
        let condition = DependencyCondition::Healthy;
        let json = serde_json::to_string(&condition).unwrap();
        assert_eq!(json, "\"healthy\"");

        let parsed: DependencyCondition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, DependencyCondition::Healthy);
    }
}
