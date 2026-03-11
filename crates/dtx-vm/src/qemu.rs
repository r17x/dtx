//! QEMU runtime implementation.
//!
//! This module provides a full QEMU/KVM implementation of the `VmRuntime` trait.
//! It supports:
//!
//! - Multiple image formats (qcow2, raw, vmdk, vdi)
//! - User and bridged networking
//! - Port forwarding
//! - SSH execution
//! - QMP (QEMU Machine Protocol) for management
//! - Snapshots
//! - Shared directories via virtio-fs or 9p

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::{GraphicsConfig, NetworkMode, VmConfig, VmHealthCheck, VmImage};
use crate::error::{Result, VmError};
use crate::runtime::{ExecResult, SnapshotInfo, VmInfo, VmRuntime, VmState};
use dtx_core::resource::HealthStatus;

/// QEMU runtime implementation.
pub struct QemuRuntime {
    /// Path to QEMU binary.
    qemu_binary: PathBuf,
    /// Running VMs.
    vms: Arc<RwLock<HashMap<String, QemuVm>>>,
    /// Base directory for VM state.
    state_dir: PathBuf,
}

/// Internal state for a running QEMU VM.
#[allow(dead_code)]
struct QemuVm {
    config: VmConfig,
    process: Child,
    qmp_socket: PathBuf,
    console_socket: PathBuf,
    ssh_port: Option<u16>,
    started_at: chrono::DateTime<chrono::Utc>,
}

impl QemuRuntime {
    /// Create a new QEMU runtime with auto-detected binary.
    pub fn new() -> Self {
        let qemu_binary = which::which("qemu-system-x86_64")
            .or_else(|_| which::which("qemu-kvm"))
            .unwrap_or_else(|_| PathBuf::from("qemu-system-x86_64"));

        Self {
            qemu_binary,
            vms: Arc::new(RwLock::new(HashMap::new())),
            state_dir: std::env::temp_dir().join("dtx-vm"),
        }
    }

    /// Create a new QEMU runtime with a specific binary.
    pub fn with_binary(binary: PathBuf) -> Self {
        Self {
            qemu_binary: binary,
            vms: Arc::new(RwLock::new(HashMap::new())),
            state_dir: std::env::temp_dir().join("dtx-vm"),
        }
    }

    /// Create a new QEMU runtime with custom state directory.
    pub fn with_state_dir(mut self, state_dir: PathBuf) -> Self {
        self.state_dir = state_dir;
        self
    }

    /// Build QEMU command line arguments.
    fn build_qemu_args(&self, config: &VmConfig, image_path: &Path) -> Vec<String> {
        let mut args = Vec::new();

        // Machine type
        if config.kvm {
            args.extend(["-machine".to_string(), "q35,accel=kvm:tcg".to_string()]);
        } else {
            args.extend(["-machine".to_string(), "q35,accel=tcg".to_string()]);
        }

        // CPU
        let cpu_model = config.cpu.model.as_deref().unwrap_or("host");
        args.extend(["-cpu".to_string(), cpu_model.to_string()]);
        args.extend(["-smp".to_string(), config.cpu.count.to_string()]);

        // Memory
        args.extend(["-m".to_string(), config.memory.size.clone()]);

        // Enable KVM if requested and available
        if config.kvm {
            args.push("-enable-kvm".to_string());
        }

        // Boot drive
        let image_format = match &config.image {
            VmImage::File { format, .. } => format.as_str(),
            _ => "qcow2",
        };
        args.extend([
            "-drive".to_string(),
            format!(
                "file={},format={},if=virtio",
                image_path.display(),
                image_format
            ),
        ]);

        // Additional disks
        for (i, disk) in config.disks.iter().enumerate() {
            let mut drive_spec = format!(
                "file={},format={},if={},index={}",
                disk.path.display(),
                disk.format.as_str(),
                disk.interface.as_str(),
                i + 1
            );
            if disk.read_only {
                drive_spec.push_str(",readonly=on");
            }
            args.extend(["-drive".to_string(), drive_spec]);
        }

        // Network configuration
        match &config.network.mode {
            NetworkMode::User => {
                let mut netdev = "user,id=net0".to_string();

                // Port forwards
                for pf in &config.port_forwards {
                    netdev.push_str(&format!(
                        ",hostfwd={}::{}-:{}",
                        pf.protocol.as_str(),
                        pf.host,
                        pf.guest
                    ));
                }

                // SSH forwarding
                if let Some(ssh) = &config.ssh {
                    if let Some(host_port) = ssh.host_port {
                        netdev.push_str(&format!(",hostfwd=tcp::{}-:{}", host_port, ssh.port));
                    }
                }

                args.extend(["-netdev".to_string(), netdev]);
                args.extend([
                    "-device".to_string(),
                    "virtio-net-pci,netdev=net0".to_string(),
                ]);
            }
            NetworkMode::Bridged => {
                if let Some(bridge) = &config.network.bridge {
                    args.extend([
                        "-netdev".to_string(),
                        format!("bridge,id=net0,br={}", bridge),
                    ]);
                    args.extend([
                        "-device".to_string(),
                        "virtio-net-pci,netdev=net0".to_string(),
                    ]);
                }
            }
            NetworkMode::Tap => {
                if let Some(tap) = &config.network.tap {
                    args.extend([
                        "-netdev".to_string(),
                        format!("tap,id=net0,ifname={},script=no,downscript=no", tap),
                    ]);
                    args.extend([
                        "-device".to_string(),
                        "virtio-net-pci,netdev=net0".to_string(),
                    ]);
                }
            }
            NetworkMode::None => {
                args.extend(["-net".to_string(), "none".to_string()]);
            }
        }

        // Shared directories
        for share in &config.shared_dirs {
            let mut virtfs_spec = format!(
                "local,path={},mount_tag={},security_model=mapped-xattr",
                share.source.display(),
                share.tag
            );
            if share.read_only {
                virtfs_spec.push_str(",readonly=on");
            }
            args.extend(["-virtfs".to_string(), virtfs_spec]);
        }

        // Graphics
        match &config.graphics {
            GraphicsConfig::None => {
                args.extend(["-display".to_string(), "none".to_string()]);
            }
            GraphicsConfig::Vnc { port, password } => {
                let display = port
                    .map(|p| format!(":{}", p.saturating_sub(5900)))
                    .unwrap_or_else(|| ":0".to_string());
                if password.is_some() {
                    args.extend(["-vnc".to_string(), format!("{},password=on", display)]);
                } else {
                    args.extend(["-vnc".to_string(), display]);
                }
            }
            GraphicsConfig::Spice { port } => {
                let p = port.unwrap_or(5930);
                args.extend([
                    "-spice".to_string(),
                    format!("port={},disable-ticketing=on", p),
                ]);
            }
            GraphicsConfig::Gtk => {
                args.extend(["-display".to_string(), "gtk".to_string()]);
            }
        }

        // QMP socket for control
        let qmp_socket = self.state_dir.join(format!("{}.qmp", config.id.as_str()));
        args.extend([
            "-qmp".to_string(),
            format!("unix:{},server,nowait", qmp_socket.display()),
        ]);

        // Console output
        let console_socket = self
            .state_dir
            .join(format!("{}.console", config.id.as_str()));
        args.extend([
            "-serial".to_string(),
            format!("unix:{},server,nowait", console_socket.display()),
        ]);

        // Daemonize
        args.push("-daemonize".to_string());

        // Extra arguments
        args.extend(config.extra_args.iter().cloned());

        args
    }

    /// Send a QMP command to a VM.
    async fn qmp_command(&self, socket: &Path, command: &str) -> Result<serde_json::Value> {
        let stream = UnixStream::connect(socket)
            .await
            .map_err(|e| VmError::Qmp(format!("Failed to connect to QMP socket: {}", e)))?;

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // Read greeting
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| VmError::Qmp(format!("Failed to read QMP greeting: {}", e)))?;

        // Enter command mode
        write_half
            .write_all(b"{\"execute\":\"qmp_capabilities\"}\n")
            .await
            .map_err(|e| VmError::Qmp(format!("Failed to send qmp_capabilities: {}", e)))?;
        line.clear();
        reader.read_line(&mut line).await.map_err(|e| {
            VmError::Qmp(format!("Failed to read qmp_capabilities response: {}", e))
        })?;

        // Send the actual command
        let cmd = format!("{{\"execute\":\"{}\"}}\n", command);
        write_half
            .write_all(cmd.as_bytes())
            .await
            .map_err(|e| VmError::Qmp(format!("Failed to send command: {}", e)))?;
        line.clear();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| VmError::Qmp(format!("Failed to read command response: {}", e)))?;

        serde_json::from_str(&line)
            .map_err(|e| VmError::Qmp(format!("Failed to parse QMP response: {}", e)))
    }

    /// Execute a command via SSH.
    async fn ssh_exec(&self, config: &VmConfig, command: &[String]) -> Result<ExecResult> {
        let ssh = config
            .ssh
            .as_ref()
            .ok_or_else(|| VmError::Ssh("SSH not configured".to_string()))?;

        let host_port = ssh
            .host_port
            .ok_or_else(|| VmError::Ssh("SSH host port not configured".to_string()))?;

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
            &host_port.to_string(),
        ]);

        // Add identity file if specified
        if let Some(key) = &ssh.identity_file {
            cmd.args(["-i", &key.to_string_lossy()]);
        }

        // Add any extra SSH options
        for opt in &ssh.options {
            cmd.arg(opt);
        }

        cmd.arg(format!("{}@localhost", ssh.user));
        cmd.args(command);

        let output = cmd
            .output()
            .await
            .map_err(|e| VmError::Ssh(format!("Failed to execute SSH command: {}", e)))?;

        Ok(ExecResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    /// Wait for SSH to be available.
    async fn wait_for_ssh(&self, config: &VmConfig, timeout: Duration) -> Result<()> {
        let ssh = config
            .ssh
            .as_ref()
            .ok_or_else(|| VmError::Ssh("SSH not configured".to_string()))?;

        let host_port = ssh
            .host_port
            .ok_or_else(|| VmError::Ssh("SSH host port not configured".to_string()))?;

        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(VmError::Timeout(timeout));
            }

            // Try to connect to the SSH port
            let connect_result = tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::TcpStream::connect(format!("127.0.0.1:{}", host_port)),
            )
            .await;

            if let Ok(Ok(_)) = connect_result {
                // Port is open, try actual SSH
                let result = self.ssh_exec(config, &["true".to_string()]).await;

                if result.is_ok() {
                    return Ok(());
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

impl Default for QemuRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VmRuntime for QemuRuntime {
    fn name(&self) -> &str {
        "qemu"
    }

    async fn is_available(&self) -> bool {
        Command::new(&self.qemu_binary)
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn prepare_image(&self, config: &VmConfig) -> Result<PathBuf> {
        match &config.image {
            VmImage::File { path, .. } => {
                if !path.exists() {
                    return Err(VmError::ImageNotFound(path.clone()));
                }
                Ok(path.clone())
            }
            VmImage::NixosFlake { flake, attribute } => {
                info!(flake = %flake, attribute = ?attribute, "Building NixOS VM from flake");
                let attr = attribute.as_deref().unwrap_or("vm");
                let output = Command::new("nix")
                    .args([
                        "build",
                        "--no-link",
                        "--print-out-paths",
                        &format!("{}#{}", flake, attr),
                    ])
                    .output()
                    .await
                    .map_err(|e| VmError::ImagePreparation(format!("nix build failed: {}", e)))?;

                if !output.status.success() {
                    return Err(VmError::ImagePreparation(
                        String::from_utf8_lossy(&output.stderr).to_string(),
                    ));
                }

                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let vm_path = PathBuf::from(&path);

                // NixOS VMs typically have a run-*-vm script
                let run_script = vm_path.join("bin").join("run-nixos-vm");
                if run_script.exists() {
                    return Ok(run_script);
                }

                // Or a qcow2 image
                let qcow2 = vm_path.join("nixos.qcow2");
                if qcow2.exists() {
                    return Ok(qcow2);
                }

                Ok(vm_path)
            }
            VmImage::Url { url, checksum } => {
                info!(url = %url, "Downloading VM image");
                let cache_dir = self.state_dir.join("images");
                tokio::fs::create_dir_all(&cache_dir)
                    .await
                    .map_err(|e| VmError::ImagePreparation(format!("Failed to create cache dir: {}", e)))?;

                let filename = url.split('/').next_back().unwrap_or("image.qcow2");
                let target = cache_dir.join(filename);

                if target.exists() {
                    // TODO: verify checksum if provided
                    return Ok(target);
                }

                let output = Command::new("curl")
                    .args(["-L", "-o", &target.to_string_lossy(), url])
                    .output()
                    .await
                    .map_err(|e| VmError::ImagePreparation(format!("curl failed: {}", e)))?;

                if !output.status.success() {
                    return Err(VmError::ImagePreparation(
                        String::from_utf8_lossy(&output.stderr).to_string(),
                    ));
                }

                // Verify checksum if provided
                if let Some(expected) = checksum {
                    let actual = Command::new("sha256sum")
                        .arg(&target)
                        .output()
                        .await
                        .map_err(|e| VmError::ImagePreparation(format!("sha256sum failed: {}", e)))?;

                    let actual_hash = String::from_utf8_lossy(&actual.stdout)
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();

                    if actual_hash != *expected {
                        let _ = tokio::fs::remove_file(&target).await;
                        return Err(VmError::ImagePreparation(format!(
                            "Checksum mismatch: expected {}, got {}",
                            expected, actual_hash
                        )));
                    }
                }

                Ok(target)
            }
            VmImage::Container { image } => {
                Err(VmError::InvalidConfig(format!(
                    "Container images are not supported by QEMU runtime. Use Firecracker for container image: {}",
                    image
                )))
            }
            VmImage::NixBuild { expression, attribute } => {
                info!(expression = %expression, attribute = %attribute, "Building VM from Nix expression");
                let output = Command::new("nix-build")
                    .args(["-E", expression, "-A", attribute, "--no-out-link"])
                    .output()
                    .await
                    .map_err(|e| VmError::ImagePreparation(format!("nix-build failed: {}", e)))?;

                if !output.status.success() {
                    return Err(VmError::ImagePreparation(
                        String::from_utf8_lossy(&output.stderr).to_string(),
                    ));
                }

                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                Ok(PathBuf::from(path))
            }
        }
    }

    async fn create(&self, config: &VmConfig) -> Result<String> {
        tokio::fs::create_dir_all(&self.state_dir)
            .await
            .map_err(|e| VmError::Backend(format!("Failed to create state directory: {}", e)))?;

        // Just validate config and return ID
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

        info!(id = %id, image = %image_path.display(), "Starting QEMU VM");

        // Build command line
        let args = self.build_qemu_args(config, image_path);
        debug!(args = ?args, "QEMU command line");

        // Start QEMU
        let process = Command::new(&self.qemu_binary)
            .args(&args)
            .spawn()
            .map_err(|e| VmError::Qemu(format!("Failed to spawn QEMU: {}", e)))?;

        let qmp_socket = self.state_dir.join(format!("{}.qmp", id));
        let console_socket = self.state_dir.join(format!("{}.console", id));
        let ssh_port = config.ssh.as_ref().and_then(|s| s.host_port);

        // Wait for QMP socket to be available
        let qmp_timeout = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < qmp_timeout {
            if qmp_socket.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if !qmp_socket.exists() {
            return Err(VmError::Qemu("QMP socket not created".to_string()));
        }

        let started_at = Utc::now();

        // Store VM state
        {
            let mut vms = self.vms.write().await;
            vms.insert(
                id.to_string(),
                QemuVm {
                    config: config.clone(),
                    process,
                    qmp_socket: qmp_socket.clone(),
                    console_socket: console_socket.clone(),
                    ssh_port,
                    started_at,
                },
            );
        }

        Ok(VmInfo {
            id: id.to_string(),
            state: VmState::Running,
            pid: None, // Daemonized, so child PID is not the QEMU PID
            started_at: Some(started_at),
            ip_address: None,
            ssh_port,
            display_port: None,
            qmp_socket: Some(qmp_socket),
            console_socket: Some(console_socket),
        })
    }

    async fn stop(&self, id: &str, timeout: Duration) -> Result<()> {
        let qmp_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.qmp_socket.clone()
        };

        info!(id = %id, "Stopping QEMU VM via ACPI shutdown");

        // Send ACPI shutdown via QMP
        if let Err(e) = self.qmp_command(&qmp_socket, "system_powerdown").await {
            warn!(id = %id, error = %e, "Failed to send ACPI shutdown, will force kill");
        }

        // Wait for VM to stop
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if tokio::time::Instant::now() > deadline {
                warn!(id = %id, "Graceful shutdown timed out, forcing kill");
                return self.kill(id).await;
            }

            // Check if QMP socket still exists (VM stopped)
            if !qmp_socket.exists() {
                break;
            }

            // Try to query VM status
            match self.qmp_command(&qmp_socket, "query-status").await {
                Ok(_) => {
                    // VM still running
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(_) => {
                    // VM stopped
                    break;
                }
            }
        }

        // Cleanup
        {
            let mut vms = self.vms.write().await;
            vms.remove(id);
        }

        // Remove sockets
        let _ = tokio::fs::remove_file(&qmp_socket).await;
        let console_socket = self.state_dir.join(format!("{}.console", id));
        let _ = tokio::fs::remove_file(&console_socket).await;

        Ok(())
    }

    async fn kill(&self, id: &str) -> Result<()> {
        info!(id = %id, "Force killing QEMU VM");

        let qmp_socket = {
            let mut vms = self.vms.write().await;
            if let Some(mut vm) = vms.remove(id) {
                vm.process.kill().await.ok();
                Some(vm.qmp_socket)
            } else {
                None
            }
        };

        // Also try QMP quit
        if let Some(socket) = &qmp_socket {
            let _ = self.qmp_command(socket, "quit").await;
        }

        // Cleanup sockets
        if let Some(socket) = qmp_socket {
            let _ = tokio::fs::remove_file(&socket).await;
        }
        let console_socket = self.state_dir.join(format!("{}.console", id));
        let _ = tokio::fs::remove_file(&console_socket).await;

        Ok(())
    }

    async fn pause(&self, id: &str) -> Result<()> {
        let qmp_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.qmp_socket.clone()
        };

        info!(id = %id, "Pausing QEMU VM");
        self.qmp_command(&qmp_socket, "stop").await?;
        Ok(())
    }

    async fn resume(&self, id: &str) -> Result<()> {
        let qmp_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.qmp_socket.clone()
        };

        info!(id = %id, "Resuming QEMU VM");
        self.qmp_command(&qmp_socket, "cont").await?;
        Ok(())
    }

    async fn restart(&self, id: &str, config: &VmConfig) -> Result<()> {
        let qmp_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.qmp_socket.clone()
        };

        info!(id = %id, "Restarting QEMU VM");
        self.qmp_command(&qmp_socket, "system_reset").await?;

        // Wait for boot if SSH is configured
        if config.ssh.is_some() {
            self.wait_for_ssh(config, config.boot_timeout).await?;
        }

        Ok(())
    }

    async fn inspect(&self, id: &str) -> Result<VmInfo> {
        let vms = self.vms.read().await;
        let vm = vms
            .get(id)
            .ok_or_else(|| VmError::NotFound(id.to_string()))?;

        Ok(VmInfo {
            id: id.to_string(),
            state: VmState::Running,
            pid: vm.process.id(),
            started_at: Some(vm.started_at),
            ip_address: None,
            ssh_port: vm.ssh_port,
            display_port: None,
            qmp_socket: Some(vm.qmp_socket.clone()),
            console_socket: Some(vm.console_socket.clone()),
        })
    }

    async fn is_running(&self, id: &str) -> Result<bool> {
        let vms = self.vms.read().await;
        Ok(vms.contains_key(id))
    }

    async fn wait_for_boot(&self, id: &str, config: &VmConfig, timeout: Duration) -> Result<()> {
        // First check if VM is in our map
        {
            let vms = self.vms.read().await;
            if !vms.contains_key(id) {
                return Err(VmError::NotFound(id.to_string()));
            }
        }

        // Wait for SSH if configured
        if config.ssh.is_some() {
            info!(id = %id, "Waiting for SSH to be available");
            self.wait_for_ssh(config, timeout).await
        } else {
            // No SSH, just wait a bit for boot
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(())
        }
    }

    async fn exec(&self, id: &str, command: &[String], config: &VmConfig) -> Result<ExecResult> {
        // Verify VM exists
        {
            let vms = self.vms.read().await;
            if !vms.contains_key(id) {
                return Err(VmError::NotFound(id.to_string()));
            }
        }

        self.ssh_exec(config, command).await
    }

    async fn console_log(&self, id: &str, lines: Option<usize>) -> Result<String> {
        let console_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.console_socket.clone()
        };

        // Try to read from console socket
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let mut stream = UnixStream::connect(&console_socket).await?;
            let mut buffer = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut buffer).await?;
            Ok::<_, std::io::Error>(buffer)
        })
        .await;

        let output = match result {
            Ok(Ok(buffer)) => String::from_utf8_lossy(&buffer).to_string(),
            _ => String::new(),
        };

        if let Some(n) = lines {
            Ok(output
                .lines()
                .rev()
                .take(n)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n"))
        } else {
            Ok(output)
        }
    }

    async fn remove(&self, id: &str) -> Result<()> {
        // Kill if running
        let _ = self.kill(id).await;

        // Cleanup state files
        let _ = tokio::fs::remove_file(self.state_dir.join(format!("{}.qmp", id))).await;
        let _ = tokio::fs::remove_file(self.state_dir.join(format!("{}.console", id))).await;

        Ok(())
    }

    async fn health(&self, id: &str, config: &VmConfig) -> Result<HealthStatus> {
        if !self.is_running(id).await? {
            return Ok(HealthStatus::Unhealthy {
                reason: "VM not running".to_string(),
            });
        }

        match &config.health_check {
            Some(VmHealthCheck::Ssh { .. }) => {
                match self.ssh_exec(config, &["true".to_string()]).await {
                    Ok(result) if result.exit_code == 0 => Ok(HealthStatus::Healthy),
                    Ok(result) => Ok(HealthStatus::Unhealthy {
                        reason: format!("SSH check failed with exit code {}", result.exit_code),
                    }),
                    Err(e) => Ok(HealthStatus::Unhealthy {
                        reason: format!("SSH check failed: {}", e),
                    }),
                }
            }
            Some(VmHealthCheck::Exec { command, .. }) => {
                match self.exec(id, command, config).await {
                    Ok(result) if result.exit_code == 0 => Ok(HealthStatus::Healthy),
                    Ok(result) => Ok(HealthStatus::Unhealthy {
                        reason: format!(
                            "Health check command failed with exit code {}",
                            result.exit_code
                        ),
                    }),
                    Err(e) => Ok(HealthStatus::Unhealthy {
                        reason: format!("Health check command failed: {}", e),
                    }),
                }
            }
            Some(VmHealthCheck::Http { port, path, .. }) => {
                // Try HTTP health check via port forward
                let url = format!("http://localhost:{}{}", port, path);
                let result = Command::new("curl")
                    .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", &url])
                    .output()
                    .await;

                match result {
                    Ok(output) if output.status.success() => {
                        let code = String::from_utf8_lossy(&output.stdout);
                        if code.starts_with('2') {
                            Ok(HealthStatus::Healthy)
                        } else {
                            Ok(HealthStatus::Unhealthy {
                                reason: format!("HTTP check returned status {}", code),
                            })
                        }
                    }
                    _ => Ok(HealthStatus::Unhealthy {
                        reason: "HTTP check failed".to_string(),
                    }),
                }
            }
            Some(VmHealthCheck::Qmp { .. }) => {
                let qmp_socket = {
                    let vms = self.vms.read().await;
                    vms.get(id).map(|vm| vm.qmp_socket.clone())
                };

                if let Some(socket) = qmp_socket {
                    match self.qmp_command(&socket, "query-status").await {
                        Ok(_) => Ok(HealthStatus::Healthy),
                        Err(e) => Ok(HealthStatus::Unhealthy {
                            reason: format!("QMP check failed: {}", e),
                        }),
                    }
                } else {
                    Ok(HealthStatus::Unknown)
                }
            }
            None => {
                // No health check configured, assume healthy if running
                Ok(HealthStatus::Healthy)
            }
        }
    }

    async fn snapshot(&self, id: &str, name: &str) -> Result<String> {
        let qmp_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.qmp_socket.clone()
        };

        info!(id = %id, name = %name, "Creating VM snapshot");

        // Use human-monitor-command to run savevm
        let cmd = format!(
            "{{\"execute\":\"human-monitor-command\",\"arguments\":{{\"command-line\":\"savevm {}\"}}}}\n",
            name
        );

        // Connect and send command
        let stream = UnixStream::connect(&qmp_socket)
            .await
            .map_err(|e| VmError::Qmp(format!("Failed to connect: {}", e)))?;

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        // Read greeting and enter command mode
        reader.read_line(&mut line).await?;
        write_half
            .write_all(b"{\"execute\":\"qmp_capabilities\"}\n")
            .await?;
        line.clear();
        reader.read_line(&mut line).await?;

        // Send savevm
        write_half.write_all(cmd.as_bytes()).await?;
        line.clear();
        reader.read_line(&mut line).await?;

        Ok(name.to_string())
    }

    async fn restore_snapshot(&self, id: &str, snapshot_name: &str) -> Result<()> {
        let qmp_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.qmp_socket.clone()
        };

        info!(id = %id, snapshot = %snapshot_name, "Restoring VM snapshot");

        let cmd = format!(
            "{{\"execute\":\"human-monitor-command\",\"arguments\":{{\"command-line\":\"loadvm {}\"}}}}\n",
            snapshot_name
        );

        let stream = UnixStream::connect(&qmp_socket)
            .await
            .map_err(|e| VmError::Qmp(format!("Failed to connect: {}", e)))?;

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        reader.read_line(&mut line).await?;
        write_half
            .write_all(b"{\"execute\":\"qmp_capabilities\"}\n")
            .await?;
        line.clear();
        reader.read_line(&mut line).await?;

        write_half.write_all(cmd.as_bytes()).await?;
        line.clear();
        reader.read_line(&mut line).await?;

        Ok(())
    }

    async fn list_snapshots(&self, id: &str) -> Result<Vec<SnapshotInfo>> {
        let qmp_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.qmp_socket.clone()
        };

        // Query snapshots using HMP
        let cmd = "{\"execute\":\"human-monitor-command\",\"arguments\":{\"command-line\":\"info snapshots\"}}\n";

        let stream = UnixStream::connect(&qmp_socket)
            .await
            .map_err(|e| VmError::Qmp(format!("Failed to connect: {}", e)))?;

        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        reader.read_line(&mut line).await?;
        write_half
            .write_all(b"{\"execute\":\"qmp_capabilities\"}\n")
            .await?;
        line.clear();
        reader.read_line(&mut line).await?;

        write_half.write_all(cmd.as_bytes()).await?;
        line.clear();
        reader.read_line(&mut line).await?;

        // Parse the response - this is HMP output which is human-readable text
        // The actual parsing would need to handle the specific QEMU output format
        // For now, return an empty list as parsing HMP output is complex
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ImageFormat, SshConfig};
    use dtx_core::resource::ResourceId;

    #[test]
    fn qemu_runtime_new() {
        let runtime = QemuRuntime::new();
        assert_eq!(runtime.name(), "qemu");
    }

    #[test]
    fn qemu_runtime_with_binary() {
        let runtime = QemuRuntime::with_binary(PathBuf::from("/custom/qemu"));
        assert_eq!(runtime.qemu_binary, PathBuf::from("/custom/qemu"));
    }

    #[test]
    fn qemu_runtime_with_state_dir() {
        let runtime = QemuRuntime::new().with_state_dir(PathBuf::from("/tmp/test-vms"));
        assert_eq!(runtime.state_dir, PathBuf::from("/tmp/test-vms"));
    }

    #[test]
    fn build_qemu_args_basic() {
        let runtime = QemuRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/path/to/image.qcow2"),
                format: ImageFormat::Qcow2,
            },
        );

        let args = runtime.build_qemu_args(&config, Path::new("/path/to/image.qcow2"));

        assert!(args.contains(&"-machine".to_string()));
        assert!(args.contains(&"-cpu".to_string()));
        assert!(args.contains(&"-m".to_string()));
        assert!(args.contains(&"-drive".to_string()));
    }

    #[test]
    fn build_qemu_args_with_ssh() {
        let runtime = QemuRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/path/to/image.qcow2"),
                format: ImageFormat::Qcow2,
            },
        )
        .with_ssh(SshConfig::new("root", 2222));

        let args = runtime.build_qemu_args(&config, Path::new("/path/to/image.qcow2"));

        // Should have SSH port forward
        let netdev_idx = args.iter().position(|a| a.contains("user,id=net0"));
        assert!(netdev_idx.is_some());
        let netdev = &args[netdev_idx.unwrap()];
        assert!(netdev.contains("hostfwd=tcp::2222-:22"));
    }

    #[test]
    fn build_qemu_args_with_vnc() {
        let runtime = QemuRuntime::new();
        let mut config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/path/to/image.qcow2"),
                format: ImageFormat::Qcow2,
            },
        );
        config.graphics = GraphicsConfig::Vnc {
            port: Some(5900),
            password: None,
        };

        let args = runtime.build_qemu_args(&config, Path::new("/path/to/image.qcow2"));

        assert!(args.contains(&"-vnc".to_string()));
    }

    #[test]
    fn build_qemu_args_without_kvm() {
        let runtime = QemuRuntime::new();
        let mut config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/path/to/image.qcow2"),
                format: ImageFormat::Qcow2,
            },
        );
        config.kvm = false;

        let args = runtime.build_qemu_args(&config, Path::new("/path/to/image.qcow2"));

        assert!(!args.contains(&"-enable-kvm".to_string()));
    }

    #[tokio::test]
    async fn qemu_runtime_create() {
        let runtime = QemuRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/path/to/image.qcow2"),
                format: ImageFormat::Qcow2,
            },
        );

        let result = runtime.create(&config).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-vm");
    }

    #[tokio::test]
    async fn qemu_is_running_not_found() {
        let runtime = QemuRuntime::new();
        let result = runtime.is_running("nonexistent").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }
}
