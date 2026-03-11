# RFC: Clan Plugin for dtx

**Date:** 2026-02-22
**Status:** Draft
**Author:** Principal Engineer Analysis

---

## Executive Summary

Extend dtx with:
1. **`dtx agent`** — daemon mode that exposes JSON-RPC API (similar to `dtx mcp` but persistent)
2. **`dtx-plugin-clan`** — plugin that provides Clan integration without polluting dtx core

This keeps dtx general-purpose (process-compose alternative) while enabling first-class NixOS/Clan support for users who need it.

---

## 1. Problem Statement

### Current Pain Points (from Boltstart project)

| Pain | Description |
|------|-------------|
| Cognitive overhead | Too many manual steps when adding services/machines |
| Deployment friction | Remote operations limited to `clan machines update` |
| Discoverability | Can't understand what's deployed where without reading code |
| Dev-prod gap | Local dev (dtx/process-compose) ≠ production (Clan) |

### Why NOT core integration?

- **dtx is like process-compose** — general purpose, not Clan-specific
- **Not all users use Clan** — would bloat core for no benefit
- **Plugin system exists** — designed exactly for this use case

---

## 2. Architecture

### High-Level Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│ LOCAL (macOS dev machine)                                               │
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ dtx CLI                                                         │   │
│  │                                                                 │   │
│  │  [Core Commands]        [Plugin: dtx-plugin-clan]               │   │
│  │  ├─ dtx start/stop      ├─ dtx machines list                   │   │
│  │  ├─ dtx status          ├─ dtx machines status <host>          │   │
│  │  ├─ dtx logs            ├─ dtx machines update <host>          │   │
│  │  ├─ dtx web             ├─ dtx machines exec <host> <cmd>      │   │
│  │  └─ dtx agent (NEW)     └─ dtx machines logs <host> [service]  │   │
│  │                                                                 │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                    │                                    │
│                                    │ SSH + JSON-RPC tunnel              │
│                                    ▼                                    │
└─────────────────────────────────────────────────────────────────────────┘
                                     │
                                     │
┌────────────────────────────────────▼────────────────────────────────────┐
│ REMOTE (NixOS machine deployed via Clan)                                │
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ dtx agent (systemd service)                                     │   │
│  │                                                                 │   │
│  │  ├─ Listens on socket/port                                      │   │
│  │  ├─ Manages local processes (same as dtx start/stop)            │   │
│  │  ├─ Exposes JSON-RPC API (dtx-protocol)                         │   │
│  │  ├─ Publishes events via EventBus                               │   │
│  │  └─ MCP-compatible for AI agent integration                     │   │
│  │                                                                 │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ Application Services (managed by dtx agent)                     │   │
│  │  ├─ boltstart-app       ├─ vault                                │   │
│  │  ├─ postgres            └─ imgproxy                             │   │
│  └─────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

| Component | Responsibility |
|-----------|----------------|
| `dtx agent` | Run dtx as daemon, expose JSON-RPC API, manage processes |
| `dtx-plugin-clan` | Introspect Clan inventory, deploy agents, connect via SSH |
| `dtx-protocol` | JSON-RPC types, MCP tools, transport layer (existing) |
| Clan clanService | NixOS module that runs `dtx agent` as systemd service |

---

## 3. Component 1: `dtx agent` (Core Addition)

### Purpose

Run dtx as a persistent daemon that exposes the protocol API. Similar to `dtx mcp` but:
- Runs persistently (not just for single MCP session)
- Listens on socket/port (not just stdio)
- Can be run as systemd service

### CLI Interface

```bash
# Start agent daemon (foreground)
dtx agent

# Start with specific socket
dtx agent --socket /run/dtx.sock

# Start with TCP port
dtx agent --port 9000

# Start with project directory
dtx agent --project /var/lib/app

# Start in background (for systemd)
dtx agent --daemon
```

### Implementation

```rust
// crates/dtx/src/cmd/agent.rs

pub struct AgentArgs {
    /// Unix socket path
    #[arg(long)]
    socket: Option<PathBuf>,

    /// TCP port
    #[arg(long)]
    port: Option<u16>,

    /// Project directory
    #[arg(long, short, env = "DTX_PROJECT")]
    project: Option<PathBuf>,

    /// Run in daemon mode (background)
    #[arg(long)]
    daemon: bool,
}

pub async fn run(args: AgentArgs) -> Result<()> {
    let ctx = Context::new(args.project).await?;

    // Create protocol handler with full dtx capabilities
    let handler = DtxProtocolHandler::new(ctx);

    // Choose transport based on args
    let server = match (args.socket, args.port) {
        (Some(socket), _) => UnixSocketServer::bind(socket)?,
        (_, Some(port)) => TcpServer::bind(("0.0.0.0", port))?,
        _ => UnixSocketServer::bind("/run/dtx.sock")?,
    };

    // Serve JSON-RPC
    server.serve(handler).await
}
```

### Protocol Methods (existing, exposed by agent)

Already defined in `dtx-protocol/src/methods.rs`:
- `resource/start`, `resource/stop`, `resource/status`
- `resource/logs`, `resource/health`
- `events/subscribe`
- MCP methods: `tools/list`, `tools/call`, `resources/list`

---

## 4. Component 2: `dtx-plugin-clan`

### Purpose

A dtx plugin that adds Clan-specific functionality:
1. **Introspection** — Read Clan inventory from flake.nix
2. **Agent deployment** — Generate NixOS module for dtx agent
3. **Remote operations** — Connect to agents via SSH tunnel
4. **CLI commands** — `dtx machines *`

### Directory Structure

```
crates/dtx-plugin-clan/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Plugin registration
│   ├── introspection.rs    # Native Nix eval for Clan
│   ├── manifest.rs         # ClanManifest cache
│   ├── ssh.rs              # SSH tunnel to remote agents
│   ├── commands/
│   │   ├── mod.rs
│   │   ├── list.rs         # dtx machines list
│   │   ├── status.rs       # dtx machines status
│   │   ├── update.rs       # dtx machines update (wraps clan)
│   │   ├── exec.rs         # dtx machines exec
│   │   └── logs.rs         # dtx machines logs
│   └── nix/
│       ├── module.rs       # Generate NixOS module
│       └── service.rs      # clanService definition
└── nix/
    └── clanServices/
        └── dtx-agent/
            └── default.nix # Clan module for dtx agent
```

### Plugin Registration

```rust
// crates/dtx-plugin-clan/src/lib.rs

use dtx_plugin::{dtx_plugin, Plugin, PluginInfo};

pub struct ClanPlugin {
    manifest: Arc<RwLock<Option<ClanManifest>>>,
    nix_eval: Arc<Mutex<NativeNixEvaluator>>,
}

impl Plugin for ClanPlugin {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "clan",
            version: env!("CARGO_PKG_VERSION"),
            description: "Clan/NixOS integration for dtx",
        }
    }

    fn commands(&self) -> Vec<PluginCommand> {
        vec![
            PluginCommand {
                name: "machines",
                description: "Manage NixOS machines via Clan",
                subcommands: vec![
                    "list", "status", "update", "exec", "logs", "ssh"
                ],
            },
        ]
    }
}

dtx_plugin!(ClanPlugin);
```

### Introspection (using existing native Nix bindings)

```rust
// crates/dtx-plugin-clan/src/introspection.rs

use dtx_core::nix::native::NativeNixEvaluator;

impl ClanIntrospector {
    /// Extract Clan manifest from flake.nix
    pub fn introspect(&mut self, flake_path: &Path) -> Result<ClanManifest> {
        let outputs = self.nix_eval.get_flake_outputs(flake_path)?;

        // clanInternals.inventoryClass.modulesPerSource.self
        let clan_internals = self.select(&outputs, "clanInternals")?;
        let inventory_class = self.select(&clan_internals, "inventoryClass")?;

        let modules = self.extract_modules(&inventory_class)?;
        let machines = self.extract_machines(&inventory_class)?;
        let instances = self.extract_instances(&inventory_class)?;

        Ok(ClanManifest {
            modules,
            machines,
            instances,
            generated_at: Utc::now(),
        })
    }
}
```

### Remote Transport Architecture

**Separation of concerns:**

```
┌─────────────────────────────────────────────────────────────────┐
│ dtx-protocol (REUSABLE)                                         │
│                                                                 │
│   SshTransport                                                  │
│   ├─ Generic SSH transport for JSON-RPC                         │
│   ├─ Input: SshConfig { host, user, port, key, proxy_command } │
│   ├─ Output: impl Transport                                     │
│   └─ Reusable: cloud VMs, bare metal, any SSH-accessible host  │
└─────────────────────────────────────────────────────────────────┘
                              ▲
                              │ uses
┌─────────────────────────────┴───────────────────────────────────┐
│ dtx-plugin-clan (CLAN-SPECIFIC)                                 │
│                                                                 │
│   ClanNetworkResolver                                           │
│   ├─ Reads: targetHost from inventory                           │
│   ├─ Reads: zerotier-ip, tor/hostname from vars                 │
│   ├─ Implements: Clan's network priority logic                  │
│   └─ Output: SshConfig for a machine                            │
└─────────────────────────────────────────────────────────────────┘
```

### Component 1: SshTransport (dtx-protocol, reusable)

```rust
// crates/dtx-protocol/src/transport/ssh.rs

/// SSH connection configuration
#[derive(Clone, Debug)]
pub struct SshConfig {
    /// Target host (IP or hostname)
    pub host: String,

    /// SSH user
    pub user: String,

    /// SSH port (default 22)
    pub port: u16,

    /// Path to private key (optional, uses ssh-agent if None)
    pub identity_file: Option<PathBuf>,

    /// ProxyCommand for tunneling (e.g., torsocks, jump hosts)
    pub proxy_command: Option<String>,

    /// Remote command to execute (e.g., "socat - UNIX-CONNECT:/run/dtx.sock")
    pub remote_command: String,

    /// Host key verification mode
    pub host_key_check: HostKeyCheck,
}

#[derive(Clone, Debug, Default)]
pub enum HostKeyCheck {
    Strict,
    #[default]
    AcceptNew,  // TOFU (Trust On First Use)
    None,
}

/// Generic SSH transport - reusable by any plugin
pub struct SshTransport {
    config: SshConfig,
}

impl SshTransport {
    pub fn new(config: SshConfig) -> Self {
        Self { config }
    }

    /// Build SSH command arguments
    fn build_ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(),
            format!("User={}", self.config.user),
            "-p".to_string(),
            self.config.port.to_string(),
        ];

        // Host key verification
        match self.config.host_key_check {
            HostKeyCheck::Strict => {
                args.extend(["-o".to_string(), "StrictHostKeyChecking=yes".to_string()]);
            }
            HostKeyCheck::AcceptNew => {
                args.extend(["-o".to_string(), "StrictHostKeyChecking=accept-new".to_string()]);
            }
            HostKeyCheck::None => {
                args.extend([
                    "-o".to_string(), "StrictHostKeyChecking=no".to_string(),
                    "-o".to_string(), "UserKnownHostsFile=/dev/null".to_string(),
                ]);
            }
        }

        // Identity file
        if let Some(ref key) = self.config.identity_file {
            args.extend(["-i".to_string(), key.to_string_lossy().to_string()]);
        }

        // Proxy command (for tor, jump hosts, etc.)
        if let Some(ref proxy) = self.config.proxy_command {
            args.extend(["-o".to_string(), format!("ProxyCommand={}", proxy)]);
        }

        // Host and remote command
        args.push(self.config.host.clone());
        args.push(self.config.remote_command.clone());

        args
    }
}

#[async_trait]
impl Transport for SshTransport {
    async fn send(&self, request: Request) -> Result<Response, TransportError> {
        let args = self.build_ssh_args();

        let mut child = Command::new("ssh")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| TransportError::Io(e))?;

        // Send JSON-RPC request
        let stdin = child.stdin.as_mut().unwrap();
        serde_json::to_writer(stdin, &request)?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        drop(child.stdin.take()); // Close stdin to signal EOF

        // Read response
        let output = child.wait_with_output().await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(TransportError::Other(format!("SSH failed: {}", stderr)));
        }

        let response: Response = serde_json::from_slice(&output.stdout)?;
        Ok(response)
    }
}
```

### Component 2: ClanNetworkResolver (dtx-plugin-clan)

```rust
// crates/dtx-plugin-clan/src/network.rs

use dtx_protocol::transport::{SshConfig, HostKeyCheck};

/// Network backends supported by Clan
#[derive(Clone, Debug, PartialEq)]
pub enum NetworkBackend {
    /// Direct internet (targetHost)
    Internet { host: String, user: String },

    /// Zerotier overlay network
    Zerotier { ip: String, user: String },

    /// Tor hidden service
    Tor { onion: String, user: String },
}

/// Clan-specific network resolution
pub struct ClanNetworkResolver {
    manifest: ClanManifest,
}

impl ClanNetworkResolver {
    /// Resolve network backends for a machine (priority ordered)
    pub fn resolve_backends(&self, machine: &str) -> Result<Vec<NetworkBackend>> {
        let machine_info = self.manifest.machines.get(machine)
            .ok_or_else(|| anyhow!("Unknown machine: {}", machine))?;

        let mut backends = Vec::new();

        // 1. Internet (from targetHost in config)
        if let Some(target) = &machine_info.target_host {
            let (user, host) = parse_target_host(target)?;
            backends.push(NetworkBackend::Internet { host, user });
        }

        // 2. Zerotier (from vars)
        if let Some(zt_ip) = &machine_info.vars.get("zerotier/zerotier-ip") {
            backends.push(NetworkBackend::Zerotier {
                ip: zt_ip.clone(),
                user: "root".to_string(),
            });
        }

        // 3. Tor (from vars)
        if let Some(onion) = &machine_info.vars.get("tor_tor/hostname") {
            backends.push(NetworkBackend::Tor {
                onion: onion.clone(),
                user: "root".to_string(),
            });
        }

        Ok(backends)
    }

    /// Convert a NetworkBackend to SshConfig
    pub fn backend_to_ssh_config(
        &self,
        backend: &NetworkBackend,
        remote_command: &str,
    ) -> SshConfig {
        match backend {
            NetworkBackend::Internet { host, user } => SshConfig {
                host: host.clone(),
                user: user.clone(),
                port: 22,
                identity_file: None,
                proxy_command: None,
                remote_command: remote_command.to_string(),
                host_key_check: HostKeyCheck::AcceptNew,
            },

            NetworkBackend::Zerotier { ip, user } => SshConfig {
                host: ip.clone(),
                user: user.clone(),
                port: 22,
                identity_file: None,
                proxy_command: None,
                remote_command: remote_command.to_string(),
                host_key_check: HostKeyCheck::AcceptNew,
            },

            NetworkBackend::Tor { onion, user } => SshConfig {
                host: onion.clone(),
                user: user.clone(),
                port: 22,
                identity_file: None,
                // Use torsocks or socat through tor
                proxy_command: Some(format!(
                    "torsocks nc %h %p"
                )),
                remote_command: remote_command.to_string(),
                host_key_check: HostKeyCheck::AcceptNew,
            },
        }
    }

    /// Try backends in priority order until one succeeds
    pub async fn connect(
        &self,
        machine: &str,
        remote_command: &str,
    ) -> Result<SshTransport> {
        let backends = self.resolve_backends(machine)?;

        for backend in &backends {
            let config = self.backend_to_ssh_config(backend, remote_command);

            // Test connection
            if self.test_connection(&config).await.is_ok() {
                return Ok(SshTransport::new(config));
            }
        }

        Err(anyhow!("No reachable backend for machine: {}", machine))
    }

    /// Quick connectivity test
    async fn test_connection(&self, config: &SshConfig) -> Result<()> {
        let test_config = SshConfig {
            remote_command: "echo ok".to_string(),
            ..config.clone()
        };

        let transport = SshTransport::new(test_config);
        // Quick timeout test
        tokio::time::timeout(
            Duration::from_secs(5),
            transport.send(Request::notification("ping", None))
        ).await??;

        Ok(())
    }
}
```

### Usage in dtx-plugin-clan Commands

```rust
// crates/dtx-plugin-clan/src/commands/status.rs

pub async fn run(machine: &str) -> Result<()> {
    let manifest = load_cached_manifest()?;
    let resolver = ClanNetworkResolver::new(manifest);

    // Get SshTransport with automatic backend selection
    let transport = resolver.connect(
        machine,
        "socat - UNIX-CONNECT:/run/dtx/agent.sock"
    ).await?;

    // Use generic dtx-protocol methods
    let request = Request::method("resource/list", None);
    let response = transport.send(request).await?;

    // Display results...
    Ok(())
}
```

### Why This Design is Better

| Aspect | Old (clan ssh) | New (SshTransport + Resolver) |
|--------|----------------|-------------------------------|
| **Reusability** | Clan-only | Any SSH host |
| **Performance** | Subprocess per call | Single connection |
| **Testability** | Mock clan CLI | Mock Transport trait |
| **Extensibility** | Clan-locked | Add new backends easily |
| **Debugging** | Opaque | Clear connection params |

### Other Use Cases for SshTransport

```rust
// Cloud VM without Clan
let config = SshConfig {
    host: "ec2-xx-xx-xx-xx.compute.amazonaws.com".to_string(),
    user: "ubuntu".to_string(),
    port: 22,
    identity_file: Some("~/.ssh/aws-key.pem".into()),
    proxy_command: None,
    remote_command: "socat - UNIX-CONNECT:/run/dtx.sock".to_string(),
    host_key_check: HostKeyCheck::AcceptNew,
};

let transport = SshTransport::new(config);

// Raspberry Pi on local network
let config = SshConfig {
    host: "192.168.1.50".to_string(),
    user: "pi".to_string(),
    ..Default::default()
};
```

### CLI Commands

```bash
# List machines (from Clan manifest)
$ dtx machines list
┌───────────┬───────────────────────┬──────────────────────────────┐
│ Machine   │ Tags                  │ Services                     │
├───────────┼───────────────────────┼──────────────────────────────┤
│ ovh       │ nixos, observable     │ app, vault, garage, webserver│
│ dalang    │ nixos, observable     │ hydra, openobserve           │
│ boltstart │ nixos                 │ -                            │
└───────────┴───────────────────────┴──────────────────────────────┘

# Show status (queries remote dtx agent)
$ dtx machines status ovh
┌────────────────┬─────────┬────────┬─────────────────────────┐
│ Service        │ State   │ PID    │ Health                  │
├────────────────┼─────────┼────────┼─────────────────────────┤
│ boltstart-app  │ running │ 12345  │ healthy (http:3002)     │
│ vault          │ running │ 12346  │ healthy (http:3001)     │
│ postgres       │ running │ 12347  │ healthy (pg_isready)    │
│ imgproxy       │ running │ 12348  │ healthy                 │
└────────────────┴─────────┴────────┴─────────────────────────┘

# Get logs from remote agent
$ dtx machines logs ovh --service boltstart-app --follow

# Execute command via remote agent
$ dtx machines exec ovh "systemctl restart boltstart-app"

# Deploy (wraps clan machines update)
$ dtx machines update ovh
```

---

## 5. Component 3: Clan Module for dtx Agent

### NixOS Module

```nix
# crates/dtx-plugin-clan/nix/clanServices/dtx-agent/default.nix
{ lib, pkgs, config, ... }:

with lib;

{
  _class = "clan.service";

  manifest = {
    name = "dtx-agent";
    description = "dtx orchestration agent for remote process management";
    categories = [ "Infrastructure" ];
  };

  roles.default = {
    interface.options = {
      port = mkOption {
        type = types.nullOr types.port;
        default = null;
        description = "TCP port (if null, uses Unix socket)";
      };

      socketPath = mkOption {
        type = types.str;
        default = "/run/dtx/agent.sock";
        description = "Unix socket path";
      };

      projectDir = mkOption {
        type = types.str;
        default = "/var/lib/dtx";
        description = "Directory containing .dtx/config.yaml";
      };

      # Services managed by dtx agent (defined in .dtx/config.yaml)
      services = mkOption {
        type = types.listOf types.str;
        default = [];
        description = "Services to manage (from dtx config)";
      };
    };

    perInstance = { settings, ... }: {
      nixosModule = { config, pkgs, inputs, ... }:
        let
          dtxPackage = inputs.self.packages.${pkgs.system}.dtx;
        in
        {
          # User and group
          users.users.dtx = {
            isSystemUser = true;
            group = "dtx";
            home = settings.projectDir;
          };
          users.groups.dtx = {};

          # Ensure project directory exists
          systemd.tmpfiles.rules = [
            "d ${settings.projectDir} 0750 dtx dtx -"
          ];

          # dtx agent service
          systemd.services.dtx-agent = {
            description = "dtx Orchestration Agent";
            after = [ "network-online.target" ];
            wants = [ "network-online.target" ];
            wantedBy = [ "multi-user.target" ];

            serviceConfig = {
              Type = "simple";
              User = "dtx";
              Group = "dtx";
              RuntimeDirectory = "dtx";
              StateDirectory = "dtx";
              WorkingDirectory = settings.projectDir;
              Restart = "on-failure";
              RestartSec = 5;

              ExecStart = lib.concatStringsSep " " ([
                "${dtxPackage}/bin/dtx"
                "agent"
                "--project" settings.projectDir
              ] ++ (if settings.port != null
                then [ "--port" (toString settings.port) ]
                else [ "--socket" settings.socketPath ]
              ));

              # Security hardening
              NoNewPrivileges = true;
              ProtectSystem = "strict";
              ProtectHome = true;
              PrivateTmp = true;
              ReadWritePaths = [ settings.projectDir "/run/dtx" ];
            };
          };

          # Firewall (if using TCP port)
          networking.firewall = lib.mkIf (settings.port != null) {
            allowedTCPPorts = [ settings.port ];
          };
        };
    };
  };
}
```

### Integration in Clan Inventory

```nix
# nix/infra.nix (in Boltstart project)
{
  clan.inventory.instances = {
    # ... existing instances ...

    # dtx agent for remote process management
    dtx-agent = {
      module.name = "dtx-agent";
      module.input = "dtx";  # or "self" if bundled

      roles.default.machines.ovh = {
        settings.projectDir = "/var/lib/boltstart";
        settings.socketPath = "/run/dtx/agent.sock";
        settings.services = [ "boltstart-app" "vault" "imgproxy" ];
      };
    };
  };
}
```

---

## 6. Implementation Phases

### Phase 1: `dtx agent` Command (dtx-core)
**Goal:** Run dtx as persistent daemon exposing JSON-RPC API

```
crates/dtx/src/cmd/agent.rs
```

- [ ] Add `agent` subcommand to dtx CLI
- [ ] Implement Unix socket server (`tokio::net::UnixListener`)
- [ ] Implement TCP server option (`tokio::net::TcpListener`)
- [ ] Wire up existing `DtxProtocolHandler`
- [ ] Add systemd socket activation support (optional)
- [ ] Test: `dtx agent --socket /tmp/test.sock` + `socat` client

**Deliverable:** `dtx agent` runs and accepts JSON-RPC over socket/TCP

---

### Phase 2: SshTransport (dtx-protocol, REUSABLE)
**Goal:** Generic SSH transport usable by any plugin

```
crates/dtx-protocol/src/transport/ssh.rs
```

- [ ] Define `SshConfig` struct (host, user, port, identity_file, proxy_command)
- [ ] Define `HostKeyCheck` enum (Strict, AcceptNew, None)
- [ ] Implement `SshTransport` with `build_ssh_args()`
- [ ] Implement `Transport` trait for `SshTransport`
- [ ] Add connection pooling (optional, for performance)
- [ ] Test: SSH to local VM, send JSON-RPC, receive response

**Deliverable:** `SshTransport::new(config).send(request)` works for any SSH host

---

### Phase 3: Plugin Skeleton (dtx-plugin-clan)
**Goal:** Create plugin structure with feature flag

```
crates/dtx-plugin-clan/
├── Cargo.toml          # feature = "clan" in workspace
├── src/
│   ├── lib.rs          # Plugin registration
│   └── commands/mod.rs # Command stubs
```

- [ ] Create `dtx-plugin-clan` crate
- [ ] Add `clan` feature flag to dtx workspace (default = false)
- [ ] Implement `Plugin` trait with `machines` command group
- [ ] Register plugin in dtx CLI when feature enabled
- [ ] Test: `cargo build --features clan` compiles

**Deliverable:** `dtx machines --help` shows subcommands (stubs)

---

### Phase 4: Clan Introspection (dtx-plugin-clan)
**Goal:** Extract ClanManifest from flake.nix using native Nix

```
crates/dtx-plugin-clan/src/
├── introspection.rs    # NativeNixEvaluator usage
├── manifest.rs         # ClanManifest types + cache
```

- [ ] Define `ClanManifest` struct (modules, machines, instances, vars)
- [ ] Define `MachineInfo` struct (target_host, tags, services, vars)
- [ ] Implement `ClanIntrospector` using `dtx_core::nix::NativeNixEvaluator`
- [ ] Extract from `clanInternals.inventoryClass.*`
- [ ] Extract vars from `clan vars list` or native eval
- [ ] Cache manifest to `.dtx/clan-manifest.json`
- [ ] Detect staleness via `flake.lock` hash
- [ ] Test: `dtx machines refresh` generates valid manifest

**Deliverable:** ClanManifest cached with machines, services, vars

---

### Phase 5: ClanNetworkResolver (dtx-plugin-clan)
**Goal:** Implement Clan's network resolution logic

```
crates/dtx-plugin-clan/src/network.rs
```

- [ ] Define `NetworkBackend` enum (Internet, Zerotier, Tor)
- [ ] Implement `resolve_backends()` from manifest vars
- [ ] Implement `backend_to_ssh_config()` for each backend
- [ ] Implement `connect()` with priority-based fallback
- [ ] Add Tor support via `proxy_command: "torsocks nc %h %p"`
- [ ] Add connection test with timeout
- [ ] Test: Resolve backends for machine, connect to first available

**Deliverable:** `ClanNetworkResolver::connect("ovh")` returns working `SshTransport`

---

### Phase 6: Remote Operations (dtx-plugin-clan)
**Goal:** Implement `dtx machines` commands

```
crates/dtx-plugin-clan/src/commands/
├── list.rs      # From cached manifest
├── status.rs    # Via SshTransport → resource/list
├── logs.rs      # Via SshTransport → resource/logs
├── exec.rs      # Via SshTransport → custom exec
├── update.rs    # Wraps `clan machines update`
├── vars.rs      # Wraps `clan vars list`
```

- [ ] `dtx machines list` — Read from ClanManifest (offline)
- [ ] `dtx machines status <machine>` — Query remote agent
- [ ] `dtx machines logs <machine> [--service] [--follow]` — Stream logs
- [ ] `dtx machines exec <machine> <cmd>` — Execute via agent
- [ ] `dtx machines update <machine>` — Delegate to `clan machines update`
- [ ] `dtx machines vars <machine>` — Delegate to `clan vars list`
- [ ] `dtx machines ssh <machine>` — Passthrough to `clan ssh`
- [ ] Test: Full workflow on Boltstart project

**Deliverable:** All `dtx machines` commands functional

---

### Phase 7: NixOS Module (clanService)
**Goal:** Deploy dtx agent via Clan

```
crates/dtx-plugin-clan/nix/clanServices/dtx-agent/default.nix
```

- [ ] Create clanService module for dtx-agent
- [ ] Define interface options (socketPath, projectDir, services)
- [ ] Configure systemd service with security hardening
- [ ] Add to Boltstart project's Clan inventory
- [ ] Deploy to ovh machine via `clan machines update`
- [ ] Test: `dtx machines status ovh` works end-to-end

**Deliverable:** dtx agent running on production NixOS machine

---

### Phase 8: Polish & Documentation
**Goal:** Production-ready release

- [ ] Error messages with actionable guidance
- [ ] `--verbose` mode for debugging network issues
- [ ] Shell completions for `dtx machines`
- [ ] README for dtx-plugin-clan
- [ ] Integration tests with NixOS VM
- [ ] Nixpkgs packaging with `--features clan` option

**Deliverable:** Ready for users

---

## 7. Anti-Patterns (What NOT to Do)

1. **Don't add Clan-specific code to dtx-core** — use plugin system
2. **Don't re-implement Clan deployment** — wrap `clan machines update`
3. **Don't require network for introspection** — cache manifest locally
4. **Don't break existing dtx workflows** — plugin is additive
5. **Don't couple dtx agent to Clan** — agent is generic, plugin adds Clan

---

## 8. Secrets and Vars Integration

### Clan's Secret Model

Clan has two distinct systems:

| System | Purpose | Examples |
|--------|---------|----------|
| **secrets** | Age keys for machines | `ovh-age.key`, `dalang-age.key` |
| **vars** | Machine-scoped variables | `zerotier/zerotier-ip`, `garage/admin_token` |

Vars are further divided:
- **Public vars** — Can be read by other machines (e.g., `zerotier-ip`)
- **Secret vars** — Encrypted, only readable by target machine (e.g., `admin_token`)

### How dtx-plugin-clan Handles Secrets

**Principle:** dtx does NOT manage secrets directly. Clan handles secret generation, encryption, and distribution.

```
┌─────────────────────────────────────────────────────────────────┐
│ Clan Vars System                                                │
│                                                                 │
│  clan vars generate      # Generate secrets for services        │
│  clan vars upload        # Upload secrets to machines           │
│  clan vars list <machine># List vars for a machine              │
│                                                                 │
│  Stored in: vars/<machine>/                                     │
│  Encrypted with: sops/secrets/<machine>-age.key                 │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│ dtx agent (on machine)                                          │
│                                                                 │
│  Secrets are decrypted at runtime by NixOS:                     │
│  config.clan.core.vars.<service>.<var>.path                     │
│  → /run/secrets/<service>/<var>                                 │
│                                                                 │
│  dtx agent reads from runtime paths, NOT raw vars               │
└─────────────────────────────────────────────────────────────────┘
```

### dtx Config with Clan Vars References

When dtx-plugin-clan generates `.dtx/config.yaml` for remote machines, it can reference Clan vars:

```yaml
# .dtx/config.yaml (on NixOS machine)
resources:
  boltstart-app:
    kind: process
    command: bun run start
    env:
      # Reference Clan var paths (decrypted at runtime)
      DATABASE_URL:
        from_file: /run/secrets/vault-bootstrap/database-url
      ENCRYPTION_KEY:
        from_file: /run/secrets/vault-bootstrap/bs-encryption-key
    port: 3002

  vault:
    kind: process
    command: bun run start
    env:
      PG_PASSWORD:
        from_file: /run/secrets/vault-bootstrap/bs-vault-pg-password
    port: 3001
```

### Commands for Secrets Introspection

```bash
# List vars for a machine (via clan)
$ dtx machines vars ovh
┌─────────────────────────────────┬──────────┬───────────────────┐
│ Var                             │ Type     │ Value             │
├─────────────────────────────────┼──────────┼───────────────────┤
│ zerotier/zerotier-ip            │ public   │ fd2d:bf77:...     │
│ zerotier/zerotier-network-id    │ public   │ 2dbf773121...     │
│ openssh/ssh.id_ed25519.pub      │ public   │ ssh-ed25519 AA... │
│ garage/admin_token              │ secret   │ ********          │
│ vault-bootstrap/bs-pg-password  │ secret   │ ********          │
└─────────────────────────────────┴──────────┴───────────────────┘

# Generate missing vars
$ dtx machines vars generate ovh --service vault-bootstrap
```

---

## 9. flake.nix as Single Source of Truth

### Why flake.nix?

All configuration flows from `flake.nix`:

```
flake.nix
├── clan.inventory           # Machines, tags, instances
├── clanServices/*           # Service definitions
├── nixosConfigurations.*    # Per-machine NixOS config
└── clanInternals            # Introspection data
```

### dtx-plugin-clan Reads, Never Writes

**Principle:** dtx-plugin-clan is **read-only** for Nix config. It introspects, never modifies.

```
┌───────────────────────────────────────────────────────────────────────┐
│ flake.nix (source of truth)                                          │
│                                                                       │
│   Human edits:  nix/infra.nix, clanServices/*, machines/*/config.nix │
│   Clan manages: vars/, sops/secrets/                                  │
└───────────────────────────────────────────────────────────────────────┘
                              │
                              │ nix eval (native bindings)
                              ▼
┌───────────────────────────────────────────────────────────────────────┐
│ dtx-plugin-clan introspection                                         │
│                                                                       │
│   ClanManifest {                                                      │
│     modules: ["app", "vault", "garage", ...],                         │
│     machines: { "ovh": { tags: [...], services: [...] } },            │
│     instances: { "app": { machines: ["ovh"] } },                      │
│   }                                                                   │
│                                                                       │
│   Cached to: .dtx/clan-manifest.json                                  │
└───────────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌───────────────────────────────────────────────────────────────────────┐
│ dtx CLI                                                               │
│                                                                       │
│   dtx machines list      # Read from cached manifest                  │
│   dtx machines status    # Query remote via clan ssh                  │
│   dtx machines update    # Delegate to clan machines update           │
└───────────────────────────────────────────────────────────────────────┘
```

### Refreshing the Manifest

```bash
# Refresh manifest from flake.nix
$ dtx machines refresh
Evaluating flake.nix...
Found 3 machines, 6 services, 10 instances
Manifest cached to .dtx/clan-manifest.json

# Auto-refresh on change
$ dtx machines list --refresh
```

---

## 10. Open Questions

| Question | Options | Leaning |
|----------|---------|---------|
| **Plugin distribution** | Bundled (feature flag) vs separate crate | Bundled, `--features clan` |
| **Socket vs TCP** | Unix socket vs TCP port | Socket default, TCP optional |
| **Authentication** | None vs token vs mTLS | None (Clan handles SSH auth) |
| **Config sync** | Manual vs auto-sync .dtx/config.yaml | Manual with `dtx machines sync` |
| **Manifest staleness** | Time-based vs hash-based refresh | Hash of flake.lock |
| **Offline mode** | Cached manifest vs require eval | Cached manifest for introspection |

### Detailed Considerations

**1. Config sync direction**
- Option A: dtx config → Clan inventory (dtx is source)
- Option B: Clan inventory → dtx config (Clan is source)
- **Recommendation**: Clan is source, dtx reads. Don't duplicate config.

**2. What dtx agent manages vs systemd**
- dtx agent: User-space processes (app, vault)
- systemd: System services (postgres, caddy, garage)
- **Boundary**: dtx manages what's in `.dtx/config.yaml`

**3. Health check ownership**
- Clan services define NixOS systemd health checks
- dtx agent can add additional application-level health checks
- Both are queryable via `dtx machines status`

---

## 9. Success Criteria

- [ ] `dtx agent` runs as systemd service on NixOS
- [ ] `dtx machines list` shows Clan machines in <1s (cached)
- [ ] `dtx machines status ovh` returns service status via SSH
- [ ] `dtx machines logs ovh --follow` streams logs in real-time
- [ ] No changes required to dtx-core for Clan support
- [ ] Plugin can be installed/removed independently
