//! HTMX partial handlers.

use askama::Template as AskamaRender;
use askama_axum::Template;
use axum::extract::{Path, Query, State};
use axum::Form;
use dtx_core::graph::SymbolSource;
use dtx_core::model::Service as ModelService;
use serde::{Deserialize, Deserializer};

use crate::error::{AppError, AppResult};
use crate::service::ops::CreateServiceParams;
use crate::state::AppState;
use crate::types::PackageAnalysis;

use super::api;

/// Deserialize an empty form field as `None` instead of failing.
fn empty_string_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt.as_deref() {
        None | Some("") => Ok(None),
        Some(s) => s.parse::<T>().map(Some).map_err(serde::de::Error::custom),
    }
}

/// Services list partial template.
#[derive(Template)]
#[template(path = "partials/services_list.html")]
pub struct ServicesListTemplate {
    pub services: Vec<ModelService>,
}

/// Render the services list partial.
pub async fn services_list(State(state): State<AppState>) -> AppResult<ServicesListTemplate> {
    let services = state.service_ops().list_services().await?;
    Ok(ServicesListTemplate { services })
}

/// Service detail panel template.
#[derive(Template)]
#[template(path = "partials/detail_panel.html")]
pub struct ServiceDetailTemplate {
    pub service: ModelService,
    pub status: String,
}

/// Render the service detail panel partial.
pub async fn service_detail(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
) -> AppResult<ServiceDetailTemplate> {
    let service = state.service_ops().get_service(&service_name).await?;
    let status = "stopped".to_string();
    Ok(ServiceDetailTemplate { service, status })
}

/// Status panel partial template.
#[derive(Template)]
#[template(path = "partials/status_panel.html")]
pub struct StatusPanelTemplate {
    pub running: bool,
    pub status: String,
}

/// Render the status panel partial.
pub async fn status_panel(State(state): State<AppState>) -> AppResult<StatusPanelTemplate> {
    let running = state.orchestrator_handle().is_running();
    let status = if running {
        "Running".to_string()
    } else {
        "Stopped".to_string()
    };

    Ok(StatusPanelTemplate { running, status })
}

/// Search results partial template.
#[derive(Template)]
#[template(path = "partials/search_results.html")]
pub struct SearchResultsTemplate {
    pub packages: Vec<dtx_core::Package>,
    pub query: String,
}

/// Query parameters for search.
#[derive(Deserialize)]
pub struct SearchParams {
    pub q: String,
    #[serde(default = "crate::config::WebConfig::default_search_limit")]
    pub limit: usize,
}

/// Render the search results partial.
pub async fn search_results(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> AppResult<SearchResultsTemplate> {
    let packages = state
        .service_ops()
        .nix_search(&params.q, Some(params.limit))
        .await?;

    Ok(SearchResultsTemplate {
        packages,
        query: params.q,
    })
}

/// Service form partial template.
#[derive(Template)]
#[template(path = "partials/service_form.html")]
pub struct ServiceFormTemplate {}

/// Render the service form partial.
pub async fn service_form() -> AppResult<ServiceFormTemplate> {
    Ok(ServiceFormTemplate {})
}

/// Logs panel partial template.
#[derive(Template)]
#[template(path = "partials/logs_panel.html")]
pub struct LogsPanelTemplate {
    pub service: String,
    pub logs: String,
}

/// Render the logs panel partial.
pub async fn logs_panel(Path(service): Path<String>) -> AppResult<LogsPanelTemplate> {
    Ok(LogsPanelTemplate {
        service,
        logs: "Logs will be streamed here...".to_string(),
    })
}

/// Live status panel template (SSE-enabled).
#[derive(Template)]
#[template(path = "partials/status_panel_live.html")]
pub struct StatusPanelLiveTemplate {}

/// Render the live status panel partial with SSE support.
pub async fn status_panel_live() -> AppResult<StatusPanelLiveTemplate> {
    Ok(StatusPanelLiveTemplate {})
}

/// Live logs panel template (SSE-enabled).
#[derive(Template)]
#[template(path = "partials/logs_panel_live.html")]
pub struct LogsPanelLiveTemplate {
    pub service: String,
}

/// Render the live logs panel partial with SSE support.
pub async fn logs_panel_live(Path(service): Path<String>) -> AppResult<LogsPanelLiveTemplate> {
    Ok(LogsPanelLiveTemplate { service })
}

/// Nix panel partial template.
#[derive(Template)]
#[template(path = "partials/nix_panel.html")]
pub struct NixPanelTemplate {}

/// Render the Nix panel partial.
pub async fn nix_panel() -> AppResult<NixPanelTemplate> {
    Ok(NixPanelTemplate {})
}

// === Form Handlers (HTMX) ===

/// Form data for creating a service.
#[derive(Deserialize)]
pub struct CreateServiceForm {
    pub name: String,
    pub command: String,
    pub package: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub port: Option<u16>,
}

/// Create a service via form submission, returns HTML partial.
pub async fn create_service(
    State(state): State<AppState>,
    Form(form): Form<CreateServiceForm>,
) -> AppResult<ServicesListTemplate> {
    let params = CreateServiceParams {
        name: form.name,
        command: form.command,
        package: form.package,
        port: form.port,
        working_dir: None,
    };
    let (_, services) = state.service_ops().create_service(params).await?;
    Ok(ServicesListTemplate { services })
}

/// Delete a service via HTMX, returns HTML partial.
pub async fn delete_service(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
) -> AppResult<ServicesListTemplate> {
    let services = state.service_ops().delete_service(&service_name).await?;
    Ok(ServicesListTemplate { services })
}

/// Form data for adding a package from search.
#[derive(Deserialize)]
pub struct AddPackageForm {
    pub package: String,
}

/// Add package response template.
#[derive(Template)]
#[template(path = "partials/add_package_result.html")]
pub struct AddPackageResultTemplate {
    pub success: bool,
    pub message: String,
    pub package: String,
    pub project_name: Option<String>,
}

/// Add a package as a service from the search page.
pub async fn add_package(
    State(state): State<AppState>,
    Form(form): Form<AddPackageForm>,
) -> AppResult<AddPackageResultTemplate> {
    let ops = state.service_ops();

    // Validate package exists
    let valid = ops.nix_validate(&form.package).await?;

    if !valid {
        let info = ops.get_project().await?;
        return Ok(AddPackageResultTemplate {
            success: false,
            message: format!("Package '{}' not found", form.package),
            package: form.package,
            project_name: Some(info.name),
        });
    }

    let params = CreateServiceParams {
        name: form.package.clone(),
        command: form.package.clone(),
        package: Some(form.package.clone()),
        port: None,
        working_dir: None,
    };
    let info = ops.get_project().await?;
    let project_name = info.name;

    ops.create_service(params).await?;

    Ok(AddPackageResultTemplate {
        success: true,
        message: format!("Added '{}' to project '{}'", form.package, project_name),
        package: form.package,
        project_name: Some(project_name),
    })
}

// === Package Mappings Handlers ===

/// Mappings panel partial template.
#[derive(Template)]
#[template(path = "partials/mappings_table.html")]
pub struct MappingsPanelTemplate {
    pub packages: Vec<PackageAnalysis>,
}

/// Render the mappings panel partial.
pub async fn mappings_panel(State(state): State<AppState>) -> AppResult<MappingsPanelTemplate> {
    let analysis = state.service_ops().analyze_packages().await?;
    let packages = to_package_analysis_display(analysis);
    Ok(MappingsPanelTemplate { packages })
}

/// Custom mappings list partial template.
#[derive(Template)]
#[template(path = "partials/mappings_list.html")]
pub struct MappingsListTemplate {
    pub custom_mappings: Vec<(String, String)>,
}

/// Form data for adding a mapping.
#[derive(Deserialize)]
pub struct AddMappingForm {
    pub command: String,
    pub package: String,
}

/// Add a custom command-to-package mapping via HTMX.
pub async fn add_mapping(
    State(state): State<AppState>,
    Form(form): Form<AddMappingForm>,
) -> AppResult<MappingsListTemplate> {
    let ops = state.service_ops();
    ops.add_mapping(&form.command, &form.package).await?;

    let store_lock = state.store();
    let store = store_lock.read().await;
    let project_root = store.project_root().to_path_buf();
    drop(store);

    let config = dtx_core::ProjectConfig::load(&project_root).unwrap_or_default();
    let custom_mappings: Vec<(String, String)> = config.mappings.packages.into_iter().collect();

    Ok(MappingsListTemplate { custom_mappings })
}

/// Remove a custom mapping via HTMX.
pub async fn remove_mapping(
    State(state): State<AppState>,
    Path(command): Path<String>,
) -> AppResult<MappingsListTemplate> {
    let ops = state.service_ops();
    ops.remove_mapping(&command).await?;

    let store_lock = state.store();
    let store = store_lock.read().await;
    let project_root = store.project_root().to_path_buf();
    drop(store);

    let config = dtx_core::ProjectConfig::load(&project_root).unwrap_or_default();
    let custom_mappings: Vec<(String, String)> = config.mappings.packages.into_iter().collect();

    Ok(MappingsListTemplate { custom_mappings })
}

/// Mark a command as a local binary via HTMX.
pub async fn mark_as_local(
    State(state): State<AppState>,
    Path(command): Path<String>,
) -> AppResult<MappingsPanelTemplate> {
    let analysis = state.service_ops().mark_local(&command).await?;
    let packages = to_package_analysis_display(analysis);
    Ok(MappingsPanelTemplate { packages })
}

/// Mark a command as ignored via HTMX.
pub async fn mark_as_ignore(
    State(state): State<AppState>,
    Path(command): Path<String>,
) -> AppResult<MappingsPanelTemplate> {
    let analysis = state.service_ops().mark_ignore(&command).await?;
    let packages = to_package_analysis_display(analysis);
    Ok(MappingsPanelTemplate { packages })
}

/// Convert service-layer PackageAnalysis to display-ready PackageAnalysis with status_class and mapping_key.
fn to_package_analysis_display(
    analyses: Vec<crate::service::ops::PackageAnalysis>,
) -> Vec<PackageAnalysis> {
    analyses
        .into_iter()
        .map(|a| {
            let status_class = match a.status.as_str() {
                "explicit" => "status--explicit".to_string(),
                "auto" => "status--auto".to_string(),
                "local" => "status--local".to_string(),
                "needs_attention" => "status--warning".to_string(),
                _ => String::new(),
            };
            let display_status = match a.status.as_str() {
                "explicit" => "Explicit".to_string(),
                "auto" => "Auto-detected".to_string(),
                "local" => "Local Binary".to_string(),
                "needs_attention" => "Needs Attention".to_string(),
                _ => a.status.clone(),
            };
            let mapping_key = a.executable.clone().unwrap_or_else(|| {
                a.command
                    .split_whitespace()
                    .next()
                    .unwrap_or(&a.command)
                    .to_string()
            });
            PackageAnalysis {
                service_name: a.service_name,
                command: a.command,
                status: display_status,
                status_class,
                package: a.package,
                executable: a.executable,
                mapping_key,
            }
        })
        .collect()
}

// === Import/Export/Graph Panel Handlers ===

#[derive(Template)]
#[template(path = "partials/import_form.html")]
pub struct ImportFormTemplate {}

pub async fn import_form() -> AppResult<ImportFormTemplate> {
    Ok(ImportFormTemplate {})
}

#[derive(Deserialize)]
pub struct ImportForm {
    pub content: String,
    #[serde(default = "crate::config::WebConfig::default_import_format")]
    pub format: String,
}

#[derive(Template)]
#[template(path = "partials/import_result.html")]
pub struct ImportResultTemplate {
    pub success: bool,
    pub imported: usize,
    pub warnings: Vec<String>,
    pub services: Vec<ModelService>,
}

pub async fn do_import(
    State(state): State<AppState>,
    Form(form): Form<ImportForm>,
) -> AppResult<ImportResultTemplate> {
    match state
        .service_ops()
        .import_config(&form.content, &form.format)
        .await
    {
        Ok(result) => Ok(ImportResultTemplate {
            success: true,
            imported: result.imported,
            warnings: result.warnings,
            services: result.services,
        }),
        Err(e) => Ok(ImportResultTemplate {
            success: false,
            imported: 0,
            warnings: vec![e.message],
            services: Vec::new(),
        }),
    }
}

#[derive(Template)]
#[template(path = "partials/export_panel.html")]
pub struct ExportPanelTemplate {}

pub async fn export_panel() -> AppResult<ExportPanelTemplate> {
    Ok(ExportPanelTemplate {})
}

#[derive(Template)]
#[template(path = "partials/graph_panel.html")]
pub struct GraphPanelTemplate {}

pub async fn graph_panel(State(_state): State<AppState>) -> AppResult<GraphPanelTemplate> {
    Ok(GraphPanelTemplate {})
}

// === Sidebar Contextual Lists ===

/// Render an Askama template into an `Html` response.
fn render_partial(tmpl: &impl AskamaRender) -> AppResult<axum::response::Html<String>> {
    Ok(axum::response::Html(
        tmpl.render()
            .map_err(|e| AppError::internal(format!("Template error: {e}")))?,
    ))
}

#[derive(Template)]
#[template(path = "partials/sidebar_symbols.html")]
pub struct SidebarSymbolsTemplate {
    pub symbols: Vec<SymbolSource>,
}

/// A memory entry for sidebar display.
pub struct SidebarMemory {
    pub name: String,
    pub kind: String,
    pub preview: String,
}

#[derive(Template)]
#[template(path = "partials/sidebar_memories.html")]
pub struct SidebarMemoriesTemplate {
    pub memories: Vec<SidebarMemory>,
}

/// A file entry for sidebar display.
pub struct SidebarFile {
    pub name: String,
    pub path: String,
}

#[derive(Template)]
#[template(path = "partials/sidebar_files.html")]
pub struct SidebarFilesTemplate {
    pub files: Vec<SidebarFile>,
}

/// An entity entry for sidebar display (knowledge view — top connected nodes).
pub struct SidebarEntity {
    pub id: String,
    pub domain: String,
    pub label: String,
}

#[derive(Template)]
#[template(path = "partials/sidebar_entities.html")]
pub struct SidebarEntitiesTemplate {
    pub entities: Vec<SidebarEntity>,
}

/// Sidebar services list (reuses existing template).
#[derive(Template)]
#[template(path = "partials/sidebar_services.html")]
pub struct SidebarServicesTemplate {
    pub services: Vec<ModelService>,
}

/// Render sidebar list content based on the current view.
pub async fn sidebar_list(
    State(state): State<AppState>,
    Path(view): Path<String>,
) -> AppResult<axum::response::Html<String>> {
    match view.as_str() {
        "processes" => {
            let services = state.service_ops().list_services().await?;
            render_partial(&SidebarServicesTemplate { services })
        }
        "code" => {
            let symbols = api::collect_symbols(&state);
            render_partial(&SidebarSymbolsTemplate { symbols })
        }
        "memories" => {
            let memories: Vec<SidebarMemory> = api::collect_memories(&state)
                .into_iter()
                .map(|m| SidebarMemory {
                    name: m.name,
                    kind: m.kind,
                    preview: if m.content_preview.len() > 80 {
                        format!("{}...", &m.content_preview[..80])
                    } else {
                        m.content_preview
                    },
                })
                .collect();
            render_partial(&SidebarMemoriesTemplate { memories })
        }
        "files" => {
            let files: Vec<SidebarFile> = api::collect_files(&state)
                .into_iter()
                .map(|f| {
                    let name = std::path::Path::new(&f.path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&f.path)
                        .to_string();
                    SidebarFile {
                        name,
                        path: f.path,
                    }
                })
                .collect();
            render_partial(&SidebarFilesTemplate { files })
        }
        "knowledge" => {
            let store_lock = state.store();
            let store = store_lock.read().await;
            let services = dtx_core::model::services_from_config(store.config());
            drop(store);
            let graph = api::build_graph(&state, &services);

            let mut edge_count: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for edge in &graph.edges {
                *edge_count.entry(&edge.source).or_default() += 1;
                *edge_count.entry(&edge.target).or_default() += 1;
            }

            let mut entities: Vec<SidebarEntity> = graph
                .nodes
                .values()
                .map(|n| SidebarEntity {
                    id: n.id.clone(),
                    domain: format!("{:?}", n.domain).to_lowercase(),
                    label: n.label.clone(),
                })
                .collect();

            entities.sort_by(|a, b| {
                let ca = edge_count.get(a.id.as_str()).copied().unwrap_or(0);
                let cb = edge_count.get(b.id.as_str()).copied().unwrap_or(0);
                cb.cmp(&ca)
            });
            entities.truncate(50);

            render_partial(&SidebarEntitiesTemplate { entities })
        }
        _ => Err(AppError::bad_request(format!(
            "Unknown sidebar view: '{view}'"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Project switcher
// ---------------------------------------------------------------------------

pub struct ProjectSwitcherEntry {
    pub id: String,
    pub name: String,
    pub active: bool,
}

#[derive(Template)]
#[template(path = "partials/project_switcher.html")]
pub struct ProjectSwitcherTemplate {
    pub active_name: String,
    pub projects: Vec<ProjectSwitcherEntry>,
}

/// Render the project switcher dropdown.
pub async fn project_switcher(State(state): State<AppState>) -> AppResult<ProjectSwitcherTemplate> {
    let list = state.registry().list();
    let active = state.registry().active();
    let active_store = active.store().read().await;
    let active_name = active_store.project_name().to_string();
    drop(active_store);

    let mut projects = Vec::new();
    for (id, root, is_active) in &list {
        let name = if *is_active {
            active_name.clone()
        } else {
            root.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| id.clone())
        };
        projects.push(ProjectSwitcherEntry {
            id: id.clone(),
            name,
            active: *is_active,
        });
    }

    Ok(ProjectSwitcherTemplate {
        active_name,
        projects,
    })
}
