# tpt-frontier-proxy

[![Crates.io](https://img.shields.io/crates/v/tpt-frontier-proxy.svg)](https://crates.io/crates/tpt-frontier-proxy)
[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](../../LICENSE)

Data-plane library for **TPT Frontier** — a Wasm-native edge service mesh and API gateway with zero-downtime hot-reloadable Wasm plugins.

## What it does

- **HTTP/1.1 + HTTP/2** — Hyper-based listener, full duplex, TLS via rustls
- **Wasm plugin execution** — plugins loaded by Wasmtime; hot-swapped without dropping connections
- **Routing** — `Router` and `UpstreamManager` behind `Arc<RwLock<>>` for atomic hot-reload
- **JWT auth** — built-in JWT validation middleware
- **Rate limiting** — token-bucket rate limiter per route
- **OCI pull** — pulls plugin Wasm modules from OCI registries at startup or on reload
- **Config** — reads JSON written atomically by the Go control plane at `/var/run/frontier/config.json`

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
tpt-frontier-proxy = "0.1"
```

The proxy is typically run as a binary driven by the Go control plane. For embedding:

```rust
use tpt_frontier_proxy::{FrontierProxy, ProxyConfig};

let config = ProxyConfig::from_file("/var/run/frontier/config.json").await?;
FrontierProxy::new(config).run().await?;
```

## Plugin authoring

Use the [`tpt-frontier-plugin-sdk`](https://crates.io/crates/tpt-frontier-plugin-sdk) crate to write Wasm plugins in Rust, or the TypeScript SDK in `frontier/typescript-sdk/`.

## Part of TPT Boxcar

See the [monorepo README](../../README.md) for the full product suite.

## License

Apache-2.0
