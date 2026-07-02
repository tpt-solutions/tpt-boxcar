# TPT Boxcar

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

An open-source, unified cloud-native tooling suite — from local development to the edge.

## Overview

TPT Boxcar provides five integrated products that cover the full application lifecycle:

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
git clone https://github.com/tpt-solutions/tpt-boxcar.git && cd tpt-boxcar

# 2. Build the Rust workspace
cargo build --workspace

# 3. Build the Go components
go build ./...
```

## Products

| Product | Description | Docs |
|---------|-------------|------|
| **Origin** | Unified local sandbox — OCI containers + Wasm side by side | [origin/README.md](origin/README.md) |
| **Tether** | State & connection proxy for Wasm workloads | [tether/README.md](tether/README.md) |
| **Scope** | Hybrid eBPF observability — zero-code distributed tracing | [scope/README.md](scope/README.md) |
| **Chisel** | Automated image distiller & Wasm migrator | [chisel/README.md](chisel/README.md) |
| **Frontier** | Wasm-native edge service mesh & API gateway | [frontier/README.md](frontier/README.md) |

## Monorepo Structure

```
tpt-boxcar/
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
