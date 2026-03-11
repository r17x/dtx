//! Show service status.

use crate::context::Context;
use crate::output::{Cell, Output};
use anyhow::Result;
use tracing::debug;

/// Run the status command.
pub async fn run(ctx: &Context, out: &Output, service: Option<String>) -> Result<()> {
    // Show project info
    out.step("project")
        .done_untimed(ctx.store.project_name());

    // Check if web server is running (try live status, fall back to config)
    let dtx_dir = ctx.store.project_root().join(".dtx");
    let web_port = dtx_core::read_web_port(&dtx_dir);

    if let Some(port) = web_port {
        let project_id = ctx.store.project_root().to_string_lossy().to_string();
        show_live_status(out, &project_id, port, service.as_deref()).await?;
    } else {
        show_config_status(ctx, out, service.as_deref())?;
    }

    Ok(())
}

/// Shows live service status from the API.
async fn show_live_status(out: &Output, project_id: &str, port: u16, filter: Option<&str>) -> Result<()> {
    let url = format!(
        "http://127.0.0.1:{}/api/projects/{}/status",
        port, project_id
    );

    debug!(url = %url, "Fetching status from API");

    let client = reqwest::Client::new();
    match client.get(&url).send().await {
        Ok(response) => {
            if !response.status().is_success() {
                out.step("status").fail_untimed("web server returned error");
                return Ok(());
            }

            let json: serde_json::Value = response.json().await?;

            let running = json
                .get("running")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if !running {
                out.step("status").done_untimed("none running");
                return Ok(());
            }

            let services = json.get("services").and_then(|v| v.as_array());

            if let Some(services) = services {
                let mut table = out
                    .table()
                    .headers(vec!["SERVICE", "STATUS", "PID", "UPTIME"]);

                for svc in services {
                    let name = svc.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let status = svc.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                    let pid = svc.get("pid").and_then(|v| v.as_u64()).unwrap_or(0);

                    if let Some(filter_name) = filter {
                        if name != filter_name {
                            continue;
                        }
                    }

                    let pid_str = if pid > 0 {
                        pid.to_string()
                    } else {
                        "-".to_string()
                    };

                    let status_cell = match status {
                        "Running" => Cell::colored("Running", "\x1b[32m"),
                        "Starting" => Cell::colored("Starting", "\x1b[33m"),
                        "Failed" => Cell::colored("Failed", "\x1b[31m"),
                        other => Cell::new(other),
                    };

                    table = table.row(vec![
                        Cell::new(name),
                        status_cell,
                        Cell::new(pid_str),
                        Cell::new("-"),
                    ]);
                }

                out.print_table(table);
            }

            Ok(())
        }
        Err(e) => {
            if e.is_connect() {
                out.step("status").fail_untimed(&format!("can't reach web server on port {}", port));
                Ok(())
            } else {
                out.step("status").fail_untimed(&format!("{}", e));
                Ok(())
            }
        }
    }
}

/// Shows service status from config (no live status).
fn show_config_status(ctx: &Context, out: &Output, filter: Option<&str>) -> Result<()> {
    let resources: Vec<_> = ctx.store.list_resources().collect();

    if resources.is_empty() {
        out.step("status").done_untimed("no services configured");
        return Ok(());
    }

    // Filter if specific service requested
    let resources: Vec<_> = if let Some(name) = filter {
        resources.into_iter().filter(|(n, _)| *n == name).collect()
    } else {
        resources
    };

    if resources.is_empty() {
        if let Some(name) = filter {
            out.step(name).fail_untimed("not found");
            return Ok(());
        }
    }

    let mut table = out
        .table()
        .headers(vec!["SERVICE", "ENABLED", "PORT", "COMMAND"]);

    for (name, rc) in resources {
        let enabled = if rc.enabled { "yes" } else { "no" };
        let port = rc
            .port
            .map(|p| p.to_string())
            .unwrap_or_else(|| "-".to_string());
        let cmd = rc.command.as_deref().unwrap_or("-").to_string();

        table = table.row(vec![
            Cell::new(name),
            Cell::new(enabled),
            Cell::new(port),
            Cell::new(cmd),
        ]);
    }

    out.print_table(table);

    Ok(())
}
