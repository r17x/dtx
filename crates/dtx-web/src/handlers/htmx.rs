//! HTMX partial handlers.

use askama_axum::Template;
use axum::extract::{Path, Query, State};
use axum::Form;
use dtx_core::config::schema::ResourceConfig;
use dtx_core::model::Service as ModelService;
use serde::{Deserialize, Deserializer};
use std::collections::HashSet;

use crate::error::{AppError, AppResult};
use crate::state::AppState;

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
    let store = state.store.read().await;
    let services = dtx_core::model::services_from_config(store.config());
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
    let store = state.store.read().await;
    let rc = store
        .get_resource(&service_name)
        .ok_or_else(|| AppError::not_found(format!("Service '{}' not found", service_name)))?;
    let service = ModelService::from_resource_config(&service_name, rc);
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
    let orch = state.orchestrator.read().await;
    let running = orch.as_ref().map(|o| o.is_running()).unwrap_or(false);
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
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    20
}

/// Render the search results partial.
pub async fn search_results(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> AppResult<SearchResultsTemplate> {
    let mut packages = state
        .nix_client
        .search(&params.q)
        .await
        .map_err(crate::error::AppError::from)?;

    packages.truncate(params.limit);

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
    // Validate package if provided
    if let Some(ref pkg) = form.package {
        if !pkg.is_empty()
            && !state
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

    let pkg = form.package.filter(|s| !s.is_empty());
    let rc = ResourceConfig {
        command: Some(form.command),
        port: form.port,
        nix: pkg.as_ref().map(|p| dtx_core::config::schema::NixConfig {
            packages: vec![p.clone()],
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut store = state.store.write().await;
    store.add_resource(&form.name, rc).map_err(AppError::from)?;

    // Sync flake.nix if service has a package
    if let Some(ref p) = pkg {
        let project_root = store.project_root().to_path_buf();
        let project_name = store.project_name().to_string();
        if let Err(e) = dtx_core::sync_add_package(&project_root, &project_name, p) {
            tracing::warn!("Failed to sync flake.nix: {}", e);
        }
    }

    store.save().map_err(AppError::from)?;

    let services = dtx_core::model::services_from_config(store.config());
    Ok(ServicesListTemplate { services })
}

/// Delete a service via HTMX, returns HTML partial.
pub async fn delete_service(
    State(state): State<AppState>,
    Path(service_name): Path<String>,
) -> AppResult<ServicesListTemplate> {
    let mut store = state.store.write().await;

    let removed_rc = store
        .remove_resource(&service_name)
        .map_err(AppError::from)?;

    // Sync flake.nix if the removed service had a package
    let removed_package = removed_rc
        .nix
        .as_ref()
        .and_then(|n| n.packages.first().cloned());
    if let Some(ref pkg) = removed_package {
        let project_root = store.project_root().to_path_buf();
        let remaining_packages: HashSet<String> = store
            .list_resources()
            .filter_map(|(_, rc)| rc.nix.as_ref().and_then(|n| n.packages.first().cloned()))
            .collect();
        if let Err(e) = dtx_core::sync_remove_package(&project_root, pkg, &remaining_packages) {
            tracing::warn!("Failed to sync flake.nix: {}", e);
        }
    }

    store.save().map_err(AppError::from)?;

    let services = dtx_core::model::services_from_config(store.config());
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
    // Validate package exists
    if !state
        .nix_client
        .validate(&form.package)
        .await
        .map_err(AppError::from)?
    {
        let store = state.store.read().await;
        let project_name = store.project_name().to_string();
        return Ok(AddPackageResultTemplate {
            success: false,
            message: format!("Package '{}' not found", form.package),
            package: form.package,
            project_name: Some(project_name),
        });
    }

    let mut store = state.store.write().await;
    let project_name = store.project_name().to_string();

    let rc = ResourceConfig {
        command: Some(form.package.clone()),
        nix: Some(dtx_core::config::schema::NixConfig {
            packages: vec![form.package.clone()],
            ..Default::default()
        }),
        ..Default::default()
    };

    store
        .add_resource(&form.package, rc)
        .map_err(AppError::from)?;

    // Sync flake.nix
    let project_root = store.project_root().to_path_buf();
    if let Err(e) = dtx_core::sync_add_package(&project_root, &project_name, &form.package) {
        tracing::warn!("Failed to sync flake.nix: {}", e);
    }

    store.save().map_err(AppError::from)?;

    Ok(AddPackageResultTemplate {
        success: true,
        message: format!("Added '{}' to project '{}'", form.package, project_name),
        package: form.package,
        project_name: Some(project_name),
    })
}

// === Package Mappings Handlers ===

/// Package analysis info for HTMX responses.
#[derive(Clone)]
pub struct PackageAnalysisInfo {
    pub service_name: String,
    pub command: String,
    pub status: String,
    pub status_class: String,
    pub package: Option<String>,
    pub executable: Option<String>,
    /// Key to use for mapping operations (executable or command).
    pub mapping_key: String,
}

/// Mappings panel partial template.
#[derive(Template)]
#[template(path = "partials/mappings_table.html")]
pub struct MappingsPanelTemplate {
    pub packages: Vec<PackageAnalysisInfo>,
}

/// Render the mappings panel partial.
pub async fn mappings_panel(State(state): State<AppState>) -> AppResult<MappingsPanelTemplate> {
    let store = state.store.read().await;
    let services = dtx_core::model::services_from_config(store.config());

    let analysis = dtx_core::analyze_service_packages(&services);

    let packages: Vec<PackageAnalysisInfo> = analysis
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
            PackageAnalysisInfo {
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
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let mut config = dtx_core::ProjectConfig::load(&project_root).unwrap_or_default();
    config.add_mapping(&form.command, &form.package);
    config
        .save(&project_root)
        .map_err(|e| AppError::internal(e.to_string()))?;

    let custom_mappings: Vec<(String, String)> = config.mappings.packages.into_iter().collect();

    Ok(MappingsListTemplate { custom_mappings })
}

/// Remove a custom mapping via HTMX.
pub async fn remove_mapping(
    State(state): State<AppState>,
    Path(command): Path<String>,
) -> AppResult<MappingsListTemplate> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let mut config = dtx_core::ProjectConfig::load(&project_root).unwrap_or_default();
    config.remove_mapping(&command);
    config
        .save(&project_root)
        .map_err(|e| AppError::internal(e.to_string()))?;

    let custom_mappings: Vec<(String, String)> = config.mappings.packages.into_iter().collect();

    Ok(MappingsListTemplate { custom_mappings })
}

/// Mark a command as a local binary via HTMX.
pub async fn mark_as_local(
    State(state): State<AppState>,
    Path(command): Path<String>,
) -> AppResult<MappingsPanelTemplate> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let mut config = dtx_core::ProjectConfig::load(&project_root).unwrap_or_default();
    config.add_local(&command);
    config
        .save(&project_root)
        .map_err(|e| AppError::internal(e.to_string()))?;

    // Drop store read lock before re-calling
    drop(store);

    mappings_panel(State(state)).await
}

/// Mark a command as ignored via HTMX.
pub async fn mark_as_ignore(
    State(state): State<AppState>,
    Path(command): Path<String>,
) -> AppResult<MappingsPanelTemplate> {
    let store = state.store.read().await;
    let project_root = store.project_root().to_path_buf();

    let mut config = dtx_core::ProjectConfig::load(&project_root).unwrap_or_default();
    config.add_ignore(&command);
    config
        .save(&project_root)
        .map_err(|e| AppError::internal(e.to_string()))?;

    // Drop store read lock before re-calling
    drop(store);

    mappings_panel(State(state)).await
}
