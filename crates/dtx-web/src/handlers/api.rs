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
use dtx_code::EntryKind;
use dtx_core::graph::{DependencyGraph, GraphNode, GraphSources, MemorySource, SymbolSource};
use dtx_core::model::Service as ModelService;
use dtx_core::resource::{ResourceId, ResourceState};
use dtx_core::{
    EdgeConfidence, EdgeKind, FileSource, GraphEdge, GraphStats, GraphValidator, GraphView,
    ImpactSet, NodeDomain, NodeMetadata, ServiceName,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::service::ops::{CreateServiceParams, EditableFileType, UpdateServiceParams};
use crate::state::AppState;
use crate::types::ProcessResponse;

/// Build a multi-domain graph from all available sources.
pub(crate) fn build_graph(state: &AppState, services: &[ModelService]) -> DependencyGraph {
    let symbols = collect_symbols(state);
    let memories = collect_memories(state);
    let files = collect_files(state);

    DependencyGraph::build(GraphSources {
        services,
        symbols,
        memories,
        files,
    })
}

pub(crate) fn collect_symbols(state: &AppState) -> Vec<SymbolSource> {
    let idx = state.workspace_index();
    let files = idx.list_files();
    let mut symbols = Vec::new();
    for file in &files {
        if let Ok(file_symbols) = idx.symbols_in_file(file) {
            let file_str = file.to_string_lossy().to_string();
            for sym in file_symbols {
                symbols.push(SymbolSource {
                    name: sym.name_path.clone(),
                    kind: format!("{:?}", sym.kind).to_lowercase(),
                    file: file_str.clone(),
                    line: sym.start_line,
                });
            }
        }
    }
    symbols
}

pub(crate) fn collect_memories(state: &AppState) -> Vec<MemorySource> {
    let Some(store) = state.memory_store() else {
        return Vec::new();
    };
    let Ok(metas) = store.list() else {
        return Vec::new();
    };
    metas
        .into_iter()
        .filter_map(|meta| {
            let content_preview = store
                .read(&meta.name)
                .ok()
                .map(|m| m.content.chars().take(200).collect::<String>())
                .unwrap_or_default();
            Some(MemorySource {
                name: meta.name,
                kind: format!("{:?}", meta.kind).to_lowercase(),
                tags: meta.tags,
                content_preview,
            })
        })
        .collect()
}

pub(crate) fn collect_files(state: &AppState) -> Vec<FileSource> {
    let idx = state.workspace_index();
    let Ok(entries) = idx.list_dir_with_depth(std::path::Path::new("."), true, Some(2)) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .map(|entry| {
            let kind = match entry.entry_type {
                EntryKind::Dir => "directory",
                EntryKind::File => "file",
            };
            let extension = std::path::Path::new(&entry.name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|s| s.to_string());
            FileSource {
                path: entry.name,
                kind: kind.to_string(),
                extension,
                size: entry.size,
            }
        })
        .collect()
}

/// Aggregate large graphs by collapsing symbols into per-file group nodes.
fn aggregate_graph(mut graph: DependencyGraph, threshold: usize) -> DependencyGraph {
    let symbol_count = graph
        .nodes
        .values()
        .filter(|n| n.domain == NodeDomain::Symbol)
        .count();

    if symbol_count <= threshold {
        return graph;
    }

    // Group symbol node IDs by their file path
    let mut file_groups: HashMap<String, Vec<String>> = HashMap::new();
    for node in graph.nodes.values() {
        if let NodeMetadata::Symbol { ref file, .. } = node.metadata {
            file_groups
                .entry(file.clone())
                .or_default()
                .push(node.id.clone());
        }
    }

    // For each file group, replace individual symbols with a single group node
    let mut nodes_to_remove: Vec<String> = Vec::new();
    let mut group_nodes: Vec<GraphNode> = Vec::new();

    for (file, symbol_ids) in &file_groups {
        let group_id = format!("group:symbol:{}", file);
        let label = std::path::Path::new(file)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(file)
            .to_string();

        group_nodes.push(GraphNode {
            id: group_id,
            domain: NodeDomain::Group,
            label,
            metadata: NodeMetadata::Group {
                child_domain: NodeDomain::Symbol,
                count: symbol_ids.len(),
                representative: file.clone(),
            },
            depth: 0,
        });

        nodes_to_remove.extend(symbol_ids.iter().cloned());
    }

    // Build set of removed IDs → their group ID for edge redirection
    let mut redirect: HashMap<String, String> = HashMap::new();
    for (file, symbol_ids) in &file_groups {
        let group_id = format!("group:symbol:{}", file);
        for sid in symbol_ids {
            redirect.insert(sid.clone(), group_id.clone());
        }
    }

    // Remove individual symbol nodes
    for id in &nodes_to_remove {
        graph.nodes.remove(id);
    }

    // Add group nodes
    for node in group_nodes {
        graph.nodes.insert(node.id.clone(), node);
    }

    // Redirect edges and deduplicate
    let mut seen_edges: HashSet<(String, String, String)> = HashSet::new();
    let mut new_edges = Vec::new();

    for mut edge in graph.edges {
        if let Some(group_id) = redirect.get(&edge.source) {
            edge.source = group_id.clone();
        }
        if let Some(group_id) = redirect.get(&edge.target) {
            edge.target = group_id.clone();
        }
        // Skip edges where source or target no longer exists
        if !graph.nodes.contains_key(&edge.source) || !graph.nodes.contains_key(&edge.target) {
            continue;
        }
        // Skip self-loops
        if edge.source == edge.target {
            continue;
        }
        let key = (
            edge.source.clone(),
            edge.target.clone(),
            format!("{:?}", edge.kind),
        );
        if seen_edges.insert(key) {
            new_edges.push(edge);
        }
    }

    graph.edges = new_edges;

    // Derive inter-group edges so single-domain views retain connectivity.
    // Pattern 1: group→non-group — groups sharing a common target get connected.
    // Pattern 2: non-group→group — groups sharing a common source get connected.
    let mut pivot_to_groups: HashMap<String, Vec<String>> = HashMap::new();
    for edge in &graph.edges {
        let source_is_group = graph
            .nodes
            .get(&edge.source)
            .is_some_and(|n| n.is_group());
        let target_is_group = graph
            .nodes
            .get(&edge.target)
            .is_some_and(|n| n.is_group());

        if source_is_group && !target_is_group {
            pivot_to_groups
                .entry(edge.target.clone())
                .or_default()
                .push(edge.source.clone());
        } else if !source_is_group && target_is_group {
            pivot_to_groups
                .entry(edge.source.clone())
                .or_default()
                .push(edge.target.clone());
        }
    }

    let mut inter_group_seen: HashSet<(String, String)> = HashSet::new();
    for groups in pivot_to_groups.values() {
        if groups.len() < 2 {
            continue;
        }
        for i in 0..groups.len() {
            for j in (i + 1)..groups.len() {
                let (a, b) = if groups[i] < groups[j] {
                    (&groups[i], &groups[j])
                } else {
                    (&groups[j], &groups[i])
                };
                if inter_group_seen.insert((a.clone(), b.clone())) {
                    graph.edges.push(GraphEdge {
                        source: a.clone(),
                        target: b.clone(),
                        kind: EdgeKind::References,
                        confidence: EdgeConfidence::Speculative,
                    });
                }
            }
        }
    }

    // Recompute roots and leaves from the aggregated node/edge sets
    let node_ids: HashSet<&str> = graph.nodes.keys().map(|s| s.as_str()).collect();
    let has_incoming: HashSet<&str> = graph.edges.iter().map(|e| e.target.as_str()).collect();
    let has_outgoing: HashSet<&str> = graph.edges.iter().map(|e| e.source.as_str()).collect();
    graph.roots = node_ids
        .difference(&has_incoming)
        .map(|s| s.to_string())
        .collect();
    graph.leaves = node_ids
        .difference(&has_outgoing)
        .map(|s| s.to_string())
        .collect();

    graph
}

/// Expand a group node into its individual children, returning a partial graph.
fn expand_group_node(state: &AppState, node_id: &str) -> Option<DependencyGraph> {
    // Parse group:symbol:{file_path}
    let file_path = node_id.strip_prefix("group:symbol:")?;

    let idx = state.workspace_index();
    let file_symbols = idx.symbols_in_file(std::path::Path::new(file_path)).ok()?;

    let mut nodes = HashMap::new();
    let mut edges = Vec::new();

    for sym in file_symbols {
        let id = format!("symbol:{}", sym.name_path);
        nodes.insert(
            id.clone(),
            GraphNode {
                id,
                domain: NodeDomain::Symbol,
                label: sym.name_path.clone(),
                metadata: NodeMetadata::Symbol {
                    kind: format!("{:?}", sym.kind).to_lowercase(),
                    file: file_path.to_string(),
                    line: sym.start_line,
                },
                depth: 0,
            },
        );
    }

    // Build edges between these symbols and any visible nodes
    // For now, return just the symbol nodes — the frontend can re-fetch if needed
    let _ = &mut edges;

    Some(DependencyGraph {
        roots: Vec::new(),
        leaves: Vec::new(),
        max_depth: 0,
        domains: dtx_core::DomainStatus {
            resource: false,
            symbol: true,
            memory: false,
            file: false,
        },
        nodes,
        edges,
    })
}

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

/// Query parameter for project selection.
#[derive(Deserialize, Default)]
pub struct ProjectQuery {
    pub project: Option<String>,
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

    let store_lock = state.store();
    let store = store_lock.read().await;
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

    let store_lock = state.store();
    let store = store_lock.read().await;
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
    #[serde(default = "crate::config::WebConfig::default_search_limit")]
    pub limit: usize,
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

    let store_lock = state.store();
    let store = store_lock.read().await;
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
    #[serde(default = "crate::config::WebConfig::default_import_format")]
    pub format: String,
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

#[derive(Debug, Deserialize)]
pub struct GraphParams {
    #[serde(default = "default_graph_view")]
    pub view: String,
}

fn default_graph_view() -> String {
    "knowledge".to_string()
}

const SYMBOL_AGGREGATION_THRESHOLD: usize = 150;

/// Get the unified knowledge graph, optionally filtered by view.
pub async fn get_graph(
    State(state): State<AppState>,
    Query(params): Query<GraphParams>,
) -> AppResult<Json<DependencyGraph>> {
    let store_lock = state.store();
    let store = store_lock.read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let graph = build_graph(&state, &services);
    let graph = aggregate_graph(graph, SYMBOL_AGGREGATION_THRESHOLD);

    let view = match params.view.as_str() {
        "knowledge" => GraphView::Knowledge,
        "processes" => GraphView::Processes,
        "code" => GraphView::Code,
        "memories" => GraphView::Memories,
        "files" => GraphView::Files,
        other => {
            return Err(AppError::bad_request(format!(
                "Unknown graph view: '{other}'"
            )))
        }
    };

    let filtered = graph.filter_by_view(view);
    Ok(Json(filtered))
}

/// Expand a group node, returning its children as a partial graph.
pub async fn expand_node(
    State(state): State<AppState>,
    Path(node_id): Path<String>,
) -> AppResult<Json<DependencyGraph>> {
    // Axum wildcard captures include a leading '/'
    let node_id = node_id.strip_prefix('/').unwrap_or(&node_id);
    let graph = expand_group_node(&state, node_id).ok_or_else(|| {
        AppError::not_found(format!("Cannot expand node '{}'", node_id))
    })?;
    Ok(Json(graph))
}

/// Get the impact set for a specific node.
pub async fn get_impact(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<ImpactSet>> {
    let store_lock = state.store();
    let store = store_lock.read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let graph = build_graph(&state, &services);

    let impact = graph.impact(&id, EdgeConfidence::Speculative);
    Ok(Json(impact))
}

/// Get graph statistics.
pub async fn get_graph_stats(State(state): State<AppState>) -> AppResult<Json<GraphStats>> {
    let store_lock = state.store();
    let store = store_lock.read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let graph = build_graph(&state, &services);

    Ok(Json(graph.stats()))
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
    let store_lock = state.store();
    let mut store = store_lock.write().await;

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

// ---------------------------------------------------------------------------
// Project management endpoints
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RegisterProjectRequest {
    pub root: String,
}

#[derive(Serialize)]
pub struct RegisterProjectResponse {
    pub id: String,
    pub url: String,
}

/// Register a new project with this server instance.
pub async fn register_project(
    State(state): State<AppState>,
    Json(req): Json<RegisterProjectRequest>,
) -> AppResult<Json<RegisterProjectResponse>> {
    let root = PathBuf::from(&req.root);

    let store = dtx_core::store::ConfigStore::discover_and_load_from(&root)
        .map_err(|e| AppError::bad_request(format!("Cannot load project at '{}': {}", req.root, e)))?;

    let id = state
        .registry()
        .add(store, state.event_bus(), state.config())
        .map_err(|e| AppError::internal(e.to_string()))?;

    Ok(Json(RegisterProjectResponse {
        id: id.clone(),
        url: format!("/?project={}", id),
    }))
}

#[derive(Serialize)]
pub struct ProjectListEntry {
    pub id: String,
    pub root: String,
    pub active: bool,
}

/// List all registered projects.
pub async fn list_projects(
    State(state): State<AppState>,
) -> AppResult<Json<Vec<ProjectListEntry>>> {
    let projects = state.registry().list();
    let entries: Vec<_> = projects
        .into_iter()
        .map(|(id, root, active)| ProjectListEntry {
            id,
            root: root.display().to_string(),
            active,
        })
        .collect();
    Ok(Json(entries))
}

/// Activate a project by ID.
pub async fn activate_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    state
        .registry()
        .set_active(&id)
        .map_err(|e| AppError::not_found(e))?;

    Ok(Json(serde_json::json!({ "status": "activated", "id": id })))
}

/// Remove a project by ID.
pub async fn remove_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Json<serde_json::Value>> {
    let removed = state.registry().remove(&id);
    match removed {
        Some(_) => Ok(Json(
            serde_json::json!({ "status": "removed", "id": id }),
        )),
        None => Err(AppError::bad_request(
            "Cannot remove active project".to_string(),
        )),
    }
}
