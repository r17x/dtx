//! JSON API handlers.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use dtx_core::config::schema::ResourceConfig;
use dtx_core::events::{LifecycleEvent, ResourceEventBus};
use dtx_core::model::Service as ModelService;
use dtx_core::process::{analyze_services, run_preflight};
use dtx_core::resource::{LogStreamKind, Resource, ResourceState};
use dtx_core::{GraphValidator, ServiceName, ShellCommand};
use dtx_process::{ProcessResourceConfig, ResourceOrchestrator};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

// === Project ===

/// Response for project metadata.
#[derive(Debug, Serialize)]
pub struct ProjectResponse {
    pub name: String,
    pub description: Option<String>,
    pub path: String,
}

/// Get project metadata.
pub async fn get_project(State(state): State<AppState>) -> AppResult<Json<ProjectResponse>> {
    let store = state.store.read().await;
    Ok(Json(ProjectResponse {
        name: store.project_name().to_string(),
        description: store.project_description().map(|s| s.to_string()),
        path: store.project_root().display().to_string(),
    }))
}

/// Response for init-cwd endpoint.
#[derive(Debug, Serialize)]
pub struct InitCwdResponse {
    pub name: String,
    pub path: String,
}

/// Initialize a project from current working directory.
pub async fn init_cwd(
    State(state): State<AppState>,
) -> AppResult<(
    StatusCode,
    [(axum::http::HeaderName, axum::http::HeaderValue); 1],
    Json<InitCwdResponse>,
)> {
    let store = state.store.read().await;
    let name = store.project_name().to_string();
    let path = store.project_root().display().to_string();

    let headers = [(
        axum::http::header::HeaderName::from_static("hx-redirect"),
        axum::http::HeaderValue::from_static("/"),
    )];

    Ok((StatusCode::OK, headers, Json(InitCwdResponse { name, path })))
}

// === Services ===

/// List all services.
pub async fn list_services(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<ModelService>>> {
    let store = state.store.read().await;
    let services = dtx_core::model::services_from_config(store.config());
    Ok(Json(services))
}

/// Request body for creating a service.
#[derive(Debug, Deserialize)]
pub struct CreateServiceRequest {
    pub name: ServiceName,
    pub command: ShellCommand,
    pub package: Option<String>,
    pub port: Option<u16>,
    pub working_dir: Option<String>,
}

/// Create a new service.
pub async fn create_service(
    State(state): State<AppState>,
    Json(req): Json<CreateServiceRequest>,
) -> AppResult<Json<ModelService>> {
    // Validate package if provided
    if let Some(ref pkg) = req.package {
        if !state
            .nix_client
            .validate(pkg)
            .await
            .map_err(AppError::from)?
        {
            return Err(AppError::bad_request(format!(
                "Package '{}' not found",
                pkg
            )));
        }
    }

    let name = req.name.to_string();

    let mut rc = ResourceConfig::default();
    rc.command = Some(req.command.to_string());
    rc.port = req.port;
    rc.working_dir = req.working_dir.map(PathBuf::from);
    if let Some(ref pkg) = req.package {
        rc.nix = Some(dtx_core::config::schema::NixConfig {
            packages: vec![pkg.clone()],
            ..Default::default()
        });
    }

    let mut store = state.store.write().await;
    store.add_resource(&name, rc.clone()).map_err(AppError::from)?;
    store.save().map_err(AppError::from)?;

    // Sync flake.nix if service has a package
    if let Some(ref pkg) = req.package {
        let project_root = store.project_root().to_path_buf();
        let project_name = store.project_name().to_string();
        if let Err(e) = dtx_core::sync_add_package(&project_root, &project_name, pkg) {
            tracing::warn!("Failed to sync flake.nix: {}", e);
        }
    }

    let service = ModelService::from_resource_config(&name, &rc);
    Ok(Json(service))
}

/// Get a service by name.
pub async fn get_service(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<Json<ModelService>> {
    let store = state.store.read().await;
    let rc = store
        .get_resource(&name)
        .ok_or_else(|| AppError::not_found(format!("Service '{}' not found", name)))?;
    let service = ModelService::from_resource_config(&name, rc);
    Ok(Json(service))
}

/// Request body for updating a service.
#[derive(Debug, Deserialize)]
pub struct UpdateServiceRequest {
    pub name: Option<ServiceName>,
    pub command: Option<ShellCommand>,
    pub package: Option<String>,
    pub port: Option<u16>,
    pub working_dir: Option<String>,
    pub enabled: Option<bool>,
}

/// Update a service.
pub async fn update_service(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateServiceRequest>,
) -> AppResult<Json<ModelService>> {
    // Validate package if changing
    if let Some(ref pkg) = req.package {
        if !state
            .nix_client
            .validate(pkg)
            .await
            .map_err(AppError::from)?
        {
            return Err(AppError::bad_request(format!(
                "Package '{}' not found",
                pkg
            )));
        }
    }

    let mut store = state.store.write().await;

    let rc = store
        .get_resource_mut(&name)
        .ok_or_else(|| AppError::not_found(format!("Service '{}' not found", name)))?;

    let old_package = rc
        .nix
        .as_ref()
        .and_then(|n| n.packages.first().cloned());

    if let Some(ref cmd) = req.command {
        rc.command = Some(cmd.to_string());
    }
    if let Some(port) = req.port {
        rc.port = Some(port);
    }
    if let Some(ref wd) = req.working_dir {
        rc.working_dir = Some(PathBuf::from(wd));
    }
    if let Some(enabled) = req.enabled {
        rc.enabled = enabled;
    }
    if let Some(ref pkg) = req.package {
        rc.nix = Some(dtx_core::config::schema::NixConfig {
            packages: vec![pkg.clone()],
            ..Default::default()
        });
    }

    let updated_rc = rc.clone();
    let final_name = if let Some(ref new_name) = req.name {
        let new_name_str = new_name.to_string();
        if new_name_str != name {
            let rc_clone = store.remove_resource(&name).map_err(AppError::from)?;
            drop(rc_clone);
            store
                .add_resource(&new_name_str, updated_rc.clone())
                .map_err(AppError::from)?;
        }
        new_name_str
    } else {
        name.clone()
    };

    store.save().map_err(AppError::from)?;

    // Sync flake.nix if package changed
    let new_package = updated_rc
        .nix
        .as_ref()
        .and_then(|n| n.packages.first().cloned());
    if old_package.as_deref() != new_package.as_deref() {
        let project_root = store.project_root().to_path_buf();
        let project_name = store.project_name().to_string();

        if let Some(ref pkg) = new_package {
            if let Err(e) = dtx_core::sync_add_package(&project_root, &project_name, pkg) {
                tracing::warn!("Failed to sync flake.nix (add): {}", e);
            }
        }

        if let Some(ref pkg) = old_package {
            let remaining_packages: HashSet<String> = store
                .list_resources()
                .filter_map(|(_, rc)| {
                    rc.nix.as_ref().and_then(|n| n.packages.first().cloned())
                })
                .collect();
            if let Err(e) =
                dtx_core::sync_remove_package(&project_root, pkg, &remaining_packages)
            {
                tracing::warn!("Failed to sync flake.nix (remove): {}", e);
            }
        }
    }

    let service = ModelService::from_resource_config(&final_name, &updated_rc);
    Ok(Json(service))
}

/// Delete a service.
pub async fn delete_service(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<StatusCode> {
    let mut store = state.store.write().await;

    let removed_rc = store.remove_resource(&name).map_err(AppError::from)?;
    store.save().map_err(AppError::from)?;

    // Sync flake.nix if the removed service had a package
    let removed_package = removed_rc
        .nix
        .as_ref()
        .and_then(|n| n.packages.first().cloned());
    if let Some(ref pkg) = removed_package {
        let project_root = store.project_root().to_path_buf();
        let remaining_packages: HashSet<String> = store
            .list_resources()
            .filter_map(|(_, rc)| {
                rc.nix.as_ref().and_then(|n| n.packages.first().cloned())
            })
            .collect();
        if let Err(e) =
            dtx_core::sync_remove_package(&project_root, pkg, &remaining_packages)
        {
            tracing::warn!("Failed to sync flake.nix: {}", e);
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

// === Process Control ===

/// Response for process control operations.
#[derive(Serialize)]
pub struct ProcessResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_reassignments: Option<Vec<PortReassignmentInfo>>,
}

/// Information about a port reassignment.
#[derive(Serialize)]
pub struct PortReassignmentInfo {
    pub service: String,
    pub original_port: u16,
    pub assigned_port: u16,
}

/// Convert a ModelService to a ProcessResourceConfig.
fn service_to_process_config(
    service: &ModelService,
    project_root: &std::path::Path,
) -> ProcessResourceConfig {
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

/// Spawns a background task that polls the orchestrator for log collection.
fn spawn_polling_task(
    orchestrator: std::sync::Arc<tokio::sync::RwLock<Option<ResourceOrchestrator>>>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    tokio::spawn(async move {
        let poll_interval = std::time::Duration::from_millis(100);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = tokio::time::sleep(poll_interval) => {
                    let mut orch = orchestrator.write().await;
                    match &mut *orch {
                        Some(o) if o.is_running() => o.poll().await,
                        _ => break,
                    }
                }
            }
        }
        tracing::debug!("Polling task exited");
    });
}

/// Spawns a console logging task that subscribes to ResourceEventBus and logs lifecycle events.
fn spawn_console_logger(
    event_bus: std::sync::Arc<ResourceEventBus>,
    running_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    use std::sync::atomic::Ordering;

    if running_flag.swap(true, Ordering::SeqCst) {
        return;
    }

    let flag = running_flag.clone();
    tokio::spawn(async move {
        let mut subscriber = event_bus.subscribe();

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                maybe = subscriber.recv() => {
                    match maybe {
                        Some(event) => match &event {
                            LifecycleEvent::Starting { id, .. } => {
                                tracing::info!(resource = %id, "Resource starting");
                            }
                            LifecycleEvent::Running { id, pid, .. } => {
                                tracing::info!(resource = %id, pid = ?pid, "Resource running");
                            }
                            LifecycleEvent::Stopped { id, exit_code, .. } => {
                                tracing::info!(
                                    resource = %id,
                                    exit_code = ?exit_code,
                                    "Resource stopped"
                                );
                            }
                            LifecycleEvent::Failed { id, error, exit_code, .. } => {
                                tracing::error!(
                                    resource = %id,
                                    exit_code = ?exit_code,
                                    error = %error,
                                    "Resource failed"
                                );
                            }
                            LifecycleEvent::Restarting { id, attempt, max_attempts, .. } => {
                                let max = max_attempts.map(|m| m.to_string()).unwrap_or_else(|| "unlimited".to_string());
                                tracing::warn!(
                                    resource = %id,
                                    attempt = attempt,
                                    max_attempts = %max,
                                    "Resource restarting"
                                );
                            }
                            LifecycleEvent::Log { id, stream, line, .. } => {
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
                                tracing::warn!(
                                    resource = %id,
                                    reason = %reason,
                                    "Health check failed"
                                );
                            }
                            LifecycleEvent::Stopping { .. }
                            | LifecycleEvent::DependencyWaiting { .. }
                            | LifecycleEvent::DependencyResolved { .. }
                            | LifecycleEvent::ConfigChanged { .. } => {}
                        },
                        None => break,
                    }
                }
            }
        }
        flag.store(false, Ordering::SeqCst);
        tracing::debug!("Console logger exited");
    });
}

/// Internal helper: read services from store, create Orchestrator, start services.
async fn do_start_services(state: &AppState) -> AppResult<Json<ProcessResponse>> {
    let store = state.store.read().await;

    let services = dtx_core::model::enabled_services_from_config(store.config());

    if services.is_empty() {
        return Err(AppError::bad_request("No enabled services to start"));
    }

    // Validate dependency graph before starting
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

    // Determine flake directory
    let project_root = store.project_root().to_path_buf();
    let project_name = store.project_name().to_string();
    let dtx_dir = project_root.join(".dtx");
    let flake_path = project_root.join("flake.nix");
    let dtx_flake_path = dtx_dir.join("flake.nix");

    let _flake_dir = if flake_path.exists() {
        tracing::info!(path = %flake_path.display(), "Using existing flake.nix");
        Some(project_root.clone())
    } else {
        let flake_content = dtx_core::FlakeGenerator::generate(&services, &project_name);
        std::fs::create_dir_all(&dtx_dir)
            .map_err(|e| AppError::internal(format!("Failed to create .dtx dir: {}", e)))?;
        std::fs::write(&dtx_flake_path, &flake_content)
            .map_err(|e| AppError::internal(format!("Failed to write flake.nix: {}", e)))?;
        tracing::info!(path = %dtx_flake_path.display(), "Generated flake.nix");
        Some(dtx_dir.clone())
    };

    // Create ResourceOrchestrator
    let mut orchestrator = ResourceOrchestrator::new(state.event_bus.clone());

    for svc in &services {
        let config = service_to_process_config(svc, &project_root);
        orchestrator.add_resource(config);
    }

    let result = orchestrator.start_all().await.map_err(AppError::internal)?;

    for (id, error) in &result.failed {
        tracing::error!(resource = %id, error = %error, "Failed to start resource");
    }

    // Store orchestrator in state
    {
        let mut orch = state.orchestrator.write().await;
        *orch = Some(orchestrator);
    }

    spawn_polling_task(state.orchestrator.clone(), state.shutdown_token.clone());
    spawn_console_logger(
        state.event_bus.clone(),
        state.console_logger_running.clone(),
        state.shutdown_token.clone(),
    );

    let port_reassignments = if reassignments.is_empty() {
        None
    } else {
        Some(
            reassignments
                .into_iter()
                .map(|r| PortReassignmentInfo {
                    service: r.service_name,
                    original_port: r.original_port,
                    assigned_port: r.new_port,
                })
                .collect(),
        )
    };

    let status = if result.failed.is_empty() {
        "started".to_string()
    } else {
        format!(
            "started with {} failures: {}",
            result.failed.len(),
            result
                .failed
                .iter()
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };

    Ok(Json(ProcessResponse {
        status,
        port_reassignments,
    }))
}

/// Start services.
pub async fn start_services(
    State(state): State<AppState>,
) -> AppResult<Json<ProcessResponse>> {
    // Check if already running
    {
        let orch = state.orchestrator.read().await;
        if let Some(ref orchestrator) = *orch {
            if orchestrator.is_running() {
                return Err(AppError::conflict("Services already running"));
            }
        }
    }

    do_start_services(&state).await
}

/// Restart services (stop, regenerate config, start).
pub async fn restart_services(
    State(state): State<AppState>,
) -> AppResult<Json<ProcessResponse>> {
    // Stop existing orchestrator if running
    {
        let mut orch = state.orchestrator.write().await;
        if let Some(ref mut orchestrator) = *orch {
            tracing::info!("Stopping orchestrator for restart");
            if let Err(e) = orchestrator.stop_all().await {
                tracing::warn!(?e, "Error stopping orchestrator during restart, continuing");
            }
        }
        *orch = None;
    }

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    do_start_services(&state).await
}

/// Stop services.
pub async fn stop_services(
    State(state): State<AppState>,
) -> AppResult<Json<ProcessResponse>> {
    let mut orch = state.orchestrator.write().await;

    if let Some(ref mut orchestrator) = *orch {
        orchestrator
            .stop_all()
            .await
            .map_err(|e| AppError::bad_request(e.to_string()))?;
    }
    *orch = None;

    Ok(Json(ProcessResponse {
        status: "stopped".to_string(),
        port_reassignments: None,
    }))
}

/// Get status of services.
pub async fn get_status(
    State(state): State<AppState>,
) -> AppResult<Json<serde_json::Value>> {
    let orch = state.orchestrator.read().await;

    let status = if let Some(ref orchestrator) = *orch {
        if orchestrator.is_running() {
            let mut processes = Vec::new();
            for id in orchestrator.resource_ids() {
                if let Some(resource) = orchestrator.get_resource(id) {
                    let proc = resource.read().await;
                    let state = proc.state();
                    let (status_str, is_running, pid) = match state {
                        ResourceState::Pending => ("Pending".to_string(), false, 0),
                        ResourceState::Starting { .. } => ("Starting".to_string(), false, 0),
                        ResourceState::Running { pid, .. } => {
                            ("Running".to_string(), true, pid.unwrap_or(0))
                        }
                        ResourceState::Stopping { .. } => ("Stopping".to_string(), true, 0),
                        ResourceState::Stopped { exit_code, .. } => {
                            let msg = exit_code
                                .map(|c| format!("Completed ({})", c))
                                .unwrap_or_else(|| "Stopped".to_string());
                            (msg, false, 0)
                        }
                        ResourceState::Failed {
                            exit_code, error, ..
                        } => {
                            let msg = exit_code
                                .map(|c| format!("exit {}", c))
                                .unwrap_or_else(|| error.clone());
                            (format!("Failed: {}", msg), false, 0)
                        }
                    };
                    processes.push(serde_json::json!({
                        "name": id.to_string(),
                        "status": status_str,
                        "is_running": is_running,
                        "pid": pid,
                    }));
                }
            }
            serde_json::json!({"running": true, "processes": processes})
        } else {
            serde_json::json!({"running": false})
        }
    } else {
        serde_json::json!({"running": false})
    };

    Ok(Json(status))
}

// === Nix ===

/// Query parameters for package search.
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

/// Search for Nix packages.
pub async fn search_packages(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Vec<dtx_core::Package>>> {
    let mut packages = state
        .nix_client
        .search(&query.q)
        .await
        .map_err(AppError::from)?;

    packages.truncate(query.limit);

    Ok(Json(packages))
}

/// Validate a Nix package.
pub async fn validate_package(
    State(state): State<AppState>,
    Path(package): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let valid = state
        .nix_client
        .validate(&package)
        .await
        .map_err(AppError::from)?;

    Ok(Json(
        serde_json::json!({"valid": valid, "package": package}),
    ))
}

// === Nix Environment ===

/// Response for Nix status endpoint.
#[derive(Debug, Serialize)]
pub struct NixStatusResponse {
    pub has_flake: bool,
    pub has_envrc: bool,
    pub packages: Vec<String>,
}

/// Response for Nix init endpoint.
#[derive(Debug, Serialize)]
pub struct NixInitResponse {
    pub status: String,
    pub files: Vec<String>,
}

/// Get Nix environment status.
pub async fn get_nix_status(
    State(state): State<AppState>,
) -> AppResult<Json<NixStatusResponse>> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let has_flake = project_root.join("flake.nix").exists();
    let has_envrc = project_root.join(".envrc").exists();

    let packages: Vec<String> = store
        .list_enabled_resources()
        .filter_map(|(_, rc)| {
            rc.nix.as_ref().and_then(|n| n.packages.first().cloned())
        })
        .collect();

    Ok(Json(NixStatusResponse {
        has_flake,
        has_envrc,
        packages,
    }))
}

/// Initialize Nix environment.
pub async fn nix_init(
    State(state): State<AppState>,
) -> AppResult<Json<NixInitResponse>> {
    let store = state.store.read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let project_root = store.project_root().to_path_buf();
    let project_name = store.project_name().to_string();

    let flake = dtx_core::FlakeGenerator::generate(&services, &project_name);
    let envrc = dtx_core::EnvrcGenerator::generate_with_layout(&services);

    std::fs::write(project_root.join("flake.nix"), &flake)
        .map_err(|e| AppError::bad_request(format!("Failed to write flake.nix: {}", e)))?;
    std::fs::write(project_root.join(".envrc"), &envrc)
        .map_err(|e| AppError::bad_request(format!("Failed to write .envrc: {}", e)))?;

    Ok(Json(NixInitResponse {
        status: "success".to_string(),
        files: vec!["flake.nix".to_string(), ".envrc".to_string()],
    }))
}

/// Regenerate .envrc only.
pub async fn nix_envrc(
    State(state): State<AppState>,
) -> AppResult<Json<NixInitResponse>> {
    let store = state.store.read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let project_root = store.project_root().to_path_buf();

    let envrc = dtx_core::EnvrcGenerator::generate_with_layout(&services);

    std::fs::write(project_root.join(".envrc"), &envrc)
        .map_err(|e| AppError::bad_request(format!("Failed to write .envrc: {}", e)))?;

    Ok(Json(NixInitResponse {
        status: "success".to_string(),
        files: vec![".envrc".to_string()],
    }))
}

/// Download flake.nix.
pub async fn download_flake(
    State(state): State<AppState>,
) -> AppResult<String> {
    let store = state.store.read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let project_name = store.project_name().to_string();

    let flake = dtx_core::FlakeGenerator::generate(&services, &project_name);
    Ok(flake)
}

// === Project Config ===

/// Response for project config.
#[derive(Debug, Serialize)]
pub struct ProjectConfigResponse {
    pub config: dtx_core::ProjectConfig,
    pub path: String,
}

/// Get project configuration.
pub async fn get_project_config(
    State(state): State<AppState>,
) -> AppResult<Json<ProjectConfigResponse>> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let config = dtx_core::ProjectConfig::load(&project_root)
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    let config_path = dtx_core::ProjectConfig::config_path(&project_root);

    Ok(Json(ProjectConfigResponse {
        config,
        path: config_path.to_string_lossy().to_string(),
    }))
}

/// Request to add a package mapping.
#[derive(Debug, Deserialize)]
pub struct AddMappingRequest {
    pub command: String,
    pub package: String,
}

/// Add a package mapping to project config.
pub async fn add_mapping(
    State(state): State<AppState>,
    Json(req): Json<AddMappingRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let mut config = dtx_core::ProjectConfig::load(&project_root)
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    config.add_mapping(&req.command, &req.package);

    config
        .save(&project_root)
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    tracing::info!(
        command = %req.command,
        package = %req.package,
        "Added package mapping"
    );

    Ok(Json(serde_json::json!({
        "status": "success",
        "command": req.command,
        "package": req.package
    })))
}

/// Remove a package mapping from project config.
pub async fn remove_mapping(
    State(state): State<AppState>,
    Path(command): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let mut config = dtx_core::ProjectConfig::load(&project_root)
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    let removed = config.remove_mapping(&command);

    if removed.is_some() {
        config
            .save(&project_root)
            .map_err(|e| AppError::bad_request(e.to_string()))?;

        tracing::info!(command = %command, "Removed package mapping");
    }

    Ok(Json(serde_json::json!({
        "status": if removed.is_some() { "removed" } else { "not_found" },
        "command": command
    })))
}

/// Request to add command to local/ignore list.
#[derive(Debug, Deserialize)]
pub struct AddCommandListRequest {
    pub command: String,
    #[serde(rename = "type")]
    pub list_type: String,
}

/// Add a command to local or ignore list.
pub async fn add_to_command_list(
    State(state): State<AppState>,
    Json(req): Json<AddCommandListRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let mut config = dtx_core::ProjectConfig::load(&project_root)
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    match req.list_type.as_str() {
        "local" => config.add_local(&req.command),
        "ignore" => config.add_ignore(&req.command),
        _ => {
            return Err(AppError::bad_request(
                "Invalid type, must be 'local' or 'ignore'",
            ))
        }
    }

    config
        .save(&project_root)
        .map_err(|e| AppError::bad_request(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "status": "success",
        "command": req.command,
        "list": req.list_type
    })))
}

/// Get package analysis for all services.
pub async fn get_package_analysis(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<ServicePackageInfo>>> {
    let store = state.store.read().await;
    let services = dtx_core::model::services_from_config(store.config());

    let analysis = dtx_core::analyze_service_packages(&services);

    let result: Vec<ServicePackageInfo> = analysis
        .into_iter()
        .map(|a| {
            let (status, package, executable) = match a.result {
                dtx_core::PackageAnalysisResult::Explicit(p) => {
                    ("explicit".to_string(), Some(p), None)
                }
                dtx_core::PackageAnalysisResult::AutoDetected(p) => {
                    ("auto".to_string(), Some(p), None)
                }
                dtx_core::PackageAnalysisResult::LocalBinary => ("local".to_string(), None, None),
                dtx_core::PackageAnalysisResult::NeedsAttention(e) => {
                    ("needs_attention".to_string(), None, Some(e))
                }
            };
            ServicePackageInfo {
                service_name: a.service_name,
                command: a.command,
                status,
                package,
                executable,
            }
        })
        .collect();

    Ok(Json(result))
}

/// Package analysis info for a service.
#[derive(Debug, Serialize)]
pub struct ServicePackageInfo {
    pub service_name: String,
    pub command: String,
    pub status: String,
    pub package: Option<String>,
    pub executable: Option<String>,
}

// === Inline File Editing ===

/// Resolves a file type to its path relative to the project.
fn resolve_file_path(
    project_path: &std::path::Path,
    file_type: &str,
) -> Option<std::path::PathBuf> {
    match file_type {
        "config" => Some(project_path.join(".dtx").join("config.yaml")),
        "flake" => Some(project_path.join("flake.nix")),
        "mappings" => Some(project_path.join(".dtx").join("mappings.toml")),
        _ => None,
    }
}

/// Response for reading a file.
#[derive(Debug, Serialize)]
pub struct ReadFileResponse {
    pub file_type: String,
    pub path: String,
    pub content: String,
    pub exists: bool,
}

/// Read a project file (config.yaml, flake.nix, or mappings.toml).
pub async fn read_file(
    State(state): State<AppState>,
    Path(file_type): Path<String>,
) -> AppResult<Json<ReadFileResponse>> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();
    let project_name = store.project_name().to_string();

    let file_path = resolve_file_path(&project_root, &file_type)
        .ok_or_else(|| AppError::bad_request(format!("Unknown file type: {}", file_type)))?;

    let (content, exists) = if file_path.exists() {
        let content = std::fs::read_to_string(&file_path)
            .map_err(|e| AppError::internal(format!("Failed to read file: {}", e)))?;
        (content, true)
    } else {
        let example = match file_type.as_str() {
            "config" => dtx_core::ProjectConfig::example(),
            "mappings" => dtx_core::MappingsConfig::example(),
            "flake" => {
                let services = dtx_core::model::services_from_config(store.config());
                dtx_core::FlakeGenerator::generate(&services, &project_name)
            }
            _ => String::new(),
        };
        (example, false)
    };

    Ok(Json(ReadFileResponse {
        file_type,
        path: file_path.to_string_lossy().to_string(),
        content,
        exists,
    }))
}

/// Request body for validating file content.
#[derive(Debug, Deserialize)]
pub struct ValidateFileRequest {
    pub content: String,
}

/// Response for file validation.
#[derive(Debug, Serialize)]
pub struct ValidateFileResponse {
    pub valid: bool,
    pub error: Option<String>,
}

/// Validate file content without saving.
pub async fn validate_file(
    State(_state): State<AppState>,
    Path(file_type): Path<String>,
    Json(req): Json<ValidateFileRequest>,
) -> AppResult<Json<ValidateFileResponse>> {
    let result = match file_type.as_str() {
        "config" => dtx_core::ProjectConfig::parse(&req.content).map(|_| ()),
        "mappings" => dtx_core::MappingsConfig::parse(&req.content).map(|_| ()),
        "flake" => dtx_core::nix::ast::validate_flake_nix(&req.content),
        _ => {
            return Err(AppError::bad_request(format!(
                "Unknown file type: {}",
                file_type
            )))
        }
    };

    Ok(Json(ValidateFileResponse {
        valid: result.is_ok(),
        error: result.err(),
    }))
}

/// Request body for saving file content.
#[derive(Debug, Deserialize)]
pub struct SaveFileRequest {
    pub content: String,
}

/// Response for saving a file.
#[derive(Debug, Serialize)]
pub struct SaveFileResponse {
    pub status: String,
    pub path: String,
}

/// Save file content after server-side re-validation.
pub async fn save_file(
    State(state): State<AppState>,
    Path(file_type): Path<String>,
    Json(req): Json<SaveFileRequest>,
) -> AppResult<Json<SaveFileResponse>> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let file_path = resolve_file_path(&project_root, &file_type)
        .ok_or_else(|| AppError::bad_request(format!("Unknown file type: {}", file_type)))?;

    // Re-validate server-side before writing
    let validation_error = match file_type.as_str() {
        "config" => dtx_core::ProjectConfig::parse(&req.content).err(),
        "mappings" => dtx_core::MappingsConfig::parse(&req.content).err(),
        "flake" => dtx_core::nix::ast::parse_nix(&req.content)
            .err()
            .map(|e| e.to_string()),
        _ => {
            return Err(AppError::bad_request(format!(
                "Unknown file type: {}",
                file_type
            )))
        }
    };

    if let Some(err) = validation_error {
        return Err(AppError::bad_request(format!("Validation failed: {}", err)));
    }

    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| AppError::internal(format!("Failed to create directory: {}", e)))?;
    }

    std::fs::write(&file_path, &req.content)
        .map_err(|e| AppError::internal(format!("Failed to write file: {}", e)))?;

    tracing::info!(
        file_type = %file_type,
        path = %file_path.display(),
        "Saved file via inline editor"
    );

    // Publish config changed event
    let project_name = store.project_name().to_string();
    state.event_bus.publish(LifecycleEvent::ConfigChanged {
        project_id: project_name,
        timestamp: chrono::Utc::now(),
    });

    Ok(Json(SaveFileResponse {
        status: "saved".to_string(),
        path: file_path.to_string_lossy().to_string(),
    }))
}
