# tpt-tether-proxy

[![Crates.io](https://img.shields.io/crates/v/tpt-tether-proxy.svg)](https://crates.io/crates/tpt-tether-proxy)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../../LICENSE)

Data-plane library for **TPT Tether** — a state and connection proxy that exposes WASI Preview 2 / WIT interfaces to Wasm modules, backed by hand-written wire-protocol drivers for Postgres, MySQL, and Redis.

## What it does

- **WASI Preview 2 host** — Wasm callers use standard WIT interfaces; Tether handles the actual I/O
- **Custom wire drivers** — no sqlx or redis-rs; all protocol framing is hand-written for correctness and minimal overhead
  - PostgreSQL (binary resultsets, affected-row counts, pipelining)
  - MySQL (binary protocol, auth handshake)
  - Redis (RESP2/RESP3 inline + bulk)
- **Connection pooling** — reusable `WireDriver` trait with a shared pool
- **TLS** — rustls-backed encrypted connections to backends
- **Routing** — routes incoming Wasm calls to the correct backend pool

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
tpt-tether-proxy = "0.1"
```

The control plane (Go, in `tether/control-plane/`) writes JSON config that the proxy reads at startup and on reload. For embedding, construct a `ProxyConfig` directly and call `TetherProxy::new(config).await?.run().await`.

## Part of TPT Boxcar

See the [monorepo README](../../README.md) for the full product suite.

## License

Apache-2.0
