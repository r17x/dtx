# dtx v2 Examples

> Complete examples demonstrating dtx features.

---

## Examples

| Example | Description | Complexity |
|---------|-------------|------------|
| [simple-api](./simple-api/) | Minimal Node.js API with health check | Beginner |
| [python-worker](./python-worker/) | Background job processor with Redis | Intermediate |
| [web-app](./web-app/) | Full-stack with Postgres, Redis, API, worker, frontend | Advanced |
| [saas-platform](./saas-platform/) | Production SaaS with reverse proxy, S3, multi-stage init | Expert |
| [microservices-gateway](./microservices-gateway/) | API gateway with multiple backend services | Expert |
| [data-pipeline](./data-pipeline/) | ETL pipeline with Kafka streaming and ClickHouse | Expert |

---

## Quick Start

```bash
# Clone or copy an example
cp -r docs/v2/examples/simple-api my-project
cd my-project

# Initialize dtx
dtx init my-project

# Start all resources
dtx start
```

---

## Example Structure

Each example contains:

```
example/
├── README.md               # Description and usage
└── .dtx/
    └── config.yaml         # All configuration (resources + settings)
```

---

## Choosing an Example

### Beginner: simple-api
Best for: Learning dtx basics
- Single service
- Health check
- Environment variables

### Intermediate: python-worker
Best for: Background processing
- Redis dependency
- Worker process
- Nix integration

### Advanced: web-app
Best for: Full applications
- Multiple services
- Database + cache
- Frontend + API + worker
- Complete dependency graph

### Expert: saas-platform
Best for: Production-like environments
- Reverse proxy (Caddy) with TLS
- S3-compatible storage (Garage)
- Multi-stage initialization chains
- File watchers for hot reload
- Image processing (imgproxy)
- Email testing (mailpit)

### Expert: microservices-gateway
Best for: Microservices architecture
- API gateway pattern (Kong)
- Multiple backend services
- Service-to-service communication
- Elasticsearch for search
- Database per service pattern
- Background workers

### Expert: data-pipeline
Best for: Data engineering
- Kafka streaming
- Multi-stage ETL workers
- ClickHouse OLAP storage
- Batch job scheduling
- Dead letter queue handling
- Grafana dashboards

---

## Feature Matrix

| Feature | simple-api | python-worker | web-app | saas-platform | microservices | data-pipeline |
|---------|:----------:|:-------------:|:-------:|:-------------:|:-------------:|:-------------:|
| Health checks | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| Dependencies | - | ✓ | ✓ | ✓ | ✓ | ✓ |
| PostgreSQL | - | - | ✓ | ✓ | ✓ | - |
| Redis | - | ✓ | ✓ | ✓ | ✓ | ✓ |
| Reverse proxy | - | - | - | ✓ | ✓ | - |
| S3 storage | - | - | - | ✓ | - | - |
| Kafka | - | - | - | - | - | ✓ |
| Elasticsearch | - | - | - | - | ✓ | - |
| ClickHouse | - | - | - | - | - | ✓ |
| Oneshot jobs | - | - | - | ✓ | ✓ | ✓ |
| File watchers | - | - | - | ✓ | - | - |
| Multi-service | - | ✓ | ✓ | ✓ | ✓ | ✓ |

---

## Contributing Examples

Add new examples by:

1. Create directory: `docs/v2/examples/my-example/`
2. Add `README.md` with description
3. Add `.dtx/config.yaml` with all configuration (resources + settings)
4. Update this README with link
