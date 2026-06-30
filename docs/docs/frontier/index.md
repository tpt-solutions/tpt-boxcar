---
sidebar_position: 1
title: TPT Frontier
---

# TPT Frontier — Wasm-Native Edge Service Mesh

Ultra-lightweight API gateway with a Wasm plugin system — route, secure, and extend traffic at the edge.

## Features

- **High performance** — Rust + Hyper, targets >100k req/s on single core
- **Wasm plugins** — hot-load custom logic without restarting
- **Multi-protocol** — HTTP/1.1, HTTP/2, TLS with SNI
- **Load balancing** — round-robin, least-connections, consistent hashing
- **xDS compatible** — integrates with Istio and Consul

## Quick Start

```bash
# Start Frontier
frontier serve --config frontier.yaml

# Load a plugin
frontier plugin load ./jwt-validator.wasm
```

## Configuration

```yaml
# frontier.yaml
listeners:
  - addr: "0.0.0.0:8080"
    protocol: http2
    tls:
      cert: /etc/frontier/cert.pem
      key: /etc/frontier/key.pem

routes:
  - match:
      path_prefix: /api
    upstream: api-backend
    plugins:
      - jwt-validator
      - rate-limiter

upstreams:
  api-backend:
    endpoints:
      - host: api-1.internal
        port: 8080
      - host: api-2.internal
        port: 8080
    load_balancer: round-robin
```

## Writing Plugins

Plugins are Wasm modules that implement the Frontier host ABI:

```rust
// Rust plugin example
#[no_mangle]
pub extern "C" fn on_request(req_ptr: *const u8, req_len: u32) -> u32 {
    // Modify request, return filter result
    0 // ALLOW
}
```
