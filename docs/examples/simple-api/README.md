# Simple API Example

> Minimal Node.js API with health check.

---

## Overview

A single-service example demonstrating:
- Basic process resource
- Health check configuration
- Environment variables
- Restart policy

---

## Resources

| Resource | Port | Description |
|----------|------|-------------|
| api | 3000 | Express.js API server |

---

## Quick Start

```bash
# Copy example
cp -r docs/v2/examples/simple-api my-api
cd my-api

# Initialize dtx
dtx init simple-api

# Create a simple server (or use your own)
cat > server.js << 'EOF'
const http = require('http');
const port = process.env.PORT || 3000;

const server = http.createServer((req, res) => {
  if (req.url === '/health') {
    res.writeHead(200);
    res.end('OK');
    return;
  }
  res.writeHead(200);
  res.end('Hello from dtx!');
});

server.listen(port, () => {
  console.log(`Server running on port ${port}`);
});
EOF

# Start
dtx start
```

---

## Files

```
simple-api/
├── README.md               # This file
├── server.js               # API server (create your own)
└── .dtx/
    └── config.yaml         # All configuration
```

---

## Configuration

### .dtx/config.yaml

```yaml
project:
  name: simple-api
  description: Simple API example

settings:
  log_level: info

resources:
  api:
    kind: process
    command: node server.js
    port: 3000
    environment:
      NODE_ENV: development
      PORT: "3000"
    health:
      http:
        path: /health
        port: 3000
      interval: 5s
      timeout: 3s
    restart: on-failure
```

---

## Customization

### Add Nix Package

```yaml
resources:
  api:
    nix:
      packages:
        - nodejs_20
    command: node server.js
```

### Add Database

```yaml
resources:
  postgres:
    kind: process
    command: postgres -D $PGDATA
    port: 5432
    environment:
      PGDATA: ./.dtx/data/postgres

  api:
    depends_on:
      - postgres: healthy
    environment:
      DATABASE_URL: postgres://localhost:5432/mydb
```

---

## Next Steps

- [Configuration Guide](../../guides/configuration.md) - Add more options
- [web-app Example](../web-app/) - Full-stack application
