//! View service logs.

use crate::context::Context;
use crate::output::Output;
use anyhow::Result;
use dtx_core::event_socket_path;
use futures::StreamExt;
use tracing::debug;

/// Default port for web server.
const DEFAULT_WEB_PORT: u16 = 3000;

/// Run the logs command.
pub async fn run(
    ctx: &Context,
    out: &Output,
    service: Option<String>,
    _all: bool,
    follow: bool,
) -> Result<()> {
    // Check if web server is running by looking for socket
    let socket_path = event_socket_path();
    let web_running = socket_path.as_ref().map(|p| p.exists()).unwrap_or(false);

    if !web_running {
        out.step("logs").fail_untimed("no running instance");
        return Ok(());
    }

    let project_id = ctx.store.project_root().to_string_lossy().to_string();

    if follow {
        stream_logs_sse(out, &project_id, service.as_deref()).await
    } else {
        out.step("logs").fail_untimed("use -f to stream logs");
        Ok(())
    }
}

/// Streams logs via SSE endpoint.
async fn stream_logs_sse(out: &Output, project_id: &str, service: Option<&str>) -> Result<()> {
    let url = match service {
        Some(name) => format!(
            "http://127.0.0.1:{}/sse/logs/{}/{}",
            DEFAULT_WEB_PORT, project_id, name
        ),
        None => format!(
            "http://127.0.0.1:{}/sse/logs/{}",
            DEFAULT_WEB_PORT, project_id
        ),
    };

    debug!(url = %url, "Connecting to SSE log stream");

    out.separator("logs (ctrl+c to stop)");

    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Accept", "text/event-stream")
        .send()
        .await;

    match response {
        Ok(resp) => {
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                out.step("logs")
                    .fail_untimed(&format!("HTTP {} — {}", status, body.trim()));
                return Ok(());
            }

            let mut stream = resp.bytes_stream();

            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes);
                        for line in text.lines() {
                            if let Some(data) = line.strip_prefix("data: ") {
                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                    if let (Some(service), Some(content)) = (
                                        json.get("service").and_then(|v| v.as_str()),
                                        json.get("content").and_then(|v| v.as_str()),
                                    ) {
                                        let is_stderr = json
                                            .get("is_stderr")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false);

                                        out.log(service, content, is_stderr);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        debug!(error = ?e, "Stream error");
                    }
                }
            }

            Ok(())
        }
        Err(e) => {
            if e.is_connect() {
                out.step("logs").fail_untimed(&format!(
                    "can't reach web server on port {}",
                    DEFAULT_WEB_PORT
                ));
                Ok(())
            } else {
                out.step("logs").fail_untimed(&format!("{}", e));
                Ok(())
            }
        }
    }
}
