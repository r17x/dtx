//! VM runtime trait and common types.
//!
//! This module defines the abstract `VmRuntime` trait that all VM backends
//! (QEMU, Firecracker, NixOS) implement.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::config::VmConfig;
use crate::error::Result;
use dtx_core::resource::HealthStatus;

/// VM information returned by inspect.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmInfo {
    /// VM identifier.
    pub id: String,
    /// Current state.
    pub state: VmState,
    /// Process ID of the VM hypervisor.
    pub pid: Option<u32>,
    /// Boot time.
    pub started_at: Option<DateTime<Utc>>,
    /// IP address (if known).
    pub ip_address: Option<String>,
    /// SSH port on host.
    pub ssh_port: Option<u16>,
    /// VNC/SPICE port.
    pub display_port: Option<u16>,
    /// QMP socket path (QEMU).
    pub qmp_socket: Option<PathBuf>,
    /// Console socket path.
    pub console_socket: Option<PathBuf>,
}

impl VmInfo {
    /// Create a new VM info with just ID and state.
    pub fn new(id: impl Into<String>, state: VmState) -> Self {
        Self {
            id: id.into(),
            state,
            pid: None,
            started_at: None,
            ip_address: None,
            ssh_port: None,
            display_port: None,
            qmp_socket: None,
            console_socket: None,
        }
    }

    /// Check if the VM is running.
    pub fn is_running(&self) -> bool {
        matches!(self.state, VmState::Running)
    }
}

/// VM state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmState {
    /// VM is created but not started.
    Pending,
    /// VM is booting.
    Booting,
    /// VM is running.
    Running,
    /// VM is paused/suspended.
    Paused,
    /// VM is shut off.
    Shutoff,
    /// VM has crashed.
    Crashed,
}

impl VmState {
    /// Check if the VM is in a running state.
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if the VM is in a stopped state.
    pub fn is_stopped(&self) -> bool {
        matches!(self, Self::Shutoff | Self::Crashed)
    }
}

impl std::fmt::Display for VmState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Booting => write!(f, "booting"),
            Self::Running => write!(f, "running"),
            Self::Paused => write!(f, "paused"),
            Self::Shutoff => write!(f, "shutoff"),
            Self::Crashed => write!(f, "crashed"),
        }
    }
}

/// Result of command execution in a VM.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecResult {
    /// Exit code of the command.
    pub exit_code: i32,
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
}

impl ExecResult {
    /// Create a new exec result.
    pub fn new(exit_code: i32, stdout: impl Into<String>, stderr: impl Into<String>) -> Self {
        Self {
            exit_code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        }
    }

    /// Check if the command succeeded (exit code 0).
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Snapshot information.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotInfo {
    /// Snapshot name.
    pub name: String,
    /// When the snapshot was created.
    pub created_at: DateTime<Utc>,
    /// Snapshot size in bytes.
    pub size_bytes: u64,
    /// Optional description.
    pub description: Option<String>,
}

impl SnapshotInfo {
    /// Create a new snapshot info.
    pub fn new(name: impl Into<String>, created_at: DateTime<Utc>, size_bytes: u64) -> Self {
        Self {
            name: name.into(),
            created_at,
            size_bytes,
            description: None,
        }
    }
}

/// Abstract VM runtime interface.
///
/// All VM backends (QEMU, Firecracker, NixOS) implement this trait,
/// providing a uniform interface for VM lifecycle management.
#[async_trait]
pub trait VmRuntime: Send + Sync {
    /// Runtime name (e.g., "qemu", "firecracker", "nixos").
    fn name(&self) -> &str;

    /// Check if runtime is available on this system.
    async fn is_available(&self) -> bool;

    /// Prepare VM image (download, build, convert as needed).
    ///
    /// Returns the path to the prepared image.
    async fn prepare_image(&self, config: &VmConfig) -> Result<PathBuf>;

    /// Create VM (without starting).
    ///
    /// Returns the VM ID.
    async fn create(&self, config: &VmConfig) -> Result<String>;

    /// Start VM with configuration.
    ///
    /// This starts a previously created or new VM.
    async fn start(&self, config: &VmConfig, image_path: &std::path::Path) -> Result<VmInfo>;

    /// Stop VM gracefully (ACPI shutdown).
    async fn stop(&self, id: &str, timeout: Duration) -> Result<()>;

    /// Force stop VM (SIGKILL).
    async fn kill(&self, id: &str) -> Result<()>;

    /// Pause VM.
    async fn pause(&self, id: &str) -> Result<()>;

    /// Resume paused VM.
    async fn resume(&self, id: &str) -> Result<()>;

    /// Restart VM.
    async fn restart(&self, id: &str, config: &VmConfig) -> Result<()>;

    /// Get VM info.
    async fn inspect(&self, id: &str) -> Result<VmInfo>;

    /// Check if VM is running.
    async fn is_running(&self, id: &str) -> Result<bool>;

    /// Wait for VM to boot (SSH available or other check).
    async fn wait_for_boot(&self, id: &str, config: &VmConfig, timeout: Duration) -> Result<()>;

    /// Execute command in VM via SSH or other mechanism.
    async fn exec(&self, id: &str, command: &[String], config: &VmConfig) -> Result<ExecResult>;

    /// Get VM console output.
    async fn console_log(&self, id: &str, lines: Option<usize>) -> Result<String>;

    /// Remove VM and cleanup resources.
    async fn remove(&self, id: &str) -> Result<()>;

    /// Get health status of VM.
    async fn health(&self, id: &str, config: &VmConfig) -> Result<HealthStatus>;

    /// Create a snapshot of the VM.
    async fn snapshot(&self, id: &str, name: &str) -> Result<String>;

    /// Restore a snapshot.
    async fn restore_snapshot(&self, id: &str, snapshot_name: &str) -> Result<()>;

    /// List available snapshots.
    async fn list_snapshots(&self, id: &str) -> Result<Vec<SnapshotInfo>>;
}

/// Select the best available VM runtime.
///
/// Tries runtimes in order of preference: QEMU, Firecracker, NixOS.
#[cfg(any(feature = "qemu", feature = "firecracker", feature = "nixos"))]
pub async fn detect_runtime() -> Option<Box<dyn VmRuntime>> {
    #[cfg(feature = "qemu")]
    {
        let qemu = crate::qemu::QemuRuntime::new();
        if qemu.is_available().await {
            return Some(Box::new(qemu));
        }
    }

    #[cfg(feature = "firecracker")]
    {
        let firecracker = crate::firecracker::FirecrackerRuntime::new();
        if firecracker.is_available().await {
            return Some(Box::new(firecracker));
        }
    }

    #[cfg(feature = "nixos")]
    {
        let nixos = crate::nixos::NixosVmRuntime::new();
        if nixos.is_available().await {
            return Some(Box::new(nixos));
        }
    }

    None
}

/// Select runtime based on configuration.
#[cfg(any(feature = "qemu", feature = "firecracker", feature = "nixos"))]
pub async fn runtime_for_config(config: &VmConfig) -> Option<Box<dyn VmRuntime>> {
    use crate::config::VmRuntimeType;

    match config.runtime {
        VmRuntimeType::Auto => detect_runtime().await,
        #[cfg(feature = "qemu")]
        VmRuntimeType::Qemu => {
            let rt = crate::qemu::QemuRuntime::new();
            if rt.is_available().await {
                Some(Box::new(rt))
            } else {
                None
            }
        }
        #[cfg(feature = "firecracker")]
        VmRuntimeType::Firecracker => {
            let rt = crate::firecracker::FirecrackerRuntime::new();
            if rt.is_available().await {
                Some(Box::new(rt))
            } else {
                None
            }
        }
        #[cfg(feature = "nixos")]
        VmRuntimeType::NixOS => {
            let rt = crate::nixos::NixosVmRuntime::new();
            if rt.is_available().await {
                Some(Box::new(rt))
            } else {
                None
            }
        }
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_state_display() {
        assert_eq!(VmState::Pending.to_string(), "pending");
        assert_eq!(VmState::Running.to_string(), "running");
        assert_eq!(VmState::Shutoff.to_string(), "shutoff");
    }

    #[test]
    fn vm_state_checks() {
        assert!(VmState::Running.is_running());
        assert!(!VmState::Pending.is_running());

        assert!(VmState::Shutoff.is_stopped());
        assert!(VmState::Crashed.is_stopped());
        assert!(!VmState::Running.is_stopped());
    }

    #[test]
    fn exec_result_success() {
        let result = ExecResult::new(0, "output", "");
        assert!(result.success());

        let result = ExecResult::new(1, "", "error");
        assert!(!result.success());
    }

    #[test]
    fn vm_info_new() {
        let info = VmInfo::new("test-vm", VmState::Running);
        assert_eq!(info.id, "test-vm");
        assert!(info.is_running());
        assert!(info.pid.is_none());
    }

    #[test]
    fn snapshot_info_new() {
        let now = Utc::now();
        let info = SnapshotInfo::new("snapshot-1", now, 1024 * 1024 * 100);

        assert_eq!(info.name, "snapshot-1");
        assert_eq!(info.size_bytes, 1024 * 1024 * 100);
        assert!(info.description.is_none());
    }

    #[test]
    fn vm_state_serde() {
        let state = VmState::Running;
        let json = serde_json::to_string(&state).expect("serialize");
        assert_eq!(json, "\"running\"");

        let parsed: VmState = serde_json::from_str("\"paused\"").expect("deserialize");
        assert_eq!(parsed, VmState::Paused);
    }

    #[test]
    fn exec_result_serde() {
        let result = ExecResult::new(0, "hello", "");
        let json = serde_json::to_string(&result).expect("serialize");
        let parsed: ExecResult = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.exit_code, 0);
        assert_eq!(parsed.stdout, "hello");
    }
}
