//! Event bus for resource lifecycle events.
//!
//! Provides pub-sub distribution of `LifecycleEvent` via tokio broadcast
//! channels, with filtering and replay buffer for late subscribers.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use tokio::sync::broadcast;
use tracing::warn;

use super::filter::EventFilter;
use super::lifecycle::LifecycleEvent;

/// Default capacity for the broadcast channel.
const DEFAULT_CAPACITY: usize = 1000;

/// Size of the replay buffer for late subscribers.
const REPLAY_BUFFER_SIZE: usize = 500;

/// Event bus for resource lifecycle events.
///
/// The bus uses a tokio broadcast channel for pub-sub distribution
/// and maintains a replay buffer for late subscribers.
///
/// # Example
///
/// ```ignore
/// use dtx_core::events::{ResourceEventBus, LifecycleEvent, EventFilter};
///
/// let bus = ResourceEventBus::new();
///
/// // Subscribe to all events
/// let mut sub = bus.subscribe();
///
/// // Or subscribe with a filter
/// let filter = EventFilter::new().resource("api");
/// let mut filtered_sub = bus.subscribe_filtered(filter);
///
/// // Publish an event
/// bus.publish(LifecycleEvent::starting("api", ResourceKind::Process));
/// ```
#[derive(Clone)]
pub struct ResourceEventBus {
    sender: Arc<broadcast::Sender<LifecycleEvent>>,
    replay_buffer: Arc<RwLock<ReplayBuffer>>,
    metrics: Arc<EventBusMetrics>,
}

/// Internal replay buffer for storing recent events.
struct ReplayBuffer {
    events: VecDeque<LifecycleEvent>,
    capacity: usize,
}

impl ReplayBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    fn push(&mut self, event: LifecycleEvent) {
        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }

    fn replay(&self, filter: &EventFilter) -> Vec<LifecycleEvent> {
        self.events
            .iter()
            .filter(|e| filter.matches(e))
            .cloned()
            .collect()
    }

    fn clear(&mut self) {
        self.events.clear();
    }

    fn len(&self) -> usize {
        self.events.len()
    }
}

/// Metrics for the event bus.
#[derive(Default)]
struct EventBusMetrics {
    /// Total events published.
    published: AtomicU64,
    /// Events dropped due to no subscribers.
    dropped: AtomicU64,
}

impl ResourceEventBus {
    /// Create a new event bus with default capacity.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Create a new event bus with the specified channel capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self {
            sender: Arc::new(sender),
            replay_buffer: Arc::new(RwLock::new(ReplayBuffer::new(REPLAY_BUFFER_SIZE))),
            metrics: Arc::new(EventBusMetrics::default()),
        }
    }

    /// Publish an event to all subscribers.
    ///
    /// Returns the number of subscribers that received the event.
    pub fn publish(&self, event: LifecycleEvent) -> usize {
        // Store in replay buffer
        if let Ok(mut buffer) = self.replay_buffer.write() {
            buffer.push(event.clone());
        }

        // Broadcast to subscribers
        let receivers = self.sender.send(event).unwrap_or(0);

        self.metrics.published.fetch_add(1, Ordering::Relaxed);
        if receivers == 0 {
            self.metrics.dropped.fetch_add(1, Ordering::Relaxed);
        }

        receivers
    }

    /// Create a new subscriber that receives all events.
    pub fn subscribe(&self) -> ResourceEventSubscriber {
        ResourceEventSubscriber {
            receiver: self.sender.subscribe(),
            filter: EventFilter::all(),
        }
    }

    /// Create a new subscriber with a filter.
    pub fn subscribe_filtered(&self, filter: EventFilter) -> ResourceEventSubscriber {
        ResourceEventSubscriber {
            receiver: self.sender.subscribe(),
            filter,
        }
    }

    /// Get recent events matching the filter from the replay buffer.
    ///
    /// This is useful for late subscribers who want to catch up on
    /// events they may have missed.
    pub fn replay(&self, filter: &EventFilter) -> Vec<LifecycleEvent> {
        self.replay_buffer
            .read()
            .map(|b| b.replay(filter))
            .unwrap_or_default()
    }

    /// Get all recent events from the replay buffer.
    pub fn recent_events(&self) -> Vec<LifecycleEvent> {
        self.replay(&EventFilter::all())
    }

    /// Clear the replay buffer.
    pub fn clear_replay_buffer(&self) {
        if let Ok(mut buffer) = self.replay_buffer.write() {
            buffer.clear();
        }
    }

    /// Get the number of events in the replay buffer.
    pub fn replay_buffer_len(&self) -> usize {
        self.replay_buffer.read().map(|b| b.len()).unwrap_or(0)
    }

    /// Get the current subscriber count.
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Get metrics (published, dropped).
    pub fn metrics(&self) -> (u64, u64) {
        (
            self.metrics.published.load(Ordering::Relaxed),
            self.metrics.dropped.load(Ordering::Relaxed),
        )
    }
}

impl Default for ResourceEventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// Subscriber to the resource event bus.
pub struct ResourceEventSubscriber {
    receiver: broadcast::Receiver<LifecycleEvent>,
    filter: EventFilter,
}

impl ResourceEventSubscriber {
    /// Receive the next event that matches the filter.
    ///
    /// Blocks until an event is available or the bus is closed.
    /// Returns `None` if the bus is closed.
    pub async fn recv(&mut self) -> Option<LifecycleEvent> {
        loop {
            match self.receiver.recv().await {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        return Some(event);
                    }
                    // Event didn't match filter, continue waiting
                }
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("ResourceEventSubscriber lagged by {} events", n);
                    // Continue to next event
                }
            }
        }
    }

    /// Try to receive an event without blocking.
    ///
    /// Returns `Some(event)` if an event matching the filter is available,
    /// `None` if no matching event is available.
    pub fn try_recv(&mut self) -> Option<LifecycleEvent> {
        loop {
            match self.receiver.try_recv() {
                Ok(event) => {
                    if self.filter.matches(&event) {
                        return Some(event);
                    }
                    // Event didn't match filter, try again
                }
                Err(broadcast::error::TryRecvError::Empty) => return None,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    warn!("ResourceEventSubscriber lagged by {} events", n);
                    // Try again
                }
                Err(broadcast::error::TryRecvError::Closed) => return None,
            }
        }
    }

    /// Update the filter for this subscriber.
    pub fn set_filter(&mut self, filter: EventFilter) {
        self.filter = filter;
    }

    /// Get a reference to the current filter.
    pub fn filter(&self) -> &EventFilter {
        &self.filter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::{LogStreamKind, ResourceId, ResourceKind};
    use chrono::Utc;

    fn make_starting(id: &str) -> LifecycleEvent {
        LifecycleEvent::Starting {
            id: ResourceId::new(id),
            kind: ResourceKind::Process,
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

    #[tokio::test]
    async fn eventbus_publish_receive() {
        let bus = ResourceEventBus::new();
        let mut sub = bus.subscribe();

        let event = make_starting("api");
        bus.publish(event.clone());

        let received = sub.recv().await.unwrap();
        assert_eq!(received.resource_id(), Some(&ResourceId::new("api")));
    }

    #[tokio::test]
    async fn eventbus_filtered_subscriber() {
        let bus = ResourceEventBus::new();
        let filter = EventFilter::new().resource("api");
        let mut sub = bus.subscribe_filtered(filter);

        // This should be filtered out
        bus.publish(make_starting("db"));

        // This should pass
        bus.publish(make_starting("api"));

        let received = sub.try_recv().unwrap();
        assert_eq!(received.resource_id(), Some(&ResourceId::new("api")));
    }

    #[tokio::test]
    async fn eventbus_multiple_subscribers() {
        let bus = ResourceEventBus::new();
        let mut sub1 = bus.subscribe();
        let mut sub2 = bus.subscribe();

        assert_eq!(bus.subscriber_count(), 2);

        bus.publish(make_starting("api"));

        let r1 = sub1.recv().await.unwrap();
        let r2 = sub2.recv().await.unwrap();

        assert_eq!(r1.resource_id(), r2.resource_id());
    }

    #[test]
    fn eventbus_replay() {
        let bus = ResourceEventBus::new();

        for i in 0..10 {
            bus.publish(make_starting(&format!("svc-{}", i)));
        }

        assert_eq!(bus.replay_buffer_len(), 10);

        let filter = EventFilter::new().resource("svc-5");
        let replayed = bus.replay(&filter);
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].resource_id(), Some(&ResourceId::new("svc-5")));
    }

    #[test]
    fn eventbus_replay_all() {
        let bus = ResourceEventBus::new();

        for i in 0..5 {
            bus.publish(make_starting(&format!("svc-{}", i)));
        }

        let all = bus.recent_events();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn eventbus_metrics() {
        let bus = ResourceEventBus::new();

        // No subscribers, events dropped
        bus.publish(make_starting("api"));

        let (published, dropped) = bus.metrics();
        assert_eq!(published, 1);
        assert_eq!(dropped, 1);

        // With subscriber
        let _sub = bus.subscribe();
        bus.publish(make_starting("api"));

        let (published, dropped) = bus.metrics();
        assert_eq!(published, 2);
        assert_eq!(dropped, 1); // Still 1 from before
    }

    #[test]
    fn eventbus_clear_replay() {
        let bus = ResourceEventBus::new();

        bus.publish(make_starting("api"));
        assert_eq!(bus.replay_buffer_len(), 1);

        bus.clear_replay_buffer();
        assert_eq!(bus.replay_buffer_len(), 0);
    }

    #[tokio::test]
    async fn subscriber_set_filter() {
        let bus = ResourceEventBus::new();
        let mut sub = bus.subscribe_filtered(EventFilter::new().resource("api"));

        // Initially filtering for "api"
        bus.publish(make_starting("web"));
        bus.publish(make_starting("api"));

        let received = sub.try_recv().unwrap();
        assert_eq!(received.resource_id(), Some(&ResourceId::new("api")));

        // Change filter to "web"
        sub.set_filter(EventFilter::new().resource("web"));

        bus.publish(make_starting("web"));
        let received = sub.try_recv().unwrap();
        assert_eq!(received.resource_id(), Some(&ResourceId::new("web")));
    }

    #[test]
    fn subscriber_filter_logs() {
        let bus = ResourceEventBus::new();

        // Default filter excludes logs
        let mut sub = bus.subscribe_filtered(EventFilter::new());

        bus.publish(make_log("api"));
        bus.publish(make_starting("api"));

        // Should only get the starting event
        let received = sub.try_recv().unwrap();
        assert!(received.is_state_transition());
        assert!(sub.try_recv().is_none());
    }

    #[tokio::test]
    async fn eventbus_closed() {
        let bus = ResourceEventBus::new();
        let mut sub = bus.subscribe();

        drop(bus);

        let received = sub.recv().await;
        assert!(received.is_none());
    }
}
