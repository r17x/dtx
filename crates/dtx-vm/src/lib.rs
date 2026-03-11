//! VM backend for dtx.
//!
//! This crate provides virtual machine support as a Resource backend for dtx,
//! enabling VM orchestration alongside native processes and containers.
//!
//! # Supported Runtimes
//!
//! - **QEMU/KVM** - Full-featured virtualization with extensive hardware support
//! - **Firecracker** - Lightweight microVMs for fast startup and minimal overhead
//! - **NixOS VM** - NixOS-specific VMs built from flakes or configurations
//!
//! # Features
//!
//! - `qemu` - Enable QEMU runtime (default)
//! - `firecracker` - Enable Firecracker runtime (default)
//! - `nixos` - Enable NixOS VM runtime (default)
//!
//! # Example
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use std::path::PathBuf;
//!
//! use dtx_vm::{VmConfig, VmImage, VmResource};
//! use dtx_vm::config::{CpuConfig, MemoryConfig, SshConfig, ImageFormat};
//! use dtx_core::events::ResourceEventBus;
//! use dtx_core::resource::{Resource, ResourceId, Context};
//!
//! async fn example() -> dtx_vm::Result<()> {
//!     // Create configuration
//!     let config = VmConfig::new(
//!         ResourceId::new("my-vm"),
//!         VmImage::File {
//!             path: PathBuf::from("/path/to/image.qcow2"),
//!             format: ImageFormat::Qcow2,
//!         },
//!     )
//!     .with_cpu(CpuConfig::new(4))
//!     .with_memory(MemoryConfig::new("4G"))
//!     .with_ssh(SshConfig::new("root", 2222));
//!
//!     // Detect available runtime
//!     let runtime = dtx_vm::detect_runtime().await
//!         .expect("No VM runtime available");
//!
//!     // Create resource
//!     let event_bus = Arc::new(ResourceEventBus::new());
//!     let mut vm = VmResource::new(config, Arc::from(runtime), event_bus);
//!
//!     // Start VM
//!     let ctx = Context::new();
//!     vm.start(&ctx).await?;
//!
//!     // Execute command
//!     let result = vm.exec(&["uname".to_string(), "-a".to_string()]).await?;
//!     println!("Output: {}", result.stdout);
//!
//!     // Stop VM
//!     vm.stop(&ctx).await?;
//!     Ok(())
//! }
//! ```
//!
//! # YAML Configuration
//!
//! VMs can be configured in .dtx/config.yaml:
//!
//! ```yaml
//! version: "2"
//!
//! resources:
//!   # NixOS VM from flake
//!   dev-vm:
//!     kind: vm
//!     runtime: nixos
//!     image:
//!       type: nixos_flake
//!       flake: "./dev-vm"
//!       attribute: "vm"
//!     cpu:
//!       count: 4
//!     memory:
//!       size: "4G"
//!     ssh:
//!       port: 22
//!       user: dev
//!       host_port: 2222
//!     health_check:
//!       type: ssh
//!       interval: 10s
//!
//!   # QEMU VM from disk image
//!   legacy-vm:
//!     kind: vm
//!     runtime: qemu
//!     image:
//!       type: file
//!       path: ./images/legacy.qcow2
//!       format: qcow2
//!     cpu:
//!       count: 2
//!     memory:
//!       size: "2G"
//!     port_forwards:
//!       - host: 8080
//!         guest: 80
//! ```

pub mod config;
pub mod error;
pub mod resource;
pub mod runtime;

#[cfg(feature = "qemu")]
pub mod qemu;

#[cfg(feature = "firecracker")]
pub mod firecracker;

#[cfg(feature = "nixos")]
pub mod nixos;

// Re-exports for convenience
pub use config::{
    CpuConfig, DiskConfig, GraphicsConfig, ImageFormat, MemoryConfig, NetworkConfig, NetworkMode,
    PortForward, Protocol, SharedDir, SshConfig, VmConfig, VmHealthCheck, VmImage, VmRuntimeType,
};
pub use error::{Result, VmError};
pub use resource::VmResource;
pub use runtime::{ExecResult, SnapshotInfo, VmInfo, VmRuntime, VmState};

#[cfg(feature = "qemu")]
pub use qemu::QemuRuntime;

#[cfg(feature = "firecracker")]
pub use firecracker::FirecrackerRuntime;

#[cfg(feature = "nixos")]
pub use nixos::NixosVmRuntime;

#[cfg(any(feature = "qemu", feature = "firecracker", feature = "nixos"))]
pub use runtime::{detect_runtime, runtime_for_config};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::const_is_empty)] // VERSION is compile-time constant from env!()
    fn version_defined() {
        // VERSION is set at compile time from Cargo.toml
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn config_export() {
        // Test that config types are accessible
        let _config = VmConfig::default();
        let _cpu = CpuConfig::default();
        let _memory = MemoryConfig::default();
    }

    #[test]
    fn runtime_types_export() {
        // Test that runtime types are accessible
        let _state = VmState::Pending;
        let _result = ExecResult::new(0, "", "");
    }

    #[test]
    fn error_types_export() {
        // Test that error types are accessible
        let _err = VmError::NotFound("test".to_string());
    }
}
