---
sidebar_position: 1
title: TPT Tether
---

# TPT Tether — State & Connection Proxy

The "Missing Link" for Wasm — handles connection pooling, TLS, and auth between stateless Wasm modules and databases.

## How It Works

Tether sits between your Wasm workloads and backend databases. Thousands of Wasm instances share a small pool of persistent database connections.

```
[Wasm Module] → [Tether Proxy] → [Postgres/MySQL/Redis]
```

## Configuration

```yaml
# tether.yaml
backends:
  - id: main-db
    type: postgres
    host: db.internal
    port: 5432
    database: myapp
    pool:
      min: 5
      max: 50
      idle_timeout: 300
      acquire_timeout: 10

  - id: cache
    type: redis
    host: cache.internal
    port: 6379

tls:
  enabled: true
  cert: /etc/tether/cert.pem
  key: /etc/tether/key.pem
  ca: /etc/tether/ca.pem
  mtls: true
```

## API

```bash
# Start the proxy
tether serve --config tether.yaml

# Health check
curl http://localhost:8080/health
```
