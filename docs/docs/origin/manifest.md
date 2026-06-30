---
sidebar_position: 2
title: Manifest Reference
---

# Manifest Reference

The `manifest.yaml` file defines your local environment.

## Top-Level Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Environment name |
| `version` | string | No | Semantic version |
| `services` | map | Yes | Service definitions |
| `networks` | map | No | Network configurations |
| `volumes` | map | No | Persistent volume definitions |

## Service Types

### OCI Container

```yaml
services:
  db:
    type: oci
    image: postgres:16
    ports:
      - host: 5432
        container: 5432
    environment:
      POSTGRES_PASSWORD: changeme
    volumes:
      - source: pgdata
        target: /var/lib/postgresql/data
    healthcheck:
      command: ["pg_isready"]
      interval_secs: 10
      timeout_secs: 5
      retries: 5
    resources:
      cpu: "1.0"
      memory: "512m"
```

### Wasm Module

```yaml
services:
  api:
    type: wasm
    path: ./target/api.wasm
    args: ["--port", "8080"]
    environment:
      DB_HOST: db
    memory_limit: "256m"
    depends_on:
      - db
```
