# SaaS Platform Example

> Production-like SaaS with reverse proxy, S3 storage, image processing, and multi-stage initialization.

---

## Architecture

```
                              ┌─────────────────────────────────────────┐
                              │           Caddy (Reverse Proxy)         │
                              │    vault.localhost → :3001              │
                              │    app.localhost   → :3002              │
                              │    s3.localhost    → :3900              │
                              │    mail.localhost  → :8025              │
                              └──────────────────┬──────────────────────┘
                                                 │
        ┌────────────────────────────────────────┼────────────────────────────────────────┐
        │                    │                   │                   │                    │
   ┌────▼────┐         ┌─────▼─────┐       ┌─────▼─────┐       ┌─────▼─────┐       ┌──────▼──────┐
   │  Vault  │         │    App    │       │  Garage   │       │ Imgproxy  │       │   Mailpit   │
   │ (Auth)  │         │   (API)   │       │   (S3)    │       │  (Image)  │       │   (SMTP)    │
   │  :3001  │         │   :3002   │       │   :3900   │       │   :8080   │       │    :8025    │
   └────┬────┘         └─────┬─────┘       └─────┬─────┘       └───────────┘       └─────────────┘
        │                    │                   │
        │         ┌──────────┴──────────┐        │
        │         │                     │        │
   ┌────▼─────────▼────┐         ┌──────▼────────▼──────┐
   │     PostgreSQL    │         │        Redis         │
   │      (Database)   │         │       (Cache)        │
   │       :5432       │         │        :6379         │
   └───────────────────┘         └──────────────────────┘
```

---

## Initialization Sequence

```
node_modules (bun install)
        │
        ├──────────────────┬─────────────────────────────────────┐
        ▼                  ▼                                     ▼
   postgres            garage                                  caddy
        │                  │
        ▼                  ▼
 vault-bootstrap     garage-init
        │                  │
        ▼                  ▼
    pg-setup           imgproxy
        │
        ▼
      vault
        │
        ▼
       app
        │
        ▼
     watcher
```

---

## Resources

| Resource | Port | Description |
|----------|------|-------------|
| caddy | 443 | TLS reverse proxy |
| postgres | 5432 | PostgreSQL database |
| redis | 6379 | Redis cache |
| garage | 3900 | S3-compatible object storage |
| imgproxy | 8080 | Image processing service |
| mailpit | 8025 | Email testing server |
| vault | 3001 | Authentication service |
| app | 3002 | Main application API |

---

## Key Features Demonstrated

1. **Reverse Proxy with TLS** - Caddy with multiple vhosts
2. **S3-Compatible Storage** - Garage with bucket initialization
3. **Multi-Stage Initialization** - Sequential bootstrap chain
4. **File Watchers** - Auto-restart on config changes
5. **Health Check Variety** - HTTP, TCP, exec probes
6. **Dependency Conditions** - healthy, completed, started

---

## Quick Start

```bash
# Copy example
cp -r docs/v2/examples/saas-platform my-saas
cd my-saas

# Generate TLS certificates (requires mkcert)
mkdir -p data/certs
mkcert -key-file data/certs/key.pem -cert-file data/certs/cert.pem \
  "*.localhost" "localhost"

# Start all services
dtx start

# View status
dtx status

# View logs
dtx logs -f
```

---

## Files

```
saas-platform/
├── README.md               # This file
├── data/                   # Runtime data (gitignored)
│   ├── certs/              # TLS certificates
│   ├── postgres/           # PostgreSQL data
│   ├── garage/             # S3 storage data
│   └── caddy/              # Caddy data
└── .dtx/
    └── config.yaml         # All configuration
```

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| SSL_CERT_PATH | Path to TLS certificate |
| SSL_KEY_PATH | Path to TLS private key |
| ROOT_REPO | Project root directory |

---

## Endpoints

After startup:

| URL | Description |
|-----|-------------|
| https://app.localhost | Main application |
| https://vault.localhost | Authentication |
| https://s3.localhost | S3 API |
| https://mail.localhost | Email viewer |
| https://admin.garage.localhost | Garage admin |

---

## Customization

### Adding a New Service

```yaml
resources:
  my-service:
    kind: process
    command: ./my-service
    port: 4000
    depends_on:
      - postgres: healthy
      - redis: started
    environment:
      DATABASE_URL: postgres://app:dev@localhost:5432/mydb
```

### Adding Caddy Route

Add to the Caddy vhosts section in the config to route a new subdomain.

---

## Next Steps

- [Configuration Guide](../../guides/configuration.md) - Full configuration reference
- [microservices-gateway Example](../microservices-gateway/) - API gateway pattern
- [data-pipeline Example](../data-pipeline/) - ETL and streaming
