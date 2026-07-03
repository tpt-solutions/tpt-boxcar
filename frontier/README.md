# TPT Frontier

Wasm-native edge service mesh & API gateway: hot-reloadable Wasm plugins, routes/upstreams managed through a Go control plane exposed over both REST and gRPC.

## Quickstart (5 minutes)

Start the control plane:

```bash
go run ./frontier/control-plane
# TPT Frontier Control Plane listening on :8090 (HTTP) / :8091 (gRPC)
```

Register an upstream and a route using the bundled examples:

```bash
curl -X POST localhost:8090/api/v1/upstreams \
  -H 'Content-Type: application/json' \
  -d @frontier/examples/getting-started/upstream.json

curl -X POST localhost:8090/api/v1/routes \
  -H 'Content-Type: application/json' \
  -d @frontier/examples/getting-started/route.json

curl localhost:8090/api/v1/routes
curl localhost:8090/api/v1/upstreams
```

Register a plugin the same way:

```bash
curl -X POST localhost:8090/api/v1/plugins \
  -H 'Content-Type: application/json' \
  -d @frontier/examples/getting-started/plugin.json
```

Set `FRONTIER_API_KEY` to require an `X-API-Key` header on requests (dev mode allows unauthenticated requests when unset).

Alternatively, [`examples/demo-stack/manifest.yaml`](../examples/demo-stack/manifest.yaml) at the repo root starts this control plane (alongside Scope's ingest/backend) with a single `tpt origin up` — see the root README.

## Data plane

`frontier/proxy` (Rust, `tpt-frontier-proxy`) is the Hyper-based listener with the Wasmtime plugin loader and hot-reload logic — it is currently a library crate with no standalone binary (`[[bin]]` is not defined in `frontier/proxy/Cargo.toml`). Wiring it to watch the control plane's `WatchConfig` gRPC stream and run as a standalone data-plane process is tracked follow-up work.

Two plugin SDKs exist for authoring Wasm plugins: Rust (`frontier/plugin-sdk/`) and TypeScript (`frontier/typescript-sdk/`), with example plugins in `frontier/examples/` (`custom_auth.ts`, `jwt_validator.rs`, `rate_limiter.ts`).

## Current limitations

- REST covers Route/Upstream/Plugin/TLS-cert CRUD; the gRPC surface (`WatchConfig` + CRUD) only covers Route/Upstream.
- The proxy data plane isn't yet a runnable standalone binary — see above.
- `frontier/examples/jwt_validator.rs` has an unimplemented JWT signature check (tracked in root `TODO.md`/`CLAUDE.md`).
