//! JSON API handlers.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use dtx_core::config::schema::DependencyConfig;
use dtx_core::export::{
    DockerComposeExporter, ExportFormat, ExportableProject, ExportableService, Exporter,
    KubernetesExporter, ProcessComposeExporter,
};
use dtx_core::graph::DependencyGraph;
use dtx_core::model::Service as ModelService;
use dtx_core::resource::{ResourceId, ResourceState};
use dtx_core::{GraphValidator, ServiceName};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::{AppError, AppResult};
use crate::service::ops::{CreateServiceParams, EditableFileType, UpdateServiceParams};
use crate::state::AppState;
use crate::types::ProcessResponse;

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
    let info = state.service_ops().get_project().await?;
    Ok(Json(ProjectResponse {
        name: info.name,
        description: info.description,
        path: info.path,
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
    let info = state.service_ops().get_project().await?;

    let headers = [(
        axum::http::header::HeaderName::from_static("hx-redirect"),
        axum::http::HeaderValue::from_static("/"),
    )];

    Ok((
        StatusCode::OK,
        headers,
        Json(InitCwdResponse {
            name: info.name,
            path: info.path,
        }),
    ))
}

// === Services ===

/// List all services.
pub async fn list_services(State(state): State<AppState>) -> AppResult<Json<Vec<ModelService>>> {
    let services = state.service_ops().list_services().await?;
    Ok(Json(services))
}

/// Request body for creating a service.
#[derive(Debug, Deserialize)]
pub struct CreateServiceRequest {
    pub name: ServiceName,
    pub command: String,
    pub package: Option<String>,
    pub port: Option<u16>,
    pub working_dir: Option<String>,
}

/// Create a new service.
pub async fn create_service(
    State(state): State<AppState>,
    Json(req): Json<CreateServiceRequest>,
) -> AppResult<Json<ModelService>> {
    let params = CreateServiceParams {
        name: req.name.to_string(),
        command: req.command,
        package: req.package,
        port: req.port,
        working_dir: req.working_dir,
    };
    let (service, _) = state.service_ops().create_service(params).await?;
    Ok(Json(service))
}

/// Get a service by name.
pub async fn get_service(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<Json<ModelService>> {
    let service = state.service_ops().get_service(&name).await?;
    Ok(Json(service))
}

/// Request body for updating a service.
#[derive(Debug, Deserialize)]
pub struct UpdateServiceRequest {
    pub name: Option<ServiceName>,
    pub command: Option<String>,
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
    let params = UpdateServiceParams {
        name: req.name.map(|n| n.to_string()),
        command: req.command,
        package: req.package,
        port: req.port,
        working_dir: req.working_dir,
        enabled: req.enabled,
    };
    let service = state.service_ops().update_service(&name, params).await?;
    Ok(Json(service))
}

/// Delete a service.
pub async fn delete_service(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> AppResult<StatusCode> {
    state.service_ops().delete_service(&name).await?;
    Ok(StatusCode::NO_CONTENT)
}

// === Process Control ===

/// Start services.
pub async fn start_services(State(state): State<AppState>) -> AppResult<Json<ProcessResponse>> {
    let services = state.service_ops().list_services().await?;
    let enabled: Vec<_> = services.into_iter().filter(|s| s.enabled).collect();

    let store = state.store().read().await;
    let project_root = store.project_root().to_path_buf();
    let project_name = store.project_name().to_string();
    drop(store);

    let result = state
        .orchestrator_handle()
        .start(
            enabled,
            &project_root,
            &project_name,
            state.shutdown_token(),
        )
        .await?;

    let port_reassignments = if result.port_reassignments.is_empty() {
        None
    } else {
        Some(result.port_reassignments)
    };

    let status = if result.failed.is_empty() {
        "started".to_string()
    } else {
        format!(
            "started with {} failures: {}",
            result.failed.len(),
            result.failed.join(", ")
        )
    };

    Ok(Json(ProcessResponse {
        status,
        port_reassignments,
    }))
}

/// Restart services (stop, regenerate config, start).
pub async fn restart_services(State(state): State<AppState>) -> AppResult<Json<ProcessResponse>> {
    let services = state.service_ops().list_services().await?;
    let enabled: Vec<_> = services.into_iter().filter(|s| s.enabled).collect();

    let store = state.store().read().await;
    let project_root = store.project_root().to_path_buf();
    let project_name = store.project_name().to_string();
    drop(store);

    let result = state
        .orchestrator_handle()
        .restart(
            enabled,
            &project_root,
            &project_name,
            state.shutdown_token(),
        )
        .await?;

    let port_reassignments = if result.port_reassignments.is_empty() {
        None
    } else {
        Some(result.port_reassignments)
    };

    let status = if result.failed.is_empty() {
        "started".to_string()
    } else {
        format!(
            "started with {} failures: {}",
            result.failed.len(),
            result.failed.join(", ")
        )
    };

    Ok(Json(ProcessResponse {
        status,
        port_reassignments,
    }))
}

/// Stop services.
pub async fn stop_services(State(state): State<AppState>) -> AppResult<Json<ProcessResponse>> {
    state.orchestrator_handle().stop().await?;

    Ok(Json(ProcessResponse {
        status: "stopped".to_string(),
        port_reassignments: None,
    }))
}

/// Get status of services.
pub async fn get_status(State(state): State<AppState>) -> AppResult<Json<serde_json::Value>> {
    let statuses = state.orchestrator_handle().status().await?;

    let status = match statuses {
        Some(statuses) => {
            let mut processes = Vec::new();
            for (id, resource_state) in &statuses {
                let (status_str, is_running, pid) = match resource_state {
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

            let any_running = statuses.values().any(|s| {
                matches!(
                    s,
                    ResourceState::Running { .. } | ResourceState::Starting { .. }
                )
            });

            serde_json::json!({"running": any_running, "processes": processes})
        }
        None => serde_json::json!({"running": false}),
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
    crate::config::WebConfig::default().default_search_limit
}

/// Search for Nix packages.
pub async fn search_packages(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> AppResult<Json<Vec<dtx_core::Package>>> {
    let packages = state
        .service_ops()
        .nix_search(&query.q, Some(query.limit))
        .await?;
    Ok(Json(packages))
}

/// Validate a Nix package.
pub async fn validate_package(
    State(state): State<AppState>,
    Path(package): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let valid = state.service_ops().nix_validate(&package).await?;
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
pub async fn get_nix_status(State(state): State<AppState>) -> AppResult<Json<NixStatusResponse>> {
    let nix_status = state.service_ops().nix_status().await?;
    Ok(Json(NixStatusResponse {
        has_flake: nix_status.has_flake,
        has_envrc: nix_status.has_envrc,
        packages: nix_status.packages,
    }))
}

/// Initialize Nix environment.
pub async fn nix_init(State(state): State<AppState>) -> AppResult<Json<NixInitResponse>> {
    let result = state.service_ops().nix_init().await?;
    Ok(Json(NixInitResponse {
        status: "success".to_string(),
        files: result.files,
    }))
}

/// Regenerate .envrc only.
pub async fn nix_envrc(State(state): State<AppState>) -> AppResult<Json<NixInitResponse>> {
    let result = state.service_ops().nix_envrc().await?;
    Ok(Json(NixInitResponse {
        status: "success".to_string(),
        files: result.files,
    }))
}

/// Download flake.nix.
pub async fn download_flake(State(state): State<AppState>) -> AppResult<String> {
    state.service_ops().nix_flake().await
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
    let info = state.service_ops().get_config().await?;
    Ok(Json(ProjectConfigResponse {
        config: info.config,
        path: info.path,
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
    state
        .service_ops()
        .add_mapping(&req.command, &req.package)
        .await?;

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
    state.service_ops().remove_mapping(&command).await?;

    Ok(Json(serde_json::json!({
        "status": "removed",
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
    let ops = state.service_ops();
    match req.list_type.as_str() {
        "local" => ops.mark_local(&req.command).await?,
        "ignore" => ops.mark_ignore(&req.command).await?,
        _ => {
            return Err(AppError::bad_request(
                "Invalid type, must be 'local' or 'ignore'",
            ))
        }
    };

    Ok(Json(serde_json::json!({
        "status": "success",
        "command": req.command,
        "list": req.list_type
    })))
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

/// Get package analysis for all services.
pub async fn get_package_analysis(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<ServicePackageInfo>>> {
    let analysis = state.service_ops().analyze_packages().await?;

    let result: Vec<ServicePackageInfo> = analysis
        .into_iter()
        .map(|a| ServicePackageInfo {
            service_name: a.service_name,
            command: a.command,
            status: a.status,
            package: a.package,
            executable: a.executable,
        })
        .collect();

    Ok(Json(result))
}

// === Inline File Editing ===

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
    let ft = EditableFileType::parse(&file_type)?;
    let result = state.service_ops().read_file(ft).await?;

    Ok(Json(ReadFileResponse {
        file_type,
        path: result.path,
        content: result.content,
        exists: result.exists,
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
    State(state): State<AppState>,
    Path(file_type): Path<String>,
    Json(req): Json<ValidateFileRequest>,
) -> AppResult<Json<ValidateFileResponse>> {
    let ft = EditableFileType::parse(&file_type)?;
    let result = state.service_ops().validate_file(ft, &req.content)?;

    Ok(Json(ValidateFileResponse {
        valid: result.valid,
        error: result.error,
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
    let ft = EditableFileType::parse(&file_type)?;
    let saved_path = state.service_ops().save_file(ft, &req.content).await?;

    Ok(Json(SaveFileResponse {
        status: "saved".to_string(),
        path: saved_path,
    }))
}

// === Health ===

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub resources: HashMap<String, String>,
}

pub async fn get_health(State(state): State<AppState>) -> AppResult<Json<HealthResponse>> {
    let mut resources = HashMap::new();

    if let Some(health_map) = state.orchestrator_handle().health().await? {
        for (id, status) in health_map {
            resources.insert(id.to_string(), format!("{:?}", status));
        }
    }

    Ok(Json(HealthResponse { resources }))
}

// === Export ===

#[derive(Debug, Deserialize)]
pub struct ExportParams {
    #[serde(default = "default_export_format")]
    pub format: String,
}

fn default_export_format() -> String {
    "process-compose".to_string()
}

fn build_exportable_project(services: &[ModelService], project_name: &str) -> ExportableProject {
    let exportable_services: Vec<ExportableService> = services
        .iter()
        .map(|svc| {
            let mut es = ExportableService::new(ResourceId::new(&svc.name), &svc.name)
                .with_enabled(svc.enabled);
            es = es.with_command(&svc.command);
            if let Some(ref wd) = svc.working_dir {
                es = es.with_working_dir(wd.clone());
            }
            if let Some(port) = svc.port {
                es = es.with_port(port);
            }
            if let Some(ref env) = svc.environment {
                for (k, v) in env {
                    es = es.with_env(k, v);
                }
            }
            if let Some(ref deps) = svc.depends_on {
                for dep in deps {
                    es = es.depends_on(ResourceId::new(&dep.service));
                }
            }
            es
        })
        .collect();

    ExportableProject::new(project_name).with_services(exportable_services)
}

pub async fn export_config(
    State(state): State<AppState>,
    Query(params): Query<ExportParams>,
) -> AppResult<impl IntoResponse> {
    let format: ExportFormat = params
        .format
        .parse()
        .map_err(|e: dtx_core::ExportError| AppError::bad_request(e.to_string()))?;

    let store = state.store().read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let project_name = store.project_name().to_string();

    let project = build_exportable_project(&services, &project_name);

    let content = match format {
        ExportFormat::ProcessCompose => ProcessComposeExporter::new()
            .export(&project)
            .map_err(|e| AppError::internal(e.to_string()))?,
        ExportFormat::DockerCompose => DockerComposeExporter::new()
            .export(&project)
            .map_err(|e| AppError::internal(e.to_string()))?,
        ExportFormat::Kubernetes => KubernetesExporter::new()
            .export(&project)
            .map_err(|e| AppError::internal(e.to_string()))?,
        ExportFormat::Dtx => {
            let config_path = store.project_root().join(".dtx").join("config.yaml");
            drop(store);
            tokio::fs::read_to_string(&config_path)
                .await
                .map_err(|e| AppError::internal(format!("Failed to read config.yaml: {}", e)))?
        }
    };

    let content_type = "text/yaml; charset=utf-8";
    let filename = format.default_filename();

    Ok((
        StatusCode::OK,
        [
            (axum::http::header::CONTENT_TYPE, content_type.to_string()),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{}\"", filename),
            ),
        ],
        content,
    ))
}

// === Import ===

#[derive(Debug, Deserialize)]
pub struct ImportRequest {
    pub content: String,
    #[serde(default = "default_import_format")]
    pub format: String,
}

fn default_import_format() -> String {
    "auto".to_string()
}

#[derive(Debug, Serialize)]
pub struct ImportResponse {
    pub imported: usize,
    pub warnings: Vec<String>,
    pub services: Vec<String>,
}

pub async fn import_config(
    State(state): State<AppState>,
    Json(req): Json<ImportRequest>,
) -> AppResult<Json<ImportResponse>> {
    let result = state
        .service_ops()
        .import_config(&req.content, &req.format)
        .await?;

    Ok(Json(ImportResponse {
        imported: result.imported,
        warnings: result.warnings,
        services: result.service_names,
    }))
}

// === Dependency Graph ===

#[derive(Debug, Serialize)]
pub struct GraphNodeResponse {
    pub id: String,
    pub depth: usize,
    pub dependencies: Vec<String>,
    pub dependents: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct GraphEdgeResponse {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Serialize)]
pub struct GraphResponse {
    pub nodes: Vec<GraphNodeResponse>,
    pub edges: Vec<GraphEdgeResponse>,
    pub roots: Vec<String>,
    pub max_depth: usize,
}

pub async fn get_dependency_graph(State(state): State<AppState>) -> AppResult<Json<GraphResponse>> {
    let store = state.store().read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let graph = DependencyGraph::from_services(&services);

    let nodes: Vec<GraphNodeResponse> = graph
        .nodes
        .values()
        .map(|node| GraphNodeResponse {
            id: node.name.clone(),
            depth: node.depth,
            dependencies: node.dependencies.clone(),
            dependents: node.dependents.clone(),
        })
        .collect();

    let mut edges = Vec::new();
    for node in graph.nodes.values() {
        for dep in &node.dependencies {
            edges.push(GraphEdgeResponse {
                source: node.name.clone(),
                target: dep.clone(),
            });
        }
    }

    Ok(Json(GraphResponse {
        nodes,
        edges,
        roots: graph.roots,
        max_depth: graph.max_depth,
    }))
}

// === Update Dependencies ===

#[derive(Debug, Deserialize)]
pub struct UpdateDepsRequest {
    pub depends_on: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateDepsResponse {
    pub status: String,
    pub service: String,
    pub depends_on: Vec<String>,
}

pub async fn update_dependencies(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateDepsRequest>,
) -> AppResult<Json<UpdateDepsResponse>> {
    let mut store = state.store().write().await;

    if store.get_resource(&name).is_none() {
        return Err(AppError::not_found(format!("Service '{}' not found", name)));
    }

    let mut services = dtx_core::model::services_from_config(store.config());
    if let Some(svc) = services.iter_mut().find(|s| s.name == name) {
        svc.depends_on = if req.depends_on.is_empty() {
            None
        } else {
            Some(
                req.depends_on
                    .iter()
                    .map(|d| dtx_core::model::Dependency {
                        service: d.clone(),
                        condition: dtx_core::model::DependencyCondition::ProcessStarted,
                    })
                    .collect(),
            )
        };
    }

    if let Err(errors) = GraphValidator::validate_all(&services) {
        return Err(AppError::bad_request(format!(
            "Dependency validation failed: {}",
            errors.join("; ")
        )));
    }

    let rc = store
        .get_resource_mut(&name)
        .ok_or_else(|| AppError::not_found(format!("Service '{}' not found", name)))?;

    rc.depends_on = req
        .depends_on
        .iter()
        .map(|d| DependencyConfig::Simple(d.clone()))
        .collect();

    store.save().map_err(AppError::from)?;

    tracing::info!(service = %name, deps = ?req.depends_on, "Updated service dependencies");

    Ok(Json(UpdateDepsResponse {
        status: "updated".to_string(),
        service: name,
        depends_on: req.depends_on,
    }))
}
