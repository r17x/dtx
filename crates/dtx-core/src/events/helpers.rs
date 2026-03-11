//! Helper constructors for lifecycle events.

use chrono::Utc;

use super::lifecycle::{DependencyCondition, LifecycleEvent};
use crate::resource::{LogStreamKind, ResourceId, ResourceKind};

/// Builder helpers for creating lifecycle events with automatic timestamps.
impl LifecycleEvent {
    /// Create a Starting event.
    pub fn starting(id: impl Into<ResourceId>, kind: ResourceKind) -> Self {
        Self::Starting {
            id: id.into(),
            kind,
            timestamp: Utc::now(),
        }
    }

    /// Create a Running event.
    pub fn running(id: impl Into<ResourceId>, kind: ResourceKind, pid: Option<u32>) -> Self {
        Self::Running {
            id: id.into(),
            kind,
            pid,
            timestamp: Utc::now(),
        }
    }

    /// Create a Stopping event.
    pub fn stopping(id: impl Into<ResourceId>, kind: ResourceKind) -> Self {
        Self::Stopping {
            id: id.into(),
            kind,
            timestamp: Utc::now(),
        }
    }

    /// Create a Stopped event.
    pub fn stopped(id: impl Into<ResourceId>, kind: ResourceKind, exit_code: Option<i32>) -> Self {
        Self::Stopped {
            id: id.into(),
            kind,
            exit_code,
            timestamp: Utc::now(),
        }
    }

    /// Create a Failed event.
    pub fn failed(
        id: impl Into<ResourceId>,
        kind: ResourceKind,
        error: impl Into<String>,
        exit_code: Option<i32>,
    ) -> Self {
        Self::Failed {
            id: id.into(),
            kind,
            error: error.into(),
            exit_code,
            timestamp: Utc::now(),
        }
    }

    /// Create a Restarting event.
    pub fn restarting(
        id: impl Into<ResourceId>,
        kind: ResourceKind,
        attempt: u32,
        max_attempts: Option<u32>,
    ) -> Self {
        Self::Restarting {
            id: id.into(),
            kind,
            attempt,
            max_attempts,
            timestamp: Utc::now(),
        }
    }

    /// Create a HealthCheckPassed event.
    pub fn health_passed(id: impl Into<ResourceId>) -> Self {
        Self::HealthCheckPassed {
            id: id.into(),
            timestamp: Utc::now(),
        }
    }

    /// Create a HealthCheckFailed event.
    pub fn health_failed(id: impl Into<ResourceId>, reason: impl Into<String>) -> Self {
        Self::HealthCheckFailed {
            id: id.into(),
            reason: reason.into(),
            timestamp: Utc::now(),
        }
    }

    /// Create a Log event.
    pub fn log(id: impl Into<ResourceId>, stream: LogStreamKind, line: impl Into<String>) -> Self {
        Self::Log {
            id: id.into(),
            stream,
            line: line.into(),
            timestamp: Utc::now(),
        }
    }

    /// Create a stdout Log event.
    pub fn stdout(id: impl Into<ResourceId>, line: impl Into<String>) -> Self {
        Self::log(id, LogStreamKind::Stdout, line)
    }

    /// Create a stderr Log event.
    pub fn stderr(id: impl Into<ResourceId>, line: impl Into<String>) -> Self {
        Self::log(id, LogStreamKind::Stderr, line)
    }

    /// Create a DependencyWaiting event.
    pub fn dependency_waiting(
        id: impl Into<ResourceId>,
        dependency: impl Into<ResourceId>,
        condition: DependencyCondition,
    ) -> Self {
        Self::DependencyWaiting {
            id: id.into(),
            dependency: dependency.into(),
            condition,
            timestamp: Utc::now(),
        }
    }

    /// Create a DependencyResolved event.
    pub fn dependency_resolved(
        id: impl Into<ResourceId>,
        dependency: impl Into<ResourceId>,
    ) -> Self {
        Self::DependencyResolved {
            id: id.into(),
            dependency: dependency.into(),
            timestamp: Utc::now(),
        }
    }

    /// Create a ConfigChanged event.
    pub fn config_changed(project_id: impl Into<String>) -> Self {
        Self::ConfigChanged {
            project_id: project_id.into(),
            timestamp: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helper_starting() {
        let event = LifecycleEvent::starting("api", ResourceKind::Process);
        assert_eq!(event.event_type(), "starting");
        assert_eq!(event.resource_id(), Some(&ResourceId::new("api")));
        assert_eq!(event.kind(), Some(ResourceKind::Process));
    }

    #[test]
    fn helper_running() {
        let event = LifecycleEvent::running("api", ResourceKind::Container, Some(1234));
        assert_eq!(event.event_type(), "running");
        if let LifecycleEvent::Running { pid, .. } = event {
            assert_eq!(pid, Some(1234));
        } else {
            panic!("Expected Running event");
        }
    }

    #[test]
    fn helper_stopped() {
        let event = LifecycleEvent::stopped("api", ResourceKind::Process, Some(0));
        assert_eq!(event.event_type(), "stopped");
        if let LifecycleEvent::Stopped { exit_code, .. } = event {
            assert_eq!(exit_code, Some(0));
        } else {
            panic!("Expected Stopped event");
        }
    }

    #[test]
    fn helper_failed() {
        let event = LifecycleEvent::failed("api", ResourceKind::Process, "timeout", Some(1));
        assert_eq!(event.event_type(), "failed");
        if let LifecycleEvent::Failed {
            error, exit_code, ..
        } = event
        {
            assert_eq!(error, "timeout");
            assert_eq!(exit_code, Some(1));
        } else {
            panic!("Expected Failed event");
        }
    }

    #[test]
    fn helper_restarting() {
        let event = LifecycleEvent::restarting("api", ResourceKind::Process, 2, Some(5));
        if let LifecycleEvent::Restarting {
            attempt,
            max_attempts,
            ..
        } = event
        {
            assert_eq!(attempt, 2);
            assert_eq!(max_attempts, Some(5));
        } else {
            panic!("Expected Restarting event");
        }
    }

    #[test]
    fn helper_health_passed() {
        let event = LifecycleEvent::health_passed("api");
        assert_eq!(event.event_type(), "health_check_passed");
    }

    #[test]
    fn helper_health_failed() {
        let event = LifecycleEvent::health_failed("api", "connection refused");
        if let LifecycleEvent::HealthCheckFailed { reason, .. } = event {
            assert_eq!(reason, "connection refused");
        } else {
            panic!("Expected HealthCheckFailed event");
        }
    }

    #[test]
    fn helper_stdout() {
        let event = LifecycleEvent::stdout("api", "Hello, world!");
        if let LifecycleEvent::Log { stream, line, .. } = event {
            assert_eq!(stream, LogStreamKind::Stdout);
            assert_eq!(line, "Hello, world!");
        } else {
            panic!("Expected Log event");
        }
    }

    #[test]
    fn helper_stderr() {
        let event = LifecycleEvent::stderr("api", "Error!");
        if let LifecycleEvent::Log { stream, line, .. } = event {
            assert_eq!(stream, LogStreamKind::Stderr);
            assert_eq!(line, "Error!");
        } else {
            panic!("Expected Log event");
        }
    }

    #[test]
    fn helper_dependency_waiting() {
        let event = LifecycleEvent::dependency_waiting("api", "db", DependencyCondition::Healthy);
        if let LifecycleEvent::DependencyWaiting {
            dependency,
            condition,
            ..
        } = event
        {
            assert_eq!(dependency.as_str(), "db");
            assert_eq!(condition, DependencyCondition::Healthy);
        } else {
            panic!("Expected DependencyWaiting event");
        }
    }

    #[test]
    fn helper_dependency_resolved() {
        let event = LifecycleEvent::dependency_resolved("api", "db");
        if let LifecycleEvent::DependencyResolved { dependency, .. } = event {
            assert_eq!(dependency.as_str(), "db");
        } else {
            panic!("Expected DependencyResolved event");
        }
    }

    #[test]
    fn helper_config_changed() {
        let event = LifecycleEvent::config_changed("my-project");
        if let LifecycleEvent::ConfigChanged { project_id, .. } = event {
            assert_eq!(project_id, "my-project");
        } else {
            panic!("Expected ConfigChanged event");
        }
    }
}
