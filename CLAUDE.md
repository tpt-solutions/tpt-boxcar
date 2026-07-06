# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository Overview

TPT Boxcar is a monorepo of five integrated open-source products covering the full application lifecycle from local dev to edge deployment. The repo uses a Cargo workspace for all Rust crates and a Go workspace (`go.work`) for all Go modules.

**Products:**
- **Origin** — Unified local sandbox (OCI containers + Wasm modules via containerd + Wasmtime, eBPF networking)
- **Tether** — State & connection proxy exposing WASI Preview 2 / WIT interfaces to Wasm callers (custom wire-protocol drivers for Postgres, MySQL, Redis)
- **Scope** — Hybrid eBPF observability: custom OTLP ingest pipeline → ClickHouse, React dashboard
- **Chisel** — Automated image distiller & Wasm migrator with LLM analysis (Ollama + Claude)
- **Frontier** — Wasm-native edge service mesh & API gateway with hot-reloadable Wasm plugins

## Build Commands

### Rust (all crates)
```bash
cargo build --workspace
cargo build --workspace --release
```

### Individual Rust crate
```bash
cargo build -p tpt-origin-core
cargo build -p tpt-tether-proxy
cargo build -p tpt-scope-agent
cargo build -p tpt-chisel-core
cargo build -p tpt-frontier-proxy
cargo build -p tpt-frontier-plugin-sdk
```

### Go control planes
```bash
# From repo root (go.work covers all three)
go build ./tether/control-plane/...
go build ./scope/backend/...
go build ./frontier/control-plane/...
```

### Scope dashboard
```bash
cd scope/dashboard && npm install && npm run build
```

### Origin GUI (Tauri)
```bash
cd origin/gui && npm install && npm run tauri build
```

## Test Commands

### Rust
```bash
# All crates
cargo test --workspace

# Single crate
cargo test -p tpt-origin-core
cargo test -p tpt-frontier-proxy

# Single test by name
cargo test -p tpt-origin-core dns_resolves_service_name

# Integration tests only
cargo test -p tpt-origin-core --test integration_test
cargo test -p tpt-origin-core --test flywheel_test

# Root-level flywheel integration test
cargo test --test flywheel_test
```

### Go
```bash
go test ./tether/control-plane/...
go test ./scope/backend/...
go test ./frontier/control-plane/...
```

### Scope dashboard (unit + e2e)
```bash
cd scope/dashboard && npm test           # vitest
cd scope/dashboard && npm run test:e2e   # Playwright
```

## Lint & Format

### Rust
```bash
cargo clippy --workspace -- -D warnings
cargo fmt --all
cargo fmt --all -- --check   # CI check
```

### Go
```bash
golangci-lint run ./...      # from any Go module dir or repo root
gofmt -w .
```

### TypeScript
```bash
cd scope/dashboard && npm run lint
cd origin/gui && npm run lint
```

## Architecture

### Rust Workspace
`Cargo.toml` at root declares all seven crates as workspace members. Shared dependencies (tokio, serde, anyhow, thiserror, tracing) are defined once as `[workspace.dependencies]` and referenced with `workspace = true` in each crate. Rust edition 2021, stable toolchain, with `wasm32-wasi` target added via `rust-toolchain.toml`.

### Go Workspace
`go.work` at root covers three Go modules: `tether/control-plane`, `scope/backend`, `frontier/control-plane`. Each is independently buildable.

### Cross-Cutting Patterns

**Tether data plane** (`tether/proxy/src/`): custom wire-protocol drivers in `drivers/` implement a `WireDriver` trait (`wire.rs`). The pool (`pool.rs`) holds `Box<dyn WireDriver>`. The WIT/WASI layer (`wit.rs`) delegates to the pool. No sqlx or redis-rs — all protocol framing is hand-written.

**Frontier data plane** (`frontier/proxy/src/`): Hyper-based HTTP/1.1+2 listener, Wasmtime plugin loader with hot-reload (zero dropped connections). `Router` and `UpstreamManager` are `Arc<RwLock<>>` for atomic hot-reload. Config comes from a JSON file at `/var/run/frontier/config.json` written atomically by the Go control plane.

**Frontier control plane** (`frontier/control-plane/`): Protobuf-defined config types (`frontier/proto/frontier/v1/`). In-memory `Store` with monotonic versioning and fan-out pub/sub. Exposes both a gRPC streaming `WatchConfig` API and a REST CRUD API. Syncs from Consul catalog via HTTP blocking queries.

**Scope ingest pipeline** (`scope/backend/`): split into `cmd/ingest` (OTLP gRPC port 4317, HTTP port 4318) and `cmd/backend` (query REST API). Shared `internal/` packages: `schema/` (ClickHouse DDL + record types), `buffer/` (ring buffer + flusher), `transform/` (OTLP → internal records). Uses `clickhouse-go/v2` native protocol.

**Chisel AI layer** (`chisel/core/src/ai/`): unified `LlmProvider` trait with `OllamaProvider`, `ClaudeProvider`, `OpenAiProvider`. `AiOrchestrator` holds a `Vec<Arc<dyn LlmProvider>>` with shared `PromptCache` (SHA-256 keyed, configurable TTL) and `RetryPolicy` (exponential backoff + jitter). Backend toggled via `ai.backend: local | cloud` in `chisel.yaml`.

**Origin core** (`origin/core/src/`): manifest parser (`manifest.rs`, serde-based YAML/TOML), runtime manager (`runtime.rs`, containerd + Wasmtime), eBPF networking via Aya on Linux (`network.rs`), local DNS resolver (`dns.rs`), lifecycle manager (`lifecycle.rs`).

### Plugin System (Frontier)
Wasm plugins expose a host ABI defined in `frontier/plugin-sdk/`. Two SDKs: Rust (`frontier/plugin-sdk/`) and TypeScript (`frontier/typescript-sdk/`). Example plugins live in `frontier/examples/`. Plugins are loaded by Wasmtime in the proxy and can be hot-swapped without dropping connections.

### Key Open Items (Phase 11 — Docker-Replacement Hardening)
Phase 9 (Tether driver parity/correctness) is complete — both wire drivers report real affected-row counts and decode binary resultsets correctly; see `TODO.md` if you need the history. Current work is Phase 11: closing stub/placeholder gaps found across all five products (Origin containerd integration and networking/DNS/resource-limit/health-check hardening, Frontier plugin `ResourceLimiter` enforcement, Chisel real Wasm compilation and SBOM/CVE scanning, Scope real eBPF probes, and a cross-product `boxcar.yaml` manifest). See `TODO.md` for the full task list.
