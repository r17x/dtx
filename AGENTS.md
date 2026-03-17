# CLAUDE.md — dtx Project Context

## Development Phase

Active development. APIs break freely, no deprecation warnings. No backwards-compat re-exports — clean up source and fix consumers directly.

## Project

**dtx** (Dev Tools eXperience) — Unified resource orchestration with Nix integration.

Single Rust binary providing:
- CLI with smart inference (package, port, command detection)
- TUI for real-time process management (ratatui)
- Web UI with live updates (HTMX + SSE + Tailwind)
- Resource abstraction: Process, Container, VM, Agent — single orchestration model
- Nix-first environment management (flake generation, package discovery, env extraction)
- MCP server for AI agent integration (JSON-RPC over stdio)

> IMPORTANT `.worktree/dtx-docs` is git worktree referenced to branch `docs` for DOCUMENTATION related (e.g: architecture, design, and etc). create if not exist

## Tech Stack

| Layer | Technology |
|-------|------------|
| Language | Rust (see `rust-toolchain` in flake.nix for version) |
| Async | Tokio (full features) |
| CLI | Clap 4 (derive) |
| TUI | ratatui + crossterm |
| Web | Axum 0.7 + Askama templates |
| Data | ConfigStore (YAML, file-backed) |
| Frontend | HTMX + Tailwind CSS (no SPA) |
| Real-time | SSE + ResourceEventBus (tokio::broadcast) + Unix socket IPC |
| Protocol | JSON-RPC 2.0 + MCP (stdio, HTTP, WebSocket transports) |
| Nix | nix-bindings-rust (native, feature-gated), CLI fallback |
| Plugins | Dynamic (libloading) + WASM sandbox (wasmtime, feature-gated) |

> Canonical dependency versions live in the workspace `Cargo.toml` `[workspace.dependencies]` section. Do not hardcode versions elsewhere.

## Architecture

### Layered Design

```
┌─────────────────────────────────────────────────┐
│  Presentation: CLI · TUI · Web UI · MCP Server  │  dtx, dtx-web
├─────────────────────────────────────────────────┤
│  Protocol: JSON-RPC methods + transports         │  dtx-protocol
├─────────────────────────────────────────────────┤
│  Intelligence: Code · Memory · Onboarding        │  dtx-code, dtx-memory
├─────────────────────────────────────────────────┤
│  Middleware: Logging · Metrics · Retry · AI      │  dtx-middleware
├─────────────────────────────────────────────────┤
│  Orchestration: Dependency graph · Health · Life │  dtx-process
├─────────────────────────────────────────────────┤
│  Core: Resource trait · Events · Domain types    │  dtx-core
├─────────────────────────────────────────────────┤
│  Data: ConfigStore · Model types                  │  dtx-core
├─────────────────────────────────────────────────┤
│  Backends: Process · Container · VM · Agent      │  dtx-process, dtx-vm, dtx-agent
├─────────────────────────────────────────────────┤
│  Extensions: Plugin loader · Sandbox             │  dtx-plugin
└─────────────────────────────────────────────────┘
```

### Core Invariant: Resource Trait

Everything dtx manages implements `Resource`. This is the single orchestration abstraction.

```rust
// dtx-core/src/resource/traits.rs
#[async_trait]
pub trait Resource: Send + Sync {
    fn id(&self) -> &ResourceId;
    fn kind(&self) -> ResourceKind;          // Process | Container | VM | Agent | Custom(u16)
    fn state(&self) -> &ResourceState;       // Pending → Starting → Running → Stopping → Stopped | Failed
    async fn start(&mut self, ctx: &Context) -> ResourceResult<()>;
    async fn stop(&mut self, ctx: &Context) -> ResourceResult<()>;
    async fn kill(&mut self, ctx: &Context) -> ResourceResult<()>;
    async fn restart(&mut self, ctx: &Context) -> ResourceResult<()>;
    async fn health(&self) -> HealthStatus;  // Unknown | Healthy | Unhealthy
    fn logs(&self) -> Option<Box<dyn LogStream>>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

### Event System

All lifecycle events flow through `ResourceEventBus` using `LifecycleEvent`:

- `ResourceEventBus` — tokio::broadcast pub-sub with replay buffer for late subscribers
- `LifecycleEvent` — Starting, Running, Stopping, Stopped, Failed, Restarting, HealthCheck*, Log, Dependency*, ConfigChanged
- `EventFilter` — selective subscription by resource, kind, event type, with/without logs
- Unix socket IPC (`.dtx/events.sock`) — cross-process notification from CLI to Web server

## Crate Structure

```
crates/
├── dtx/             # Binary: CLI + TUI entrypoint
│   └── src/
│       ├── cmd/     # Subcommands (one file per command)
│       ├── tui/     # Terminal UI (app.rs, ui.rs)
│       └── context.rs
├── dtx-core/        # Core abstractions (no heavy implementations)
│   └── src/
│       ├── resource/      # Resource trait, ResourceId, ResourceKind, ResourceState, HealthStatus
│       ├── events/        # ResourceEventBus, LifecycleEvent, EventFilter, socket IPC
│       ├── domain/        # Validated types: ServiceName, Port, ShellCommand, Environment
│       ├── config/        # ProjectConfig, ConfigStore, ProcessComposeConfig, discovery
│       ├── middleware/    # Middleware trait, MiddlewareStack, Operation, Response
│       ├── graph/         # DependencyGraph, GraphValidator (cycle detection)
│       ├── nix/           # NixClient, FlakeGenerator, PackageMappings, DevEnvironment
│       ├── process/       # Port utilities, preflight checks
│       ├── translation/   # Translator traits, container config, codebase inference, import/
│       └── export/        # Exporter trait: Docker Compose, Kubernetes, process-compose
├── dtx-code/        # Code intelligence (ast-grep based)
│   └── src/
│       ├── symbol.rs      # Symbol, SymbolKind, SymbolOverview
│       ├── workspace.rs   # WorkspaceIndex (file indexing, symbol search)
│       ├── references.rs  # find_references, find_referencing_symbols
│       ├── rename.rs      # rename_symbol (cross-file)
│       ├── edit.rs        # replace_symbol_body, insert_*_symbol, replace_lines
│       ├── search.rs      # search_pattern (regex with context)
│       ├── parser.rs      # parse_source (AST parsing)
│       └── language.rs    # Language detection
├── dtx-memory/      # Cross-session persistent memory
│   └── src/
│       ├── store.rs       # MemoryStore (file-backed CRUD)
│       ├── types.rs       # Memory, MemoryKind (User, Project, Feedback, Reference)
│       └── search.rs      # MemoryFilter, search
├── dtx-process/     # Resource implementations + orchestrator
│   └── src/
│       ├── process.rs     # ProcessResource (implements Resource)
│       ├── orchestrator.rs # ResourceOrchestrator (topological startup/shutdown)
│       ├── probe.rs       # ProbeRunner (exec, HTTP health checks)
│       ├── config.rs      # ProcessResourceConfig, RestartPolicy, BackoffConfig
│       ├── container.rs   # ContainerResource (feature: container)
│       ├── vm.rs          # VMResource
│       ├── agent.rs       # AgentResource
│       └── translator.rs  # ProcessToContainerTranslator
├── dtx-web/         # Axum server: handlers (api, html, htmx, sse), routes, state
├── dtx-protocol/    # JSON-RPC 2.0, MCP integration, NL parser, transports
├── dtx-middleware/   # Concrete middleware: logging, metrics, retry, timeout, AI
├── dtx-plugin/      # Plugin loader, manifest, sandbox (WASM), signing
├── dtx-vm/          # VM backends: QEMU, Firecracker, NixOS
└── dtx-agent/       # AI agent backends: Claude, OpenAI, Ollama, LlamaCpp
```

## Key Design Patterns

### Parse Don't Validate (Domain Types)

Domain types enforce invariants at construction. If you have one, it's valid.

```rust
// dtx-core/src/domain/
let name: ServiceName = "api".parse()?;       // 2-63 chars, lowercase+hyphens, no reserved words
let port = Port::try_from(3000u16)?;          // >= 1024 (non-privileged)
let cmd: ShellCommand = "npm start".parse()?; // Non-empty, balanced quotes
let env = Environment::new().with("NODE_ENV", "production");
```

### Resource Orchestration

```rust
// dtx-process/src/orchestrator.rs
let mut orchestrator = ResourceOrchestrator::new(event_bus.clone());
orchestrator.add_resource(Box::new(postgres_resource));
orchestrator.add_resource(Box::new(api_resource));  // depends_on: [postgres]

let result = orchestrator.start_all().await?;  // Topological order, dependency conditions
// result.started, result.failed, result.skipped

orchestrator.stop_all().await?;  // Reverse order
```

### Event-Driven Architecture

```rust
let bus = ResourceEventBus::new();
let filter = EventFilter::new()
    .resource("api")
    .kind(ResourceKind::Process)
    .without_logs();
let mut sub = bus.subscribe_filtered(filter);

bus.publish(LifecycleEvent::starting("api", ResourceKind::Process));
// LifecycleEvent variants: Starting, Running, Stopping, Stopped, Failed, Restarting,
//   HealthCheckPassed, HealthCheckFailed, Log, DependencyWaiting, DependencyResolved, ConfigChanged

// Cross-process notification (CLI → Web via .dtx/events.sock)
notify_config_changed(project_id).await;
start_event_listener(bus.clone()).await;       // Web server listens
```

### Middleware Stack (Tower-style)

```rust
// dtx-core/src/middleware/
let chain = MiddlewareStack::new()
    .layer(LoggingMiddleware::new(LogLevel::Info))
    .layer(TimeoutMiddleware::new(Duration::from_secs(30)))
    .layer(RetryMiddleware::new(ExponentialBackoff::default()))
    .build(handler);

let response = chain.handle(Operation::Start, context).await?;
// Operation variants: Start, Stop, Restart, Kill, Status, Health, Logs, StartAll, StopAll, Configure, Custom
```

## CLI Commands

```
dtx init [name]                    # Create project (--detect for codebase inference, -y auto-accept)
dtx add <service>                  # Add service (smart inference: --package, --port, --command, --depends-on, --health-check, --restart, --init)
dtx edit <service>                 # Edit service (--add-env, --remove-env, --add-dep, --remove-dep, --enable, --disable)
dtx remove <service>               # Remove service (-y skip confirmation)
dtx list [-s]                      # List projects or services
dtx start [service]                # Start services in TUI mode (-f for foreground)
dtx stop [service]                 # Stop services (calls web API if running)
dtx status [service]               # Show status (live from web API or config fallback)
dtx logs [service] [-f] [-a]       # View/stream logs via SSE
dtx search <query> [-l N]          # Search Nix packages
dtx config [key] [value]           # Hierarchical config (--global, --project)
dtx export [-f format] [-o file]   # Export: process-compose (default), docker-compose, kubernetes, dtx
dtx import <file>                  # Import: process-compose, docker-compose, Procfile (auto-detect)
dtx nix {init,envrc,shell,packages} # Nix environment management
dtx web [-p port] [-o]             # Start web UI (Axum + HTMX)
dtx mcp [-p path]                  # Start MCP server (stdio JSON-RPC)
dtx code <subcommand>              # Code intelligence (symbols, references, search)
dtx memory <subcommand>            # Memory operations (list, read, write, edit, delete)
dtx completions <shell>            # Generate shell completions
```

## Web Architecture

```rust
// dtx-web/src/state.rs — shared across all handlers
pub struct AppState {
    store: Arc<RwLock<ConfigStore>>,
    nix_client: Arc<NixClient>,
    orchestrator: Arc<RwLock<Option<ResourceOrchestrator>>>,
    event_bus: Arc<ResourceEventBus>,
    sse_tracker: Arc<ConnectionTracker>,
    shutdown_token: CancellationToken,
}
```

**Route groups**: `/api/*` (JSON REST), `/` (HTML pages), `/htmx/*` (partial components), `/sse/*` (event streams)

## File Locations

| Content | Location |
|---------|----------|
| Workspace deps | `Cargo.toml` `[workspace.dependencies]` |
| Templates | `crates/dtx-web/templates/` |
| Static assets | `static/` (CSS, JS, fonts — served at `/static/`) |
| Domain types | `crates/dtx-core/src/domain/` |
| Resource trait | `crates/dtx-core/src/resource/traits.rs` |
| Event system | `crates/dtx-core/src/events/` (resource_bus.rs, lifecycle.rs, filter.rs, socket.rs) |
| Nix integration | `crates/dtx-core/src/nix/` (client, flake, mappings, ast/, backend/) |
| Process orchestrator | `crates/dtx-process/src/orchestrator.rs` |
| MCP integration | `crates/dtx-protocol/src/mcp/` |
| Code intelligence | `crates/dtx-code/src/` (symbol, workspace, references, rename, edit, search) |
| Memory store | `crates/dtx-memory/src/` (store, types, search) |
| Plugin sandbox | `crates/dtx-plugin/src/sandbox/` |
| TUI | `crates/dtx/src/tui/` (app.rs, ui.rs) |
| CLI commands | `crates/dtx/src/cmd/` (one file per command) |
| Tests | `crates/*/tests/` or inline `#[cfg(test)]` |

## Code Conventions

### Rust
- `thiserror` for library errors, `anyhow` for application errors
- Async everywhere (Tokio runtime)
- Askama compile-time template verification
- `tracing` for structured logging (never `println!`)
- Feature gates for optional backends (`native-nix`, `container`, `sandbox`, `dynamic`, `code`, `memory`)

### Naming
- Crates: `dtx-*` (kebab-case)
- Modules: snake_case
- Types: PascalCase
- Functions: snake_case
- Constants: SCREAMING_SNAKE_CASE

### Error Handling
```rust
// Library crates: specific errors
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Port conflict: {0}")]
    PortConflict(#[from] PortConflictError),
    #[error("Validation: {0}")]
    Validation(String),
    // ...
}

// Application code: anyhow
pub async fn run() -> anyhow::Result<()> { ... }
```

## Testing

```bash
cargo test                    # All tests
cargo test -p dtx-core        # Single crate
cargo test -p dtx-core graph  # Pattern match within crate
```

- Unit tests: inline `#[cfg(test)]` module
- Integration tests: `crates/*/tests/`
- Use `tempfile` for filesystem tests

## Don'ts

- Don't use `println!` for logging (use `tracing`)
- Don't block async (use `tokio::task::spawn_blocking`)
- Don't hardcode paths (use config/env)
- Don't skip error handling (no `.unwrap()` in production code)
- Don't create SPA (use HTMX server-driven UI)
- Don't add dependencies without justification
- Don't read files in `**/done/` directories (completed phases, archived content)
- Don't bypass domain types — use `ServiceName`, `Port`, `ShellCommand` instead of raw strings/numbers
- Don't put process implementations in `dtx-core` — core defines traits, `dtx-process` implements them
- Don't HARDCODE
- Don't MVP

## Phase Tracking

See `TODO.md` for active phase status.

## Principle

Code should be self-documenting. If you need a comment to explain WHAT the code does, refactor to make it clearer.
