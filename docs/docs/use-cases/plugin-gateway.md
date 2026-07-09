---
sidebar_position: 6
title: Hot-reloadable Wasm plugin gateway
---

# Hot-reloadable Wasm plugin gateway

**Problem:** you need an API gateway with custom auth or rate-limiting logic,
and you don't want to redeploy (and drop connections) every time that logic
changes.

**How:** Frontier routes HTTP traffic through Wasmtime-hosted Wasm plugins
that hot-swap without dropping connections. Routes, upstreams, and plugins
are all managed through the Go control plane's REST/gRPC API.

```bash
go run ./frontier/control-plane
# TPT Frontier Control Plane listening on :8090 (HTTP) / :8091 (gRPC)
```

Register an upstream and a route using the bundled examples
(`frontier/examples/getting-started/`):

```bash
curl -X POST localhost:8090/api/v1/upstreams \
  -H 'Content-Type: application/json' \
  -d @frontier/examples/getting-started/upstream.json

curl -X POST localhost:8090/api/v1/routes \
  -H 'Content-Type: application/json' \
  -d @frontier/examples/getting-started/route.json
```

Attach a rate-limiting plugin the same way:

```bash
curl -X POST localhost:8090/api/v1/plugins \
  -H 'Content-Type: application/json' \
  -d @frontier/examples/getting-started/plugin.json
```

```json title="plugin.json"
{
  "name": "rate-limiter",
  "type": "wasm",
  "config": {
    "module_path": "./frontier/examples/rate_limiter.ts",
    "requests_per_minute": "300"
  }
}
```

**What you should see:** `curl localhost:8090/api/v1/routes` and
`.../upstreams` echo back your config. Updating the plugin config and
re-`POST`ing hot-swaps it — see `frontier/proxy` for the reload path.

Other example plugins to try: `frontier/examples/custom_auth.ts` (custom auth
logic in TypeScript) — `jwt_validator.rs` currently has an unimplemented
signature check, tracked in the root `TODO.md`.

Set `FRONTIER_API_KEY` to require an `X-API-Key` header (dev mode allows
unauthenticated requests when unset).

**Where to go next:** [frontier/README.md](https://github.com/tpt-boxcar/tpt-boxcar/blob/main/frontier/README.md)
for the plugin SDKs (Rust and TypeScript) and current gRPC/REST coverage
differences.
