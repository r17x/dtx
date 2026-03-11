# Configuration Guide

> Complete reference for .dtx/config.yaml and config options.

---

## File Structure

```yaml
project:
  name: my-app         # Project name (required)
  description: "..."   # Optional description

resources:
  service-name:        # Resource definition
    # ... config
```

---

## Resource Types

### Process

Native OS process.

```yaml
resources:
  api:
    kind: process
    command: node server.js
    working_dir: ./api
    port: 3000
    environment:
      NODE_ENV: development
      PORT: "3000"
```

### Container

Docker/Podman container.

```yaml
resources:
  postgres:
    kind: container
    image: postgres:15
    ports:
      - 5432:5432
    environment:
      POSTGRES_PASSWORD: secret
    volumes:
      - ./.dtx/data/postgres:/var/lib/postgresql/data
```

---

## Health Checks

### Exec Probe

```yaml
health:
  exec: pg_isready -h 127.0.0.1 -p 5432
  initial_delay: 2s
  period: 5s
  timeout: 3s
  success_threshold: 1
  failure_threshold: 3
```

### HTTP Probe

```yaml
health:
  http: http://localhost:3000/health
  initial_delay: 5s
  period: 10s
```

### TCP Probe

```yaml
health:
  tcp: 127.0.0.1:5432
  initial_delay: 2s
```

---

## Dependencies

```yaml
resources:
  api:
    depends_on:
      - postgres: healthy    # Wait for health check
      - redis: started       # Wait for process start
      - migrations: completed # Wait for exit code 0
```

### Conditions

| Condition | Description |
|-----------|-------------|
| `started` | Process has started |
| `healthy` | Health check passed |
| `completed` | Exited with code 0 |

---

## Restart Policies

```yaml
restart: no              # Never restart
restart: always          # Always restart
restart: on-failure      # Restart on non-zero exit

# With options
restart:
  policy: on-failure
  max_retries: 5
  backoff:
    initial: 1s
    max: 60s
    multiplier: 2
```

---

## Shutdown

```yaml
shutdown:
  command: pg_ctl stop -D $PGDATA  # Custom shutdown command
  signal: SIGTERM                   # Signal to send
  timeout: 30s                      # Wait before SIGKILL
```

---

## Environment Variables

```yaml
environment:
  # Static values
  NODE_ENV: production

  # Reference other vars
  API_URL: "http://localhost:${API_PORT}"

  # Reference system env
  HOME: ${HOME}
```

---

## Nix Integration

```yaml
resources:
  api:
    package: nodejs_20    # Nix package to use
    command: node app.js
```

With flake:

```yaml
project:
  flake: ./flake.nix     # Custom flake
```

---

## Global Config

`~/.config/dtx/config.yaml`:

```yaml
defaults:
  timeout: 60s
  restart_policy: on-failure

middleware:
  logging: true
  metrics: true
  ai: false

ai:
  provider: openai
  api_key: ${OPENAI_API_KEY}

web:
  port: 3000
  host: 127.0.0.1
```

---

## Project Config

`.dtx/config.yaml`:

```yaml
project:
  name: my-app

middleware:
  # Override defaults
  ai: true
```

---

## Environment-Specific

```yaml
# .dtx/config.yaml (default)
resources:
  api:
    command: npm run dev

# .dtx/config.prod.yaml
resources:
  api:
    command: npm start
    environment:
      NODE_ENV: production
```

```bash
dtx start --config .dtx/config.prod.yaml
```
