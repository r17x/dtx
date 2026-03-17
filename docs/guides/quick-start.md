# Quick Start

> Get dtx running in 5 minutes.

---

## Install

```bash
# Via Nix
nix profile install github:r17x/dtx

# Via Cargo
cargo install dtx

# Via Homebrew
brew install r17x/tap/dtx
```

---

## Initialize Project

```bash
cd my-project
dtx init
```

Creates:
```
my-project/
└── .dtx/
    ├── config.yaml    # All configuration
    └── dtx.db         # Runtime state
```

---

## Define Resources

Edit `.dtx/config.yaml`:

```yaml
project:
  name: my-app

resources:
  postgres:
    kind: process
    command: postgres -D $PGDATA
    port: 5432
    environment:
      PGDATA: ./.dtx/data/postgres
    health:
      exec: pg_isready -h 127.0.0.1 -p 5432
    restart: on-failure

  redis:
    kind: process
    command: redis-server --port 6379
    port: 6379
    health:
      tcp: 127.0.0.1:6379

  api:
    kind: process
    command: npm run dev
    port: 3000
    depends_on:
      - postgres: healthy
      - redis: started
    environment:
      DATABASE_URL: postgres://localhost:5432/myapp
      REDIS_URL: redis://localhost:6379
```

---

## Start Services

```bash
# Interactive TUI mode
dtx start

# Foreground mode (logs to stdout)
dtx start -f

# Start specific service
dtx start api
```

---

## Check Status

```bash
dtx status

# Output:
# NAME       KIND      STATE     PORT    HEALTH
# postgres   process   running   5432    healthy
# redis      process   running   6379    healthy
# api        process   running   3000    healthy
```

---

## View Logs

```bash
# All logs
dtx logs

# Specific service
dtx logs api

# Follow logs
dtx logs -f api
```

---

## Stop Services

```bash
# Stop all
dtx stop

# Stop specific
dtx stop api
```

---

## Web UI

```bash
dtx web

# Open http://localhost:3000
```

---

## Next Steps

- [Configuration Guide](./configuration.md) - Detailed config options
- [CLI Reference](./cli-reference.md) - All commands
- [MCP Integration](./mcp-integration.md) - Use with AI agents
