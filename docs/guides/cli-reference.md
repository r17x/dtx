# CLI Reference

> Complete command reference for dtx.

---

## Global Options

```bash
dtx [OPTIONS] <COMMAND>

Options:
  -v, --verbose     Enable verbose logging
  -h, --help        Print help
  -V, --version     Print version
```

---

## Commands

### init

Initialize a new project.

```bash
dtx init <name> [OPTIONS]

Arguments:
  <name>              Project name

Options:
  -p, --path <PATH>   Project path (defaults to current directory)
  -d, --description   Project description
```

**Example:**
```bash
dtx init myapp
dtx init myapp --path ./projects/myapp --description "My web application"
```

---

### add

Add a service to the project.

```bash
dtx add <name> [OPTIONS]

Arguments:
  <name>              Service name

Options:
  -c, --command <CMD>        Command to run (auto-detected for known packages)
  -P, --package <PKG>        Nix package name (auto-inferred if omitted)
  -p, --port <PORT>          Port number
  -w, --working-dir <DIR>    Working directory
  -e, --env <KEY=VALUE>      Environment variables (repeatable)
  --depends-on <SERVICES>    Dependencies (comma-separated)
  -i, --init <CMD>           Initialization command (runs once before service)
  --disabled                 Disable the service initially
  --health-check <CHECK>     Health check (exec:cmd or http:host:port/path)
```

**Examples:**
```bash
# Add PostgreSQL with auto-detection
dtx add postgres

# Add custom API server
dtx add api --command "npm run dev" --port 3000 --depends-on postgres,redis

# Add with environment variables
dtx add worker --command "python worker.py" -e QUEUE=jobs -e WORKERS=4

# Add with health check
dtx add api --command "node server.js" --health-check "http:localhost:3000/health"
```

---

### edit

Edit a service configuration.

```bash
dtx edit <name> [OPTIONS]

Arguments:
  <name>              Service name

Options:
  --add-env <KEY=VALUE>       Add environment variable (repeatable)
  --remove-env <KEY>          Remove environment variable (repeatable)
  --add-dep <SERVICE>         Add dependency (repeatable)
  --remove-dep <SERVICE>      Remove dependency (repeatable)
  --enable                    Enable the service
  --disable                   Disable the service
  -c, --command <CMD>         Update command
  -p, --port <PORT>           Update port
  -P, --package <PKG>         Update Nix package
```

**Examples:**
```bash
dtx edit api --add-env LOG_LEVEL=debug
dtx edit api --add-dep redis --remove-dep cache
dtx edit worker --disable
dtx edit api --port 8080 --command "npm run start:prod"
```

---

### list

List projects or services.

```bash
dtx list [OPTIONS]

Options:
  -s, --services    List services in current project instead of projects
```

**Examples:**
```bash
dtx list              # List all projects
dtx list --services   # List services in current project
```

---

### start

Start services.

```bash
dtx start [service] [OPTIONS]

Arguments:
  [service]         Specific service to start (defaults to all enabled)

Options:
  -f, --foreground  Run in foreground (no TUI, logs to stdout)
```

**Examples:**
```bash
dtx start              # Start all services (TUI mode)
dtx start -f           # Start all services (foreground mode)
dtx start api          # Start specific service
dtx start api -f       # Start specific service in foreground
```

---

### stop

Stop services.

```bash
dtx stop [service]

Arguments:
  [service]         Specific service to stop (defaults to all)
```

**Examples:**
```bash
dtx stop               # Stop all services
dtx stop api           # Stop specific service
```

---

### status

Show service status.

```bash
dtx status [service]

Arguments:
  [service]         Specific service (defaults to all)
```

**Example output:**
```
NAME       KIND      STATE     PORT    HEALTH
postgres   process   running   5432    healthy
redis      process   running   6379    healthy
api        process   running   3000    healthy
worker     process   running   -       -
```

---

### logs

View service logs.

```bash
dtx logs [service] [OPTIONS]

Arguments:
  [service]         Specific service to view logs for

Options:
  -a, --all         Show logs for all services
  -f, --follow      Follow log output (stream new logs)
```

**Examples:**
```bash
dtx logs api           # View api logs
dtx logs api -f        # Follow api logs
dtx logs --all         # View all service logs
```

---

### remove

Remove a service.

```bash
dtx remove <name> [OPTIONS]

Arguments:
  <name>            Service name

Options:
  -y, --yes         Skip confirmation prompt
```

**Example:**
```bash
dtx remove worker
dtx remove worker -y   # Skip confirmation
```

---

### export

Export configuration to various formats.

```bash
dtx export [OPTIONS]

Options:
  -o, --output <FILE>         Output file (defaults to stdout)
  -f, --format <FORMAT>       Export format [default: process-compose]
  --namespace <NS>            Kubernetes namespace
  --default-image <IMAGE>     Default container image
  --services <SERVICES>       Filter to specific services (comma-separated)

Formats:
  process-compose   Process-compose YAML
  docker-compose    Docker Compose YAML
  kubernetes        Kubernetes manifests
  dtx               Canonical dtx format
```

**Examples:**
```bash
dtx export                                    # Export to stdout
dtx export -o compose.yaml                    # Export to file
dtx export -f docker-compose                  # Export as Docker Compose
dtx export -f kubernetes --namespace prod     # Export as K8s manifests
```

---

### import

Import configuration from external formats.

```bash
dtx import <file> [OPTIONS]

Arguments:
  <file>            File to import

Options:
  -f, --format <FORMAT>   Force format (auto-detected by default)
  --no-nix                Skip Nix package inference
  --dry-run               Show what would be imported

Formats:
  process-compose   Process-compose YAML
  docker-compose    Docker Compose YAML
  procfile          Procfile
  auto              Auto-detect (default)
```

**Examples:**
```bash
dtx import process-compose.yaml
dtx import docker-compose.yml --format docker-compose
dtx import Procfile --dry-run
```

---

### search

Search for Nix packages.

```bash
dtx search <query> [OPTIONS]

Arguments:
  <query>           Search query

Options:
  -l, --limit <N>   Maximum results [default: 20]
```

**Example:**
```bash
dtx search postgres
dtx search "node 20" --limit 10
```

---

### web

Start the web UI.

```bash
dtx web [OPTIONS]

Options:
  -p, --port <PORT>   Port to listen on [default: 3000]
  -o, --open          Open browser after starting
```

**Examples:**
```bash
dtx web                    # Start on port 3000
dtx web --port 8080        # Custom port
dtx web --open             # Open browser automatically
```

---

### mcp

Run as MCP server for AI agent integration.

```bash
dtx mcp [OPTIONS]

Options:
  -p, --project <PATH>   Project directory (defaults to current, or DTX_PROJECT env)
```

**Example:**
```bash
dtx mcp                           # Use current directory
dtx mcp --project /path/to/proj   # Specify project
```

See [MCP Integration Guide](./mcp-integration.md) for setup with Claude.

---

### code

Code intelligence operations.

```bash
dtx code <COMMAND>

Commands:
  symbols <file>             Show symbol overview (functions, structs, impls) with line ranges
  find <name>                Find symbol by name path (e.g., "MyStruct/method")
  references <name> <file>   Find all references to a symbol across the codebase
  search <pattern>           Regex search with context lines
  rename <old> <new> <file>  Rename a symbol across all files
```

**Examples:**
```bash
dtx code symbols src/main.rs               # Show file structure
dtx code find "Resource/start"             # Find symbol by path
dtx code references "ResourceId" src/lib.rs # Find all usages
dtx code search "async fn.*start"          # Regex search
dtx code rename "old_name" "new_name" src/lib.rs  # Cross-file rename
```

---

### memory

Cross-session memory operations.

```bash
dtx memory <COMMAND>

Commands:
  list [OPTIONS]            List all memories
  read <name>               Read a specific memory
  write <name>              Write a new memory
  edit <name>               Edit an existing memory
  delete <name>             Delete a memory
```

**Examples:**
```bash
dtx memory list                           # List all memories
dtx memory list --kind project            # Filter by kind
dtx memory read onboarding               # Read specific memory
dtx memory write decision-auth            # Write new memory
dtx memory delete old-note                # Delete memory
```

Memory kinds: `user`, `project`, `feedback`, `reference`

---

### completions

Generate shell completions.

```bash
dtx completions <shell>

Arguments:
  <shell>           Shell type: bash, zsh, fish, powershell, elvish
```

**Examples:**
```bash
# Bash
dtx completions bash > ~/.local/share/bash-completion/completions/dtx

# Zsh
dtx completions zsh > ~/.zfunc/_dtx

# Fish
dtx completions fish > ~/.config/fish/completions/dtx.fish
```

---

### config

Manage hierarchical configuration.

```bash
dtx config [key] [value] [OPTIONS]

Arguments:
  [key]              Config key (dot-separated path)
  [value]            Value to set

Options:
  --global            Target global config (~/.config/dtx/config.yaml)
  --project           Target project config (.dtx/config.yaml)
```

**Examples:**
```bash
dtx config                                 # View effective config (merged)
dtx config --global                        # View global config only
dtx config settings.log_level debug        # Set project-level value
dtx config --global defaults.log_level info # Set global default
```

---

### nix

Nix environment management.

```bash
dtx nix <COMMAND>

Commands:
  init      Generate flake.nix and .envrc for the project
  envrc     Regenerate .envrc only
  shell     Run command in nix shell (or enter interactive shell)
  packages  List Nix packages from services
```

**Examples:**
```bash
dtx nix init                       # Generate flake.nix and .envrc
dtx nix envrc                      # Regenerate .envrc
dtx nix shell                      # Enter interactive shell
dtx nix shell "npm install"        # Run command in nix shell
dtx nix packages                   # List required packages
```

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DTX_PROJECT` | Project directory for MCP server |

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |

---

## See Also

- [Quick Start](./quick-start.md) - Get running quickly
- [Configuration](./configuration.md) - Configuration reference
- [MCP Integration](./mcp-integration.md) - AI agent integration
