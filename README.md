# TPT Cloud-Native

[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

An open-source, unified cloud-native tooling suite — from local development to the edge.

## Overview

TPT Cloud-Native provides five integrated products that cover the full application lifecycle:

| Product | Description | Stack |
|---------|-------------|-------|
| [**TPT Origin**](./origin) | Unified local sandbox — OCI containers + Wasm side by side | Rust, containerd, Wasmtime, eBPF, Tauri |
| [**TPT Tether**](./tether) | State & connection proxy for Wasm workloads | Rust (Tokio), WIT, Go control plane |
| [**TPT Scope**](./scope) | Hybrid eBPF observability — zero-code distributed tracing | Rust/Go eBPF, OTel, ClickHouse, React |
| [**TPT Chisel**](./chisel) | Automated image distiller & Wasm migrator | Go, eBPF, AI/LLM analysis |
| [**TPT Frontier**](./frontier) | Wasm-native edge service mesh & API gateway | Rust (Hyper, Tokio), Wasmtime, Go xDS |

## Quick Start

```bash
# 1. Clone the repo
git clone https://github.com/tpt-solutions/tpt-cloud-native.git && cd tpt-cloud-native

# 2. Build the Rust workspace
cargo build --workspace

# 3. Build frontend dashboards
cd scope/dashboard && npm install && npm run build && cd ../..

# 4. Start Origin with a manifest
cargo run -p tpt-origin -- up -m manifest.yaml

# 5. Start Tether proxy
cargo run -p tpt-tether -- serve --config tether.yaml
```

## Products

| Product | Description |
|---------|-------------|
| **Origin** | Unified local sandbox — OCI containers + Wasm side by side |
| **Tether** | State & connection proxy for Wasm workloads |
| **Scope** | Hybrid eBPF observability — zero-code distributed tracing |
| **Chisel** | Automated image distiller & Wasm migrator |
| **Frontier** | Wasm-native edge service mesh & API gateway |

## Monorepo Structure

```
tpt-cloud-native/
├── origin/          # TPT Origin — local development sandbox
│   ├── core/        #   Rust core engine
│   ├── cli/         #   CLI binary
│   └── gui/         #   Tauri desktop GUI
├── tether/          # TPT Tether — state & connection proxy
│   ├── proxy/       #   Rust data plane (Tokio)
│   └── control-plane/ # Go control plane
├── scope/           # TPT Scope — observability
│   ├── agent/       #   Rust eBPF collection agent
│   ├── backend/     #   Go query API + ClickHouse
│   └── dashboard/   #   React + TypeScript frontend
├── chisel/          # TPT Chisel — image distiller & Wasm migrator
│   └── core/        #   Go core engine
├── frontier/        # TPT Frontier — edge service mesh
│   ├── proxy/       #   Rust data plane
│   └── plugin-sdk/  #   Plugin SDK crate
└── docs/            # Documentation site (Docusaurus)
```

## License

Apache 2.0 — see [LICENSE](./LICENSE).

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md) for branch, commit, and PR conventions.
