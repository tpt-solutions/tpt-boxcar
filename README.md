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
| [**TPT Chisel**](./chisel) | Automated image distiller & Wasm migrator | Rust, AI/LLM analysis (Ollama, Claude, OpenAI) |
| [**TPT Frontier**](./frontier) | Wasm-native edge service mesh & API gateway | Rust (Hyper, Tokio), Wasmtime, Go xDS |

## Quick Start

```bash
git clone https://github.com/tpt-solutions/tpt-boxcar.git && cd tpt-boxcar
cargo build --workspace   # Origin, Chisel, Tether/Frontier data-plane libs
go build ./...            # Tether, Scope, Frontier control planes
```

That builds everything, but doesn't run anything yet. Pick a product below and follow its 5-minute quickstart — each is independently runnable:

| Product | Try it in 5 minutes | What it demos |
|---------|----------------------|----------------|
| [**Origin**](origin/README.md) | `cargo build -p tpt-origin && tpt origin init --dir demo && cd demo && tpt origin up` | Spin up a local sandbox mixing OCI containers and Wasm modules from one manifest |
| [**Chisel**](chisel/README.md) | `cargo build -p tpt-chisel && chisel analyze <image-dir> && chisel distill <image-dir> --dockerfile` | Distill a container image into a minimal, Wasm-migration-aware image with SBOM + CVE scan; `chisel migrate`/`chisel audit` add LLM-generated migration/security plans |
| [**Frontier**](frontier/README.md) | `go run ./frontier/control-plane` then `curl` in a route/upstream from `frontier/examples/getting-started/` | Configure an edge gateway route through REST/gRPC |
| [**Tether**](tether/README.md) | `go run ./tether/control-plane` then `curl` in a backend/route from `tether/examples/` | Register a DB backend + route for the Wasm-facing connection proxy |
| [**Scope**](scope/README.md) | `go run ./scope/backend/cmd/ingest` + `cmd/backend`, then send an OTLP trace | Query traces/metrics/logs ingested from a running service |

We deliberately don't ship a Docker/docker-compose quickstart — Origin's whole point is replacing container tooling with containerd + Wasmtime directly, so bootstrapping the demo through Docker would undercut the pitch. Instead, [`examples/demo-stack/manifest.yaml`](examples/demo-stack/manifest.yaml) brings up Scope's ingest/query API and Frontier's control plane together as real child processes through Origin itself:

```bash
cargo build -p tpt-origin
# requires a ClickHouse server at 127.0.0.1:9000 — Origin's OCI runtime doesn't spawn one yet (see origin/README.md)
./target/debug/tpt origin up --manifest examples/demo-stack/manifest.yaml
```

This works today because Origin's `type: process` service kind spawns and tears down real OS processes (unlike `type: oci`/`type: wasm`, which are still bookkeeping-only pending containerd/Wasmtime integration).

## Common Use Cases

Five concrete problems TPT Boxcar solves today — each links to a full walkthrough with a real, runnable example already in this repo:

| Use case | Product | Try it |
|----------|---------|--------|
| [Local dev sandbox without a Docker daemon](docs/docs/use-cases/local-dev-sandbox.md) | Origin | `tpt origin init --dir demo && cd demo && tpt origin up` |
| [DB proxy for Wasm workloads](docs/docs/use-cases/db-proxy.md) | Tether | `go run ./tether/control-plane`, then register `tether/examples/postgres-backend.json` |
| [Zero-code observability](docs/docs/use-cases/zero-code-observability.md) | Scope | `go run ./scope/backend/cmd/ingest` + `cmd/backend`, then `./scope/examples/send-demo-trace.sh` |
| [Shrink an image and get a Wasm migration plan](docs/docs/use-cases/image-distillation.md) | Chisel | `chisel analyze <image-dir> && chisel distill <image-dir> --dockerfile` |
| [Hot-reloadable Wasm plugin gateway](docs/docs/use-cases/plugin-gateway.md) | Frontier | `go run ./frontier/control-plane`, then register `frontier/examples/getting-started/plugin.json` |

## AI agent integration (MCP)

[`mcp-server/`](mcp-server/README.md) exposes all five products as MCP tools over stdio, so an AI agent (Claude Code, Claude Desktop, etc.) can drive them directly — start a sandbox, distill an image, configure a gateway route, query traces — instead of a human running curl/CLI commands by hand.

```bash
cd mcp-server && go build -o mcp-server .
```

Then point your MCP client's config at the built binary. See [mcp-server/README.md](mcp-server/README.md) for the full tool list and configuration options.

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
│   ├── core/        #   Rust core engine (analysis, distillation, AI orchestration)
│   └── cli/         #   CLI binary
├── frontier/        # TPT Frontier — edge service mesh
│   ├── proxy/       #   Rust data plane
│   └── plugin-sdk/  #   Plugin SDK crate
├── mcp-server/      # MCP server exposing all 5 products as AI agent tools
├── examples/        # Cross-product examples (e.g. demo-stack/ for Origin-driven demos)
└── docs/            # Documentation site (Docusaurus)
```

## License

Apache 2.0 — see [LICENSE](./LICENSE).
