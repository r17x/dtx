# Web App Example

> Full-stack web application with database, cache, API, and frontend.

---

## Architecture

```
┌─────────────┐     ┌─────────────┐
│   Frontend  │────►│     API     │
│  (Vite/React)    │  (Node.js)  │
└─────────────┘     └──────┬──────┘
                          │
             ┌────────────┼────────────┐
             │            │            │
       ┌─────▼─────┐ ┌────▼────┐ ┌─────▼─────┐
       │  Postgres │ │  Redis  │ │  Worker   │
       │   (DB)    │ │ (Cache) │ │  (Jobs)   │
       └───────────┘ └─────────┘ └───────────┘
```

---

## Resources

| Resource | Port | Description |
|----------|------|-------------|
| postgres | 5432 | PostgreSQL database |
| redis | 6379 | Redis cache |
| api | 3000 | Backend API server |
| worker | - | Background job processor |
| frontend | 5173 | Frontend dev server |

---

## Dependencies

```
postgres ◄─── api ◄─── frontend
              │
redis ◄───────┤
              │
        ◄─── worker
```

---

## Quick Start

```bash
# Copy to your project
cp -r docs/v2/examples/web-app my-project
cd my-project

# Start all resources
dtx start

# View status
dtx status

# View logs
dtx logs -f
```

---

## Files

```
web-app/
├── README.md               # This file
└── .dtx/
    └── config.yaml         # All configuration
```
