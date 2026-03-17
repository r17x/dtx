//! Start the web UI.

use crate::output::Output;
use anyhow::Result;
use dtx_core::store::ConfigStore;
use std::net::SocketAddr;
use std::path::Path;

/// Run the web UI.
pub async fn run(out: &Output, port: u16, open: bool) -> Result<()> {
    let store = ConfigStore::discover_and_load()?;
    let project_dir = store.project_root().to_path_buf();

    let config = dtx_web::config::WebConfig::from_env();
    let state = dtx_web::AppState::new(store, config);

    // Clone shutdown token and orchestrator handle for graceful shutdown
    let shutdown_token = state.shutdown_token().clone();
    let orchestrator_handle = state.orchestrator_handle().clone();

    // Start Unix socket listener for CLI -> Web event notifications
    let socket_guard = match dtx_core::events::start_event_listener(state.event_bus().clone()).await
    {
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
        let url = format!("http://{}", addr);
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&url).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
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
