//! Global instance registry for multi-project web server discovery.
//!
//! Tracks running `dtx web` instances in `~/.dtx/instances.json` so that
//! subsequent `dtx web` invocations can detect an existing server and register
//! their project instead of starting a new one.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::config::project::global_dtx_dir;

const INSTANCES_FILE: &str = "instances.json";

/// A running dtx-web server instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceEntry {
    pub port: u16,
    pub pid: u32,
    pub started_at: DateTime<Utc>,
}

/// Guard that removes the instance entry on drop.
pub struct InstanceGuard {
    port: u16,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if let Err(e) = remove_instance(self.port) {
            debug!("Failed to remove instance entry for port {}: {}", self.port, e);
        }
    }
}

/// Register a running instance. Returns a guard that cleans up on drop.
pub fn register_instance(port: u16) -> io::Result<InstanceGuard> {
    let path = instances_path()?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut instances = load_instances(&path);
    cleanup_stale(&mut instances);

    instances.insert(
        port.to_string(),
        InstanceEntry {
            port,
            pid: std::process::id(),
            started_at: Utc::now(),
        },
    );

    write_atomic(&path, &instances)?;
    debug!("Registered instance on port {}", port);

    Ok(InstanceGuard { port })
}

/// Find a running instance, cleaning up stale entries.
pub fn find_running_instance() -> Option<InstanceEntry> {
    let path = instances_path().ok()?;
    let mut instances = load_instances(&path);
    cleanup_stale(&mut instances);

    // Write back cleaned entries
    let _ = write_atomic(&path, &instances);

    // Return the first live instance
    instances.into_values().next()
}

fn remove_instance(port: u16) -> io::Result<()> {
    let path = instances_path()?;
    let mut instances = load_instances(&path);
    instances.remove(&port.to_string());
    write_atomic(&path, &instances)
}

fn instances_path() -> io::Result<PathBuf> {
    global_dtx_dir()
        .map(|dir| dir.join(INSTANCES_FILE))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Cannot determine home directory"))
}

fn load_instances(path: &PathBuf) -> HashMap<String, InstanceEntry> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Remove entries whose PID is no longer alive.
fn cleanup_stale(instances: &mut HashMap<String, InstanceEntry>) {
    instances.retain(|_, entry| is_pid_alive(entry.pid));
}

fn is_pid_alive(pid: u32) -> bool {
    // kill(pid, 0) checks if process exists without sending a signal
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Atomic write: write to tmp file, then rename.
fn write_atomic(path: &PathBuf, instances: &HashMap<String, InstanceEntry>) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string_pretty(instances)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_find() {
        let guard = register_instance(19876).unwrap();

        // Verify our instance exists in the registry
        let path = instances_path().unwrap();
        let instances = load_instances(&path);
        let entry = instances.get("19876").expect("our instance must be in registry");
        assert_eq!(entry.port, 19876);
        assert_eq!(entry.pid, std::process::id());

        drop(guard);

        // After drop, our entry should be removed
        let instances = load_instances(&path);
        assert!(!instances.contains_key("19876"));
    }
}
