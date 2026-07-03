# TPT Tether

State & connection proxy exposing WASI Preview 2 / WIT interfaces to Wasm callers, with hand-written wire-protocol drivers (Postgres, MySQL, Redis) — no sqlx or redis-rs.

## Quickstart (5 minutes)

Start the control plane (Go), which holds the backend/route configuration in memory and serves a REST API:

```bash
go run ./tether/control-plane
# TPT Tether Control Plane listening on :8080
```

Register a backend and a route using the bundled examples:

```bash
curl -X POST localhost:8080/api/v1/backends \
  -d @tether/examples/postgres-backend.json

curl -X POST localhost:8080/api/v1/routes \
  -d @tether/examples/postgres-route.json

curl localhost:8080/api/v1/backends
curl localhost:8080/api/v1/routes
```

Set `TETHER_API_KEY` before starting the control plane to require an `X-API-Key` header on requests (dev mode allows unauthenticated requests when unset).

## Data plane

`tether/proxy` (Rust, `tpt-tether-proxy`) is the library that actually terminates client connections and speaks the Postgres/MySQL/Redis wire protocols — it implements the `WireDriver` trait (`wire.rs`) and is driven through a WASI Preview 2 / WIT interface (`wit.rs`) by a Wasm host. It is a library, not a standalone binary: it's meant to be embedded by a runtime such as **TPT Origin**, not run directly. There is currently no standalone `tether-proxy` executable — that's the next integration point once Origin can drive Wasm services that hold pool connections.

## Current limitations

- The control plane stores backends/routes in memory only — restarting it loses all configuration.
- No CLI yet; the REST API + `curl` is the current interface.
