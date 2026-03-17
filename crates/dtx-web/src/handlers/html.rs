//! HTML page handlers.

use askama_axum::Template;
use axum::extract::{Query, State};
use dtx_core::model::Service as ModelService;
use dtx_core::resource::ResourceState;
use serde::Deserialize;
use std::collections::HashMap;

use crate::error::AppResult;
use crate::state::AppState;
use crate::types::PackageAnalysis;

/// Query parameters for index page.
#[derive(Deserialize, Default)]
pub struct IndexQuery {
    pub project: Option<String>,
}

/// Project info for templates.
#[derive(Clone)]
pub struct ProjectInfo {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
}

/// Index page template - Command Center Dashboard.
#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub title: String,
    pub services: Vec<ModelService>,
    pub service_statuses_json: String,
    pub current_context: String,
    pub has_project: bool,
    pub has_services: bool,
    pub running_count: usize,
    pub error_count: usize,
    pub server_uptime_secs: u64,
    /// Current working directory for init button.
    pub cwd: String,
}

/// Render the command center dashboard.
pub async fn index(
    State(state): State<AppState>,
    Query(_query): Query<IndexQuery>,
) -> AppResult<IndexTemplate> {
    let services = state.service_ops().list_services().await?;

    let store = state.store().read().await;
    let project_name = store.project_name().to_string();
    drop(store);

    // Get current process statuses from orchestrator if running
    let mut service_statuses: HashMap<String, String> = HashMap::new();
    let mut running_count = 0;
    let mut error_count = 0;

    let statuses = state.orchestrator_handle().status().await.ok().flatten();
    if let Some(statuses) = statuses {
        for (id, resource_state) in &statuses {
            let (status, is_running, is_error) = match resource_state {
                ResourceState::Pending => ("pending", false, false),
                ResourceState::Starting { .. } => ("starting", false, false),
                ResourceState::Running { .. } => ("running", true, false),
                ResourceState::Stopping { .. } => ("stopping", false, false),
                ResourceState::Stopped { .. } => ("stopped", false, false),
                ResourceState::Failed { .. } => ("error", false, true),
            };
            service_statuses.insert(id.to_string(), status.to_string());

            if is_running {
                running_count += 1;
            }
            if is_error {
                error_count += 1;
            }
        }
    }

    let has_project = true;
    let has_services = !services.is_empty();

    let service_statuses_json =
        serde_json::to_string(&service_statuses).unwrap_or_else(|_| "{}".to_string());

    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(IndexTemplate {
        title: "dtx - Command Center".to_string(),
        services,
        service_statuses_json,
        current_context: project_name,
        has_project,
        has_services,
        running_count,
        error_count,
        server_uptime_secs: state.uptime_secs(),
        cwd,
    })
}

/// Services page template.
#[derive(Template)]
#[template(path = "services.html")]
pub struct ServicesTemplate {
    pub title: String,
    pub project: ProjectInfo,
    pub services: Vec<ModelService>,
}

/// Render the services page.
pub async fn services_page(State(state): State<AppState>) -> AppResult<ServicesTemplate> {
    let info = state.service_ops().get_project().await?;
    let services = state.service_ops().list_services().await?;

    let project = ProjectInfo {
        name: info.name.clone(),
        path: info.path,
        description: info.description,
    };

    Ok(ServicesTemplate {
        title: format!("{} - Services", project.name),
        project,
        services,
    })
}

/// Search page template.
#[derive(Template)]
#[template(path = "search.html")]
pub struct SearchTemplate {
    pub title: String,
}

/// Render the package search page.
pub async fn search_page() -> AppResult<SearchTemplate> {
    Ok(SearchTemplate {
        title: "Search Packages".to_string(),
    })
}

/// Mappings page template.
#[derive(Template)]
#[template(path = "mappings.html")]
pub struct MappingsTemplate {
    pub title: String,
    pub project: ProjectInfo,
    pub packages: Vec<PackageAnalysis>,
    pub custom_mappings: Vec<(String, String)>,
    pub local_binaries: Vec<String>,
    pub ignored_commands: Vec<String>,
    pub needs_attention_count: usize,
}

/// Render the package mappings page.
pub async fn mappings_page(State(state): State<AppState>) -> AppResult<MappingsTemplate> {
    let ops = state.service_ops();
    let info = ops.get_project().await?;
    let project = ProjectInfo {
        name: info.name.clone(),
        path: info.path,
        description: info.description,
    };

    let store = state.store().read().await;
    let services = dtx_core::model::services_from_config(store.config());
    let project_root = store.project_root().to_path_buf();
    drop(store);

    // Analyze packages
    let analysis = dtx_core::analyze_service_packages(&services);

    let packages: Vec<PackageAnalysis> = analysis
        .into_iter()
        .map(|a| {
            let (status, status_class, package, executable) = match a.result {
                dtx_core::PackageAnalysisResult::Explicit(p) => (
                    "Explicit".to_string(),
                    "status--explicit".to_string(),
                    Some(p),
                    None,
                ),
                dtx_core::PackageAnalysisResult::AutoDetected(p) => (
                    "Auto-detected".to_string(),
                    "status--auto".to_string(),
                    Some(p),
                    None,
                ),
                dtx_core::PackageAnalysisResult::LocalBinary => (
                    "Local Binary".to_string(),
                    "status--local".to_string(),
                    None,
                    None,
                ),
                dtx_core::PackageAnalysisResult::NeedsAttention(e) => (
                    "Needs Attention".to_string(),
                    "status--warning".to_string(),
                    None,
                    Some(e),
                ),
            };
            let mapping_key = executable.clone().unwrap_or_else(|| {
                a.command
                    .split_whitespace()
                    .next()
                    .unwrap_or(&a.command)
                    .to_string()
            });
            PackageAnalysis {
                service_name: a.service_name,
                command: a.command,
                status,
                status_class,
                package,
                executable,
                mapping_key,
            }
        })
        .collect();

    let needs_attention_count = packages
        .iter()
        .filter(|p| p.status == "Needs Attention")
        .count();

    // Load project config for custom mappings
    let config = dtx_core::ProjectConfig::load(&project_root).unwrap_or_default();

    let custom_mappings: Vec<(String, String)> = config.mappings.packages.into_iter().collect();

    Ok(MappingsTemplate {
        title: format!("{} - Package Mappings", project.name),
        project,
        packages,
        custom_mappings,
        local_binaries: config.mappings.local,
        ignored_commands: config.mappings.ignore,
        needs_attention_count,
    })
}
