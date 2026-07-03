# TPT Scope

Hybrid eBPF observability: OTLP traces/metrics/logs ingested straight into ClickHouse, queried through a REST API, visualized in a React dashboard.

## Quickstart (5 minutes)

You need a ClickHouse instance reachable at `localhost:9000` (native protocol). If you don't have one running, the fastest way to get one is `clickhouse-server` installed locally, or any ClickHouse instance you point the flags below at.

```bash
export CLICKHOUSE_PASS=changeme   # required, no default

# Terminal 1 — OTLP ingest (gRPC :4317, HTTP :4318)
go run ./scope/backend/cmd/ingest

# Terminal 2 — query API (:8081)
go run ./scope/backend/cmd/backend
```

Send a sample OTLP trace (via `curl`, `otel-cli`, or any OpenTelemetry SDK pointed at `localhost:4318`), then query it back:

```bash
curl localhost:8081/api/v1/services   # dependency graph, derived from recent trace parent/child spans
curl localhost:8081/api/v1/traces
curl localhost:8081/api/v1/metrics
curl localhost:8081/api/v1/logs
```

`/traces`, `/metrics`, and `/logs` accept `?since=<Go duration>` (default `1h`, e.g. `?since=15m`) and `?service=<name>` to filter to a single service.

Set `SCOPE_API_KEY` to require an `X-API-Key` header on the query API (dev mode allows unauthenticated requests when unset).

Alternatively, [`examples/demo-stack/manifest.yaml`](../examples/demo-stack/manifest.yaml) at the repo root starts `ingest` and `backend` together (alongside Frontier's control plane) with a single `tpt origin up` — see the root README.

## Dashboard

```bash
cd scope/dashboard && npm install && npm run dev
```

The dashboard (`scope/dashboard/src/api/client.ts`) fetches from the real query API above — there is no mock data layer. `getTraces`/`getMetrics`/`getLogs` accept an optional `{ since, service }` filter matching the backend's new query params; no component currently exposes filter controls in the UI yet (they all call the unfiltered default), so wiring up a time-range/service picker in the dashboard is a natural next step.

## Current limitations

- `CLICKHOUSE_PASS` has no default — the ingest/backend binaries will fail to start without it.
- `/api/v1/services` health is a simple heuristic (no logs in the last 5 minutes ⇒ "down", any error-level logs ⇒ "degraded", else "healthy") rather than a real health-check signal.
