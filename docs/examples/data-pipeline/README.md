# Data Pipeline Example

> ETL pipeline with Kafka streaming, batch processing, and data warehouse.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              DATA SOURCES                                    │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐      │
│  │   API    │  │  Files   │  │  Events  │  │ Webhooks │  │  Scrapers│      │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘      │
└───────┼─────────────┼─────────────┼─────────────┼─────────────┼─────────────┘
        │             │             │             │             │
        └─────────────┴─────────────┴─────────────┴─────────────┘
                                    │
                           ┌────────▼────────┐
                           │     Kafka       │
                           │   (Streaming)   │
                           │     :9092       │
                           └────────┬────────┘
                                    │
              ┌─────────────────────┼─────────────────────┐
              │                     │                     │
       ┌──────▼──────┐       ┌──────▼──────┐       ┌──────▼──────┐
       │   Ingest    │       │  Transform  │       │   Enrich    │
       │   Worker    │       │   Worker    │       │   Worker    │
       └──────┬──────┘       └──────┬──────┘       └──────┬──────┘
              │                     │                     │
              └─────────────────────┴─────────────────────┘
                                    │
                           ┌────────▼────────┐
                           │   ClickHouse    │
                           │  (Data Store)   │
                           │     :8123       │
                           └────────┬────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    │                               │
             ┌──────▼──────┐                 ┌──────▼──────┐
             │  Dashboard  │                 │     API     │
             │  (Grafana)  │                 │  (Query)    │
             │    :3000    │                 │    :8080    │
             └─────────────┘                 └─────────────┘
```

---

## Resources

| Resource | Port | Description |
|----------|------|-------------|
| zookeeper | 2181 | Kafka coordination |
| kafka | 9092 | Message streaming |
| kafka-ui | 8081 | Kafka management UI |
| clickhouse | 8123 | OLAP data warehouse |
| redis | 6379 | Job queue & cache |
| ingest-worker | - | Data ingestion |
| transform-worker | - | Data transformation |
| enrich-worker | - | Data enrichment |
| api | 8080 | Query API |
| grafana | 3000 | Dashboards |
| scheduler | - | Batch job scheduler |

---

## Data Flow

```
1. INGEST
   - Consume from external APIs
   - Read files from S3/filesystem
   - Receive webhooks
   - → Publish to Kafka topic: raw-events

2. TRANSFORM
   - Consume from: raw-events
   - Parse, validate, normalize
   - → Publish to Kafka topic: transformed-events

3. ENRICH
   - Consume from: transformed-events
   - Add metadata, geo-lookup, classifications
   - → Insert into ClickHouse

4. QUERY
   - API serves queries against ClickHouse
   - Grafana visualizes metrics
```

---

## Quick Start

```bash
# Copy example
cp -r docs/v2/examples/data-pipeline my-pipeline
cd my-pipeline

# Start infrastructure
dtx start zookeeper kafka clickhouse redis

# Wait for Kafka to be ready, then start workers
dtx start

# View Kafka UI
open http://localhost:8081

# View Grafana
open http://localhost:3000
```

---

## Files

```
data-pipeline/
├── README.md
├── workers/
│   ├── ingest/
│   ├── transform/
│   └── enrich/
├── api/
├── grafana/
│   └── dashboards/
└── .dtx/
    └── config.yaml
```

---

## Next Steps

- [Configuration Guide](../../guides/configuration.md)
- [saas-platform Example](../saas-platform/)
- [microservices-gateway Example](../microservices-gateway/)
