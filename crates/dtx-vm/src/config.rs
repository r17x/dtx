//! VM configuration types.
//!
//! This module provides comprehensive configuration for virtual machines
//! supporting QEMU, Firecracker, and NixOS VM runtimes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use dtx_core::resource::ResourceId;
use serde::{Deserialize, Serialize};

/// Configuration for a virtual machine resource.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VmConfig {
    /// Resource identifier.
    pub id: ResourceId,

    /// Human-readable name for the VM.
    #[serde(default)]
    pub name: Option<String>,

    /// VM runtime type.
    #[serde(default)]
    pub runtime: VmRuntimeType,

    /// VM image source.
    pub image: VmImage,

    /// CPU configuration.
    #[serde(default)]
    pub cpu: CpuConfig,

    /// Memory configuration.
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Additional disk configurations.
    #[serde(default)]
    pub disks: Vec<DiskConfig>,

    /// Network configuration.
    #[serde(default)]
    pub network: NetworkConfig,

    /// Port forwarding rules.
    #[serde(default)]
    pub port_forwards: Vec<PortForward>,

    /// Shared directories with host.
    #[serde(default)]
    pub shared_dirs: Vec<SharedDir>,

    /// Cloud-init or ignition configuration.
    #[serde(default)]
    pub init_config: Option<InitConfig>,

    /// SSH configuration for connecting to VM.
    #[serde(default)]
    pub ssh: Option<SshConfig>,

    /// Startup timeout.
    #[serde(default = "default_boot_timeout", with = "humantime_serde")]
    pub boot_timeout: Duration,

    /// Shutdown timeout before force kill.
    #[serde(default = "default_shutdown_timeout", with = "humantime_serde")]
    pub shutdown_timeout: Duration,

    /// Health check configuration.
    #[serde(default)]
    pub health_check: Option<VmHealthCheck>,

    /// Labels for grouping and filtering.
    #[serde(default)]
    pub labels: HashMap<String, String>,

    /// Enable KVM acceleration (requires host support).
    #[serde(default = "default_kvm")]
    pub kvm: bool,

    /// Graphics configuration (none, vnc, spice).
    #[serde(default)]
    pub graphics: GraphicsConfig,

    /// Additional runtime-specific arguments.
    #[serde(default)]
    pub extra_args: Vec<String>,
}

fn default_boot_timeout() -> Duration {
    Duration::from_secs(120)
}
fn default_shutdown_timeout() -> Duration {
    Duration::from_secs(30)
}
fn default_kvm() -> bool {
    true
}

impl VmConfig {
    /// Create a new VM configuration with minimal required fields.
    pub fn new(id: ResourceId, image: VmImage) -> Self {
        Self {
            id,
            name: None,
            runtime: VmRuntimeType::default(),
            image,
            cpu: CpuConfig::default(),
            memory: MemoryConfig::default(),
            disks: Vec::new(),
            network: NetworkConfig::default(),
            port_forwards: Vec::new(),
            shared_dirs: Vec::new(),
            init_config: None,
            ssh: None,
            boot_timeout: default_boot_timeout(),
            shutdown_timeout: default_shutdown_timeout(),
            health_check: None,
            labels: HashMap::new(),
            kvm: default_kvm(),
            graphics: GraphicsConfig::default(),
            extra_args: Vec::new(),
        }
    }

    /// Set the VM name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the runtime type.
    #[must_use]
    pub fn with_runtime(mut self, runtime: VmRuntimeType) -> Self {
        self.runtime = runtime;
        self
    }

    /// Set CPU configuration.
    #[must_use]
    pub fn with_cpu(mut self, cpu: CpuConfig) -> Self {
        self.cpu = cpu;
        self
    }

    /// Set memory configuration.
    #[must_use]
    pub fn with_memory(mut self, memory: MemoryConfig) -> Self {
        self.memory = memory;
        self
    }

    /// Add SSH configuration.
    #[must_use]
    pub fn with_ssh(mut self, ssh: SshConfig) -> Self {
        self.ssh = Some(ssh);
        self
    }

    /// Add a port forward rule.
    #[must_use]
    pub fn with_port_forward(mut self, host: u16, guest: u16) -> Self {
        self.port_forwards.push(PortForward {
            host,
            guest,
            protocol: Protocol::default(),
        });
        self
    }

    /// Set boot timeout.
    #[must_use]
    pub fn with_boot_timeout(mut self, timeout: Duration) -> Self {
        self.boot_timeout = timeout;
        self
    }

    /// Add a health check.
    #[must_use]
    pub fn with_health_check(mut self, health_check: VmHealthCheck) -> Self {
        self.health_check = Some(health_check);
        self
    }

    /// Get the effective VM name (name or id).
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or_else(|| self.id.as_str())
    }
}

impl Default for VmConfig {
    fn default() -> Self {
        Self::new(
            ResourceId::new("default"),
            VmImage::File {
                path: PathBuf::new(),
                format: ImageFormat::default(),
            },
        )
    }
}

/// VM runtime type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VmRuntimeType {
    /// Auto-detect the best available runtime.
    #[default]
    Auto,
    /// QEMU/KVM.
    Qemu,
    /// Firecracker microVM.
    Firecracker,
    /// NixOS VM (nixos-rebuild build-vm).
    #[serde(rename = "nixos")]
    NixOS,
}

impl VmRuntimeType {
    /// Get the runtime name as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Qemu => "qemu",
            Self::Firecracker => "firecracker",
            Self::NixOS => "nixos",
        }
    }
}

/// VM image source.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VmImage {
    /// Local disk image file.
    File {
        /// Path to the disk image.
        path: PathBuf,
        /// Image format.
        #[serde(default)]
        format: ImageFormat,
    },
    /// NixOS flake reference.
    NixosFlake {
        /// Flake URI (e.g., "github:nixos/nixpkgs#nixos-24.05").
        flake: String,
        /// Attribute path within the flake.
        #[serde(default)]
        attribute: Option<String>,
    },
    /// Docker/OCI container image (for Firecracker).
    Container {
        /// Container image reference.
        image: String,
    },
    /// Cloud image URL.
    Url {
        /// URL to download the image from.
        url: String,
        /// Expected checksum (sha256).
        #[serde(default)]
        checksum: Option<String>,
    },
    /// Build from Nix expression.
    NixBuild {
        /// Nix expression to evaluate.
        expression: String,
        /// Attribute to build.
        #[serde(default)]
        attribute: String,
    },
}

/// Disk image format.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    /// QEMU Copy-On-Write 2.
    #[default]
    Qcow2,
    /// Raw disk image.
    Raw,
    /// VMware disk.
    Vmdk,
    /// VirtualBox disk.
    Vdi,
}

impl ImageFormat {
    /// Get format as string for QEMU command line.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Qcow2 => "qcow2",
            Self::Raw => "raw",
            Self::Vmdk => "vmdk",
            Self::Vdi => "vdi",
        }
    }
}

/// CPU configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CpuConfig {
    /// Number of vCPUs.
    #[serde(default = "default_vcpus")]
    pub count: u32,
    /// CPU model (host, max, or specific model).
    #[serde(default)]
    pub model: Option<String>,
    /// CPU features to enable/disable.
    #[serde(default)]
    pub features: Vec<String>,
}

fn default_vcpus() -> u32 {
    2
}

impl Default for CpuConfig {
    fn default() -> Self {
        Self {
            count: default_vcpus(),
            model: None,
            features: Vec::new(),
        }
    }
}

impl CpuConfig {
    /// Create a new CPU configuration.
    pub fn new(count: u32) -> Self {
        Self {
            count,
            model: None,
            features: Vec::new(),
        }
    }

    /// Set the CPU model.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
}

/// Memory configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Memory size (e.g., "2G", "512M").
    #[serde(default = "default_memory")]
    pub size: String,
    /// Enable memory ballooning.
    #[serde(default)]
    pub balloon: bool,
    /// Huge pages configuration.
    #[serde(default)]
    pub huge_pages: bool,
}

fn default_memory() -> String {
    "2G".to_string()
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            size: default_memory(),
            balloon: false,
            huge_pages: false,
        }
    }
}

impl MemoryConfig {
    /// Create a new memory configuration.
    pub fn new(size: impl Into<String>) -> Self {
        Self {
            size: size.into(),
            balloon: false,
            huge_pages: false,
        }
    }

    /// Parse memory size to megabytes.
    pub fn size_mb(&self) -> u64 {
        parse_memory_mb(&self.size)
    }
}

/// Parse memory string to megabytes.
pub fn parse_memory_mb(mem: &str) -> u64 {
    let mem = mem.trim().to_lowercase();
    if mem.ends_with('g') {
        mem.trim_end_matches('g')
            .parse::<u64>()
            .unwrap_or(1)
            .saturating_mul(1024)
    } else if mem.ends_with('m') {
        mem.trim_end_matches('m').parse::<u64>().unwrap_or(512)
    } else if mem.ends_with('k') {
        mem.trim_end_matches('k')
            .parse::<u64>()
            .unwrap_or(512 * 1024)
            / 1024
    } else {
        mem.parse::<u64>().unwrap_or(512)
    }
}

/// Disk configuration for additional disks.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiskConfig {
    /// Path to disk image or device.
    pub path: PathBuf,
    /// Disk format.
    #[serde(default)]
    pub format: ImageFormat,
    /// Read-only disk.
    #[serde(default)]
    pub read_only: bool,
    /// Disk interface (virtio, ide, scsi).
    #[serde(default)]
    pub interface: DiskInterface,
    /// Create if doesn't exist (with size).
    #[serde(default)]
    pub create_size: Option<String>,
}

/// Disk interface type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiskInterface {
    /// VirtIO (fastest, requires guest support).
    #[default]
    Virtio,
    /// IDE (legacy).
    Ide,
    /// SCSI.
    Scsi,
    /// NVMe (modern).
    Nvme,
}

impl DiskInterface {
    /// Get interface as string for QEMU.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Virtio => "virtio",
            Self::Ide => "ide",
            Self::Scsi => "scsi",
            Self::Nvme => "nvme",
        }
    }
}

/// Network configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network mode.
    #[serde(default)]
    pub mode: NetworkMode,
    /// MAC address (auto-generated if not specified).
    #[serde(default)]
    pub mac_address: Option<String>,
    /// Bridge name (for bridged mode).
    #[serde(default)]
    pub bridge: Option<String>,
    /// TAP device name.
    #[serde(default)]
    pub tap: Option<String>,
}

/// Network mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    /// User-mode networking (SLIRP).
    #[default]
    User,
    /// Bridged networking.
    Bridged,
    /// TAP device.
    Tap,
    /// No networking.
    None,
}

/// Port forwarding rule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortForward {
    /// Host port.
    pub host: u16,
    /// Guest port.
    pub guest: u16,
    /// Protocol (tcp/udp).
    #[serde(default)]
    pub protocol: Protocol,
}

/// Network protocol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// TCP protocol.
    #[default]
    Tcp,
    /// UDP protocol.
    Udp,
}

impl Protocol {
    /// Get protocol as string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

/// Shared directory configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SharedDir {
    /// Host path.
    pub source: PathBuf,
    /// Guest mount tag.
    pub tag: String,
    /// Read-only mount.
    #[serde(default)]
    pub read_only: bool,
    /// Share protocol.
    #[serde(default)]
    pub protocol: ShareProtocol,
}

/// Share protocol for host-guest directory sharing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShareProtocol {
    /// VirtIO-FS (fast, requires guest support).
    #[default]
    #[serde(rename = "virtio-fs")]
    VirtioFs,
    /// Plan 9 filesystem (9p).
    #[serde(rename = "9p")]
    Plan9,
}

/// Cloud-init or ignition configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InitConfig {
    /// Cloud-init configuration.
    CloudInit {
        /// User data (YAML).
        user_data: String,
        /// Meta data (optional).
        #[serde(default)]
        meta_data: Option<String>,
    },
    /// Ignition configuration (CoreOS/Flatcar).
    Ignition {
        /// Ignition config JSON.
        config: String,
    },
    /// NixOS configuration module.
    NixosConfig {
        /// NixOS configuration path or expression.
        configuration: String,
    },
}

/// SSH configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SshConfig {
    /// SSH port on guest (default: 22).
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    /// Username.
    #[serde(default = "default_ssh_user")]
    pub user: String,
    /// Private key path.
    #[serde(default)]
    pub identity_file: Option<PathBuf>,
    /// Host port for SSH forwarding.
    #[serde(default)]
    pub host_port: Option<u16>,
    /// SSH options.
    #[serde(default)]
    pub options: Vec<String>,
}

fn default_ssh_port() -> u16 {
    22
}
fn default_ssh_user() -> String {
    "root".to_string()
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            port: default_ssh_port(),
            user: default_ssh_user(),
            identity_file: None,
            host_port: None,
            options: Vec::new(),
        }
    }
}

impl SshConfig {
    /// Create a new SSH configuration.
    pub fn new(user: impl Into<String>, host_port: u16) -> Self {
        Self {
            port: default_ssh_port(),
            user: user.into(),
            identity_file: None,
            host_port: Some(host_port),
            options: Vec::new(),
        }
    }

    /// Set the identity file.
    #[must_use]
    pub fn with_identity_file(mut self, path: PathBuf) -> Self {
        self.identity_file = Some(path);
        self
    }
}

/// VM health check configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VmHealthCheck {
    /// SSH connection check.
    Ssh {
        /// Check interval.
        #[serde(default = "default_health_interval", with = "humantime_serde")]
        interval: Duration,
        /// Check timeout.
        #[serde(default = "default_health_timeout", with = "humantime_serde")]
        timeout: Duration,
    },
    /// HTTP check via port forward.
    Http {
        /// Port to check.
        port: u16,
        /// Path to request.
        path: String,
        /// Check interval.
        #[serde(default = "default_health_interval", with = "humantime_serde")]
        interval: Duration,
        /// Check timeout.
        #[serde(default = "default_health_timeout", with = "humantime_serde")]
        timeout: Duration,
    },
    /// Execute command in VM.
    Exec {
        /// Command to execute.
        command: Vec<String>,
        /// Check interval.
        #[serde(default = "default_health_interval", with = "humantime_serde")]
        interval: Duration,
        /// Check timeout.
        #[serde(default = "default_health_timeout", with = "humantime_serde")]
        timeout: Duration,
    },
    /// QMP query (QEMU only).
    Qmp {
        /// Check interval.
        #[serde(default = "default_health_interval", with = "humantime_serde")]
        interval: Duration,
    },
}

fn default_health_interval() -> Duration {
    Duration::from_secs(10)
}
fn default_health_timeout() -> Duration {
    Duration::from_secs(5)
}

/// Graphics configuration.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum GraphicsConfig {
    /// No graphics (headless).
    #[default]
    None,
    /// VNC server.
    Vnc {
        /// VNC port.
        port: Option<u16>,
        /// VNC password.
        password: Option<String>,
    },
    /// SPICE server.
    Spice {
        /// SPICE port.
        port: Option<u16>,
    },
    /// GTK window (local display).
    Gtk,
}

/// Serde helper for Duration serialization using humantime format.
mod humantime_serde {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let secs = duration.as_secs();
        if secs >= 60 {
            serializer.serialize_str(&format!("{}m", secs / 60))
        } else {
            serializer.serialize_str(&format!("{}s", secs))
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_duration(&s).map_err(serde::de::Error::custom)
    }

    fn parse_duration(s: &str) -> Result<Duration, String> {
        let s = s.trim().to_lowercase();
        if s.ends_with('s') {
            let secs = s
                .trim_end_matches('s')
                .parse::<u64>()
                .map_err(|e| e.to_string())?;
            Ok(Duration::from_secs(secs))
        } else if s.ends_with('m') {
            let mins = s
                .trim_end_matches('m')
                .parse::<u64>()
                .map_err(|e| e.to_string())?;
            Ok(Duration::from_secs(mins * 60))
        } else if s.ends_with('h') {
            let hours = s
                .trim_end_matches('h')
                .parse::<u64>()
                .map_err(|e| e.to_string())?;
            Ok(Duration::from_secs(hours * 3600))
        } else {
            // Assume seconds
            let secs = s.parse::<u64>().map_err(|e| e.to_string())?;
            Ok(Duration::from_secs(secs))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_config_new() {
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::from("/path/to/image.qcow2"),
                format: ImageFormat::Qcow2,
            },
        );

        assert_eq!(config.id.as_str(), "test-vm");
        assert_eq!(config.cpu.count, 2);
        assert_eq!(config.memory.size, "2G");
    }

    #[test]
    fn vm_config_builder() {
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::NixosFlake {
                flake: "github:nixos/nixpkgs#nixos-24.05".to_string(),
                attribute: Some("vm".to_string()),
            },
        )
        .with_name("My Test VM")
        .with_runtime(VmRuntimeType::Qemu)
        .with_cpu(CpuConfig::new(4).with_model("host"))
        .with_memory(MemoryConfig::new("4G"))
        .with_ssh(SshConfig::new("admin", 2222))
        .with_port_forward(8080, 80)
        .with_boot_timeout(Duration::from_secs(180));

        assert_eq!(config.name, Some("My Test VM".to_string()));
        assert_eq!(config.runtime, VmRuntimeType::Qemu);
        assert_eq!(config.cpu.count, 4);
        assert_eq!(config.cpu.model, Some("host".to_string()));
        assert_eq!(config.memory.size, "4G");
        assert!(config.ssh.is_some());
        assert_eq!(config.port_forwards.len(), 1);
        assert_eq!(config.boot_timeout, Duration::from_secs(180));
    }

    #[test]
    fn config_serde_roundtrip() {
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::NixosFlake {
                flake: "github:nixos/nixpkgs#nixos-24.05".to_string(),
                attribute: Some("vm".to_string()),
            },
        )
        .with_cpu(CpuConfig::new(4))
        .with_memory(MemoryConfig::new("4G"))
        .with_ssh(SshConfig::new("admin", 2222));

        let json = serde_json::to_string(&config).expect("serialize");
        let parsed: VmConfig = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(parsed.id.as_str(), "test-vm");
        assert_eq!(parsed.cpu.count, 4);
        assert_eq!(parsed.memory.size, "4G");
    }

    #[test]
    fn parse_memory_mb_test() {
        assert_eq!(parse_memory_mb("512M"), 512);
        assert_eq!(parse_memory_mb("2G"), 2048);
        assert_eq!(parse_memory_mb("4g"), 4096);
        assert_eq!(parse_memory_mb("1024"), 1024);
        assert_eq!(parse_memory_mb("1024K"), 1);
    }

    #[test]
    fn image_format_as_str() {
        assert_eq!(ImageFormat::Qcow2.as_str(), "qcow2");
        assert_eq!(ImageFormat::Raw.as_str(), "raw");
        assert_eq!(ImageFormat::Vmdk.as_str(), "vmdk");
        assert_eq!(ImageFormat::Vdi.as_str(), "vdi");
    }

    #[test]
    fn runtime_type_as_str() {
        assert_eq!(VmRuntimeType::Auto.as_str(), "auto");
        assert_eq!(VmRuntimeType::Qemu.as_str(), "qemu");
        assert_eq!(VmRuntimeType::Firecracker.as_str(), "firecracker");
        assert_eq!(VmRuntimeType::NixOS.as_str(), "nixos");
    }

    #[test]
    fn vm_image_serde() {
        let image = VmImage::File {
            path: PathBuf::from("/path/to/image.qcow2"),
            format: ImageFormat::Qcow2,
        };
        let json = serde_json::to_string(&image).expect("serialize");
        assert!(json.contains("\"type\":\"file\""));

        let image = VmImage::NixosFlake {
            flake: "github:nixos/nixpkgs".to_string(),
            attribute: Some("vm".to_string()),
        };
        let json = serde_json::to_string(&image).expect("serialize");
        assert!(json.contains("\"type\":\"nixos_flake\""));
    }

    #[test]
    fn network_mode_serde() {
        let mode = NetworkMode::User;
        let json = serde_json::to_string(&mode).expect("serialize");
        assert_eq!(json, "\"user\"");

        let mode: NetworkMode = serde_json::from_str("\"bridged\"").expect("deserialize");
        assert_eq!(mode, NetworkMode::Bridged);
    }

    #[test]
    fn health_check_serde() {
        let check = VmHealthCheck::Ssh {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
        };
        let json = serde_json::to_string(&check).expect("serialize");
        assert!(json.contains("\"type\":\"ssh\""));

        let check = VmHealthCheck::Http {
            port: 8080,
            path: "/health".to_string(),
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(10),
        };
        let json = serde_json::to_string(&check).expect("serialize");
        assert!(json.contains("\"type\":\"http\""));
    }

    #[test]
    fn graphics_config_serde() {
        let graphics = GraphicsConfig::None;
        let json = serde_json::to_string(&graphics).expect("serialize");
        assert!(json.contains("\"type\":\"none\""));

        let graphics = GraphicsConfig::Vnc {
            port: Some(5900),
            password: None,
        };
        let json = serde_json::to_string(&graphics).expect("serialize");
        assert!(json.contains("\"type\":\"vnc\""));
    }

    #[test]
    fn ssh_config_builder() {
        let ssh = SshConfig::new("admin", 2222)
            .with_identity_file(PathBuf::from("/home/user/.ssh/id_rsa"));

        assert_eq!(ssh.user, "admin");
        assert_eq!(ssh.host_port, Some(2222));
        assert_eq!(
            ssh.identity_file,
            Some(PathBuf::from("/home/user/.ssh/id_rsa"))
        );
    }

    #[test]
    fn cpu_config_builder() {
        let cpu = CpuConfig::new(8).with_model("Skylake-Server");

        assert_eq!(cpu.count, 8);
        assert_eq!(cpu.model, Some("Skylake-Server".to_string()));
    }

    #[test]
    fn display_name() {
        let config = VmConfig::new(
            ResourceId::new("test-vm"),
            VmImage::File {
                path: PathBuf::new(),
                format: ImageFormat::default(),
            },
        );
        assert_eq!(config.display_name(), "test-vm");

        let config = config.with_name("My VM");
        assert_eq!(config.display_name(), "My VM");
    }
}
