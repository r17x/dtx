# Python Worker Example

> Background job processor with Redis queue.

---

## Overview

A multi-service example demonstrating:
- Redis as a dependency
- Python background worker
- Dependency conditions
- Nix package management

---

## Architecture

```
┌─────────────┐     ┌─────────────┐
│   Worker    │────►│    Redis    │
│  (Python)   │     │   (Queue)   │
└─────────────┘     └─────────────┘
```

---

## Resources

| Resource | Port | Description |
|----------|------|-------------|
| redis | 6379 | Redis message queue |
| worker | - | Python job processor |

---

## Quick Start

```bash
# Copy example
cp -r docs/v2/examples/python-worker my-worker
cd my-worker

# Initialize dtx
dtx init python-worker

# Create worker script
cat > worker.py << 'EOF'
import os
import time
import signal
import sys

running = True

def handle_signal(signum, frame):
    global running
    print("Shutting down...")
    running = False

signal.signal(signal.SIGTERM, handle_signal)
signal.signal(signal.SIGINT, handle_signal)

redis_url = os.environ.get('REDIS_URL', 'redis://localhost:6379')
print(f"Worker started, connecting to {redis_url}")

while running:
    # Simulate job processing
    print("Processing jobs...")
    time.sleep(5)

print("Worker stopped")
EOF

# Start services
dtx start
```

---

## Files

```
python-worker/
├── README.md               # This file
├── worker.py               # Worker script (create your own)
└── .dtx/
    └── config.yaml         # All configuration
```

---

## Configuration

### .dtx/config.yaml

```yaml
project:
  name: python-worker
  description: Python background worker example

settings:
  log_level: info

resources:
  redis:
    kind: process
    command: redis-server --port 6379
    port: 6379
    health:
      tcp: 127.0.0.1:6379
      interval: 2s
    restart: on-failure

  worker:
    kind: process
    command: python worker.py
    depends_on:
      - redis: healthy
    environment:
      REDIS_URL: redis://localhost:6379
      WORKER_ID: "1"
    restart: on-failure
```

---

## With Nix Packages

Use Nix to manage Python and Redis:

```yaml
resources:
  redis:
    kind: process
    nix:
      packages:
        - redis
    command: redis-server --port 6379

  worker:
    kind: process
    nix:
      packages:
        - python312
    command: python worker.py
```

Or with Python packages:

```yaml
resources:
  worker:
    kind: process
    nix:
      expr: |
        pkgs.python312.withPackages (p: [
          p.redis
          p.celery
        ])
    command: celery -A tasks worker
```

---

## Scaling Workers

Run multiple worker instances:

```yaml
resources:
  worker-1:
    kind: process
    command: python worker.py
    depends_on:
      - redis: healthy
    environment:
      WORKER_ID: "1"

  worker-2:
    kind: process
    command: python worker.py
    depends_on:
      - redis: healthy
    environment:
      WORKER_ID: "2"
```

---

## Adding a Producer

Add an API that enqueues jobs:

```yaml
resources:
  api:
    kind: process
    command: python api.py
    port: 8000
    depends_on:
      - redis: healthy
    environment:
      REDIS_URL: redis://localhost:6379
    health:
      http:
        path: /health
        port: 8000

  worker:
    depends_on:
      - redis: healthy
      - api: started
```

---

## Next Steps

- [Configuration Guide](../../guides/configuration.md) - More options
- [web-app Example](../web-app/) - Full-stack application
- [simple-api Example](../simple-api/) - Basic API setup
