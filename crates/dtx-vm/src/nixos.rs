//! NixOS VM runtime implementation.
//!
//! This module provides support for NixOS VMs using `nixos-rebuild build-vm`
//! or flake-based VM builds. This is ideal for:
//!
//! - Testing NixOS configurations before deployment
//! - Reproducible VM environments
//! - Integration testing with NixOS-based services

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::config::{VmConfig, VmImage};
use crate::error::{Result, VmError};
use crate::runtime::{ExecResult, SnapshotInfo, VmInfo, VmRuntime, VmState};
use dtx_core::resource::HealthStatus;

/// NixOS VM runtime using nixos-rebuild build-vm.
///
/// Provides first-class NixOS integration for testing configurations.
pub struct NixosVmRuntime {
    /// Base directory for VM state.
    state_dir: PathBuf,
    /// Running VMs.
    vms: Arc<RwLock<HashMap<String, NixosVm>>>,
}

/// Internal state for a running NixOS VM.
struct NixosVm {
    config: VmConfig,
    process: Child,
    ssh_port: u16,
    #[allow(dead_code)]
    qemu_pid: Option<u32>,
    started_at: chrono::DateTime<chrono::Utc>,
    #[allow(dead_code)]
    run_script: PathBuf,
    /// Path to console log file.
    log_file: PathBuf,
}

impl NixosVmRuntime {
    /// Create a new NixOS VM runtime.
    pub fn new() -> Self {
        Self {
            state_dir: std::env::temp_dir().join("dtx-vm-nixos"),
            vms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set state directory.
    pub fn with_state_dir(mut self, state_dir: PathBuf) -> Self {
        self.state_dir = state_dir;
        self
    }

    /// Build a NixOS VM from a flake reference.
    async fn build_nixos_vm(&self, flake: &str, attribute: Option<&str>) -> Result<PathBuf> {
        // Default attribute for NixOS VMs
        let attr = attribute.unwrap_or("nixosConfigurations.default.config.system.build.vm");

        info!(flake = %flake, attribute = %attr, "Building NixOS VM from flake");

        let output = Command::new("nix")
            .args([
                "build",
                "--no-link",
                "--print-out-paths",
                &format!("{}#{}", flake, attr),
            ])
            .output()
            .await
            .map_err(|e| VmError::NixOS(format!("nix build failed: {}", e)))?;

        if !output.status.success() {
            return Err(VmError::NixOS(format!(
                "nix build failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let vm_path = PathBuf::from(&path);

        // Find the run script
        let run_script = vm_path.join("bin").join("run-nixos-vm");
        if run_script.exists() {
            return Ok(run_script);
        }

        // Alternative location
        let alt_script = vm_path.join("bin").join("run-vm");
        if alt_script.exists() {
            return Ok(alt_script);
        }

        // Return the directory and let start() figure it out
        Ok(vm_path)
    }

    /// Execute SSH command to VM.
    async fn ssh_exec(&self, ssh_port: u16, user: &str, command: &[String]) -> Result<ExecResult> {
        let mut cmd = Command::new("ssh");
        cmd.args([
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "ConnectTimeout=5",
            "-o",
            "BatchMode=yes",
            "-p",
            &ssh_port.to_string(),
            &format!("{}@localhost", user),
        ]);
        cmd.args(command);

        let output = cmd
            .output()
            .await
            .map_err(|e| VmError::Ssh(format!("SSH failed: {}", e)))?;

        Ok(ExecResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Find an available port for SSH forwarding.
    async fn find_available_port(&self) -> u16 {
        // Try to find an available port starting from 2222
        for port in 2222..3000 {
            if let Ok(listener) = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await
            {
                drop(listener);
                return port;
            }
        }
        2222 // Fallback
    }

    /// Wait for SSH to be available.
    async fn wait_for_ssh(&self, ssh_port: u16, user: &str, timeout: Duration) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(VmError::Timeout(timeout));
            }

            // Try SSH
            let result = self.ssh_exec(ssh_port, user, &["true".to_string()]).await;

            if result.map(|r| r.success()).unwrap_or(false) {
                return Ok(());
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

impl Default for NixosVmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VmRuntime for NixosVmRuntime {
    fn name(&self) -> &str {
        "nixos"
    }

    async fn is_available(&self) -> bool {
        Command::new("nix")
            .args(["--version"])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn prepare_image(&self, config: &VmConfig) -> Result<PathBuf> {
        match &config.image {
            VmImage::NixosFlake { flake, attribute } => {
                self.build_nixos_vm(flake, attribute.as_deref()).await
            }
            VmImage::NixBuild {
                expression,
                attribute,
            } => {
                info!(expression = %expression, attribute = %attribute, "Building NixOS VM from expression");

                let output = Command::new("nix-build")
                    .args(["-E", expression, "-A", attribute, "--no-out-link"])
                    .output()
                    .await
                    .map_err(|e| VmError::NixOS(format!("nix-build failed: {}", e)))?;

                if !output.status.success() {
                    return Err(VmError::NixOS(
                        String::from_utf8_lossy(&output.stderr).to_string(),
                    ));
                }

                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Ok(PathBuf::from(path))
            }
            VmImage::File { path, .. } => {
                // Assume it's a NixOS VM run script
                if !path.exists() {
                    return Err(VmError::ImageNotFound(path.clone()));
                }
                Ok(path.clone())
            }
            _ => Err(VmError::InvalidConfig(
                "NixOS runtime requires NixosFlake or NixBuild image".to_string(),
            )),
        }
    }

    async fn create(&self, config: &VmConfig) -> Result<String> {
        tokio::fs::create_dir_all(&self.state_dir)
            .await
            .map_err(|e| VmError::Backend(format!("Failed to create state dir: {}", e)))?;

        Ok(config.id.as_str().to_string())
    }

    async fn start(&self, config: &VmConfig, image_path: &Path) -> Result<VmInfo> {
        let id = config.id.as_str();

        // Check if already running
        {
            let vms = self.vms.read().await;
            if vms.contains_key(id) {
                return Err(VmError::AlreadyRunning(id.to_string()));
            }
        }

        info!(id = %id, image = %image_path.display(), "Starting NixOS VM");

        // Determine SSH port - use configured port or find an available one
        let ssh_port = match config.ssh.as_ref().and_then(|s| s.host_port) {
            Some(port) => port,
            None => self.find_available_port().await,
        };

        // Find the run script
        let run_script = if image_path.is_file()
            && image_path
                .file_name()
                .map(|n| n.to_string_lossy().starts_with("run"))
                .unwrap_or(false)
        {
            image_path.to_path_buf()
        } else {
            // Look for run script in bin directory
            let bin_script = image_path.join("bin").join("run-nixos-vm");
            if bin_script.exists() {
                bin_script
            } else {
                // Fallback
                image_path.join("bin").join("run-vm")
            }
        };

        if !run_script.exists() {
            return Err(VmError::NixOS(format!(
                "Run script not found: {}",
                run_script.display()
            )));
        }

        debug!(run_script = %run_script.display(), ssh_port = %ssh_port, "Starting VM");

        // Create log file for console output
        let log_file = self.state_dir.join(format!("{}.log", id));
        let log_handle = std::fs::File::create(&log_file)
            .map_err(|e| VmError::NixOS(format!("Failed to create log file: {}", e)))?;

        // NixOS VMs are typically run via a shell script that sets up QEMU
        // We can pass QEMU options via environment variables
        let mut cmd = Command::new(&run_script);

        // Set SSH port forwarding via QEMU_NET_OPTS
        // The NixOS VM script will merge this with its default network config
        cmd.env("QEMU_NET_OPTS", format!("hostfwd=tcp::{}-:22", ssh_port));

        // Disable graphics for headless operation
        cmd.env("QEMU_OPTS", "-nographic");

        // Redirect stdout and stderr to log file
        cmd.stdout(
            log_handle
                .try_clone()
                .map_err(|e| VmError::NixOS(format!("Failed to clone log file handle: {}", e)))?,
        );
        cmd.stderr(log_handle);

        let process = cmd
            .spawn()
            .map_err(|e| VmError::NixOS(format!("Failed to spawn VM: {}", e)))?;

        let started_at = Utc::now();

        // Store VM state
        {
            let mut vms = self.vms.write().await;
            vms.insert(
                id.to_string(),
                NixosVm {
                    config: config.clone(),
                    process,
                    ssh_port,
                    qemu_pid: None,
                    started_at,
                    run_script: run_script.clone(),
                    log_file: log_file.clone(),
                },
            );
        }

        Ok(VmInfo {
            id: id.to_string(),
            state: VmState::Running,
            pid: None,
            started_at: Some(started_at),
            ip_address: Some("localhost".to_string()),
            ssh_port: Some(ssh_port),
            display_port: None,
            qmp_socket: None,
            console_socket: None,
        })
    }

    async fn stop(&self, id: &str, timeout: Duration) -> Result<()> {
        let (ssh_port, user) = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            let user = vm
                .config
                .ssh
                .as_ref()
                .map(|s| s.user.clone())
                .unwrap_or_else(|| "root".to_string());
            (vm.ssh_port, user)
        };

        info!(id = %id, "Stopping NixOS VM via SSH poweroff");

        // Try graceful shutdown via SSH
        let _ = self
            .ssh_exec(ssh_port, &user, &["poweroff".to_string()])
            .await;

        // Wait for process to exit
        tokio::time::sleep(timeout.min(Duration::from_secs(10))).await;

        // Force kill if still running
        self.kill(id).await
    }

    async fn kill(&self, id: &str) -> Result<()> {
        info!(id = %id, "Killing NixOS VM");

        let log_file = {
            let mut vms = self.vms.write().await;
            if let Some(mut vm) = vms.remove(id) {
                // Send SIGKILL to the process
                if let Err(e) = vm.process.kill().await {
                    debug!(id = %id, error = %e, "Failed to kill VM process (may have already exited)");
                }
                Some(vm.log_file)
            } else {
                None
            }
        };

        // Cleanup log file if we had one
        if let Some(path) = log_file {
            let _ = tokio::fs::remove_file(&path).await;
        } else {
            // Try the default path
            let _ = tokio::fs::remove_file(self.state_dir.join(format!("{}.log", id))).await;
        }

        Ok(())
    }

    async fn pause(&self, _id: &str) -> Result<()> {
        Err(VmError::not_supported("VM pause", "nixos"))
    }

    async fn resume(&self, _id: &str) -> Result<()> {
        Err(VmError::not_supported("VM resume", "nixos"))
    }

    async fn restart(&self, id: &str, config: &VmConfig) -> Result<()> {
        let (ssh_port, user) = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            let user = vm
                .config
                .ssh
                .as_ref()
                .map(|s| s.user.clone())
                .unwrap_or_else(|| "root".to_string());
            (vm.ssh_port, user)
        };

        info!(id = %id, "Restarting NixOS VM via SSH reboot");

        let _ = self
            .ssh_exec(ssh_port, &user, &["reboot".to_string()])
            .await;

        // Wait for SSH to come back
        self.wait_for_ssh(ssh_port, &user, config.boot_timeout)
            .await
    }

    async fn inspect(&self, id: &str) -> Result<VmInfo> {
        let mut vms = self.vms.write().await;
        let vm = vms
            .get_mut(id)
            .ok_or_else(|| VmError::NotFound(id.to_string()))?;

        // Check process state
        let state = match vm.process.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    VmState::Shutoff
                } else {
                    VmState::Crashed
                }
            }
            Ok(None) => VmState::Running,
            Err(_) => VmState::Crashed,
        };

        Ok(VmInfo {
            id: id.to_string(),
            state,
            pid: vm.process.id(),
            started_at: Some(vm.started_at),
            ip_address: Some("localhost".to_string()),
            ssh_port: Some(vm.ssh_port),
            display_port: None,
            qmp_socket: None,
            console_socket: Some(vm.log_file.clone()),
        })
    }

    async fn is_running(&self, id: &str) -> Result<bool> {
        let mut vms = self.vms.write().await;
        if let Some(vm) = vms.get_mut(id) {
            // Check if the process has exited
            match vm.process.try_wait() {
                Ok(Some(_status)) => {
                    // Process has exited, remove it from the map
                    vms.remove(id);
                    Ok(false)
                }
                Ok(None) => {
                    // Process is still running
                    Ok(true)
                }
                Err(_) => {
                    // Error checking status, assume not running
                    vms.remove(id);
                    Ok(false)
                }
            }
        } else {
            Ok(false)
        }
    }

    async fn wait_for_boot(&self, id: &str, _config: &VmConfig, timeout: Duration) -> Result<()> {
        let (ssh_port, user) = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            let user = vm
                .config
                .ssh
                .as_ref()
                .map(|s| s.user.clone())
                .unwrap_or_else(|| "root".to_string());
            (vm.ssh_port, user)
        };

        info!(id = %id, ssh_port = %ssh_port, "Waiting for NixOS VM to boot");

        self.wait_for_ssh(ssh_port, &user, timeout).await
    }

    async fn exec(&self, id: &str, command: &[String], _config: &VmConfig) -> Result<ExecResult> {
        let (ssh_port, user) = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            let user = vm
                .config
                .ssh
                .as_ref()
                .map(|s| s.user.clone())
                .unwrap_or_else(|| "root".to_string());
            (vm.ssh_port, user)
        };

        self.ssh_exec(ssh_port, &user, command).await
    }

    async fn console_log(&self, id: &str, lines: Option<usize>) -> Result<String> {
        // Try to get the log file from the VM state, or fall back to the default path
        let log_path = {
            let vms = self.vms.read().await;
            vms.get(id)
                .map(|vm| vm.log_file.clone())
                .unwrap_or_else(|| self.state_dir.join(format!("{}.log", id)))
        };

        let content = tokio::fs::read_to_string(&log_path)
            .await
            .unwrap_or_default();

        if let Some(n) = lines {
            Ok(content
                .lines()
                .rev()
                .take(n)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n"))
        } else {
            Ok(content)
        }
    }

    async fn remove(&self, id: &str) -> Result<()> {
        // Kill will also clean up the log file
        self.kill(id).await
    }

    async fn health(&self, id: &str, _config: &VmConfig) -> Result<HealthStatus> {
        let (ssh_port, user) = {
            let vms = self.vms.read().await;
            match vms.get(id) {
                Some(vm) => {
                    let user = vm
                        .config
                        .ssh
                        .as_ref()
                        .map(|s| s.user.clone())
                        .unwrap_or_else(|| "root".to_string());
                    (vm.ssh_port, user)
                }
                None => {
                    return Ok(HealthStatus::Unhealthy {
                        reason: "VM not running".to_string(),
                    })
                }
            }
        };

        match self.ssh_exec(ssh_port, &user, &["true".to_string()]).await {
            Ok(result) if result.success() => Ok(HealthStatus::Healthy),
            Ok(result) => Ok(HealthStatus::Unhealthy {
                reason: format!("SSH check failed with exit code {}", result.exit_code),
            }),
            Err(e) => Ok(HealthStatus::Unhealthy {
                reason: format!("SSH check failed: {}", e),
            }),
        }
    }

    async fn snapshot(&self, _id: &str, _name: &str) -> Result<String> {
        Err(VmError::not_supported("Snapshots", "nixos"))
    }

    async fn restore_snapshot(&self, _id: &str, _name: &str) -> Result<()> {
        Err(VmError::not_supported("Snapshot restore", "nixos"))
    }

    async fn list_snapshots(&self, _id: &str) -> Result<Vec<SnapshotInfo>> {
        // NixOS VMs don't support snapshots
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dtx_core::resource::ResourceId;

    #[test]
    fn nixos_runtime_new() {
        let runtime = NixosVmRuntime::new();
        assert_eq!(runtime.name(), "nixos");
    }

    #[test]
    fn nixos_runtime_with_state_dir() {
        let runtime = NixosVmRuntime::new().with_state_dir(PathBuf::from("/tmp/nixos-vms"));
        assert_eq!(runtime.state_dir, PathBuf::from("/tmp/nixos-vms"));
    }

    #[tokio::test]
    async fn nixos_create() {
        let runtime = NixosVmRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-nixos-vm"),
            VmImage::NixosFlake {
                flake: ".".to_string(),
                attribute: Some("vm".to_string()),
            },
        );

        let result = runtime.create(&config).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-nixos-vm");
    }

    #[tokio::test]
    async fn nixos_is_running_not_found() {
        let runtime = NixosVmRuntime::new();
        let result = runtime.is_running("nonexistent").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn nixos_invalid_image_type() {
        let runtime = NixosVmRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::Container {
                image: "alpine:latest".to_string(),
            },
        );

        let result = runtime.prepare_image(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn nixos_pause_not_supported() {
        let runtime = NixosVmRuntime::new();
        let result = runtime.pause("test-vm").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn nixos_snapshot_not_supported() {
        let runtime = NixosVmRuntime::new();
        let result = runtime.snapshot("test-vm", "snap-1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn nixos_list_snapshots_empty() {
        let runtime = NixosVmRuntime::new();
        let result = runtime.list_snapshots("test-vm").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn nixos_resume_not_supported() {
        let runtime = NixosVmRuntime::new();
        let result = runtime.resume("test-vm").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::NotSupported(_, _)));
    }

    #[tokio::test]
    async fn nixos_restore_snapshot_not_supported() {
        let runtime = NixosVmRuntime::new();
        let result = runtime.restore_snapshot("test-vm", "snap-1").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::NotSupported(_, _)));
    }

    #[tokio::test]
    async fn nixos_inspect_not_found() {
        let runtime = NixosVmRuntime::new();
        let result = runtime.inspect("nonexistent").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::NotFound(_)));
    }

    #[tokio::test]
    async fn nixos_stop_not_found() {
        let runtime = NixosVmRuntime::new();
        let result = runtime.stop("nonexistent", Duration::from_secs(10)).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::NotFound(_)));
    }

    #[tokio::test]
    async fn nixos_kill_not_found() {
        let runtime = NixosVmRuntime::new();
        // Kill for non-existent VM should succeed (no-op)
        let result = runtime.kill("nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn nixos_remove_not_found() {
        let runtime = NixosVmRuntime::new();
        // Remove for non-existent VM should succeed (no-op)
        let result = runtime.remove("nonexistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn nixos_health_not_running() {
        let runtime = NixosVmRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::NixosFlake {
                flake: ".".to_string(),
                attribute: None,
            },
        );
        let result = runtime.health("test-vm", &config).await;
        assert!(result.is_ok());
        match result.unwrap() {
            HealthStatus::Unhealthy { reason } => {
                assert!(reason.contains("not running"));
            }
            _ => panic!("Expected unhealthy status"),
        }
    }

    #[tokio::test]
    async fn nixos_console_log_not_found() {
        let runtime = NixosVmRuntime::new();
        // Console log for non-existent VM returns empty string
        let result = runtime.console_log("nonexistent", None).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn nixos_wait_for_boot_not_found() {
        let runtime = NixosVmRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::NixosFlake {
                flake: ".".to_string(),
                attribute: None,
            },
        );
        let result = runtime
            .wait_for_boot("nonexistent", &config, Duration::from_secs(5))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::NotFound(_)));
    }

    #[tokio::test]
    async fn nixos_exec_not_found() {
        let runtime = NixosVmRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::NixosFlake {
                flake: ".".to_string(),
                attribute: None,
            },
        );
        let result = runtime
            .exec(
                "nonexistent",
                &["echo".to_string(), "hello".to_string()],
                &config,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::NotFound(_)));
    }

    #[tokio::test]
    async fn nixos_restart_not_found() {
        let runtime = NixosVmRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::NixosFlake {
                flake: ".".to_string(),
                attribute: None,
            },
        );
        let result = runtime.restart("nonexistent", &config).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::NotFound(_)));
    }

    #[tokio::test]
    async fn nixos_find_available_port() {
        let runtime = NixosVmRuntime::new();
        let port = runtime.find_available_port().await;
        // Port should be in the expected range
        assert!((2222..3000).contains(&port));
    }

    #[tokio::test]
    async fn nixos_prepare_image_file_not_found() {
        let runtime = NixosVmRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/nonexistent/image.qcow2"),
                format: crate::config::ImageFormat::Qcow2,
            },
        );
        let result = runtime.prepare_image(&config).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VmError::ImageNotFound(_)));
    }
}
