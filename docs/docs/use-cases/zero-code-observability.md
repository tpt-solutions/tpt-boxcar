---
sidebar_position: 4
title: Zero-code observability
---

# Zero-code observability

**Problem:** you want traces/metrics/logs out of a service without adding
tracing SDK boilerplate, and without standing up a full observability
platform just to look at a handful of spans.

**How:** Scope's ingest pipeline speaks OTLP (gRPC and HTTP, protobuf or
JSON) straight into ClickHouse; a REST API queries it back out. Point any
existing OpenTelemetry exporter at it — no code changes to the service being
observed beyond what you'd already configure for OTel.

You need a ClickHouse instance reachable at `localhost:9000` (native
protocol).

```bash
export CLICKHOUSE_PASS=changeme

# Terminal 1 — OTLP ingest (gRPC :4317, HTTP :4318)
go run ./scope/backend/cmd/ingest

# Terminal 2 — query API (:8081)
go run ./scope/backend/cmd/backend
```

Send a sample trace with the bundled script
(`scope/examples/send-demo-trace.sh`, OTLP/HTTP JSON to `:4318/v1/traces`):

```bash
./scope/examples/send-demo-trace.sh
```

Then query it back:

```bash
curl localhost:8081/api/v1/services   # dependency graph
curl localhost:8081/api/v1/traces
```

**What you should see:** `/api/v1/traces` returns the `demo-service` span you
just sent; `/api/v1/services` shows it in the dependency graph. Both accept
`?since=<Go duration>` and `?service=<name>` to filter.

**Where to go next:** [scope/README.md](https://github.com/tpt-boxcar/tpt-boxcar/blob/main/scope/README.md)
for the React dashboard (`cd scope/dashboard && npm run dev`) and
`SCOPE_API_KEY` to lock down the query API.
