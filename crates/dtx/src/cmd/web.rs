//! Start the web UI.

use crate::output::Output;
use anyhow::Result;
use dtx_core::events::instance::{find_running_instance, register_instance, InstanceEntry};
use dtx_core::store::ConfigStore;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

/// Run the web UI.
///
/// If `port` is `Some`, always start a new server on that port.
/// If `port` is `None`, try to join an existing instance; if none found, start on 3000.
pub async fn run(out: &Output, port: Option<u16>, open: bool) -> Result<()> {
    let store = ConfigStore::discover_and_load()?;
    let project_root = store.project_root().to_path_buf();

    match port {
        Some(p) => start_server(out, store, p, open).await,
        None => {
            if let Some(instance) = find_running_instance() {
                register_with_instance(out, &instance, &project_root, open).await
            } else {
                start_server(out, store, 3000, open).await
            }
        }
    }
}

/// Register this project with an existing web server instance.
async fn register_with_instance(
    out: &Output,
    instance: &InstanceEntry,
    project_root: &PathBuf,
    open: bool,
) -> Result<()> {
    let base_url = format!("http://127.0.0.1:{}", instance.port);

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/projects/register", base_url))
        .json(&serde_json::json!({ "root": project_root.display().to_string() }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to register with existing instance: {} {}", status, body);
    }

    let body: serde_json::Value = resp.json().await?;
    let project_id = body["id"].as_str().unwrap_or("unknown");
    let url = format!("{}/?project={}", base_url, project_id);

    out.step("web")
        .done_untimed(&format!("registered with {}", base_url));

    if open {
        open_browser(&url);
    }

    Ok(())
}

/// Start a new web server instance.
async fn start_server(out: &Output, store: ConfigStore, port: u16, open: bool) -> Result<()> {
    let project_dir = store.project_root().to_path_buf();

    let config = dtx_web::config::WebConfig::from_env();
    let state = dtx_web::AppState::new(store, config);

    let shutdown_token = state.shutdown_token().clone();
    let orchestrator_handle = state.orchestrator_handle().clone();

    // Register this instance globally for discovery
    let _instance_guard = match register_instance(port) {
        Ok(guard) => Some(guard),
        Err(e) => {
            out.warning(&format!("instance registry: {}", e));
            None
        }
    };

    // Start Unix socket listener for CLI -> Web event notifications
    let socket_guard =
        match dtx_core::events::start_event_listener(state.event_bus().clone()).await {
            Ok(guard) => Some(guard),
            Err(e) => {
                out.warning(&format!("event socket: {}", e));
                None
            }
        };

    // Spawn config reload task: when CLI sends ConfigChanged via socket,
    // reload config.yaml from disk so the web UI reflects the change.
    {
        let store_for_reload = state.store().clone();
        let bus = state.event_bus().clone();
        let filter = dtx_core::EventFilter::new().without_logs();
        let mut sub = bus.subscribe_filtered(filter);
        tokio::spawn(async move {
            while let Some(event) = sub.recv().await {
                if matches!(event, dtx_core::LifecycleEvent::ConfigChanged { .. }) {
                    tracing::debug!("Config changed externally, reloading");
                    if let Err(e) = store_for_reload.write().await.reload() {
                        tracing::debug!("Failed to reload config: {}", e);
                    }
                }
            }
        });
    }

    let app = dtx_web::create_router(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    out.step("web").done_untimed(&format!("http://{}", addr));

    if open {
        open_browser(&format!("http://{}", addr));
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Handle graceful shutdown on SIGTERM/SIGINT
    let out_shutdown = out.clone();
    let shutdown = async move {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }

        let mut stop_step = out_shutdown.step("web");
        stop_step.animate("shutting down");

        shutdown_token.cancel();

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        if let Err(e) = orchestrator_handle.shutdown().await {
            tracing::warn!(error = ?e, "Orchestrator shutdown error");
        }

        stop_step.done("stopped");
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    // Cleanup: drop socket guard explicitly and remove any stale sockets
    drop(socket_guard);
    cleanup_sockets(&project_dir);

    Ok(())
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

/// Cleans up stale socket files in .dtx directory.
fn cleanup_sockets(project_dir: &Path) {
    let dtx_dir = project_dir.join(".dtx");
    if let Ok(entries) = std::fs::read_dir(&dtx_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "sock") {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}
