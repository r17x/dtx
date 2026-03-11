# dtx v2 Architecture

> Universal orchestration. Middleware composition. Protocol-first.

---

## Overview

```
dtx orchestrates resources.
Resources are processes, VMs, containers, AI agents.
Everything flows through EventBus.
Middleware transforms behavior.
Protocol enables integration.
```

---

## Principles

```
1. UNIVERSAL    - One abstraction, many backends
2. COMPOSABLE   - Middleware stacks like Tower
3. OBSERVABLE   - EventBus sees everything
4. EXTENSIBLE   - Plugins, protocols, SDKs
5. DETERMINISTIC - Same input, same output
```

---

## Core Abstraction

### Resource

```
Resource: anything with a lifecycle.

    ┌─────────┐
    │ Pending │
    └────┬────┘
         │ start()
    ┌────▼────┐
    │Starting │
    └────┬────┘
         │ ready
    ┌────▼────┐◄──────┐
    │ Running │       │ restart()
    └────┬────┘───────┘
         │ stop() / exit
    ┌────▼────┐
    │ Stopped │
    └─────────┘
```

```rust
trait Resource {
    fn id(&self) -> ResourceId;
    fn kind(&self) -> ResourceKind;

    async fn start(&mut self, ctx: &Context) -> Result<()>;
    async fn stop(&mut self, ctx: &Context) -> Result<()>;

    fn state(&self) -> ResourceState;
    async fn health(&self) -> HealthStatus;
}
```

### ResourceKind

```
Process   - Native OS process (current)
Container - Docker, Podman, Nix containers
VM        - QEMU, Nix VMs, Firecracker
Agent     - AI agents, LLM workers
Custom    - Plugin-defined
```

---

## Layer Stack

```
┌─────────────────────────────────────────────────────────────┐
│ PRESENTATION                                                │
│   CLI  │  TUI  │  Web UI  │  MCP Server                    │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│ PROTOCOL                                                    │
│   JSON-RPC commands over stdio/HTTP/WebSocket               │
│   MCP-compatible for AI integration                         │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│ MIDDLEWARE STACK                                            │
│   Request → [Auth] → [AI] → [Logging] → [Metrics] → Core   │
│   Response ← [Auth] ← [AI] ← [Logging] ← [Metrics] ← Core  │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│ ORCHESTRATOR                                                │
│   Dependency resolution, health checks, restart policies    │
└────────────────────────┬────────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────────┐
│ EVENT BUS                                                   │
│   Lifecycle events, logs, state changes                     │
│   All communication flows here                              │
└────────────────────────┬────────────────────────────────────┘
                         │
         ┌───────────────┼───────────────┐
         │               │               │
    ┌────▼────┐    ┌─────▼────┐    ┌─────▼────┐
    │ Process │    │ Container│    │  Agent   │
    │ Backend │    │ Backend  │    │ Backend  │
    └─────────┘    └──────────┘    └──────────┘
```

---

## Middleware

### Philosophy

```
Middleware wraps operations.
Each layer adds one capability.
Layers compose via function composition.
Order matters: outermost runs first.
```

### Interface

```rust
trait Middleware {
    async fn handle(&self, op: Operation, ctx: Context, next: Next) -> Result<Response>;
}
```

### Standard Middleware

```
┌──────────────┬────────────────────────────────────────────┐
│ Layer        │ Purpose                                    │
├──────────────┼────────────────────────────────────────────┤
│ Logging      │ Trace operations, timing, errors          │
│ Auth         │ Validate identity, permissions            │
│ AI           │ Suggest configs, explain errors           │
│ Metrics      │ Prometheus/OTLP export                    │
│ Audit        │ Record who did what when                  │
│ RateLimit    │ Throttle operations                       │
│ Retry        │ Auto-retry transient failures             │
│ Timeout      │ Enforce operation deadlines               │
│ Cache        │ Cache health checks, package info         │
└──────────────┴────────────────────────────────────────────┘
```

### Composition

```rust
let stack = MiddlewareStack::new()
    .layer(TimeoutLayer::new(Duration::from_secs(30)))
    .layer(RetryLayer::new(3))
    .layer(LoggingLayer::new())
    .layer(AuthLayer::new(config))
    .layer(AILayer::new(model))
    .layer(MetricsLayer::new());

let orchestrator = Orchestrator::new(event_bus).with_middleware(stack);
```

---

## Event Bus

### Role

```
EventBus is the nervous system.
All state changes publish events.
Subscribers react independently.
Decouples producers from consumers.
```

### Events

```rust
enum LifecycleEvent {
    // State transitions
    Starting { id: ResourceId, kind: ResourceKind },
    Running  { id: ResourceId, pid: Option<u32> },
    Stopping { id: ResourceId },
    Stopped  { id: ResourceId, exit: ExitStatus },
    Failed   { id: ResourceId, error: String },

    // Health
    HealthCheckPassed { id: ResourceId },
    HealthCheckFailed { id: ResourceId, reason: String },

    // Observability
    Log { id: ResourceId, stream: LogStream, line: String },
    Metric { id: ResourceId, name: String, value: f64 },

    // Configuration
    ConfigChanged { project_id: ProjectId },
    DependencyResolved { id: ResourceId, dependency: ResourceId },
}
```

### Subscribers

```
┌─────────────┬─────────────────────────────────────────────┐
│ Subscriber  │ Reacts to                                   │
├─────────────┼─────────────────────────────────────────────┤
│ TUI         │ State changes → update display              │
│ Web SSE     │ All events → stream to browser              │
│ Orchestrator│ Health/Failed → trigger restart             │
│ Metrics     │ Lifecycle → update counters/gauges          │
│ Audit Log   │ All events → persist to database            │
│ AI Observer │ Failed/Unhealthy → suggest fixes            │
└─────────────┴─────────────────────────────────────────────┘
```

---

## Protocol

### Design

```
MCP-compatible JSON-RPC.
Works over: stdio, HTTP, WebSocket.
Enables: AI agents, IDE plugins, remote control.
```

### Commands

```json
// Start a resource
{"jsonrpc": "2.0", "method": "resource/start", "params": {"id": "api"}, "id": 1}

// Stop a resource
{"jsonrpc": "2.0", "method": "resource/stop", "params": {"id": "api"}, "id": 2}

// Get status
{"jsonrpc": "2.0", "method": "resource/status", "params": {"id": "api"}, "id": 3}

// Subscribe to events
{"jsonrpc": "2.0", "method": "events/subscribe", "params": {"filter": ["*"]}, "id": 4}

// Natural language (AI layer)
{"jsonrpc": "2.0", "method": "ai/execute", "params": {"prompt": "start postgres and redis"}, "id": 5}
```

### MCP Resources

```json
{
  "resources": [
    {
      "uri": "dtx://project/myapp/resource/api",
      "name": "api",
      "mimeType": "application/json",
      "description": "API server resource"
    }
  ]
}
```

### MCP Tools

```json
{
  "tools": [
    {
      "name": "start_resource",
      "description": "Start a resource by ID",
      "inputSchema": {
        "type": "object",
        "properties": {
          "id": {"type": "string"}
        },
        "required": ["id"]
      }
    }
  ]
}
```

---

## Translation

### Purpose

```
Convert between resource types.
Process ↔ Container ↔ VM.
Enables migration, portability.
```

### Interface

```rust
trait Translator<From, To> {
    fn translate(&self, from: &From) -> Result<To>;
    fn reverse(&self, to: &To) -> Result<From>;
}
```

### Registry

```rust
let registry = TranslatorRegistry::new()
    .register(ProcessToContainer::new())
    .register(ContainerToVM::new())
    .register(ProcessToAgent::new());

// Translate process to container
let container = registry.translate::<Process, Container>(&process)?;
```

### Example: Process → Container

```
Process {                     Container {
  command: "node app.js"  →     image: "node:20"
  port: 3000                    command: ["node", "app.js"]
  env: {NODE_ENV: prod}         ports: [3000]
}                               env: {NODE_ENV: prod}
                              }
```

---

## Crates

```
crates/
├── dtx-protocol/       # MCP-compatible protocol
│   ├── commands.rs         # JSON-RPC command definitions
│   ├── resources.rs        # MCP resource schemas
│   ├── tools.rs            # MCP tool definitions
│   └── transport.rs        # stdio, HTTP, WebSocket
│
├── dtx-core/           # Core abstractions
│   ├── resource.rs         # Resource trait
│   ├── lifecycle.rs        # State machine
│   ├── events.rs           # EventBus, LifecycleEvent
│   ├── middleware.rs       # Middleware trait, stack
│   ├── orchestrator.rs     # Dependency ordering
│   ├── context.rs          # Request context
│   └── translation.rs      # Resource translation
│
├── dtx-process/        # Native process backend
│   ├── process.rs          # Process implements Resource
│   ├── probe.rs            # Health probes
│   ├── nix.rs              # Nix environment
│   └── restart.rs          # Restart policies
│
├── dtx-container/      # Container backend (future)
│   ├── container.rs        # Container implements Resource
│   ├── docker.rs           # Docker client
│   └── podman.rs           # Podman client
│
├── dtx-agent/          # AI agent backend (future)
│   ├── agent.rs            # Agent implements Resource
│   ├── runtime.rs          # Agent runtime
│   └── protocol.rs         # Agent communication
│
├── dtx-middleware/     # Standard middleware
│   ├── logging.rs          # Structured logging
│   ├── auth.rs             # Authentication
│   ├── ai.rs               # AI assistance
│   ├── metrics.rs          # Observability
│   └── audit.rs            # Audit trail
│
├── dtx-plugin/         # Plugin system
│   ├── loader.rs           # Dynamic loading
│   ├── manifest.rs         # Plugin manifest
│   ├── api.rs              # Stable API
│   └── sandbox.rs          # Isolation (WASM)
│
├── dtx-web/            # Web UI
└── dtx/                # CLI binary
```

---

## Data Flow

### Start Resource

```
1. User: `dtx start api`
2. CLI parses command
3. Protocol encodes: {"method": "resource/start", "params": {"id": "api"}}
4. Middleware stack processes:
   - Auth validates permission
   - Logging records request
   - AI checks for suggestions
5. Orchestrator resolves dependencies
6. For each dependency (in order):
   a. Backend.start() spawns process
   b. EventBus publishes Starting
   c. Health probe runs
   d. EventBus publishes Running
7. Response returns to CLI
8. Subscribers (TUI, Web) update display
```

### Event Flow

```
Process.start()
    │
    ▼
EventBus.publish(Starting)
    │
    ├──► TUI updates status
    ├──► Web SSE sends to browser
    ├──► Metrics increments counter
    └──► Audit records event

Process exits
    │
    ▼
EventBus.publish(Stopped)
    │
    ├──► Orchestrator checks restart policy
    ├──► TUI updates status
    └──► Metrics records duration
```

---

## Configuration

### Project Structure

```
myproject/
├── .dtx/
│   ├── config.yaml         # Single source of truth (resources + settings)
│   ├── config.yaml.lock    # File lock for concurrent access
│   ├── web.port            # Web server port (when running)
│   └── events.sock         # IPC socket
└── flake.nix               # Nix environment (optional)
```

**Key points:**
- No configuration file in project root by default
- All dtx state consolidated in `.dtx/` directory
- config.yaml is the single source of truth
- `.gitignore` decisions left to user

---

### Configuration Sources

dtx populates `.dtx/config.yaml` from multiple sources with clear precedence:

```
┌──────────────────────────────────────────────────────────────────┐
│ SOURCE                        COMMAND              PRIORITY     │
├──────────────────────────────────────────────────────────────────┤
│ process-compose.yaml          dtx import           1 (highest)  │
│ docker-compose.yml            dtx import           1            │
│ Procfile                      dtx import           1            │
│ CLI commands                  dtx add              2            │
│ Codebase inference            dtx init --detect    3 (lowest)   │
└──────────────────────────────────────────────────────────────────┘
```

#### Precedence Rules

1. **Import wins over inference**: Explicit configuration takes priority
2. **Later imports merge**: New imports add to existing config; conflicts favor new
3. **Nix inference requires confirmation**: Package suggestions shown for approval

#### Import Flow

```
dtx import <source-file>
    │
    ▼
┌─────────────────────────────────────┐
│ 1. DETECT FORMAT                    │
│    - process-compose.yaml           │
│    - docker-compose.yml             │
│    - Procfile                       │
│    - (auto-detect if not specified) │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│ 2. PARSE SOURCE                     │
│    - Extract services/processes     │
│    - Extract dependencies           │
│    - Extract health checks          │
│    - Extract environment variables  │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│ 3. TRANSLATE TO DTX RESOURCES       │
│    - Map fields to dtx schema       │
│    - Infer 'kind' (process/container│
│    - Normalize dependency syntax    │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│ 4. NIX PACKAGE INFERENCE            │
│    - Detect required packages       │
│    - Show suggestions to user       │
│    - Wait for confirmation          │
│    - Apply approved packages        │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│ 5. MERGE INTO CONFIG                │
│    - Load existing .dtx/config.yaml │
│    - Merge new resources            │
│    - Resolve conflicts (new wins)   │
│    - Write updated config           │
└─────────────────────────────────────┘
```

#### Codebase Inference

When `dtx init --detect` runs, it scans the project for:

```
┌────────────────────┬────────────────────────────────────────────┐
│ Detection Target   │ Files Scanned                              │
├────────────────────┼────────────────────────────────────────────┤
│ Node.js            │ package.json, .nvmrc, .node-version        │
│ Python             │ requirements.txt, pyproject.toml, setup.py │
│ Ruby               │ Gemfile, .ruby-version                     │
│ Go                 │ go.mod                                     │
│ Rust               │ Cargo.toml                                 │
│ Java               │ pom.xml, build.gradle                      │
│ PHP                │ composer.json                              │
│ Databases          │ docker-compose.yml services                │
│ Ports              │ Common framework patterns                  │
└────────────────────┴────────────────────────────────────────────┘
```

All inferred values require user confirmation before being applied.

---

### .dtx/config.yaml Schema

The configuration file is a superset of process-compose format:

```yaml
# Project metadata
project:
  name: myapp
  description: My application stack

# Global settings
settings:
  log_level: info                    # debug, info, warn, error
  health_check_interval: 5s          # Default health check interval
  shutdown_timeout: 30s              # Grace period for stopping

# Resource definitions
resources:
  # Process resource (native OS process)
  postgres:
    kind: process                    # process | container | agent
    command: postgres -D $PGDATA
    working_dir: .
    port: 5432                       # Primary port for health checks
    environment:
      PGDATA: ./.dtx/data/postgres
    health:
      exec: pg_isready -h 127.0.0.1 -p 5432
      interval: 5s
      timeout: 10s
      retries: 3
    restart: on-failure              # always | on-failure | no
    nix:                             # Nix integration (optional)
      packages:                      # Simple: list of packages
        - postgresql_16

  # Process with HTTP health check
  api:
    kind: process
    command: npm run dev
    port: 3000
    depends_on:
      - postgres: healthy            # Wait for postgres health check
      - redis: started               # Wait for redis to start
    environment:
      DATABASE_URL: postgres://localhost:5432/myapp
      NODE_ENV: development
    health:
      http: /health                  # GET http://localhost:3000/health
      interval: 10s
    nix:
      packages:                      # Simple: list of packages
        - nodejs_20
        - pnpm

  # Process with complex Nix environment
  ml-worker:
    kind: process
    command: python train.py
    port: 8080
    nix:
      expr: |                        # Complex: Nix expression
        pkgs.python312.withPackages (p: [
          p.numpy
          p.pandas
          p.scikit-learn
        ])

  # Container resource (future)
  redis:
    kind: container
    image: redis:7-alpine
    port: 6379
    health:
      tcp: 127.0.0.1:6379

  # Agent resource (future)
  worker:
    kind: agent
    runtime: openai
    model: gpt-4
    tools:
      - process_data
      - send_notification
    depends_on:
      - api: healthy
```

---

### process-compose Compatibility

dtx is designed as a **superset of process-compose v1.x**. All process-compose fields
are supported, with some renamed for clarity and additional fields for dtx features.

Target compatibility: **process-compose v1.x** (stable, widely deployed)

#### Core Fields

| process-compose | dtx | Status | Notes |
|-----------------|-----|--------|-------|
| `command` | `command` | Identical | Shell command to run |
| `working_dir` | `working_dir` | Identical | Working directory |
| `environment` | `environment` | Identical | Environment variables (map) |
| `log_location` | `settings.log_dir` | Moved | Global or per-resource |
| `namespace` | `project.name` | Renamed | Project-level setting |
| `disabled` | `enabled: false` | Inverted | Clearer semantics |
| `is_daemon` | (inferred) | Auto | Detected from behavior |
| `shutdown.command` | `shutdown.command` | Identical | Custom shutdown command |
| `shutdown.signal` | `shutdown.signal` | Identical | Signal to send (SIGTERM) |

#### Dependency Fields

| process-compose | dtx | Status | Notes |
|-----------------|-----|--------|-------|
| `depends_on` (list) | `depends_on` (list) | Identical | Simple dependency list |
| `depends_on` (map) | `depends_on` (map) | Identical | With conditions |
| `depends_on.*.condition: process_healthy` | `depends_on.*.condition: healthy` | Simplified | |
| `depends_on.*.condition: process_started` | `depends_on.*.condition: started` | Simplified | |
| `depends_on.*.condition: process_completed_successfully` | `depends_on.*.condition: completed` | Simplified | |
| `availability.restart` | `restart` | Flattened | Top-level field |
| `availability.backoff_seconds` | `restart.backoff` | Extended | Supports jitter |
| `availability.max_restarts` | `restart.max_attempts` | Renamed | Clearer name |

#### Health Check Fields

| process-compose | dtx | Status | Notes |
|-----------------|-----|--------|-------|
| `readiness_probe` | `health` | Renamed | Clearer for most use cases |
| `liveness_probe` | `liveness` | Separate | dtx adds distinct liveness |
| `readiness_probe.exec` | `health.exec` | Identical | Command to run |
| `readiness_probe.http_get` | `health.http` | Simplified | Just path, host/port inferred |
| `readiness_probe.initial_delay_seconds` | `health.initial_delay` | Renamed | Duration format |
| `readiness_probe.period_seconds` | `health.interval` | Renamed | Duration format |
| `readiness_probe.timeout_seconds` | `health.timeout` | Renamed | Duration format |
| `readiness_probe.failure_threshold` | `health.retries` | Renamed | Clearer name |

#### dtx Extensions

These fields are **not in process-compose** and are dtx-specific:

| Field | Purpose | Example |
|-------|---------|---------|
| `kind` | Resource type | `process`, `container`, `agent` |
| `port` | Primary port | Used for health checks, display |
| `nix.packages` | Nix packages (list) | `[nodejs_20, postgresql_16]` |
| `nix.expr` | Nix expression (string) | `"pkgs.python312.withPackages (p: [p.numpy])"` |
| `nix.shell` | Nix shell file path | `./shell.nix` or `./flake.nix#devShell` |
| `runtime` | Agent runtime | `openai`, `anthropic`, `local` |
| `model` | Agent model | `gpt-4`, `claude-3` |
| `tools` | Agent tools | List of available tools |
| `liveness` | Liveness probe | Separate from readiness |
| `settings.*` | Global settings | Log level, timeouts |

#### Import Behavior

When importing `process-compose.yaml`:

1. All standard fields are mapped directly
2. Missing `kind` defaults to `process`
3. `readiness_probe` is mapped to `health`
4. Duration fields accept both seconds (int) and duration strings ("5s")
5. Nix packages are inferred from commands (with confirmation)

---

### Export

Generate configurations for other tools from `.dtx/config.yaml`:

```bash
# Export to process-compose format (for standalone process-compose)
dtx export --format process-compose

# Export to Docker Compose (for container deployment)
dtx export --format docker-compose

# Export to Kubernetes manifests (for K8s deployment)
dtx export --format kubernetes

# Export canonical dtx format (for sharing)
dtx export --format dtx

# Export to specific file
dtx export --format docker-compose --output docker-compose.yml
```

#### Export Flow

```
.dtx/config.yaml
    │
    ▼
┌─────────────────────────────────────┐
│ LOAD CONFIGURATION                  │
│ - Parse resources                   │
│ - Resolve references                │
│ - Validate schema                   │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│ SELECT TRANSLATOR                   │
│ - ProcessToContainer (docker)       │
│ - ProcessToK8sPod (kubernetes)      │
│ - Identity (process-compose)        │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│ TRANSLATE RESOURCES                 │
│ - Map fields to target format       │
│ - Infer images (if needed)          │
│ - Generate manifests                │
└─────────────────────────────────────┘
    │
    ▼
┌─────────────────────────────────────┐
│ OUTPUT                              │
│ - stdout (default)                  │
│ - file (--output)                   │
│ - directory (kubernetes)            │
└─────────────────────────────────────┘
```

#### Format-Specific Notes

**process-compose:**
- Direct mapping, dtx extensions stripped
- `kind` ignored (all become processes)
- `health` becomes `readiness_probe`

**docker-compose:**
- Processes translated to containers
- Images inferred from commands
- Nix packages mapped to base images
- Volumes generated for working directories

**kubernetes:**
- Each resource becomes a Deployment + Service
- Health checks become readiness/liveness probes
- Dependencies become init containers or ordering

---

### Global vs Project Configuration

Like git, dtx supports configuration at multiple levels with cascading precedence:

```
┌─────────────────────────────────────────────────────────────────┐
│ LEVEL          LOCATION                    PRECEDENCE           │
├─────────────────────────────────────────────────────────────────┤
│ System         /etc/dtx/config.yaml        1 (lowest)           │
│ Global         ~/.config/dtx/config.yaml   2                    │
│ Project        .dtx/config.yaml            3 (highest)          │
└─────────────────────────────────────────────────────────────────┘
```

#### Global Config (~/.config/dtx/config.yaml)

User-level defaults that apply to all projects:

```yaml
defaults:
  log_level: info
  health_check_interval: 5s
  shutdown_timeout: 30s

nix:
  # Default package mappings
  mappings:
    node: nodejs_20
    python: python312
    postgres: postgresql_16

ai:
  # AI provider for suggestions (optional)
  provider: anthropic
  model: claude-3-haiku
```

#### Project Config (.dtx/config.yaml)

Project-specific configuration that overrides global defaults:

```yaml
project:
  name: myapp

settings:
  log_level: debug              # Overrides global

resources:
  # ... resource definitions
```

#### Config Commands

```bash
# View effective config (merged)
dtx config

# View specific level
dtx config --global
dtx config --project

# Set global default
dtx config --global defaults.log_level debug

# Set project value
dtx config settings.log_level info
```

---

### Storage Details

#### .dtx/config.yaml

Human-readable configuration:
- Resource definitions
- Project metadata
- User settings
- Nix package mappings

Typical size: 1-10 KB

Version controlled: Recommended

#### .dtx/config.yaml.lock

File lock for concurrent access:
- Prevents concurrent writes from multiple dtx processes
- Advisory lock via `ConfigStore` using `fs2` file locking
- Automatically acquired and released during read-modify-write cycles
- Ephemeral (safe to delete when no dtx process is running)

#### .dtx/web.port

Web server port file:
- Written when `dtx web` starts
- Contains the port number the web UI is listening on
- Used by CLI commands to communicate with running web server
- Ephemeral (removed on shutdown)

#### .dtx/events.sock

Unix socket for IPC:
- CLI to web server communication
- Event streaming
- Ephemeral (recreated on start)

---

## Extension Points

### Plugin Manifest

```toml
[plugin]
name = "dtx-kubernetes"
version = "0.1.0"
api_version = "2"

[provides]
backends = ["kubernetes"]
middleware = ["k8s-auth"]
translators = [["process", "pod"]]
```

### Plugin API

```rust
// Plugins implement these traits
trait BackendPlugin {
    fn kind(&self) -> ResourceKind;
    fn create(&self, config: Value) -> Result<Box<dyn Resource>>;
}

trait MiddlewarePlugin {
    fn name(&self) -> &str;
    fn create(&self, config: Value) -> Result<Box<dyn Middleware>>;
}

trait TranslatorPlugin {
    fn from_kind(&self) -> ResourceKind;
    fn to_kind(&self) -> ResourceKind;
    fn translate(&self, from: &dyn Resource) -> Result<Box<dyn Resource>>;
}
```

---

## Security

### Threat Model

```
┌──────────────────────┬────────┬─────────────────────────────┐
│ Threat               │ Risk   │ Mitigation                  │
├──────────────────────┼────────┼─────────────────────────────┤
│ Command injection    │ HIGH   │ Validate all inputs         │
│ Plugin malware       │ HIGH   │ WASM sandbox, signing       │
│ Unauthorized access  │ MEDIUM │ Auth middleware, localhost  │
│ Secret exposure      │ MEDIUM │ Env var redaction           │
│ Resource exhaustion  │ LOW    │ Rate limiting, quotas       │
└──────────────────────┴────────┴─────────────────────────────┘
```

### Auth Model

```
Local:  No auth (single-user)
Team:   Token-based (JWT)
Plugin: Capability-based (least privilege)
```

---

## Determinism

### Guarantees

```
Same config + same inputs = same behavior.

Achieved via:
1. Pinned Nix packages (flake.lock)
2. Deterministic dependency ordering
3. Reproducible health check timing
4. Seeded random (where needed)
```

### Non-deterministic (documented)

```
- Process execution timing
- Network latency
- External service availability
- AI suggestions (by design)
```

---

## Observability

### Tracing

```
Every operation has a trace ID.
Traces span: CLI → Protocol → Middleware → Backend.
Export to: Jaeger, Zipkin, OTLP.
```

### Metrics

```
dtx_resources_total{kind="process",state="running"}
dtx_operation_duration_seconds{op="start"}
dtx_health_check_total{result="pass"}
dtx_event_bus_events_total{type="lifecycle"}
```

### Logs

```json
{"ts": "2026-02-11T10:30:00Z", "level": "info", "resource": "api", "msg": "Starting"}
{"ts": "2026-02-11T10:30:01Z", "level": "info", "resource": "api", "msg": "Listening on :3000"}
```

---

## References

- [MCP Specification](https://modelcontextprotocol.io)
- [Tower Middleware](https://docs.rs/tower)
- [Nix](https://nixos.org)
- [process-compose](https://github.com/F1bonacc1/process-compose)

---

*Version: 2.1*
*Last Updated: 2026-02-18*
