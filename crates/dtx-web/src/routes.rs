//! Route definitions.

use axum::{
    routing::{delete, get, post, put},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::static_files::EmbeddedStaticFiles;

use crate::handlers;
use crate::state::AppState;

/// Creates the main router with all routes.
pub fn create_router(state: AppState) -> Router {
    // API routes (JSON)
    let api_routes = Router::new()
        // Project metadata
        .route("/project", get(handlers::api::get_project))
        .route("/project/init-cwd", post(handlers::api::init_cwd))
        // Services
        .route("/services", get(handlers::api::list_services))
        .route("/services", post(handlers::api::create_service))
        .route("/services/:name", get(handlers::api::get_service))
        .route("/services/:name", put(handlers::api::update_service))
        .route("/services/:name", delete(handlers::api::delete_service))
        // Process control
        .route("/start", post(handlers::api::start_services))
        .route("/stop", post(handlers::api::stop_services))
        .route("/restart", post(handlers::api::restart_services))
        .route("/status", get(handlers::api::get_status))
        // Nix package search
        .route("/nix/search", get(handlers::api::search_packages))
        .route(
            "/nix/validate/:package",
            get(handlers::api::validate_package),
        )
        // Nix environment management
        .route("/nix/status", get(handlers::api::get_nix_status))
        .route("/nix/init", post(handlers::api::nix_init))
        .route("/nix/envrc", post(handlers::api::nix_envrc))
        .route("/nix/flake", get(handlers::api::download_flake))
        // Project config & mappings
        .route("/config", get(handlers::api::get_project_config))
        .route("/mappings", post(handlers::api::add_mapping))
        .route(
            "/mappings/:command",
            delete(handlers::api::remove_mapping),
        )
        .route("/commands", post(handlers::api::add_to_command_list))
        .route("/packages", get(handlers::api::get_package_analysis))
        // Inline file editing
        .route(
            "/files/:file_type",
            get(handlers::api::read_file).put(handlers::api::save_file),
        )
        .route(
            "/files/:file_type/validate",
            post(handlers::api::validate_file),
        );

    // HTML routes (full pages)
    let html_routes = Router::new()
        .route("/", get(handlers::html::index))
        .route("/services", get(handlers::html::services_page))
        .route("/settings", get(handlers::html::mappings_page))
        .route("/search", get(handlers::html::search_page));

    // HTMX partial routes
    let htmx_routes = Router::new()
        // Partials (GET)
        .route("/partials/services", get(handlers::htmx::services_list))
        // Service detail panel
        .route(
            "/services/:service_name",
            get(handlers::htmx::service_detail),
        )
        .route("/partials/status", get(handlers::htmx::status_panel))
        .route(
            "/partials/status-live",
            get(handlers::htmx::status_panel_live),
        )
        .route(
            "/partials/search-results",
            get(handlers::htmx::search_results),
        )
        .route("/partials/service-form", get(handlers::htmx::service_form))
        .route("/partials/logs/:service", get(handlers::htmx::logs_panel))
        .route(
            "/partials/logs-live/:service",
            get(handlers::htmx::logs_panel_live),
        )
        .route("/partials/nix-panel", get(handlers::htmx::nix_panel))
        // Form actions (POST/DELETE) - return HTML partials
        .route("/services", post(handlers::htmx::create_service))
        .route(
            "/services/:service_name",
            delete(handlers::htmx::delete_service),
        )
        // Add package from search
        .route("/add-package", post(handlers::htmx::add_package))
        // Package mappings
        .route("/partials/mappings", get(handlers::htmx::mappings_panel))
        .route("/mappings", post(handlers::htmx::add_mapping))
        .route(
            "/mappings/:command",
            delete(handlers::htmx::remove_mapping),
        )
        .route(
            "/mark-local/:command",
            post(handlers::htmx::mark_as_local),
        )
        .route(
            "/mark-ignore/:command",
            post(handlers::htmx::mark_as_ignore),
        );

    // SSE routes (Server-Sent Events)
    let sse_routes = Router::new()
        .route("/status", get(handlers::sse::status_stream))
        .route("/logs/:service", get(handlers::sse::logs_stream))
        .route("/logs", get(handlers::sse::all_logs_stream))
        .route("/events", get(handlers::sse::events_stream));

    // Combine all routes
    Router::new()
        .nest("/api", api_routes)
        .nest("/htmx", htmx_routes)
        .nest("/sse", sse_routes)
        .merge(html_routes)
        .nest_service("/static", EmbeddedStaticFiles::new())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}
