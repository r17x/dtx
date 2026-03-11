//! Event filtering for selective subscription.

use super::lifecycle::LifecycleEvent;
use crate::resource::{ResourceId, ResourceKind};

/// Filter for subscribing to specific lifecycle events.
///
/// Filters are combined with AND logic: an event must match all
/// non-empty filter criteria to pass through.
///
/// # Example
///
/// ```ignore
/// use dtx_core::events::{EventFilter, LifecycleEvent};
/// use dtx_core::resource::ResourceKind;
///
/// // Filter for process events from "api" or "web"
/// let filter = EventFilter::new()
///     .resource("api")
///     .resource("web")
///     .kind(ResourceKind::Process)
///     .without_logs();
/// ```
#[derive(Clone, Debug, Default)]
pub struct EventFilter {
    /// Filter by resource IDs (empty = all).
    resource_ids: Vec<ResourceId>,
    /// Filter by resource kinds (empty = all).
    kinds: Vec<ResourceKind>,
    /// Filter by event types (empty = all).
    event_types: Vec<String>,
    /// Include log events (default: false).
    include_logs: bool,
}

impl EventFilter {
    /// Create a new empty filter (matches nothing by default for logs).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a filter that matches all events including logs.
    pub fn all() -> Self {
        Self {
            include_logs: true,
            ..Default::default()
        }
    }

    /// Create a filter that matches all events except logs.
    pub fn all_except_logs() -> Self {
        Self {
            include_logs: false,
            ..Default::default()
        }
    }

    /// Add a resource ID to filter by.
    ///
    /// If multiple IDs are added, events matching ANY of them will pass.
    pub fn resource(mut self, id: impl Into<ResourceId>) -> Self {
        self.resource_ids.push(id.into());
        self
    }

    /// Add multiple resource IDs to filter by.
    pub fn resources(mut self, ids: impl IntoIterator<Item = impl Into<ResourceId>>) -> Self {
        self.resource_ids.extend(ids.into_iter().map(Into::into));
        self
    }

    /// Add a resource kind to filter by.
    ///
    /// If multiple kinds are added, events matching ANY of them will pass.
    pub fn kind(mut self, kind: ResourceKind) -> Self {
        self.kinds.push(kind);
        self
    }

    /// Add an event type to filter by (e.g., "starting", "running").
    ///
    /// If multiple types are added, events matching ANY of them will pass.
    pub fn event_type(mut self, event_type: impl Into<String>) -> Self {
        self.event_types.push(event_type.into());
        self
    }

    /// Include log events in the filter.
    pub fn with_logs(mut self) -> Self {
        self.include_logs = true;
        self
    }

    /// Exclude log events from the filter.
    pub fn without_logs(mut self) -> Self {
        self.include_logs = false;
        self
    }

    /// Check if an event matches this filter.
    pub fn matches(&self, event: &LifecycleEvent) -> bool {
        // Check log filter first (fast path)
        if event.is_log() && !self.include_logs {
            return false;
        }

        // Check resource ID filter
        if !self.resource_ids.is_empty() {
            match event.resource_id() {
                Some(id) if self.resource_ids.contains(id) => {}
                Some(_) => return false,
                // Events without resource_id (like ConfigChanged) pass if no ID filter
                None => {}
            }
        }

        // Check kind filter
        if !self.kinds.is_empty() {
            match event.kind() {
                Some(kind) if self.kinds.contains(&kind) => {}
                Some(_) => return false,
                // Events without kind pass if no kind filter
                None => {}
            }
        }

        // Check event type filter
        if !self.event_types.is_empty() && !self.event_types.iter().any(|t| t == event.event_type())
        {
            return false;
        }

        true
    }

    /// Check if this filter would accept any logs.
    pub fn accepts_logs(&self) -> bool {
        self.include_logs
    }

    /// Get the resource IDs this filter matches (empty = all).
    pub fn resource_ids(&self) -> &[ResourceId] {
        &self.resource_ids
    }

    /// Get the kinds this filter matches (empty = all).
    pub fn kinds(&self) -> &[ResourceKind] {
        &self.kinds
    }

    /// Get the event types this filter matches (empty = all).
    pub fn event_types(&self) -> &[String] {
        &self.event_types
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::LogStreamKind;
    use chrono::Utc;

    fn make_starting(id: &str) -> LifecycleEvent {
        LifecycleEvent::Starting {
            id: ResourceId::new(id),
            kind: ResourceKind::Process,
            timestamp: Utc::now(),
        }
    }

    fn make_running(id: &str, kind: ResourceKind) -> LifecycleEvent {
        LifecycleEvent::Running {
            id: ResourceId::new(id),
            kind,
            pid: None,
            timestamp: Utc::now(),
        }
    }

    fn make_log(id: &str) -> LifecycleEvent {
        LifecycleEvent::Log {
            id: ResourceId::new(id),
            stream: LogStreamKind::Stdout,
            line: "test".to_string(),
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn filter_all_matches_everything() {
        let filter = EventFilter::all();
        assert!(filter.matches(&make_starting("api")));
        assert!(filter.matches(&make_log("api")));
    }

    #[test]
    fn filter_default_excludes_logs() {
        let filter = EventFilter::new();
        assert!(filter.matches(&make_starting("api")));
        assert!(!filter.matches(&make_log("api")));
    }

    #[test]
    fn filter_by_resource() {
        let filter = EventFilter::new().resource("api");
        assert!(filter.matches(&make_starting("api")));
        assert!(!filter.matches(&make_starting("web")));
    }

    #[test]
    fn filter_by_multiple_resources() {
        let filter = EventFilter::new().resource("api").resource("web");
        assert!(filter.matches(&make_starting("api")));
        assert!(filter.matches(&make_starting("web")));
        assert!(!filter.matches(&make_starting("db")));
    }

    #[test]
    fn filter_by_kind() {
        let filter = EventFilter::new().kind(ResourceKind::Container);
        assert!(filter.matches(&make_running("api", ResourceKind::Container)));
        assert!(!filter.matches(&make_running("api", ResourceKind::Process)));
    }

    #[test]
    fn filter_by_event_type() {
        let filter = EventFilter::new().event_type("starting");
        assert!(filter.matches(&make_starting("api")));
        assert!(!filter.matches(&make_running("api", ResourceKind::Process)));
    }

    #[test]
    fn filter_excludes_logs_by_default() {
        let filter = EventFilter::new();
        assert!(!filter.matches(&make_log("api")));
    }

    #[test]
    fn filter_with_logs() {
        let filter = EventFilter::new().with_logs();
        assert!(filter.matches(&make_log("api")));
    }

    #[test]
    fn filter_combined() {
        let filter = EventFilter::new()
            .resource("api")
            .kind(ResourceKind::Process)
            .event_type("starting");

        // Matches: api + process + starting
        assert!(filter.matches(&make_starting("api")));

        // Doesn't match: wrong resource
        assert!(!filter.matches(&make_starting("web")));

        // Doesn't match: wrong event type
        assert!(!filter.matches(&make_running("api", ResourceKind::Process)));
    }

    #[test]
    fn filter_config_changed_passes_with_no_id_filter() {
        let filter = EventFilter::new();
        let event = LifecycleEvent::ConfigChanged {
            project_id: "proj".to_string(),
            timestamp: Utc::now(),
        };
        assert!(filter.matches(&event));
    }

    #[test]
    fn filter_accessors() {
        let filter = EventFilter::new()
            .resource("api")
            .kind(ResourceKind::Process)
            .event_type("starting")
            .with_logs();

        assert_eq!(filter.resource_ids().len(), 1);
        assert_eq!(filter.kinds().len(), 1);
        assert_eq!(filter.event_types().len(), 1);
        assert!(filter.accepts_logs());
    }
}
