---
sidebar_position: 3
title: DB proxy for Wasm workloads
---

# DB proxy for Wasm workloads

**Problem:** your Wasm modules need to talk to Postgres/MySQL/Redis, but
Wasm's sandboxing model makes raw socket access and long-lived connection
pooling awkward — and you don't want to bake real DB credentials into a guest
that a Wasm plugin author might load.

**How:** Tether terminates the wire protocol on the host side and exposes a
WASI Preview 2 / WIT interface to the guest. Thousands of Wasm instances share
a small pool of real connections; the guest never sees a raw socket or a
credential.

```bash
go run ./tether/control-plane
# TPT Tether Control Plane listening on :8080
```

Register a backend and a route using the bundled examples
(`tether/examples/postgres-backend.json`, `postgres-route.json`):

```bash
curl -X POST localhost:8080/api/v1/backends \
  -d @tether/examples/postgres-backend.json

curl -X POST localhost:8080/api/v1/routes \
  -d @tether/examples/postgres-route.json
```

```json title="postgres-backend.json"
{
  "id": "postgres-primary",
  "type": "postgres",
  "host": "127.0.0.1",
  "port": 5432,
  "database": "app",
  "options": { "sslmode": "disable" }
}
```

```json title="postgres-route.json"
{
  "id": "app-to-postgres",
  "pattern": "app/*",
  "backend_id": "postgres-primary",
  "priority": 100
}
```

**What you should see:** `curl localhost:8080/api/v1/backends` and
`.../routes` echo back what you registered. A Wasm guest built against
`tether/examples/wit-guest-demo/` can now issue queries through the WIT
interface (`tether/proxy/src/wit.rs`) without holding a connection itself.

Set `TETHER_API_KEY` before starting the control plane to require an
`X-API-Key` header (dev mode allows unauthenticated requests when unset).

**Where to go next:** [tether/README.md](https://github.com/tpt-boxcar/tpt-boxcar/blob/main/tether/README.md)
for the `WireDriver` trait and current data-plane limitations (no standalone
`tether-proxy` binary yet — it's a library embedded by a runtime like Origin).
