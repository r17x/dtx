//! Application state for web server.

use dtx_core::events::ResourceEventBus;
use dtx_core::store::ConfigStore;
use dtx_core::NixClient;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::config::WebConfig;
use crate::service::{OrchestratorHandle, ServiceOps};
use crate::sse::ConnectionTracker;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    store: Arc<RwLock<ConfigStore>>,
    nix_client: Arc<NixClient>,
    orchestrator_handle: Arc<OrchestratorHandle>,
    sse_tracker: Arc<ConnectionTracker>,
    event_bus: Arc<ResourceEventBus>,
    config: Arc<WebConfig>,
    server_started_at: Instant,
    shutdown_token: CancellationToken,
}

impl AppState {
    /// Creates a new AppState with the given ConfigStore and WebConfig.
    pub fn new(store: ConfigStore, config: WebConfig) -> Self {
        let nix_client = Arc::new(NixClient::new());
        let event_bus = Arc::new(ResourceEventBus::new());
        let config = Arc::new(config);

        let orchestrator_handle =
            Arc::new(OrchestratorHandle::new(event_bus.clone(), config.clone()));

        Self {
            store: Arc::new(RwLock::new(store)),
            nix_client,
            orchestrator_handle,
            sse_tracker: ConnectionTracker::new(),
            event_bus,
            config,
            server_started_at: Instant::now(),
            shutdown_token: CancellationToken::new(),
        }
    }

    // --- Accessor methods ---

    pub fn store(&self) -> &Arc<RwLock<ConfigStore>> {
        &self.store
    }

    pub fn nix_client(&self) -> &Arc<NixClient> {
        &self.nix_client
    }

    pub fn event_bus(&self) -> &Arc<ResourceEventBus> {
        &self.event_bus
    }

    pub fn sse_tracker(&self) -> &Arc<ConnectionTracker> {
        &self.sse_tracker
    }

    pub fn config(&self) -> &Arc<WebConfig> {
        &self.config
    }

    pub fn shutdown_token(&self) -> &CancellationToken {
        &self.shutdown_token
    }

    pub fn server_started_at(&self) -> Instant {
        self.server_started_at
    }

    // --- Service constructors ---

    /// Create a ServiceOps instance from the shared state components.
    pub fn service_ops(&self) -> ServiceOps {
        ServiceOps::new(
            self.store.clone(),
            self.nix_client.clone(),
            self.event_bus.clone(),
        )
    }

    /// Get a reference to the OrchestratorHandle.
    pub fn orchestrator_handle(&self) -> &Arc<OrchestratorHandle> {
        &self.orchestrator_handle
    }

    /// Returns the server uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.server_started_at.elapsed().as_secs()
    }

    /// Returns formatted uptime string.
    pub fn uptime_formatted(&self) -> String {
        let secs = self.uptime_secs();
        let hours = secs / 3600;
        let minutes = (secs % 3600) / 60;
        let seconds = secs % 60;

        if hours > 0 {
            format!("{}h {}m", hours, minutes)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    }
}
