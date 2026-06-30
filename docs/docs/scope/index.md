---
sidebar_position: 1
title: TPT Scope
---

# TPT Scope — Hybrid eBPF Observability

Zero-code distributed tracing using eBPF — monitors containers, Wasm modules, and bare-metal binaries without SDK injection.

## Features

- **Kernel-level tracing** — eBPF probes hook into tracepoints
- **Runtime-agnostic** — traces containers, Wasm, and native processes
- **OTel-native** — exports via OpenTelemetry protocol
- **Wasm-specific metrics** — compile time, instantiation latency, memory pages

## Quick Start

```bash
# Start the Scope agent
scope-agent start --config scope.yaml

# View the dashboard
open http://localhost:3000
```

## Dashboard Views

| View | Description |
|------|-------------|
| **Service Graph** | Force-directed dependency graph |
| **Trace Waterfall** | Span timeline for distributed traces |
| **Metrics** | Latency (p50/p95/p99), throughput, error rate |
| **Wasm Metrics** | Compile time, instantiation latency, memory |
| **Logs** | Filterable by service, severity, time range |
