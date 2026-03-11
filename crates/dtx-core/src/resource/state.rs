//! ResourceState - The current state of a resource.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The current state of a resource in its lifecycle.
///
/// Each state captures relevant timestamps and metadata for debugging
/// and observability.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum ResourceState {
    /// Resource created but not yet started.
    #[default]
    Pending,

    /// Resource is starting up.
    Starting {
        /// When the start was initiated.
        started_at: DateTime<Utc>,
    },

    /// Resource is running and ready.
    Running {
        /// Process ID (for process resources).
        pid: Option<u32>,
        /// When the resource started.
        started_at: DateTime<Utc>,
    },

    /// Resource is stopping.
    Stopping {
        /// When the resource originally started.
        started_at: DateTime<Utc>,
        /// When the stop was initiated.
        stopping_at: DateTime<Utc>,
    },

    /// Resource has stopped normally.
    Stopped {
        /// Exit code if applicable.
        exit_code: Option<i32>,
        /// When the resource started.
        started_at: DateTime<Utc>,
        /// When the resource stopped.
        stopped_at: DateTime<Utc>,
    },

    /// Resource has failed.
    Failed {
        /// Error message describing the failure.
        error: String,
        /// Exit code if applicable.
        exit_code: Option<i32>,
        /// When the resource started (if it got that far).
        started_at: Option<DateTime<Utc>>,
        /// When the failure occurred.
        failed_at: DateTime<Utc>,
    },
}

impl ResourceState {
    /// Check if the resource is currently running.
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    /// Check if the resource has stopped (successfully or with failure).
    pub fn is_stopped(&self) -> bool {
        matches!(self, Self::Stopped { .. } | Self::Failed { .. })
    }

    /// Check if the resource is in a transitioning state (starting or stopping).
    pub fn is_transitioning(&self) -> bool {
        matches!(self, Self::Starting { .. } | Self::Stopping { .. })
    }

    /// Check if the resource is pending (not yet started).
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// Check if the resource has failed.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// Get the exit code if the resource has stopped.
    pub fn exit_code(&self) -> Option<i32> {
        match self {
            Self::Stopped { exit_code, .. } => *exit_code,
            Self::Failed { exit_code, .. } => *exit_code,
            _ => None,
        }
    }

    /// Get the error message if the resource has failed.
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Failed { error, .. } => Some(error),
            _ => None,
        }
    }

    /// Get the process ID if running.
    pub fn pid(&self) -> Option<u32> {
        match self {
            Self::Running { pid, .. } => *pid,
            _ => None,
        }
    }

    /// Get the started_at timestamp if available.
    pub fn started_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Pending => None,
            Self::Starting { started_at } => Some(*started_at),
            Self::Running { started_at, .. } => Some(*started_at),
            Self::Stopping { started_at, .. } => Some(*started_at),
            Self::Stopped { started_at, .. } => Some(*started_at),
            Self::Failed { started_at, .. } => *started_at,
        }
    }

    /// Get a string representation of the state.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Starting { .. } => "starting",
            Self::Running { .. } => "running",
            Self::Stopping { .. } => "stopping",
            Self::Stopped { .. } => "stopped",
            Self::Failed { .. } => "failed",
        }
    }
}

impl std::fmt::Display for ResourceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_default_is_pending() {
        assert_eq!(ResourceState::default(), ResourceState::Pending);
    }

    #[test]
    fn state_is_running() {
        let state = ResourceState::Running {
            pid: Some(1234),
            started_at: Utc::now(),
        };
        assert!(state.is_running());
        assert!(!state.is_stopped());
        assert!(!state.is_transitioning());
    }

    #[test]
    fn state_is_stopped() {
        let state = ResourceState::Stopped {
            exit_code: Some(0),
            started_at: Utc::now(),
            stopped_at: Utc::now(),
        };
        assert!(state.is_stopped());
        assert!(!state.is_running());
    }

    #[test]
    fn state_is_failed() {
        let state = ResourceState::Failed {
            error: "connection refused".to_string(),
            exit_code: Some(1),
            started_at: Some(Utc::now()),
            failed_at: Utc::now(),
        };
        assert!(state.is_stopped());
        assert!(state.is_failed());
        assert_eq!(state.error(), Some("connection refused"));
    }

    #[test]
    fn state_is_transitioning() {
        let starting = ResourceState::Starting {
            started_at: Utc::now(),
        };
        assert!(starting.is_transitioning());

        let stopping = ResourceState::Stopping {
            started_at: Utc::now(),
            stopping_at: Utc::now(),
        };
        assert!(stopping.is_transitioning());
    }

    #[test]
    fn state_exit_code() {
        let stopped = ResourceState::Stopped {
            exit_code: Some(42),
            started_at: Utc::now(),
            stopped_at: Utc::now(),
        };
        assert_eq!(stopped.exit_code(), Some(42));

        let running = ResourceState::Running {
            pid: Some(1234),
            started_at: Utc::now(),
        };
        assert_eq!(running.exit_code(), None);
    }

    #[test]
    fn state_pid() {
        let running = ResourceState::Running {
            pid: Some(1234),
            started_at: Utc::now(),
        };
        assert_eq!(running.pid(), Some(1234));

        let pending = ResourceState::Pending;
        assert_eq!(pending.pid(), None);
    }

    #[test]
    fn state_as_str() {
        assert_eq!(ResourceState::Pending.as_str(), "pending");
        assert_eq!(
            ResourceState::Running {
                pid: None,
                started_at: Utc::now()
            }
            .as_str(),
            "running"
        );
    }

    #[test]
    fn state_display() {
        assert_eq!(ResourceState::Pending.to_string(), "pending");
    }

    #[test]
    fn state_serde_pending() {
        let state = ResourceState::Pending;
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "pending");

        let parsed: ResourceState = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, state);
    }

    #[test]
    fn state_serde_running() {
        let now = Utc::now();
        let state = ResourceState::Running {
            pid: Some(1234),
            started_at: now,
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "running");
        assert_eq!(json["pid"], 1234);

        let parsed: ResourceState = serde_json::from_value(json).unwrap();
        assert!(parsed.is_running());
        assert_eq!(parsed.pid(), Some(1234));
    }

    #[test]
    fn state_serde_failed() {
        let state = ResourceState::Failed {
            error: "test error".to_string(),
            exit_code: Some(1),
            started_at: None,
            failed_at: Utc::now(),
        };
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["state"], "failed");
        assert_eq!(json["error"], "test error");
    }
}
