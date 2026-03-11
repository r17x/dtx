# Microservices Gateway Example

> API gateway pattern with multiple backend services, rate limiting, and service discovery.

---

## Architecture

```
                   ┌─────────────────────────────────────┐
                   │           API Gateway               │
                   │         (Kong/Traefik)              │
                   │            :8000                    │
                   └──────────────┬──────────────────────┘
                                  │
        ┌─────────────────────────┼─────────────────────────┐
        │           │             │             │           │
   ┌────▼────┐ ┌────▼────┐ ┌──────▼──────┐ ┌────▼────┐ ┌────▼────┐
   │  Users  │ │ Orders  │ │  Products   │ │  Auth   │ │ Search  │
   │ Service │ │ Service │ │   Service   │ │ Service │ │ Service │
   │  :3001  │ │  :3002  │ │    :3003    │ │  :3004  │ │  :3005  │
   └────┬────┘ └────┬────┘ └──────┬──────┘ └────┬────┘ └────┬────┘
        │           │             │             │           │
        └───────────┴─────────────┴─────────────┴───────────┘
                                  │
                    ┌─────────────┼─────────────┐
                    │             │             │
              ┌─────▼─────┐ ┌─────▼─────┐ ┌─────▼─────┐
              │ PostgreSQL│ │   Redis   │ │Elasticsearch
              │  (Users)  │ │ (Session) │ │ (Search)  │
              │   :5432   │ │   :6379   │ │   :9200   │
              └───────────┘ └───────────┘ └───────────┘
```

---

## Resources

| Resource | Port | Description |
|----------|------|-------------|
| gateway | 8000 | API Gateway (Kong) |
| users-svc | 3001 | User management |
| orders-svc | 3002 | Order processing |
| products-svc | 3003 | Product catalog |
| auth-svc | 3004 | Authentication/JWT |
| search-svc | 3005 | Search service |
| postgres | 5432 | Primary database |
| redis | 6379 | Session/cache |
| elasticsearch | 9200 | Search engine |

---

## Service Communication

```
Gateway Routes:
  /api/users/*     → users-svc:3001
  /api/orders/*    → orders-svc:3002
  /api/products/*  → products-svc:3003
  /api/auth/*      → auth-svc:3004
  /api/search/*    → search-svc:3005

Inter-service:
  orders-svc    → users-svc (validate user)
  orders-svc    → products-svc (check inventory)
  search-svc    → products-svc (index products)
```

---

## Key Features Demonstrated

1. **API Gateway Pattern** - Centralized routing and rate limiting
2. **Service Mesh** - Inter-service communication
3. **Health Aggregation** - Gateway health depends on all services
4. **Database Per Service** - Schema isolation
5. **Shared Infrastructure** - Redis for sessions, ES for search
6. **Graceful Degradation** - Services can run independently

---

## Quick Start

```bash
# Copy example
cp -r docs/v2/examples/microservices-gateway my-microservices
cd my-microservices

# Start infrastructure first
dtx start postgres redis elasticsearch

# Then start services
dtx start

# Check gateway health
curl http://localhost:8000/health
```

---

## Files

```
microservices-gateway/
├── README.md
├── services/
│   ├── users/
│   ├── orders/
│   ├── products/
│   ├── auth/
│   └── search/
├── gateway/
│   └── kong.yaml       # Gateway configuration
└── .dtx/
    └── config.yaml
```

---

## API Endpoints

| Method | Endpoint | Service | Description |
|--------|----------|---------|-------------|
| GET | /api/users | users-svc | List users |
| POST | /api/auth/login | auth-svc | Authenticate |
| GET | /api/products | products-svc | List products |
| POST | /api/orders | orders-svc | Create order |
| GET | /api/search?q= | search-svc | Search products |

---

## Rate Limiting

Gateway applies rate limits per route:

| Route | Limit | Window |
|-------|-------|--------|
| /api/auth/* | 10 req | 1 min |
| /api/search/* | 100 req | 1 min |
| /api/* (default) | 1000 req | 1 min |

---

## Adding a New Service

1. Create service directory:
```bash
mkdir -p services/notifications
```

2. Add to config.yaml:
```yaml
resources:
  notifications-svc:
    kind: process
    command: node services/notifications/index.js
    port: 3006
    depends_on:
      - redis: healthy
    environment:
      PORT: "3006"
      REDIS_URL: redis://localhost:6379
    health:
      http:
        path: /health
        port: 3006
```

3. Add gateway route:
```yaml
# In gateway/kong.yaml
services:
  - name: notifications
    url: http://localhost:3006
    routes:
      - paths: ["/api/notifications"]
```

---

## Next Steps

- [Configuration Guide](../../guides/configuration.md) - Full reference
- [saas-platform Example](../saas-platform/) - Production SaaS setup
- [data-pipeline Example](../data-pipeline/) - ETL and streaming
