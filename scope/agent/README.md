# tpt-scope-agent

[![Crates.io](https://img.shields.io/crates/v/tpt-scope-agent.svg)](https://crates.io/crates/tpt-scope-agent)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../../LICENSE)

Host-side observability agent for **TPT Scope** — collects metrics, distributed traces, and (optionally) eBPF kernel events, forwarding everything via OTLP to the Scope ingest pipeline.

## What it does

- **OTLP export** — pushes traces, metrics, and logs to any OTLP-compatible endpoint (Scope ingest, Jaeger, Grafana Tempo, etc.)
- **OpenTelemetry SDK** — wraps `opentelemetry` + `opentelemetry_sdk` with a Tokio runtime
- **eBPF collection** (optional, Linux only) — attaches kernel probes via `scope-ebpf-loader` when the `ebpf` feature is enabled

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
tpt-scope-agent = "0.1"
```

```rust
use tpt_scope_agent::ScopeAgent;

let agent = ScopeAgent::builder()
    .endpoint("http://localhost:4317")
    .service_name("my-service")
    .build()
    .await?;

agent.start().await?;
```

## Feature flags

| Flag | Default | Description |
|------|---------|-------------|
| `ebpf` | off | Enable eBPF kernel event collection (Linux + nightly toolchain required; needs `scope-ebpf-loader`) |

> **Note:** The `ebpf` feature requires the `scope-ebpf-loader` crate, which must be compiled with a nightly Rust toolchain on Linux. It is intentionally off by default.

## Part of TPT Boxcar

See the [monorepo README](../../README.md) for the full product suite.

## License

Apache-2.0
