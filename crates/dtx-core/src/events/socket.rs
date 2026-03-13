//! Unix socket IPC for cross-process event notification.
//!
//! When `dtx web` is running, it creates a Unix socket at `.dtx/events.sock`.
//! CLI commands (e.g., `dtx add`, `dtx remove`) connect to this socket to
//! notify the web server of configuration changes, enabling instant UI updates
//! without polling.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, warn};

use super::lifecycle::LifecycleEvent;
use super::resource_bus::ResourceEventBus;
use crate::config::project::{find_project_root_cwd, DTX_DIR};

const SOCKET_FILENAME: &str = "events.sock";

/// Returns the path to the event socket file (`.dtx/events.sock`).
///
/// Discovers the project root by walking up from CWD, then appends
/// `.dtx/events.sock`. Returns `None` if no `.dtx` directory is found.
pub fn event_socket_path() -> Option<PathBuf> {
    find_project_root_cwd().map(|root| root.join(DTX_DIR).join(SOCKET_FILENAME))
}

/// Send a JSON message to the event socket (async).
/// Fire-and-forget: silently ignores errors (web server may not be running).
async fn send_event_async(msg: serde_json::Value) {
    let Some(path) = event_socket_path() else {
        debug!("No .dtx directory found, skipping event notification");
        return;
    };

    if !path.exists() {
        debug!(
            "Event socket not found at {}, web server not running",
            path.display()
        );
        return;
    }

    let payload = format!("{}\n", msg);
    match UnixStream::connect(&path).await {
        Ok(mut stream) => {
            if let Err(e) = stream.write_all(payload.as_bytes()).await {
                debug!("Failed to write to event socket: {}", e);
            }
        }
        Err(e) => {
            debug!("Failed to connect to event socket: {}", e);
        }
    }
}

/// Send a JSON message to the event socket (sync).
/// Uses std::os::unix::net::UnixStream — no tokio runtime needed.
fn send_event_sync(msg: serde_json::Value) {
    let Some(path) = event_socket_path() else {
        return;
    };

    if !path.exists() {
        return;
    }

    let payload = format!("{}\n", msg);
    match std::os::unix::net::UnixStream::connect(&path) {
        Ok(mut stream) => {
            use std::io::Write;
            if let Err(e) = stream.write_all(payload.as_bytes()) {
                debug!("Failed to write to event socket: {}", e);
            }
        }
        Err(e) => {
            debug!("Failed to connect to event socket: {}", e);
        }
    }
}

pub async fn notify_config_changed(project_id: &str) {
    send_event_async(serde_json::json!({
        "event": "config_changed",
        "project_id": project_id,
    }))
    .await;
}

pub fn notify_config_changed_sync() {
    send_event_sync(serde_json::json!({
        "event": "config_changed",
        "project_id": "",
    }));
}

pub async fn notify_memory_changed(project_id: &str, memory_name: &str) {
    send_event_async(serde_json::json!({
        "event": "memory_changed",
        "project_id": project_id,
        "memory_name": memory_name,
    }))
    .await;
}

pub fn notify_memory_changed_sync(project_id: &str, memory_name: &str) {
    send_event_sync(serde_json::json!({
        "event": "memory_changed",
        "project_id": project_id,
        "memory_name": memory_name,
    }));
}

/// Start the Unix socket event listener.
///
/// Creates `.dtx/events.sock`, accepts connections, reads JSON messages,
/// and publishes them to the ResourceEventBus. The socket file is cleaned up
/// on drop via the returned guard.
///
/// This function runs forever — spawn it as a tokio task.
pub async fn start_event_listener(event_bus: Arc<ResourceEventBus>) -> crate::Result<SocketGuard> {
    let path = event_socket_path().ok_or_else(|| {
        crate::CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No .dtx directory found",
        ))
    })?;

    // Remove stale socket file from previous run
    let _ = std::fs::remove_file(&path);

    // Ensure .dtx directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(crate::CoreError::Io)?;
    }

    let listener = UnixListener::bind(&path).map_err(crate::CoreError::Io)?;
    debug!("Event socket listening on {}", path.display());

    let guard = SocketGuard { path: path.clone() };

    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let bus = event_bus.clone();
                    tokio::spawn(async move {
                        handle_connection(stream, bus).await;
                    });
                }
                Err(e) => {
                    warn!("Failed to accept socket connection: {}", e);
                }
            }
        }
    });

    Ok(guard)
}

/// Reads newline-delimited JSON from a socket connection and publishes events.
async fn handle_connection(stream: UnixStream, event_bus: Arc<ResourceEventBus>) {
    let reader = BufReader::new(stream);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(msg) => {
                let event_type = msg.get("event").and_then(|v| v.as_str()).unwrap_or("");
                let project_id = msg
                    .get("project_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let event = match event_type {
                    "memory_changed" => {
                        let memory_name = msg
                            .get("memory_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        LifecycleEvent::MemoryChanged {
                            project_id,
                            memory_name,
                            timestamp: Utc::now(),
                        }
                    }
                    _ => LifecycleEvent::ConfigChanged {
                        project_id,
                        timestamp: Utc::now(),
                    },
                };

                let count = event_bus.publish(event);
                debug!("Published {} event to {} subscribers", event_type, count);
            }
            Err(e) => {
                warn!("Failed to parse socket message: {}", e);
            }
        }
    }
}

/// Guard that writes a port file and removes it on drop.
///
/// Used by `dtx web` to advertise its listening port so other
/// commands (e.g., `dtx stop`, `dtx status`) can discover the
/// web server without configuration.
pub struct PortGuard {
    path: PathBuf,
}

impl PortGuard {
    /// Create a port file at `<dtx_dir>/web.port` containing the port number.
    pub fn new(dtx_dir: &std::path::Path, port: u16) -> std::io::Result<Self> {
        let path = dtx_dir.join("web.port");
        std::fs::write(&path, port.to_string())?;
        Ok(Self { path })
    }
}

impl Drop for PortGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Read the web port from a project's `.dtx` directory, if running.
///
/// Returns `None` if the port file does not exist or contains invalid data.
pub fn read_web_port(dtx_dir: &std::path::Path) -> Option<u16> {
    let path = dtx_dir.join("web.port");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Guard that removes the socket file when dropped.
pub struct SocketGuard {
    path: PathBuf,
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_file(&self.path) {
            debug!(
                "Failed to remove socket file {}: {}",
                self.path.display(),
                e
            );
        } else {
            debug!("Cleaned up event socket {}", self.path.display());
        }
    }
}
