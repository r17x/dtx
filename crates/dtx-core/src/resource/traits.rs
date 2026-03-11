//! Core Resource trait for orchestrated entities.

use async_trait::async_trait;
use std::any::Any;

use super::context::Context;
use super::health::{HealthStatus, LogStream};
use super::id::ResourceId;
use super::kind::ResourceKind;
use super::state::ResourceState;

/// Error type for resource operations.
///
/// This is a placeholder for Phase 1.1. Full error hierarchy
/// will be implemented in Phase 1.3.
pub type ResourceError = Box<dyn std::error::Error + Send + Sync>;

/// Result type for resource operations.
pub type ResourceResult<T> = std::result::Result<T, ResourceError>;

/// A resource that can be orchestrated by dtx.
///
/// Resources are the fundamental unit of orchestration. They represent
/// anything with a lifecycle that dtx can manage: processes, containers,
/// VMs, AI agents, etc.
///
/// # Lifecycle
///
/// ```text
///     ┌─────────┐
///     │ Pending │
///     └────┬────┘
///          │ start()
///     ┌────▼────┐
///     │Starting │
///     └────┬────┘
///          │ ready
///     ┌────▼────┐◄──────┐
///     │ Running │       │ restart()
///     └────┬────┘───────┘
///          │ stop() / exit
///     ┌────▼────┐
///     │ Stopped │
///     └─────────┘
/// ```
///
/// # Implementation Notes
///
/// - All methods that modify state are async and take `&mut self`
/// - State transitions must publish events via EventBus (Phase 1.2)
/// - Health checks should be non-blocking and return quickly
#[async_trait]
pub trait Resource: Send + Sync {
    /// Unique identifier for this resource.
    fn id(&self) -> &ResourceId;

    /// The kind of resource (process, container, etc.).
    fn kind(&self) -> ResourceKind;

    /// Current state of the resource.
    fn state(&self) -> &ResourceState;

    /// Start the resource.
    ///
    /// # Errors
    ///
    /// Returns an error if the resource fails to start or is in an
    /// invalid state for starting.
    async fn start(&mut self, ctx: &Context) -> ResourceResult<()>;

    /// Stop the resource gracefully.
    ///
    /// The resource should attempt to shut down cleanly, allowing
    /// in-flight operations to complete.
    ///
    /// # Errors
    ///
    /// Returns an error if the stop fails. Note that a timeout during
    /// graceful shutdown may require a subsequent `kill()` call.
    async fn stop(&mut self, ctx: &Context) -> ResourceResult<()>;

    /// Force stop the resource.
    ///
    /// This immediately terminates the resource without waiting for
    /// graceful shutdown. Use when `stop()` times out or fails.
    ///
    /// Default implementation delegates to `stop()`.
    async fn kill(&mut self, ctx: &Context) -> ResourceResult<()> {
        self.stop(ctx).await
    }

    /// Restart the resource.
    ///
    /// Stops and then starts the resource. Implementations may override
    /// for more efficient restart mechanisms.
    ///
    /// Default implementation calls `stop()` then `start()`.
    async fn restart(&mut self, ctx: &Context) -> ResourceResult<()> {
        self.stop(ctx).await?;
        self.start(ctx).await
    }

    /// Check health status of the resource.
    ///
    /// Health checks should be lightweight and non-blocking.
    /// Returns `Unknown` by default if not implemented.
    async fn health(&self) -> HealthStatus {
        HealthStatus::Unknown
    }

    /// Get a log stream for this resource.
    ///
    /// Returns `None` if logging is not supported or available.
    fn logs(&self) -> Option<Box<dyn LogStream>> {
        None
    }

    /// Downcast to concrete type.
    ///
    /// Allows accessing implementation-specific methods when the
    /// concrete type is known.
    fn as_any(&self) -> &dyn Any;

    /// Downcast to concrete type (mutable).
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Extension trait for boxed resources.
pub trait ResourceExt {
    /// Try to downcast to a specific resource type.
    fn downcast_ref<T: Resource + 'static>(&self) -> Option<&T>;

    /// Try to downcast to a specific resource type (mutable).
    fn downcast_mut<T: Resource + 'static>(&mut self) -> Option<&mut T>;
}

impl ResourceExt for dyn Resource {
    fn downcast_ref<T: Resource + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    fn downcast_mut<T: Resource + 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceState;
    use chrono::Utc;

    /// Mock resource for testing.
    struct MockResource {
        id: ResourceId,
        state: ResourceState,
        start_count: u32,
        stop_count: u32,
    }

    impl MockResource {
        fn new(id: impl Into<ResourceId>) -> Self {
            Self {
                id: id.into(),
                state: ResourceState::Pending,
                start_count: 0,
                stop_count: 0,
            }
        }
    }

    #[async_trait]
    impl Resource for MockResource {
        fn id(&self) -> &ResourceId {
            &self.id
        }

        fn kind(&self) -> ResourceKind {
            ResourceKind::Process
        }

        fn state(&self) -> &ResourceState {
            &self.state
        }

        async fn start(&mut self, _ctx: &Context) -> ResourceResult<()> {
            self.start_count += 1;
            self.state = ResourceState::Running {
                pid: Some(1234),
                started_at: Utc::now(),
            };
            Ok(())
        }

        async fn stop(&mut self, _ctx: &Context) -> ResourceResult<()> {
            self.stop_count += 1;
            let started_at = self.state.started_at().unwrap_or_else(Utc::now);
            self.state = ResourceState::Stopped {
                exit_code: Some(0),
                started_at,
                stopped_at: Utc::now(),
            };
            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[tokio::test]
    async fn mock_resource_lifecycle() {
        let mut resource = MockResource::new("test");
        let ctx = Context::new();

        assert!(resource.state().is_pending());
        assert_eq!(resource.start_count, 0);

        resource.start(&ctx).await.unwrap();
        assert!(resource.state().is_running());
        assert_eq!(resource.start_count, 1);

        resource.stop(&ctx).await.unwrap();
        assert!(resource.state().is_stopped());
        assert_eq!(resource.stop_count, 1);
    }

    #[tokio::test]
    async fn mock_resource_restart() {
        let mut resource = MockResource::new("test");
        let ctx = Context::new();

        resource.start(&ctx).await.unwrap();
        resource.restart(&ctx).await.unwrap();

        // restart calls stop then start
        assert_eq!(resource.start_count, 2);
        assert_eq!(resource.stop_count, 1);
        assert!(resource.state().is_running());
    }

    #[tokio::test]
    async fn mock_resource_health() {
        let resource = MockResource::new("test");
        let health = resource.health().await;
        assert!(health.is_unknown());
    }

    #[test]
    fn mock_resource_logs() {
        let resource = MockResource::new("test");
        assert!(resource.logs().is_none());
    }

    #[test]
    fn resource_downcast() {
        let resource = MockResource::new("test");
        let boxed: &dyn Resource = &resource;

        let downcasted = boxed.downcast_ref::<MockResource>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().id.as_str(), "test");
    }
}
