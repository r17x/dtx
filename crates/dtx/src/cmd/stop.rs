//! Stop services.

use crate::context::Context;
use crate::output::Output;
use anyhow::Result;
use dtx_core::event_socket_path;
use tracing::{debug, info};

/// Default port for web server.
const DEFAULT_WEB_PORT: u16 = 3000;

/// Run the stop command.
pub async fn run(ctx: &Context, out: &Output, service: Option<String>) -> Result<()> {
    // Check if web server is running by looking for socket
    let socket_path = event_socket_path();
    let web_running = socket_path.as_ref().map(|p| p.exists()).unwrap_or(false);

    if !web_running {
        out.step("stop").done_untimed("nothing running");
        return Ok(());
    }

    let project_id = ctx.store.project_root().to_string_lossy().to_string();

    if let Some(ref name) = service {
        out.step(name).fail_untimed("single-service stop not yet supported");
        Ok(())
    } else {
        stop_all_via_api(out, &project_id).await
    }
}

/// Stops all services by calling the web server API.
async fn stop_all_via_api(out: &Output, project_id: &str) -> Result<()> {
    let mut step = out.step("stop");
    step.animate("stopping");

    let url = format!(
        "http://127.0.0.1:{}/api/projects/{}/stop",
        DEFAULT_WEB_PORT, project_id
    );

    debug!(url = %url, "Calling stop API");

    let client = reqwest::Client::new();
    match client.post(&url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                info!(project_id = %project_id, "Services stopped via API");
                step.done_untimed("all services stopped");
                Ok(())
            } else {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                debug!(status = %status, body = %body, "API error response");

                if status.as_u16() == 400 && body.contains("not running") {
                    step.done_untimed("none running");
                } else {
                    step.fail_untimed(&format!("HTTP {} — {}", status, body.trim()));
                }
                Ok(())
            }
        }
        Err(e) => {
            if e.is_connect() {
                step.fail_untimed(&format!("can't reach web server on port {}", DEFAULT_WEB_PORT));
                Ok(())
            } else {
                step.fail_untimed(&format!("{}", e));
                Ok(())
            }
        }
    }
}
