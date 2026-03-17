//! Orchestrator lifecycle handle.
//!
//! Encapsulates the `ResourceOrchestrator`, its background tasks (polling and
//! console logging), and provides an atomic start/stop/restart/shutdown API
//! that replaces the scattered logic previously spread across `handlers::api`.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use dtx_core::events::{LifecycleEvent, ResourceEventBus};
use dtx_core::model::Service;
use dtx_core::process::{analyze_services, run_preflight};
use dtx_core::resource::{LogStreamKind, ResourceState};
use dtx_core::GraphValidator;
use dtx_process::{ProcessResourceConfig, ResourceOrchestrator};

use crate::config::WebConfig;
use crate::error::AppError;
use crate::types::PortReassignmentInfo;

/// Result of a start operation.
#[derive(Debug)]
pub struct StartResult {
    pub started: Vec<String>,
    pub failed: Vec<String>,
    pub port_reassignments: Vec<PortReassignmentInfo>,
}

/// Holds the running orchestrator and its background tasks.
struct OrchestratorState {
    orchestrator: ResourceOrchestrator,
    task_handles: Vec<JoinHandle<()>>,
    task_token: CancellationToken,
}

/// Thread-safe handle around `ResourceOrchestrator` that manages background
/// polling/logging tasks and provides atomic lifecycle operations.
pub struct OrchestratorHandle {
    inner: RwLock<Option<OrchestratorState>>,
    event_bus: Arc<ResourceEventBus>,
    config: Arc<WebConfig>,
}

impl OrchestratorHandle {
    pub fn new(event_bus: Arc<ResourceEventBus>, config: Arc<WebConfig>) -> Self {
        Self {
            inner: RwLock::new(None),
            event_bus,
            config,
        }
    }

    /// Atomic start: validate graph, run preflight, resolve port conflicts,
    /// build orchestrator, start all resources, and spawn background tasks.
    ///
    /// Holds the write lock for the entire operation to prevent TOCTOU races
    /// (two concurrent starts could both see `None` and proceed).
    pub async fn start(
        &self,
        services: Vec<Service>,
        project_root: &Path,
        project_name: &str,
        shutdown_token: &CancellationToken,
    ) -> Result<StartResult, AppError> {
        let mut guard = self.inner.write().await;
        if guard.is_some() {
            return Err(AppError::conflict("Services already running"));
        }

        if services.is_empty() {
            return Err(AppError::bad_request("No enabled services to start"));
        }

        // Validate dependency graph
        if let Err(validation_errors) = GraphValidator::validate_all(&services) {
            return Err(AppError::bad_request(format!(
                "Dependency graph validation failed: {}",
                validation_errors.join("; ")
            )));
        }

        // Pre-flight checks
        let checks = analyze_services(&services);
        let preflight_result = run_preflight(checks).await;

        if !preflight_result.is_ok() {
            let errors: Vec<String> = preflight_result
                .failed
                .iter()
                .map(|check| {
                    let mut msg = check.description.clone();
                    if !check.required_by.is_empty() {
                        msg.push_str(&format!(" (required by: {})", check.required_by.join(", ")));
                    }
                    if let Some(ref hint) = check.fix_hint {
                        msg.push_str(&format!(" -> {}", hint));
                    }
                    msg
                })
                .collect();

            return Err(AppError::bad_request(format!(
                "Pre-flight checks failed: {}",
                errors.join("; ")
            )));
        }

        // Resolve port conflicts
        let (services, reassignments) = dtx_core::resolve_port_conflicts(&services);

        for r in &reassignments {
            tracing::info!(
                service = %r.service_name,
                original = r.original_port,
                assigned = r.new_port,
                "Port reassigned due to conflict"
            );
        }

        // Generate flake.nix if none exists (filesystem I/O off async runtime)
        let flake_path = project_root.join("flake.nix");
        if !flake_path.exists() {
            let dtx_dir = project_root.join(".dtx");
            let dtx_flake_path = dtx_dir.join("flake.nix");
            let flake_content = dtx_core::FlakeGenerator::generate(&services, project_name);
            tokio::task::spawn_blocking(move || -> Result<(), AppError> {
                std::fs::create_dir_all(&dtx_dir)
                    .map_err(|e| AppError::internal(format!("Failed to create .dtx dir: {}", e)))?;
                std::fs::write(&dtx_flake_path, &flake_content)
                    .map_err(|e| AppError::internal(format!("Failed to write flake.nix: {}", e)))?;
                tracing::info!(path = %dtx_flake_path.display(), "Generated flake.nix");
                Ok(())
            })
            .await
            .map_err(|e| AppError::internal(format!("Flake generation task panicked: {}", e)))??;
        } else {
            tracing::info!(path = %flake_path.display(), "Using existing flake.nix");
        }

        // Build orchestrator
        let mut orchestrator = ResourceOrchestrator::new(self.event_bus.clone());

        for svc in &services {
            let config = service_to_process_config(svc, project_root);
            orchestrator.add_resource(config);
        }

        let result = orchestrator.start_all().await.map_err(AppError::internal)?;

        for (id, error) in &result.failed {
            tracing::error!(resource = %id, error = %error, "Failed to start resource");
        }

        // Spawn background tasks with a child token so we can cancel them
        // independently of the server-wide shutdown.
        let task_token = shutdown_token.child_token();
        let task_handles = vec![
            spawn_polling_task(
                &orchestrator,
                self.config.orchestrator_poll_interval,
                task_token.clone(),
            ),
            spawn_console_logger(self.event_bus.clone(), task_token.clone()),
        ];

        // Build start result
        let start_result = StartResult {
            started: result.started.iter().map(|id| id.to_string()).collect(),
            failed: result.failed.iter().map(|(id, _)| id.to_string()).collect(),
            port_reassignments: reassignments
                .into_iter()
                .map(|r| PortReassignmentInfo {
                    service: r.service_name,
                    original_port: r.original_port,
                    assigned_port: r.new_port,
                })
                .collect(),
        };

        // Store state under the same write lock — no TOCTOU gap
        *guard = Some(OrchestratorState {
            orchestrator,
            task_handles,
            task_token,
        });

        Ok(start_result)
    }

    /// Graceful stop: cancel background tasks, await their completion, then
    /// stop all resources in reverse dependency order.
    pub async fn stop(&self) -> Result<(), AppError> {
        let state = {
            let mut guard = self.inner.write().await;
            guard.take()
        };

        let Some(mut state) = state else {
            return Ok(());
        };

        // Cancel background tasks
        state.task_token.cancel();

        // Await task handles with timeout
        let handles_future = async {
            for handle in state.task_handles.drain(..) {
                let _ = handle.await;
            }
        };
        let _ = tokio::time::timeout(Duration::from_secs(2), handles_future).await;

        // Stop orchestrator
        state
            .orchestrator
            .stop_all()
            .await
            .map_err(|e| AppError::internal(format!("Failed to stop orchestrator: {}", e)))?;

        Ok(())
    }

    /// Stop then re-start with a drain delay in between to allow ports and
    /// file handles to be released.
    pub async fn restart(
        &self,
        services: Vec<Service>,
        project_root: &Path,
        project_name: &str,
        shutdown_token: &CancellationToken,
    ) -> Result<StartResult, AppError> {
        self.stop().await?;
        tokio::time::sleep(self.config.restart_drain_delay).await;
        self.start(services, project_root, project_name, shutdown_token)
            .await
    }

    /// Fast non-blocking check whether the orchestrator is running.
    pub fn is_running(&self) -> bool {
        // try_read avoids blocking; if the lock is held we conservatively
        // report "not running" which is safe for UI display.
        self.inner
            .try_read()
            .map(|guard| guard.as_ref().is_some_and(|s| s.orchestrator.is_running()))
            .unwrap_or(false)
    }

    /// Read the status of every managed resource.
    pub async fn status(
        &self,
    ) -> Result<
        Option<std::collections::HashMap<dtx_core::resource::ResourceId, ResourceState>>,
        AppError,
    > {
        let guard = self.inner.read().await;
        match &*guard {
            Some(state) => Ok(Some(state.orchestrator.status().await)),
            None => Ok(None),
        }
    }

    /// Read the health of every managed resource.
    pub async fn health(
        &self,
    ) -> Result<
        Option<
            std::collections::HashMap<
                dtx_core::resource::ResourceId,
                dtx_core::resource::HealthStatus,
            >,
        >,
        AppError,
    > {
        let guard = self.inner.read().await;
        match &*guard {
            Some(state) => Ok(Some(state.orchestrator.health().await)),
            None => Ok(None),
        }
    }

    /// Full shutdown with force-kill fallback on timeout. Intended for the web
    /// server shutdown handler.
    pub async fn shutdown(&self) -> Result<(), AppError> {
        let state = {
            let mut guard = self.inner.write().await;
            guard.take()
        };

        let Some(mut state) = state else {
            return Ok(());
        };

        state.task_token.cancel();

        let stop_future = async {
            // Best-effort await task handles
            for handle in state.task_handles.drain(..) {
                let _ = handle.await;
            }
            state.orchestrator.stop_all().await
        };

        match tokio::time::timeout(self.config.orchestrator_stop_timeout, stop_future).await {
            Ok(Ok(())) => {
                tracing::info!("Orchestrator shutdown completed cleanly");
            }
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "Orchestrator stop returned error");
            }
            Err(_) => {
                tracing::warn!(
                    timeout_secs = self.config.orchestrator_stop_timeout.as_secs(),
                    "Orchestrator shutdown timed out, resources may have been force-killed"
                );
            }
        }

        Ok(())
    }

    /// Borrow the inner orchestrator for direct read access (e.g. iterating
    /// resource IDs or reading individual resource state). The returned guard
    /// must not be held across `.await` points on the orchestrator itself.
    pub async fn with_orchestrator<F, T>(&self, f: F) -> Option<T>
    where
        F: FnOnce(&ResourceOrchestrator) -> T,
    {
        let guard = self.inner.read().await;
        guard.as_ref().map(|s| f(&s.orchestrator))
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Convert a `model::Service` to a `ProcessResourceConfig`.
fn service_to_process_config(service: &Service, project_root: &Path) -> ProcessResourceConfig {
    let mut config = ProcessResourceConfig::new(&service.name, &service.command);

    if let Some(ref wd) = service.working_dir {
        config = config.with_working_dir(wd.clone());
    } else {
        config = config.with_working_dir(project_root);
    }

    if let Some(ref env) = service.environment {
        config = config.with_environment(env.clone());
    }

    if let Some(port) = service.port {
        config = config.with_port(port);
    }

    if let Some(ref deps) = service.depends_on {
        for dep in deps {
            config = config.depends_on(dep.service.clone());
        }
    }

    config
}

/// Spawns a background task that periodically calls `orchestrator.poll()` to
/// detect process exits and trigger restarts.
fn spawn_polling_task(
    orchestrator: &ResourceOrchestrator,
    poll_interval: Duration,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    // We need to get Arc references to the resources so we can poll them
    // without holding the OrchestratorHandle lock. However, the orchestrator
    // owns resources behind Arc<RwLock<..>>, so we clone those arcs and poll
    // directly.  Because ResourceOrchestrator::poll(&mut self) needs &mut,
    // we instead keep a reference to the whole orchestrator via shared state.
    //
    // Current approach: the task takes a snapshot of resource IDs and the
    // event bus. The actual poll happens via OrchestratorState which holds the
    // mutable orchestrator behind the RwLock.  This task is a simplified
    // version -- it relies on the parent holding the write lock only briefly.
    //
    // For now we use the same pattern as the original api.rs: wrap the
    // orchestrator in Arc<RwLock<Option<..>>> in OrchestratorState. Since
    // OrchestratorHandle already holds it behind RwLock<Option<..>>, we cannot
    // easily share it out. Instead we collect the resource arcs upfront.
    let resource_ids: Vec<_> = orchestrator.resource_ids().cloned().collect();
    let resources: Vec<_> = resource_ids
        .iter()
        .filter_map(|id| orchestrator.get_resource(id))
        .collect();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(poll_interval) => {
                    for resource_arc in &resources {
                        let mut resource = resource_arc.write().await;
                        if let Some(_exit_code) = resource.poll() {
                            if resource.should_restart() {
                                resource.schedule_restart();
                            }
                        }
                        if resource.is_restart_due() {
                            let ctx = dtx_core::resource::Context::new();
                            if let Err(e) = resource.execute_restart(&ctx).await {
                                tracing::error!(error = %e, "Restart failed during poll");
                            }
                        }
                    }
                }
            }
        }
        tracing::debug!("Polling task exited");
    })
}

/// Spawns a background task that subscribes to the event bus and logs
/// lifecycle events to the tracing output.
fn spawn_console_logger(
    event_bus: Arc<ResourceEventBus>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut subscriber = event_bus.subscribe();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                maybe = subscriber.recv() => {
                    match maybe {
                        Some(event) => log_lifecycle_event(&event),
                        None => break,
                    }
                }
            }
        }
        tracing::debug!("Console logger exited");
    })
}

fn log_lifecycle_event(event: &LifecycleEvent) {
    match event {
        LifecycleEvent::Starting { id, .. } => {
            tracing::info!(resource = %id, "Resource starting");
        }
        LifecycleEvent::Running { id, pid, .. } => {
            tracing::info!(resource = %id, pid = ?pid, "Resource running");
        }
        LifecycleEvent::Stopped { id, exit_code, .. } => {
            tracing::info!(resource = %id, exit_code = ?exit_code, "Resource stopped");
        }
        LifecycleEvent::Failed {
            id,
            error,
            exit_code,
            ..
        } => {
            tracing::error!(
                resource = %id,
                exit_code = ?exit_code,
                error = %error,
                "Resource failed"
            );
        }
        LifecycleEvent::Restarting {
            id,
            attempt,
            max_attempts,
            ..
        } => {
            let max = max_attempts
                .map(|m| m.to_string())
                .unwrap_or_else(|| "unlimited".to_string());
            tracing::warn!(
                resource = %id,
                attempt = attempt,
                max_attempts = %max,
                "Resource restarting"
            );
        }
        LifecycleEvent::Log {
            id, stream, line, ..
        } => {
            let line = line.trim_end();
            if !line.is_empty() {
                match stream {
                    LogStreamKind::Stderr => {
                        tracing::error!(resource = %id, "{}", line);
                    }
                    LogStreamKind::Stdout => {
                        tracing::info!(resource = %id, "{}", line);
                    }
                }
            }
        }
        LifecycleEvent::HealthCheckPassed { id, .. } => {
            tracing::info!(resource = %id, "Health check passed");
        }
        LifecycleEvent::HealthCheckFailed { id, reason, .. } => {
            tracing::warn!(resource = %id, reason = %reason, "Health check failed");
        }
        LifecycleEvent::Stopping { .. }
        | LifecycleEvent::DependencyWaiting { .. }
        | LifecycleEvent::DependencyResolved { .. }
        | LifecycleEvent::ConfigChanged { .. }
        | LifecycleEvent::MemoryChanged { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_to_config_basic() {
        let svc = Service::new("api".to_string(), "cargo run".to_string());
        let config = service_to_process_config(&svc, Path::new("/project"));
        assert_eq!(config.id.as_str(), "api");
        assert_eq!(config.command, "cargo run");
        assert_eq!(
            config.working_dir,
            Some(std::path::PathBuf::from("/project"))
        );
    }

    #[test]
    fn service_to_config_with_port_and_deps() {
        let mut svc = Service::new("web".to_string(), "npm start".to_string());
        svc.port = Some(3000);
        svc.depends_on = Some(vec![dtx_core::model::Dependency {
            service: "db".to_string(),
            condition: dtx_core::model::DependencyCondition::ProcessStarted,
        }]);

        let config = service_to_process_config(&svc, Path::new("/root"));
        assert_eq!(config.port, Some(3000));
        assert_eq!(config.depends_on.len(), 1);
        assert_eq!(config.depends_on[0].as_str(), "db");
    }

    #[test]
    fn service_to_config_with_working_dir() {
        let mut svc = Service::new("worker".to_string(), "python main.py".to_string());
        svc.working_dir = Some("./workers".to_string());

        let config = service_to_process_config(&svc, Path::new("/project"));
        assert_eq!(
            config.working_dir,
            Some(std::path::PathBuf::from("./workers"))
        );
    }

    #[test]
    fn service_to_config_with_environment() {
        let mut svc = Service::new("api".to_string(), "node index.js".to_string());
        let mut env = std::collections::HashMap::new();
        env.insert("NODE_ENV".to_string(), "production".to_string());
        svc.environment = Some(env);

        let config = service_to_process_config(&svc, Path::new("/project"));
        assert_eq!(
            config.environment.get("NODE_ENV"),
            Some(&"production".to_string())
        );
    }

    #[tokio::test]
    async fn handle_new_not_running() {
        let bus = Arc::new(ResourceEventBus::new());
        let handle = OrchestratorHandle::new(bus, Arc::new(WebConfig::default()));
        assert!(!handle.is_running());
    }

    #[tokio::test]
    async fn handle_stop_when_not_running() {
        let bus = Arc::new(ResourceEventBus::new());
        let handle = OrchestratorHandle::new(bus, Arc::new(WebConfig::default()));
        // Stopping when nothing is running should succeed silently
        handle.stop().await.expect("stop should not fail");
    }

    #[tokio::test]
    async fn handle_shutdown_when_not_running() {
        let bus = Arc::new(ResourceEventBus::new());
        let handle = OrchestratorHandle::new(bus, Arc::new(WebConfig::default()));
        handle.shutdown().await.expect("shutdown should not fail");
    }

    #[tokio::test]
    async fn handle_status_when_not_running() {
        let bus = Arc::new(ResourceEventBus::new());
        let handle = OrchestratorHandle::new(bus, Arc::new(WebConfig::default()));
        let status = handle.status().await.expect("status should not fail");
        assert!(status.is_none());
    }

    #[tokio::test]
    async fn handle_health_when_not_running() {
        let bus = Arc::new(ResourceEventBus::new());
        let handle = OrchestratorHandle::new(bus, Arc::new(WebConfig::default()));
        let health = handle.health().await.expect("health should not fail");
        assert!(health.is_none());
    }

    #[tokio::test]
    async fn handle_start_empty_services() {
        let bus = Arc::new(ResourceEventBus::new());
        let handle = OrchestratorHandle::new(bus, Arc::new(WebConfig::default()));
        let token = CancellationToken::new();

        let result = handle
            .start(vec![], Path::new("/tmp"), "test", &token)
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn port_reassignment_info_fields() {
        let info = PortReassignmentInfo {
            service: "api".to_string(),
            original_port: 3000,
            assigned_port: 3001,
        };
        assert_eq!(info.service, "api");
        assert_eq!(info.original_port, 3000);
        assert_eq!(info.assigned_port, 3001);
    }
}
