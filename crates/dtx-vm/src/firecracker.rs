//! Firecracker microVM runtime implementation.
//!
//! Firecracker provides lightweight virtualization for containers and functions,
//! with boot times under 125ms and minimal memory footprint.
//!
//! This implementation supports:
//! - Rootfs images (ext4 format)
//! - Kernel boot configuration
//! - Machine configuration (CPU, memory)
//! - Network interfaces (TAP devices)
//! - Vsock for guest communication
//! - Snapshots (create and list)
//! - Pause/resume via VM state API
//!
//! **Note:** Full production setup requires:
//! - TAP networking setup (requires root or CAP_NET_ADMIN)
//! - Kernel image (vmlinux)
//! - jailer integration for security isolation

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::{NetworkMode, VmConfig, VmHealthCheck, VmImage};
use crate::error::{Result, VmError};
use crate::runtime::{ExecResult, SnapshotInfo, VmInfo, VmRuntime, VmState};
use dtx_core::resource::HealthStatus;

/// Firecracker microVM runtime.
///
/// Firecracker provides lightweight virtualization for containers,
/// with boot times under 125ms.
#[allow(dead_code)]
pub struct FirecrackerRuntime {
    /// Path to Firecracker binary.
    firecracker_binary: PathBuf,
    /// Path to jailer binary (optional).
    jailer_binary: Option<PathBuf>,
    /// Base directory for VM state.
    state_dir: PathBuf,
    /// Running VMs.
    vms: Arc<RwLock<HashMap<String, FirecrackerVm>>>,
}

/// Internal state for a running Firecracker VM.
struct FirecrackerVm {
    config: VmConfig,
    process: Child,
    api_socket: PathBuf,
    log_file: PathBuf,
    /// Context ID for vsock communication.
    /// Currently stored but not used for exec (SSH is used instead).
    #[allow(dead_code)]
    vsock_cid: Option<u32>,
    ssh_port: Option<u16>,
    started_at: chrono::DateTime<chrono::Utc>,
}

/// Firecracker boot source configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BootSource {
    kernel_image_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    boot_args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    initrd_path: Option<String>,
}

/// Firecracker drive configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Drive {
    drive_id: String,
    path_on_host: String,
    is_root_device: bool,
    is_read_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    partuuid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limiter: Option<RateLimiter>,
}

/// Rate limiter for drives or network interfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RateLimiter {
    #[serde(skip_serializing_if = "Option::is_none")]
    bandwidth: Option<TokenBucket>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ops: Option<TokenBucket>,
}

/// Token bucket configuration for rate limiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenBucket {
    size: u64,
    one_time_burst: Option<u64>,
    refill_time: u64,
}

/// Firecracker machine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MachineConfig {
    vcpu_count: u8,
    mem_size_mib: u64,
    #[serde(default)]
    smt: bool,
    #[serde(default)]
    track_dirty_pages: bool,
}

/// Firecracker network interface configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NetworkInterface {
    iface_id: String,
    guest_mac: String,
    host_dev_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    rx_rate_limiter: Option<RateLimiter>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_rate_limiter: Option<RateLimiter>,
}

/// Firecracker vsock configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Vsock {
    vsock_id: String,
    guest_cid: u32,
    uds_path: String,
}

/// Firecracker action request.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActionRequest {
    action_type: ActionType,
}

/// Firecracker action types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum ActionType {
    InstanceStart,
    SendCtrlAltDel,
    FlushMetrics,
}

/// Firecracker VM state for pause/resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // API type for future pause/resume implementation
struct VmStateRequest {
    state: VmStateType,
}

/// VM state types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
#[allow(dead_code)] // API type for future pause/resume implementation
enum VmStateType {
    Paused,
    Resumed,
}

/// Snapshot create request.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotCreateRequest {
    snapshot_type: SnapshotType,
    snapshot_path: String,
    mem_file_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

/// Snapshot load request.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotLoadRequest {
    snapshot_path: String,
    mem_backend: MemBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    enable_diff_snapshots: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resume_vm: Option<bool>,
}

/// Memory backend configuration for snapshot restore.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemBackend {
    backend_path: String,
    backend_type: MemBackendType,
}

/// Memory backend type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum MemBackendType {
    File,
    Uffd,
}

/// Snapshot type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum SnapshotType {
    Full,
    Diff,
}

impl FirecrackerRuntime {
    /// Create a new Firecracker runtime with auto-detected binaries.
    pub fn new() -> Self {
        let firecracker_binary =
            which::which("firecracker").unwrap_or_else(|_| PathBuf::from("/usr/bin/firecracker"));

        let jailer_binary = which::which("jailer").ok();

        Self {
            firecracker_binary,
            jailer_binary,
            state_dir: std::env::temp_dir().join("dtx-vm-fc"),
            vms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with custom binary paths.
    pub fn with_binaries(firecracker: PathBuf, jailer: Option<PathBuf>) -> Self {
        Self {
            firecracker_binary: firecracker,
            jailer_binary: jailer,
            state_dir: std::env::temp_dir().join("dtx-vm-fc"),
            vms: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set state directory.
    pub fn with_state_dir(mut self, state_dir: PathBuf) -> Self {
        self.state_dir = state_dir;
        self
    }

    /// Call Firecracker API using native HTTP over Unix socket.
    ///
    /// Firecracker exposes a REST API over a Unix domain socket.
    /// This method sends HTTP requests directly to the socket.
    async fn call_api(
        &self,
        socket: &Path,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<String> {
        let mut stream = UnixStream::connect(socket)
            .await
            .map_err(|e| VmError::Firecracker(format!("Failed to connect to API socket: {}", e)))?;

        // Build HTTP request
        let request = if let Some(body) = body {
            format!(
                "{} {} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                method, path, body.len(), body
            )
        } else {
            format!(
                "{} {} HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\n\r\n",
                method, path
            )
        };

        debug!(method = %method, path = %path, "Calling Firecracker API");

        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|e| VmError::Firecracker(format!("Failed to send request: {}", e)))?;

        // Read response
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .map_err(|e| VmError::Firecracker(format!("Failed to read response: {}", e)))?;

        // Parse HTTP response
        let (status_line, body) = self.parse_http_response(&response)?;

        if status_line.contains("200") || status_line.contains("201") || status_line.contains("204")
        {
            Ok(body)
        } else {
            Err(VmError::Firecracker(format!(
                "API error: {} - {}",
                status_line, body
            )))
        }
    }

    /// Parse an HTTP response into status line and body.
    fn parse_http_response(&self, response: &str) -> Result<(String, String)> {
        let mut lines = response.lines();

        let status_line = lines
            .next()
            .ok_or_else(|| VmError::Firecracker("Empty response".to_string()))?
            .to_string();

        // Skip headers until empty line
        let mut body_start = false;
        let mut body = String::new();
        for line in lines {
            if body_start {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(line);
            } else if line.is_empty() {
                body_start = true;
            }
        }

        Ok((status_line, body))
    }

    /// Configure boot source via API.
    async fn configure_boot_source(
        &self,
        socket: &Path,
        kernel_path: &Path,
        boot_args: Option<&str>,
    ) -> Result<()> {
        let boot_source = BootSource {
            kernel_image_path: kernel_path.to_string_lossy().to_string(),
            boot_args: boot_args.map(String::from),
            initrd_path: None,
        };

        let body = serde_json::to_string(&boot_source)?;
        self.call_api(socket, "PUT", "/boot-source", Some(&body))
            .await?;

        Ok(())
    }

    /// Configure a drive via API.
    async fn configure_drive(
        &self,
        socket: &Path,
        drive_id: &str,
        path: &Path,
        is_root: bool,
        read_only: bool,
    ) -> Result<()> {
        let drive = Drive {
            drive_id: drive_id.to_string(),
            path_on_host: path.to_string_lossy().to_string(),
            is_root_device: is_root,
            is_read_only: read_only,
            partuuid: None,
            rate_limiter: None,
        };

        let body = serde_json::to_string(&drive)?;
        self.call_api(socket, "PUT", &format!("/drives/{}", drive_id), Some(&body))
            .await?;

        Ok(())
    }

    /// Configure machine (CPU, memory) via API.
    async fn configure_machine(&self, socket: &Path, vcpus: u8, mem_mib: u64) -> Result<()> {
        let machine_config = MachineConfig {
            vcpu_count: vcpus,
            mem_size_mib: mem_mib,
            smt: false,
            track_dirty_pages: false,
        };

        let body = serde_json::to_string(&machine_config)?;
        self.call_api(socket, "PUT", "/machine-config", Some(&body))
            .await?;

        Ok(())
    }

    /// Configure network interface via API.
    async fn configure_network(
        &self,
        socket: &Path,
        iface_id: &str,
        tap_name: &str,
        mac: &str,
    ) -> Result<()> {
        let network = NetworkInterface {
            iface_id: iface_id.to_string(),
            guest_mac: mac.to_string(),
            host_dev_name: tap_name.to_string(),
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        };

        let body = serde_json::to_string(&network)?;
        self.call_api(
            socket,
            "PUT",
            &format!("/network-interfaces/{}", iface_id),
            Some(&body),
        )
        .await?;

        Ok(())
    }

    /// Configure vsock via API.
    async fn configure_vsock(&self, socket: &Path, cid: u32, uds_path: &Path) -> Result<()> {
        let vsock = Vsock {
            vsock_id: "vsock0".to_string(),
            guest_cid: cid,
            uds_path: uds_path.to_string_lossy().to_string(),
        };

        let body = serde_json::to_string(&vsock)?;
        self.call_api(socket, "PUT", "/vsock", Some(&body)).await?;

        Ok(())
    }

    /// Start the VM instance via API.
    async fn instance_start(&self, socket: &Path) -> Result<()> {
        let action = ActionRequest {
            action_type: ActionType::InstanceStart,
        };

        let body = serde_json::to_string(&action)?;
        self.call_api(socket, "PUT", "/actions", Some(&body))
            .await?;

        Ok(())
    }

    /// Execute a command via SSH if configured.
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

        if let Some(key) = &ssh.identity_file {
            cmd.args(["-i", &key.to_string_lossy()]);
        }

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

            let connect_result = tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::TcpStream::connect(format!("127.0.0.1:{}", host_port)),
            )
            .await;

            if let Ok(Ok(_)) = connect_result {
                if self.ssh_exec(config, &["true".to_string()]).await.is_ok() {
                    return Ok(());
                }
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    /// Get the kernel path from configuration or default.
    fn get_kernel_path(&self, config: &VmConfig) -> Option<PathBuf> {
        // Check for kernel in extra_args
        for (i, arg) in config.extra_args.iter().enumerate() {
            if arg == "--kernel" || arg == "-k" {
                if let Some(path) = config.extra_args.get(i + 1) {
                    return Some(PathBuf::from(path));
                }
            }
        }

        // Check common kernel locations
        let default_paths = [
            "/var/lib/firecracker/vmlinux",
            "/opt/firecracker/vmlinux",
            "/boot/vmlinux",
        ];

        for path in default_paths {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }

        None
    }

    /// Get boot args from configuration or default.
    fn get_boot_args(&self, config: &VmConfig) -> String {
        // Check for boot-args in extra_args
        for (i, arg) in config.extra_args.iter().enumerate() {
            if arg == "--boot-args" {
                if let Some(args) = config.extra_args.get(i + 1) {
                    return args.clone();
                }
            }
        }

        // Default boot args for a typical Linux VM
        "console=ttyS0 reboot=k panic=1 pci=off".to_string()
    }

    /// Generate a random MAC address.
    #[allow(dead_code)]
    fn generate_mac() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        format!(
            "AA:FC:00:{:02X}:{:02X}:{:02X}",
            rng.gen::<u8>(),
            rng.gen::<u8>(),
            rng.gen::<u8>()
        )
    }

    /// Parse memory size to MiB.
    fn parse_memory_mib(mem: &str) -> u64 {
        crate::config::parse_memory_mb(mem)
    }
}

impl Default for FirecrackerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VmRuntime for FirecrackerRuntime {
    fn name(&self) -> &str {
        "firecracker"
    }

    async fn is_available(&self) -> bool {
        Command::new(&self.firecracker_binary)
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
            VmImage::Container { image } => {
                info!(image = %image, "Preparing container image for Firecracker");

                // Create output directory
                let output_dir = self
                    .state_dir
                    .join("images")
                    .join(image.replace([':', '/'], "-"));
                tokio::fs::create_dir_all(&output_dir).await.map_err(|e| {
                    VmError::ImagePreparation(format!("Failed to create dir: {}", e))
                })?;

                // This is a simplified flow - real implementation would:
                // 1. Pull container image
                // 2. Export to tar
                // 3. Create ext4 filesystem
                // 4. Extract tar into filesystem

                warn!(
                    "Container to Firecracker rootfs conversion is not fully implemented. \
                     Please provide a pre-built rootfs image."
                );

                Err(VmError::not_supported(
                    "Container image conversion",
                    "firecracker",
                ))
            }
            VmImage::NixosFlake { flake, attribute } => Err(VmError::not_supported(
                format!(
                    "NixOS flake ({}#{})",
                    flake,
                    attribute.as_deref().unwrap_or("default")
                ),
                "firecracker",
            )),
            VmImage::Url { url, .. } => {
                info!(url = %url, "Downloading image for Firecracker");

                let cache_dir = self.state_dir.join("images");
                tokio::fs::create_dir_all(&cache_dir).await.map_err(|e| {
                    VmError::ImagePreparation(format!("Failed to create cache: {}", e))
                })?;

                let filename = url.split('/').next_back().unwrap_or("rootfs.ext4");
                let target = cache_dir.join(filename);

                if !target.exists() {
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
                }

                Ok(target)
            }
            VmImage::NixBuild {
                expression,
                attribute,
            } => Err(VmError::not_supported(
                format!("Nix build ({} -A {})", expression, attribute),
                "firecracker",
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

        info!(id = %id, image = %image_path.display(), "Starting Firecracker VM");

        // Create paths
        let api_socket = self.state_dir.join(format!("{}.sock", id));
        let log_file = self.state_dir.join(format!("{}.log", id));
        let vsock_path = self.state_dir.join(format!("{}.vsock", id));

        // Remove old socket if exists
        let _ = tokio::fs::remove_file(&api_socket).await;
        let _ = tokio::fs::remove_file(&vsock_path).await;

        // Create log file
        let log_fd = std::fs::File::create(&log_file)
            .map_err(|e| VmError::Firecracker(format!("Failed to create log file: {}", e)))?;

        // Start Firecracker process with logging
        let process = Command::new(&self.firecracker_binary)
            .args([
                "--api-sock",
                &api_socket.to_string_lossy(),
                "--log-path",
                &log_file.to_string_lossy(),
                "--level",
                "Info",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(log_fd.try_clone().unwrap_or(log_fd))
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| VmError::Firecracker(format!("Failed to spawn: {}", e)))?;

        let pid = process.id();
        debug!(id = %id, pid = ?pid, "Firecracker process spawned");

        // Wait for API socket to be available
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline {
            if api_socket.exists() {
                // Try to connect to verify it's ready
                if UnixStream::connect(&api_socket).await.is_ok() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if !api_socket.exists() {
            return Err(VmError::Firecracker(
                "API socket not created - check firecracker logs".to_string(),
            ));
        }

        // Configure the VM via API
        let vcpus = config.cpu.count.min(255) as u8;
        let mem_mib = Self::parse_memory_mib(&config.memory.size);

        // 1. Configure machine (CPU, memory)
        self.configure_machine(&api_socket, vcpus, mem_mib).await?;
        debug!(id = %id, vcpus = vcpus, mem_mib = mem_mib, "Machine configured");

        // 2. Configure boot source if kernel is available
        if let Some(kernel_path) = self.get_kernel_path(config) {
            let boot_args = self.get_boot_args(config);
            self.configure_boot_source(&api_socket, &kernel_path, Some(&boot_args))
                .await?;
            debug!(id = %id, kernel = %kernel_path.display(), "Boot source configured");
        } else {
            warn!(
                id = %id,
                "No kernel path configured. Set --kernel in extra_args or place vmlinux in standard locations."
            );
        }

        // 3. Configure root drive (rootfs)
        self.configure_drive(&api_socket, "rootfs", image_path, true, false)
            .await?;
        debug!(id = %id, rootfs = %image_path.display(), "Root drive configured");

        // 4. Configure additional disks
        for (i, disk) in config.disks.iter().enumerate() {
            let drive_id = format!("disk{}", i);
            self.configure_drive(&api_socket, &drive_id, &disk.path, false, disk.read_only)
                .await?;
            debug!(id = %id, drive_id = %drive_id, path = %disk.path.display(), "Additional drive configured");
        }

        // 5. Configure network if TAP is specified
        let mut ssh_port = None;
        match &config.network.mode {
            NetworkMode::Tap => {
                if let Some(tap_name) = &config.network.tap {
                    let mac = config
                        .network
                        .mac_address
                        .as_deref()
                        .map(String::from)
                        .unwrap_or_else(Self::generate_mac);
                    self.configure_network(&api_socket, "eth0", tap_name, &mac)
                        .await?;
                    debug!(id = %id, tap = %tap_name, mac = %mac, "Network configured");

                    // SSH port from config (through TAP network)
                    ssh_port = config.ssh.as_ref().and_then(|s| s.host_port);
                }
            }
            NetworkMode::None => {
                debug!(id = %id, "No network configured");
            }
            mode => {
                warn!(
                    id = %id,
                    mode = ?mode,
                    "Network mode not supported by Firecracker. Use TAP or None."
                );
            }
        }

        // 6. Configure vsock for guest communication (alternative to SSH)
        let vsock_cid = 3u32; // CID 3 is common for guest VMs (0=hypervisor, 1=reserved, 2=host)
        if let Err(e) = self
            .configure_vsock(&api_socket, vsock_cid, &vsock_path)
            .await
        {
            warn!(id = %id, error = %e, "Failed to configure vsock (non-fatal)");
        } else {
            debug!(id = %id, cid = vsock_cid, "Vsock configured");
        }

        // 7. Start the instance
        if self.get_kernel_path(config).is_some() {
            self.instance_start(&api_socket).await?;
            info!(id = %id, "Firecracker VM instance started");
        } else {
            warn!(
                id = %id,
                "VM not started: no kernel configured. Configure boot source manually via API."
            );
        }

        let started_at = Utc::now();

        // Store VM state
        {
            let mut vms = self.vms.write().await;
            vms.insert(
                id.to_string(),
                FirecrackerVm {
                    config: config.clone(),
                    process,
                    api_socket: api_socket.clone(),
                    log_file: log_file.clone(),
                    vsock_cid: Some(vsock_cid),
                    ssh_port,
                    started_at,
                },
            );
        }

        Ok(VmInfo {
            id: id.to_string(),
            state: VmState::Running,
            pid,
            started_at: Some(started_at),
            ip_address: None,
            ssh_port,
            display_port: None,
            qmp_socket: None,
            console_socket: Some(api_socket),
        })
    }

    async fn stop(&self, id: &str, timeout: Duration) -> Result<()> {
        let api_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.api_socket.clone()
        };

        info!(id = %id, timeout = ?timeout, "Stopping Firecracker VM");

        // Send shutdown via API (SendCtrlAltDel triggers ACPI shutdown)
        let action = ActionRequest {
            action_type: ActionType::SendCtrlAltDel,
        };
        let body = serde_json::to_string(&action)?;

        if let Err(e) = self
            .call_api(&api_socket, "PUT", "/actions", Some(&body))
            .await
        {
            warn!(id = %id, error = %e, "Failed to send SendCtrlAltDel, will force kill");
        }

        // Wait for graceful shutdown
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if tokio::time::Instant::now() > deadline {
                warn!(id = %id, "Graceful shutdown timed out, forcing kill");
                return self.kill(id).await;
            }

            // Check if process is still running by trying to connect to API socket
            match UnixStream::connect(&api_socket).await {
                Ok(_) => {
                    // Still running, wait a bit
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Err(_) => {
                    // Socket gone, VM stopped
                    debug!(id = %id, "Firecracker process stopped gracefully");
                    break;
                }
            }

            // Also check the process status
            {
                let mut vms = self.vms.write().await;
                if let Some(vm) = vms.get_mut(id) {
                    match vm.process.try_wait() {
                        Ok(Some(_)) => {
                            // Process exited
                            debug!(id = %id, "Firecracker process exited");
                            break;
                        }
                        Ok(None) => {
                            // Still running
                        }
                        Err(e) => {
                            warn!(id = %id, error = %e, "Error checking process status");
                        }
                    }
                }
            }
        }

        // Cleanup
        {
            let mut vms = self.vms.write().await;
            if let Some(mut vm) = vms.remove(id) {
                // Ensure process is terminated
                let _ = vm.process.kill().await;
            }
        }

        // Remove socket files
        let _ = tokio::fs::remove_file(&api_socket).await;
        let vsock_path = self.state_dir.join(format!("{}.vsock", id));
        let _ = tokio::fs::remove_file(&vsock_path).await;

        info!(id = %id, "Firecracker VM stopped");
        Ok(())
    }

    async fn kill(&self, id: &str) -> Result<()> {
        info!(id = %id, "Force killing Firecracker VM");

        let api_socket = {
            let mut vms = self.vms.write().await;
            if let Some(mut vm) = vms.remove(id) {
                // Send SIGKILL to the process
                if let Err(e) = vm.process.kill().await {
                    warn!(id = %id, error = %e, "Failed to kill process");
                }
                // Wait for process to actually terminate
                let _ = vm.process.wait().await;
                Some(vm.api_socket)
            } else {
                None
            }
        };

        // Cleanup socket files
        if let Some(socket) = api_socket {
            let _ = tokio::fs::remove_file(&socket).await;
        }
        let vsock_path = self.state_dir.join(format!("{}.vsock", id));
        let _ = tokio::fs::remove_file(&vsock_path).await;

        // Note: We keep the log file for debugging purposes

        info!(id = %id, "Firecracker VM killed");
        Ok(())
    }

    async fn pause(&self, id: &str) -> Result<()> {
        let api_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.api_socket.clone()
        };

        info!(id = %id, "Pausing Firecracker VM");

        self.call_api(&api_socket, "PATCH", "/vm", Some(r#"{"state":"Paused"}"#))
            .await?;

        Ok(())
    }

    async fn resume(&self, id: &str) -> Result<()> {
        let api_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.api_socket.clone()
        };

        info!(id = %id, "Resuming Firecracker VM");

        self.call_api(&api_socket, "PATCH", "/vm", Some(r#"{"state":"Resumed"}"#))
            .await?;

        Ok(())
    }

    async fn restart(&self, id: &str, config: &VmConfig) -> Result<()> {
        // Firecracker doesn't support in-place restart like QEMU's system_reset
        // We need to stop and recreate the VM
        info!(id = %id, "Restarting Firecracker VM (stop + start)");

        // Get the image path before stopping
        let image_path = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            match &vm.config.image {
                VmImage::File { path, .. } => path.clone(),
                _ => {
                    return Err(VmError::InvalidConfig(
                        "Cannot restart: original image path not available".to_string(),
                    ));
                }
            }
        };

        // Stop the VM
        self.stop(id, config.shutdown_timeout).await?;

        // Small delay to ensure cleanup
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Start the VM again
        self.start(config, &image_path).await?;

        // Wait for boot if SSH is configured
        if config.ssh.is_some() {
            self.wait_for_ssh(config, config.boot_timeout).await?;
        }

        info!(id = %id, "Firecracker VM restarted successfully");
        Ok(())
    }

    async fn inspect(&self, id: &str) -> Result<VmInfo> {
        let vms = self.vms.read().await;
        let vm = vms
            .get(id)
            .ok_or_else(|| VmError::NotFound(id.to_string()))?;

        // Try to determine the actual state by querying the API
        let state = match self.call_api(&vm.api_socket, "GET", "/", None).await {
            Ok(_) => VmState::Running,
            Err(_) => VmState::Shutoff,
        };

        Ok(VmInfo {
            id: id.to_string(),
            state,
            pid: vm.process.id(),
            started_at: Some(vm.started_at),
            ip_address: None,
            ssh_port: vm.ssh_port,
            display_port: None,
            qmp_socket: None,
            console_socket: Some(vm.api_socket.clone()),
        })
    }

    async fn is_running(&self, id: &str) -> Result<bool> {
        let mut vms = self.vms.write().await;

        if let Some(vm) = vms.get_mut(id) {
            // Check if process is still alive
            match vm.process.try_wait() {
                Ok(Some(_)) => {
                    // Process has exited
                    Ok(false)
                }
                Ok(None) => {
                    // Process is still running
                    Ok(true)
                }
                Err(e) => {
                    warn!(id = %id, error = %e, "Error checking process status");
                    // Assume running if we can't check
                    Ok(true)
                }
            }
        } else {
            Ok(false)
        }
    }

    async fn wait_for_boot(&self, id: &str, config: &VmConfig, timeout: Duration) -> Result<()> {
        let api_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.api_socket.clone()
        };

        let deadline = tokio::time::Instant::now() + timeout;

        info!(id = %id, timeout = ?timeout, "Waiting for Firecracker VM to boot");

        // First, wait for the API to be responsive
        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(VmError::Timeout(timeout));
            }

            if self.call_api(&api_socket, "GET", "/", None).await.is_ok() {
                debug!(id = %id, "Firecracker API is responsive");
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // If SSH is configured, also wait for SSH to be available
        if config.ssh.is_some() {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            info!(id = %id, "Waiting for SSH to be available");
            self.wait_for_ssh(config, remaining).await?;
            info!(id = %id, "SSH is available");
        }

        info!(id = %id, "Firecracker VM boot complete");
        Ok(())
    }

    async fn exec(&self, id: &str, command: &[String], config: &VmConfig) -> Result<ExecResult> {
        // Verify VM exists and is running
        {
            let vms = self.vms.read().await;
            if !vms.contains_key(id) {
                return Err(VmError::NotFound(id.to_string()));
            }
        }

        // Firecracker exec goes through SSH (requires TAP networking)
        if config.ssh.is_some() {
            self.ssh_exec(config, command).await
        } else {
            Err(VmError::InvalidConfig(
                "SSH not configured. To exec commands, configure SSH with TAP networking."
                    .to_string(),
            ))
        }
    }

    async fn console_log(&self, id: &str, lines: Option<usize>) -> Result<String> {
        // Get log file path from VM state or construct it
        let log_path = {
            let vms = self.vms.read().await;
            if let Some(vm) = vms.get(id) {
                vm.log_file.clone()
            } else {
                // VM might have been stopped, try the default path
                self.state_dir.join(format!("{}.log", id))
            }
        };

        let content = tokio::fs::read_to_string(&log_path)
            .await
            .unwrap_or_default();

        if let Some(n) = lines {
            // Return last N lines
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
        info!(id = %id, "Removing Firecracker VM and cleaning up resources");

        // Kill if running
        let _ = self.kill(id).await;

        // Cleanup all state files
        let files_to_remove = [
            format!("{}.sock", id),
            format!("{}.vsock", id),
            format!("{}.log", id),
        ];

        for file in &files_to_remove {
            let path = self.state_dir.join(file);
            if let Err(e) = tokio::fs::remove_file(&path).await {
                if e.kind() != std::io::ErrorKind::NotFound {
                    warn!(path = %path.display(), error = %e, "Failed to remove file");
                }
            }
        }

        // Also remove any snapshots for this VM
        let snapshot_pattern = format!("{}-", id);
        if let Ok(mut entries) = tokio::fs::read_dir(&self.state_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&snapshot_pattern) {
                    let _ = tokio::fs::remove_file(entry.path()).await;
                }
            }
        }

        info!(id = %id, "Firecracker VM removed");
        Ok(())
    }

    async fn health(&self, id: &str, config: &VmConfig) -> Result<HealthStatus> {
        // First check if VM exists and process is alive
        if !self.is_running(id).await? {
            return Ok(HealthStatus::Unhealthy {
                reason: "VM not running".to_string(),
            });
        }

        let api_socket = {
            let vms = self.vms.read().await;
            match vms.get(id) {
                Some(vm) => vm.api_socket.clone(),
                None => return Ok(HealthStatus::Unknown),
            }
        };

        // Check API responsiveness first
        if let Err(e) = self.call_api(&api_socket, "GET", "/", None).await {
            return Ok(HealthStatus::Unhealthy {
                reason: format!("API check failed: {}", e),
            });
        }

        // Run configured health check if any
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
                // HTTP check via port forward
                let url = format!("http://localhost:{}{}", port, path);
                let result = Command::new("curl")
                    .args([
                        "-s",
                        "-o",
                        "/dev/null",
                        "-w",
                        "%{http_code}",
                        "--connect-timeout",
                        "5",
                        &url,
                    ])
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
                // QMP is QEMU-specific, not supported by Firecracker
                Ok(HealthStatus::Unknown)
            }
            None => {
                // No specific health check, API being responsive is good enough
                Ok(HealthStatus::Healthy)
            }
        }
    }

    async fn snapshot(&self, id: &str, name: &str) -> Result<String> {
        let api_socket = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.api_socket.clone()
        };

        info!(id = %id, name = %name, "Creating Firecracker snapshot");

        // Pause the VM first (recommended for consistent snapshots)
        if let Err(e) = self.pause(id).await {
            warn!(id = %id, error = %e, "Failed to pause VM before snapshot (continuing anyway)");
        }

        let snapshot_path = self.state_dir.join(format!("{}-{}.snap", id, name));
        let mem_path = self.state_dir.join(format!("{}-{}.mem", id, name));

        let request = SnapshotCreateRequest {
            snapshot_type: SnapshotType::Full,
            snapshot_path: snapshot_path.to_string_lossy().to_string(),
            mem_file_path: mem_path.to_string_lossy().to_string(),
            version: None,
        };

        let body = serde_json::to_string(&request)?;

        let result = self
            .call_api(&api_socket, "PUT", "/snapshot/create", Some(&body))
            .await;

        // Resume the VM regardless of snapshot result
        if let Err(e) = self.resume(id).await {
            warn!(id = %id, error = %e, "Failed to resume VM after snapshot");
        }

        result?;

        info!(
            id = %id,
            name = %name,
            path = %snapshot_path.display(),
            "Firecracker snapshot created"
        );

        Ok(snapshot_path.to_string_lossy().to_string())
    }

    async fn restore_snapshot(&self, id: &str, snapshot_name: &str) -> Result<()> {
        // Firecracker snapshot restore requires a fresh Firecracker process
        // The VM must be stopped and restarted with snapshot load configuration
        info!(id = %id, snapshot = %snapshot_name, "Restoring Firecracker snapshot");

        let snapshot_path = self
            .state_dir
            .join(format!("{}-{}.snap", id, snapshot_name));
        let mem_path = self.state_dir.join(format!("{}-{}.mem", id, snapshot_name));

        // Verify snapshot files exist
        if !snapshot_path.exists() {
            return Err(VmError::Snapshot(format!(
                "Snapshot file not found: {}",
                snapshot_path.display()
            )));
        }
        if !mem_path.exists() {
            return Err(VmError::Snapshot(format!(
                "Memory file not found: {}",
                mem_path.display()
            )));
        }

        // Get the original config before stopping
        let config = {
            let vms = self.vms.read().await;
            let vm = vms
                .get(id)
                .ok_or_else(|| VmError::NotFound(id.to_string()))?;
            vm.config.clone()
        };

        // Stop the current VM
        self.kill(id).await?;

        // Create new API socket path
        let api_socket = self.state_dir.join(format!("{}.sock", id));
        let log_file = self.state_dir.join(format!("{}.log", id));

        // Remove old socket
        let _ = tokio::fs::remove_file(&api_socket).await;

        // Create log file
        let log_fd = std::fs::File::create(&log_file)
            .map_err(|e| VmError::Firecracker(format!("Failed to create log file: {}", e)))?;

        // Start fresh Firecracker process
        let process = Command::new(&self.firecracker_binary)
            .args([
                "--api-sock",
                &api_socket.to_string_lossy(),
                "--log-path",
                &log_file.to_string_lossy(),
                "--level",
                "Info",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(log_fd.try_clone().unwrap_or(log_fd))
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| VmError::Firecracker(format!("Failed to spawn: {}", e)))?;

        // Wait for API socket
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < deadline {
            if api_socket.exists() && UnixStream::connect(&api_socket).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if !api_socket.exists() {
            return Err(VmError::Firecracker("API socket not created".to_string()));
        }

        // Load the snapshot
        let request = SnapshotLoadRequest {
            snapshot_path: snapshot_path.to_string_lossy().to_string(),
            mem_backend: MemBackend {
                backend_path: mem_path.to_string_lossy().to_string(),
                backend_type: MemBackendType::File,
            },
            enable_diff_snapshots: None,
            resume_vm: Some(true),
        };

        let body = serde_json::to_string(&request)?;
        self.call_api(&api_socket, "PUT", "/snapshot/load", Some(&body))
            .await?;

        // Store VM state
        {
            let mut vms = self.vms.write().await;
            vms.insert(
                id.to_string(),
                FirecrackerVm {
                    config,
                    process,
                    api_socket,
                    log_file,
                    vsock_cid: Some(3),
                    ssh_port: None,
                    started_at: Utc::now(),
                },
            );
        }

        info!(id = %id, snapshot = %snapshot_name, "Firecracker snapshot restored");
        Ok(())
    }

    async fn list_snapshots(&self, id: &str) -> Result<Vec<SnapshotInfo>> {
        let mut snapshots = Vec::new();
        let pattern = format!("{}-", id);

        let mut entries = tokio::fs::read_dir(&self.state_dir)
            .await
            .map_err(|e| VmError::Backend(format!("Failed to read state dir: {}", e)))?;

        while let Some(entry) = entries.next_entry().await.transpose() {
            let entry = entry.map_err(|e| VmError::Backend(e.to_string()))?;
            let name = entry.file_name().to_string_lossy().to_string();

            if name.starts_with(&pattern) && name.ends_with(".snap") {
                let metadata = entry
                    .metadata()
                    .await
                    .map_err(|e| VmError::Backend(e.to_string()))?;

                let snapshot_name = name
                    .strip_prefix(&pattern)
                    .and_then(|s| s.strip_suffix(".snap"))
                    .unwrap_or(&name)
                    .to_string();

                snapshots.push(SnapshotInfo {
                    name: snapshot_name,
                    created_at: metadata
                        .modified()
                        .ok()
                        .map(chrono::DateTime::from)
                        .unwrap_or_else(Utc::now),
                    size_bytes: metadata.len(),
                    description: None,
                });
            }
        }

        Ok(snapshots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ImageFormat, MemoryConfig};
    use dtx_core::resource::ResourceId;

    #[test]
    fn firecracker_runtime_new() {
        let runtime = FirecrackerRuntime::new();
        assert_eq!(runtime.name(), "firecracker");
    }

    #[test]
    fn firecracker_runtime_with_state_dir() {
        let runtime = FirecrackerRuntime::new().with_state_dir(PathBuf::from("/tmp/fc-vms"));
        assert_eq!(runtime.state_dir, PathBuf::from("/tmp/fc-vms"));
    }

    #[test]
    fn firecracker_runtime_with_binaries() {
        let runtime = FirecrackerRuntime::with_binaries(
            PathBuf::from("/custom/firecracker"),
            Some(PathBuf::from("/custom/jailer")),
        );
        assert_eq!(
            runtime.firecracker_binary,
            PathBuf::from("/custom/firecracker")
        );
        assert_eq!(runtime.jailer_binary, Some(PathBuf::from("/custom/jailer")));
    }

    #[test]
    fn parse_memory_mib() {
        assert_eq!(FirecrackerRuntime::parse_memory_mib("512M"), 512);
        assert_eq!(FirecrackerRuntime::parse_memory_mib("2G"), 2048);
        assert_eq!(FirecrackerRuntime::parse_memory_mib("4g"), 4096);
        assert_eq!(FirecrackerRuntime::parse_memory_mib("1024"), 1024);
    }

    #[test]
    fn generate_mac() {
        let mac = FirecrackerRuntime::generate_mac();
        assert!(mac.starts_with("AA:FC:00:"));
        assert_eq!(mac.len(), 17);

        // Generate multiple MACs to ensure they're unique
        let mac2 = FirecrackerRuntime::generate_mac();
        // Note: There's a small chance they could match, but very unlikely
        // This is more of a smoke test than a strict uniqueness test
        println!("Generated MACs: {} and {}", mac, mac2);
    }

    #[tokio::test]
    async fn firecracker_create() {
        let runtime = FirecrackerRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-fc-vm"),
            VmImage::File {
                path: PathBuf::from("/path/to/rootfs.ext4"),
                format: ImageFormat::Raw,
            },
        );

        let result = runtime.create(&config).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "test-fc-vm");
    }

    #[tokio::test]
    async fn firecracker_is_running_not_found() {
        let runtime = FirecrackerRuntime::new();
        let result = runtime.is_running("nonexistent").await;
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn firecracker_image_not_found() {
        let runtime = FirecrackerRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/nonexistent/path.ext4"),
                format: ImageFormat::Raw,
            },
        );

        let result = runtime.prepare_image(&config).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VmError::ImageNotFound(_)));
    }

    #[tokio::test]
    async fn firecracker_inspect_not_found() {
        let runtime = FirecrackerRuntime::new();
        let result = runtime.inspect("nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), VmError::NotFound(_)));
    }

    #[tokio::test]
    async fn firecracker_console_log_empty() {
        let runtime = FirecrackerRuntime::new();
        let result = runtime.console_log("nonexistent", None).await;
        // Should return empty string for nonexistent log
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn firecracker_list_snapshots_empty() {
        let runtime =
            FirecrackerRuntime::new().with_state_dir(std::env::temp_dir().join("fc-test"));
        tokio::fs::create_dir_all(&runtime.state_dir).await.ok();

        let result = runtime.list_snapshots("test-vm").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn boot_source_serialization() {
        let boot = BootSource {
            kernel_image_path: "/path/to/vmlinux".to_string(),
            boot_args: Some("console=ttyS0".to_string()),
            initrd_path: None,
        };

        let json = serde_json::to_string(&boot).expect("serialize");
        assert!(json.contains("kernel_image_path"));
        assert!(json.contains("boot_args"));
        assert!(!json.contains("initrd_path")); // None should be skipped
    }

    #[test]
    fn drive_serialization() {
        let drive = Drive {
            drive_id: "rootfs".to_string(),
            path_on_host: "/path/to/rootfs.ext4".to_string(),
            is_root_device: true,
            is_read_only: false,
            partuuid: None,
            rate_limiter: None,
        };

        let json = serde_json::to_string(&drive).expect("serialize");
        assert!(json.contains("drive_id"));
        assert!(json.contains("is_root_device"));
    }

    #[test]
    fn machine_config_serialization() {
        let config = MachineConfig {
            vcpu_count: 4,
            mem_size_mib: 2048,
            smt: false,
            track_dirty_pages: false,
        };

        let json = serde_json::to_string(&config).expect("serialize");
        assert!(json.contains("\"vcpu_count\":4"));
        assert!(json.contains("\"mem_size_mib\":2048"));
    }

    #[test]
    fn network_interface_serialization() {
        let iface = NetworkInterface {
            iface_id: "eth0".to_string(),
            guest_mac: "AA:FC:00:00:00:01".to_string(),
            host_dev_name: "tap0".to_string(),
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        };

        let json = serde_json::to_string(&iface).expect("serialize");
        assert!(json.contains("iface_id"));
        assert!(json.contains("guest_mac"));
        assert!(json.contains("host_dev_name"));
    }

    #[test]
    fn action_request_serialization() {
        let action = ActionRequest {
            action_type: ActionType::InstanceStart,
        };

        let json = serde_json::to_string(&action).expect("serialize");
        assert!(json.contains("InstanceStart"));
    }

    #[test]
    fn vm_state_request_serialization() {
        let state = VmStateRequest {
            state: VmStateType::Paused,
        };

        let json = serde_json::to_string(&state).expect("serialize");
        assert!(json.contains("Paused"));
    }

    #[test]
    fn snapshot_create_request_serialization() {
        let request = SnapshotCreateRequest {
            snapshot_type: SnapshotType::Full,
            snapshot_path: "/path/to/snapshot".to_string(),
            mem_file_path: "/path/to/mem".to_string(),
            version: None,
        };

        let json = serde_json::to_string(&request).expect("serialize");
        assert!(json.contains("Full"));
        assert!(json.contains("snapshot_path"));
        assert!(json.contains("mem_file_path"));
    }

    #[test]
    fn snapshot_load_request_serialization() {
        let request = SnapshotLoadRequest {
            snapshot_path: "/path/to/snapshot".to_string(),
            mem_backend: MemBackend {
                backend_path: "/path/to/mem".to_string(),
                backend_type: MemBackendType::File,
            },
            enable_diff_snapshots: None,
            resume_vm: Some(true),
        };

        let json = serde_json::to_string(&request).expect("serialize");
        assert!(json.contains("snapshot_path"));
        assert!(json.contains("mem_backend"));
        assert!(json.contains("File"));
    }

    #[test]
    fn vm_config_with_firecracker_options() {
        let config = VmConfig::new(
            ResourceId::new("fc-test"),
            VmImage::File {
                path: PathBuf::from("/path/to/rootfs.ext4"),
                format: ImageFormat::Raw,
            },
        )
        .with_cpu(crate::config::CpuConfig::new(2))
        .with_memory(MemoryConfig::new("1G"));

        assert_eq!(config.cpu.count, 2);
        assert_eq!(config.memory.size, "1G");
    }

    #[test]
    fn parse_http_response() {
        let runtime = FirecrackerRuntime::new();

        let response =
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"state\":\"running\"}";
        let result = runtime.parse_http_response(response);
        assert!(result.is_ok());
        let (status, body) = result.unwrap();
        assert!(status.contains("200"));
        assert!(body.contains("running"));
    }

    #[test]
    fn get_boot_args_default() {
        let runtime = FirecrackerRuntime::new();
        let config = VmConfig::new(
            ResourceId::new("test"),
            VmImage::File {
                path: PathBuf::new(),
                format: ImageFormat::Raw,
            },
        );

        let args = runtime.get_boot_args(&config);
        assert!(args.contains("console=ttyS0"));
    }

    #[test]
    fn get_boot_args_custom() {
        let runtime = FirecrackerRuntime::new();
        let mut config = VmConfig::new(
            ResourceId::new("test"),
            VmImage::File {
                path: PathBuf::new(),
                format: ImageFormat::Raw,
            },
        );
        config.extra_args = vec![
            "--boot-args".to_string(),
            "console=ttyS0 root=/dev/vda".to_string(),
        ];

        let args = runtime.get_boot_args(&config);
        assert!(args.contains("root=/dev/vda"));
    }
}
