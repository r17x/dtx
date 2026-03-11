//! Capability system for sandboxed plugins.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// Capabilities granted to a sandboxed plugin.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// Allowed network operations.
    pub network: NetworkCapabilities,

    /// Allowed filesystem operations.
    pub filesystem: FilesystemCapabilities,

    /// Allowed process operations.
    pub process: ProcessCapabilities,

    /// Allowed environment access.
    pub environment: EnvironmentCapabilities,

    /// Allowed event bus operations.
    pub events: EventCapabilities,

    /// Allowed resource management.
    pub resources: ResourceCapabilities,
}

/// Network-related capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkCapabilities {
    /// Can make outbound connections.
    pub connect: bool,

    /// Can listen on ports.
    pub listen: bool,

    /// Allowed hosts for connection.
    #[serde(default)]
    pub allowed_hosts: HashSet<String>,

    /// Allowed ports for listening.
    #[serde(default)]
    pub allowed_ports: HashSet<u16>,
}

/// Filesystem-related capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FilesystemCapabilities {
    /// Can read files.
    pub read: bool,

    /// Can write files.
    pub write: bool,

    /// Paths allowed for read access.
    #[serde(default)]
    pub read_paths: HashSet<PathBuf>,

    /// Paths allowed for write access.
    #[serde(default)]
    pub write_paths: HashSet<PathBuf>,
}

/// Process-related capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProcessCapabilities {
    /// Can spawn child processes.
    pub spawn: bool,

    /// Allowed commands to spawn.
    #[serde(default)]
    pub allowed_commands: HashSet<String>,
}

/// Environment-related capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EnvironmentCapabilities {
    /// Can read environment variables.
    pub read: bool,

    /// Allowed environment variable names.
    #[serde(default)]
    pub allowed_vars: HashSet<String>,
}

/// Event bus capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EventCapabilities {
    /// Can publish events.
    pub publish: bool,

    /// Can subscribe to events.
    pub subscribe: bool,

    /// Allowed event types.
    #[serde(default)]
    pub allowed_events: HashSet<String>,
}

/// Resource management capabilities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceCapabilities {
    /// Can manage resources.
    pub manage: bool,

    /// Resource kinds allowed to manage.
    #[serde(default)]
    pub allowed_kinds: HashSet<String>,
}

impl Capabilities {
    /// Create minimal capabilities (read-only, no network, no process).
    pub fn minimal() -> Self {
        Self::default()
    }

    /// Create standard capabilities for trusted plugins.
    pub fn standard() -> Self {
        Self {
            network: NetworkCapabilities {
                connect: true,
                listen: false,
                allowed_hosts: HashSet::new(),
                allowed_ports: HashSet::new(),
            },
            filesystem: FilesystemCapabilities {
                read: true,
                write: false,
                read_paths: HashSet::new(),
                write_paths: HashSet::new(),
            },
            process: ProcessCapabilities::default(),
            environment: EnvironmentCapabilities {
                read: true,
                allowed_vars: HashSet::new(),
            },
            events: EventCapabilities {
                publish: true,
                subscribe: true,
                allowed_events: HashSet::new(),
            },
            resources: ResourceCapabilities {
                manage: true,
                allowed_kinds: HashSet::new(),
            },
        }
    }

    /// Create full capabilities (for native/signed plugins).
    pub fn full() -> Self {
        Self {
            network: NetworkCapabilities {
                connect: true,
                listen: true,
                allowed_hosts: HashSet::new(),
                allowed_ports: HashSet::new(),
            },
            filesystem: FilesystemCapabilities {
                read: true,
                write: true,
                read_paths: HashSet::new(),
                write_paths: HashSet::new(),
            },
            process: ProcessCapabilities {
                spawn: true,
                allowed_commands: HashSet::new(),
            },
            environment: EnvironmentCapabilities {
                read: true,
                allowed_vars: HashSet::new(),
            },
            events: EventCapabilities {
                publish: true,
                subscribe: true,
                allowed_events: HashSet::new(),
            },
            resources: ResourceCapabilities {
                manage: true,
                allowed_kinds: HashSet::new(),
            },
        }
    }

    /// Check if network connect is allowed for a host.
    pub fn can_connect(&self, host: &str) -> bool {
        self.network.connect
            && (self.network.allowed_hosts.is_empty() || self.network.allowed_hosts.contains(host))
    }

    /// Check if file read is allowed for a path.
    pub fn can_read_file(&self, path: &std::path::Path) -> bool {
        self.filesystem.read
            && (self.filesystem.read_paths.is_empty()
                || self
                    .filesystem
                    .read_paths
                    .iter()
                    .any(|p| path.starts_with(p)))
    }

    /// Check if file write is allowed for a path.
    pub fn can_write_file(&self, path: &std::path::Path) -> bool {
        self.filesystem.write
            && (self.filesystem.write_paths.is_empty()
                || self
                    .filesystem
                    .write_paths
                    .iter()
                    .any(|p| path.starts_with(p)))
    }

    /// Check if spawning a command is allowed.
    pub fn can_spawn(&self, command: &str) -> bool {
        self.process.spawn
            && (self.process.allowed_commands.is_empty()
                || self.process.allowed_commands.contains(command))
    }

    /// Check if reading an env var is allowed.
    pub fn can_read_env(&self, var: &str) -> bool {
        self.environment.read
            && (self.environment.allowed_vars.is_empty()
                || self.environment.allowed_vars.contains(var))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn minimal_caps_deny_all() {
        let caps = Capabilities::minimal();
        assert!(!caps.can_connect("example.com"));
        assert!(!caps.can_read_file(Path::new("/etc/passwd")));
        assert!(!caps.can_spawn("bash"));
    }

    #[test]
    fn standard_caps_allow_connect() {
        let caps = Capabilities::standard();
        assert!(caps.can_connect("example.com"));
        assert!(caps.can_read_file(Path::new("/tmp/test")));
        assert!(!caps.can_spawn("bash"));
    }

    #[test]
    fn path_restriction() {
        let mut caps = Capabilities::minimal();
        caps.filesystem.read = true;
        caps.filesystem.read_paths.insert(PathBuf::from("/project"));

        assert!(caps.can_read_file(Path::new("/project/file.txt")));
        assert!(!caps.can_read_file(Path::new("/etc/passwd")));
    }

    #[test]
    fn full_caps_allow_all() {
        let caps = Capabilities::full();
        assert!(caps.can_connect("example.com"));
        assert!(caps.can_read_file(Path::new("/etc/passwd")));
        assert!(caps.can_write_file(Path::new("/tmp/test")));
        assert!(caps.can_spawn("bash"));
        assert!(caps.can_read_env("PATH"));
    }
}
