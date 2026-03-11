//! Application state for web server.

use dtx_core::events::ResourceEventBus;
use dtx_core::store::ConfigStore;
use dtx_core::NixClient;
use dtx_process::ResourceOrchestrator;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::sse::ConnectionTracker;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Configuration store (config.yaml as single source of truth).
    pub store: Arc<RwLock<ConfigStore>>,
    /// Nix client for package operations.
    pub nix_client: Arc<NixClient>,
    /// Resource orchestrator - optional, set when services are running.
    pub orchestrator: Arc<RwLock<Option<ResourceOrchestrator>>>,
    /// SSE connection tracker.
    pub sse_tracker: Arc<ConnectionTracker>,
    /// Resource event bus for lifecycle event distribution.
    pub event_bus: Arc<ResourceEventBus>,
    /// Server start time for uptime calculation.
    pub server_started_at: Instant,
    /// Console logger running flag (prevents duplicate loggers).
    pub console_logger_running: Arc<AtomicBool>,
    /// Cancellation token for background tasks.
    pub shutdown_token: CancellationToken,
}

impl AppState {
    /// Creates a new AppState with the given ConfigStore.
    pub fn new(store: ConfigStore) -> Self {
        let nix_client = Arc::new(NixClient::new());

        Self {
            store: Arc::new(RwLock::new(store)),
            nix_client,
            orchestrator: Arc::new(RwLock::new(None)),
            sse_tracker: Arc::new(ConnectionTracker::new()),
            event_bus: Arc::new(ResourceEventBus::new()),
            server_started_at: Instant::now(),
            console_logger_running: Arc::new(AtomicBool::new(false)),
            shutdown_token: CancellationToken::new(),
        }
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
