//! Application state for web server.

use dtx_code::WorkspaceIndex;
use dtx_core::events::ResourceEventBus;
use dtx_core::store::ConfigStore;
use dtx_core::NixClient;
use dtx_memory::MemoryStore;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::config::WebConfig;
use crate::registry::{build_project_state, ProjectRegistry, ProjectState};
use crate::service::{OrchestratorHandle, ServiceOps};
use crate::sse::ConnectionTracker;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    registry: Arc<ProjectRegistry>,
    nix_client: Arc<NixClient>,
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

        let initial_project = build_project_state(store, &event_bus, &config);
        let registry = Arc::new(ProjectRegistry::new(initial_project));

        Self {
            registry,
            nix_client,
            sse_tracker: ConnectionTracker::new(),
            event_bus,
            config,
            server_started_at: Instant::now(),
            shutdown_token: CancellationToken::new(),
        }
    }

    // --- Registry ---

    pub fn registry(&self) -> &Arc<ProjectRegistry> {
        &self.registry
    }

    // --- Convenience accessors delegating to active project ---

    /// Get the active project's store.
    pub fn store(&self) -> Arc<RwLock<ConfigStore>> {
        let project = self.registry.active();
        project.store().clone()
    }

    /// Get the active project's orchestrator handle.
    pub fn orchestrator_handle(&self) -> Arc<OrchestratorHandle> {
        let project = self.registry.active();
        project.orchestrator_handle().clone()
    }

    /// Get the active project's workspace index.
    pub fn workspace_index(&self) -> Arc<WorkspaceIndex> {
        let project = self.registry.active();
        project.workspace_index().clone()
    }

    /// Get the active project's memory store.
    pub fn memory_store(&self) -> Option<Arc<MemoryStore>> {
        let project = self.registry.active();
        project.memory_store().cloned()
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

    /// Create a ServiceOps instance from the active project.
    pub fn service_ops(&self) -> ServiceOps {
        ServiceOps::new(
            self.store(),
            self.nix_client.clone(),
            self.event_bus.clone(),
        )
    }

    /// Create a ServiceOps for a specific project.
    pub fn service_ops_for(&self, project: &ProjectState) -> ServiceOps {
        ServiceOps::new(
            project.store().clone(),
            self.nix_client.clone(),
            self.event_bus.clone(),
        )
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
