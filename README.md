# dtx - Dev Tools eXperience

> **Unified resource orchestration for development environments**
>
> Manage multi-service projects with dependency ordering, health checks, and Nix integration. No containers required. No YAML editing.

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)

---

## What is dtx?

**dtx** is a native resource orchestrator for multi-service development environments. It provides a CLI, TUI, and Web UI with automatic [Nix](https://nixos.org/) integration for reproducibility.

### Key Features

- **Dependency-ordered startup** — Services start in the right order based on dependencies
- **Health check probes** — Wait for services to be healthy before starting dependents (exec, HTTP)
- **Restart policies** — Automatic restarts with configurable backoff (exponential, linear, fixed)
- **Nix integration** — Auto-generate `flake.nix`, discover packages, extract environments
- **Multiple interfaces** — CLI, TUI (ratatui), and Web UI (HTMX + SSE live updates)
- **Smart inference** — Auto-detects packages, ports, and commands from service names
- **Import/Export** — Docker Compose, process-compose, Procfile, Kubernetes formats
- **MCP server** — AI agent integration via JSON-RPC over stdio

### Who is this for?

**Primary audience**: Developers who want:
- Native performance without container overhead (especially on macOS)
- Visual interface for managing services (TUI and Web UI)
- Automatic `flake.nix` generation from service definitions
- Dependency graph visualization

**Secondary audience**: Docker Compose users looking for:
- Lighter-weight alternative for local development
- Reproducible environments via Nix
- Simpler setup for multi-service projects

---

## Quick Start

### Prerequisites

- [Nix](https://nixos.org/download.html) with flakes enabled (for Nix integration)
- [direnv](https://direnv.net/) (optional, recommended)

### Installation

#### Via Nix (Recommended)

```bash
# Run directly
nix run github:r17x/dtx

# Or install to profile
nix profile install github:r17x/dtx
```

#### From Source

```bash
git clone https://github.com/r17x/dtx.git
cd dtx
cargo build --release
sudo cp target/release/dtx /usr/local/bin/
```

### Complete Workflow Example

```bash
# 1. Create a new project (--detect scans your codebase for services)
dtx init myproject --detect
cd myproject

# 2. Add services (packages, ports, commands are auto-inferred)
dtx add api --command "node server.js" --port 3000
dtx add postgres --port 5432
dtx add redis

# 3. Generate Nix environment
dtx nix init

# 4. Enable direnv (optional)
direnv allow

# 5. Start all services (opens TUI by default)
dtx start
# or foreground mode:
dtx start -f

# 6. Check status and logs
dtx status
dtx logs api -f

# 7. Stop when done
dtx stop
```

### Import Existing Projects

```bash
# Import from Docker Compose
dtx import docker-compose.yml

# Import from process-compose
dtx import process-compose.yaml

# Import from Procfile
dtx import Procfile

# Dry-run to preview
dtx import docker-compose.yml --dry-run
```

### Using the Web UI

```bash
# Start the web interface
dtx web --open

# Opens http://localhost:3000 with:
# - Service configuration and editing
# - Dependency graph visualization
# - Real-time status updates via SSE
# - Live log streaming
# - Nix package search
```

---

## Feature Status

### Works Today

| Feature | Status |
|---------|--------|
| `dtx init` — Create projects (with `--detect` codebase inference) | Stable |
| `dtx add/edit/remove` — Full service lifecycle management | Stable |
| `dtx start/stop/status` — Dependency-ordered orchestration | Stable |
| `dtx logs` — Real-time log streaming via SSE | Stable |
| `dtx nix init/envrc/shell/packages` — Nix environment management | Stable |
| `dtx search` — Nix package search with relevance ranking | Stable |
| `dtx config` — Hierarchical config (system/global/project) | Stable |
| `dtx export` — Export to process-compose, Docker Compose, Kubernetes | Stable |
| `dtx import` — Import from Docker Compose, process-compose, Procfile | Stable |
| `dtx web` — Web UI with dependency graphs, live logs, SSE status | Stable |
| `dtx mcp` — MCP server for AI agent integration | Stable |
| Health check probes (exec, HTTP) with dependency conditions | Stable |
| Restart policies with configurable backoff | Stable |
| Dependency cycle detection and graph validation | Stable |
| Smart package/port/command inference | Stable |

### In Progress

| Feature | Notes |
|---------|-------|
| Service templates (postgres, redis, etc.) | Basic inference exists, dedicated template registry planned |
| Health check visualization in Web UI | Basic status shown, detailed probe UI planned |
| Container backend (Docker/Podman) | Architecture ready, wiring in progress |
| Plugin system | Loader and sandbox implemented, ecosystem planned |

---

## When to Use dtx

**Good fit**:
- Local development with multiple services
- Teams already using Nix who want better tooling
- Projects where Docker is too heavy (especially on macOS)
- Quick prototyping with different tech stacks
- AI-assisted development (MCP integration)

**Not designed for**:
- Production deployments (use Kubernetes, Nomad, etc.)
- Container-only workflows (use Docker Compose)
- Single-service projects (overkill)

---

## CLI Reference

```bash
# Project management
dtx init [name]                         # Create project (--detect, --path, --description, -y)
dtx list                                # List all projects
dtx list --services                     # List services in current project

# Service management
dtx add <name> [options]                # Add service (auto-infers package, port, command)
dtx edit <name> [options]               # Edit service (--add-env, --remove-dep, --enable, etc.)
dtx remove <name> [-y]                  # Remove service
dtx start [service] [-f]                # Start services (TUI default, -f foreground)
dtx stop [service]                      # Stop services
dtx status [service]                    # Show service status
dtx logs [service] [-f] [-a]            # View/stream logs

# Configuration
dtx config [key] [value]                # Get/set config (--global, --project)
dtx export [-f format] [-o file]        # Export: process-compose, docker-compose, kubernetes, dtx
dtx import <file> [-f format] [--dry-run]  # Import: docker-compose, process-compose, Procfile

# Nix integration
dtx nix init                            # Generate flake.nix and .envrc
dtx nix envrc                           # Regenerate .envrc only
dtx nix packages                        # List Nix packages from services
dtx nix shell [command]                 # Run command in Nix shell

# Utilities
dtx search <query> [-l limit]           # Search Nix packages
dtx web [-p port] [--open]              # Start web UI
dtx mcp [-p path]                       # Start MCP server (stdio JSON-RPC)
dtx completions <shell>                 # Generate shell completions (bash, zsh, fish)
```

### Add Command Options

```bash
dtx add myservice \
  --command "npm run dev" \
  --package nodejs \
  --port 3000 \
  --working-dir ./api \
  --env "NODE_ENV=development" \
  --env "LOG_LEVEL=debug" \
  --depends-on "database:healthy,cache:started" \
  --init "npm install" \
  --health-check "http:localhost:3000/health" \
  --restart on-failure
```

### Edit Command Options

```bash
dtx edit myservice \
  --command "npm run start" \
  --port 8080 \
  --add-env "NEW_VAR=value" \
  --remove-env "OLD_VAR" \
  --add-dep "newservice:healthy" \
  --remove-dep "oldservice" \
  --restart always \
  --health-check "exec:curl -f localhost:8080" \
  --enable
```

---

## Architecture

```
┌─────────────────────────────────────────────────────┐
│          CLI · TUI · Web UI · MCP Server            │  dtx, dtx-web
├─────────────────────────────────────────────────────┤
│          JSON-RPC Protocol + Transports             │  dtx-protocol
├─────────────────────────────────────────────────────┤
│    Middleware: Logging · Metrics · Retry · AI       │  dtx-middleware
├─────────────────────────────────────────────────────┤
│    Orchestrator: Dependency Graph · Health · Life   │  dtx-process
├─────────────────────────────────────────────────────┤
│    Core: Resource Trait · Events · Domain Types     │  dtx-core
├─────────────────────────────────────────────────────┤
│       Data: ConfigStore · Model Types                │  dtx-core
├─────────────────────────────────────────────────────┤
│   Backends: Process · Container · VM · Agent        │  dtx-process, dtx-vm, dtx-agent
├─────────────────────────────────────────────────────┤
│          Plugin Loader · Sandbox                    │  dtx-plugin
└─────────────────────────────────────────────────────┘
```

**Core abstraction**: Everything dtx manages implements the `Resource` trait (Process, Container, VM, Agent). The `ResourceEventBus` distributes lifecycle events to all subscribers (TUI, Web SSE, orchestrator).

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for technical details.

---

## Documentation

- **[Quick Start](docs/guides/quick-start.md)** — Setup and first project
- **[Configuration](docs/guides/configuration.md)** — Config files and options
- **[CLI Reference](docs/guides/cli-reference.md)** — Detailed command documentation
- **[Web UI](docs/guides/web-ui.md)** — Web interface guide
- **[MCP Integration](docs/guides/mcp-integration.md)** — AI agent setup
- **[Writing Middleware](docs/guides/writing-middleware.md)** — Extend the middleware stack
- **[Writing Plugins](docs/guides/writing-plugins.md)** — Build custom plugins
- **[Troubleshooting](docs/guides/troubleshooting.md)** — Common issues
- **[Architecture](docs/ARCHITECTURE.md)** — System design and decisions
- **[Development Phases](docs/phases/)** — Implementation roadmap

## Technology Stack

| Component | Technology |
|-----------|------------|
| Language | Rust (see flake.nix for toolchain version) |
| Async Runtime | Tokio |
| CLI | Clap 4 |
| TUI | ratatui + crossterm |
| Web Framework | Axum 0.7 |
| Templates | Askama (compile-time verified) |
| Frontend | HTMX + Tailwind CSS |
| Data | ConfigStore (YAML, file-backed) |
| Real-time | SSE + ResourceEventBus (tokio::broadcast) |
| Protocol | JSON-RPC 2.0 + MCP |
| Nix | nix-bindings-rust (native), CLI fallback |

---

## Status

dtx is under active development. Core functionality — project management, service orchestration, Nix integration, Web UI, import/export — is stable and works well for local development workflows. Advanced features (container backend, plugin ecosystem) are in progress.

Contributions and feedback welcome!

---

## Contributing

```bash
# Enter development shell
nix develop

# Build and test
cargo build
cargo test
cargo fmt
cargo clippy -- -D warnings

# Run in development
cargo run -- web --open
```

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

---

## Acknowledgments

- [Nix](https://nixos.org/) — Reproducible package management
- [HTMX](https://htmx.org) — Hypermedia-driven UI
- [ratatui](https://ratatui.rs/) — Terminal UI framework
- [process-compose](https://github.com/F1bonacc1/process-compose) — Inspiration for process orchestration

---

**Issues**: [GitHub Issues](https://github.com/r17x/dtx/issues) | **Docs**: [docs/](docs/)
