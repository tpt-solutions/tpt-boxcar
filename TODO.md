# TPT Cloud-Native — Master Task Checklist

> **License:** Apache 2.0 | **Repo type:** Monorepo | **Platform:** Linux · macOS · Windows (WSL2 for eBPF)
>
> **Progress:** 126 / 140 tasks complete (Phase 8 in progress)

---

## Phase 0 — Foundation

- [x] Initialize monorepo directory structure (`/origin`, `/tether`, `/scope`, `/chisel`, `/frontier`, `/docs`)
- [x] Set up Cargo workspace (`Cargo.toml`) covering all Rust crates
- [x] Set up Go workspace (`go.work`) covering all Go modules
- [x] Add `LICENSE` (Apache 2.0)
- [x] Add `CONTRIBUTING.md` with branch, commit, and PR conventions
- [x] Configure Rust toolchain (`rust-toolchain.toml`) with stable + wasm32-wasi target
- [x] Configure Go linting (`golangci-lint`)
- [x] Configure Rust linting (`clippy.toml`, `rustfmt.toml`)
- [x] Add root `README.md` with ecosystem overview and product links

---

## Product 1 — TPT Origin (Unified Local Sandbox)

### Core Engine (Rust)

- [x] Design the manifest format — YAML/TOML schema for mixed OCI + Wasm workloads (services, ports, volumes, env vars)
- [x] Implement manifest parser (serde-based, with validation and helpful error messages)
- [x] Integrate containerd for OCI container lifecycle (pull, create, start, stop, delete) — no Docker daemon dependency
- [x] Integrate Wasmtime for Wasm module execution (load `.wasm` file, configure WASI context, run)
- [x] Implement eBPF-based local networking via Aya (Linux) — virtual network bridge, packet routing between services
- [x] Implement macOS fallback networking (userspace bridge using `tun`/`vmnet` or similar)
- [x] Implement Windows support — WSL2 for eBPF features; native CLI fallback for non-eBPF paths
- [x] Implement local DNS resolver — map `<service-name>.local` to the correct container/Wasm address automatically
- [x] Implement local service mesh — zero-config routing so containers and Wasm modules reach each other by name
- [x] Implement lifecycle management — start, stop, restart, health-check polling, graceful shutdown
- [x] Implement CLI: `tpt origin init` (scaffold manifest)
- [x] Implement CLI: `tpt origin up` (spin up all services)
- [x] Implement CLI: `tpt origin down` (tear down all services)
- [x] Implement CLI: `tpt origin ps` (list running services with status)
- [x] Implement CLI: `tpt origin logs <service>` (stream logs)
- [x] Implement CLI: `tpt origin exec <service> <cmd>` (exec into container or Wasm instance)
- [x] Write unit tests for manifest parser, DNS resolver, lifecycle manager
- [x] Write integration tests — spin up a Postgres container + a Wasm module that connects to it

### Tauri Desktop GUI

- [x] Design UI wireframes — dashboard, logs, manifest editor (lo-fi mockups acceptable)
- [x] Scaffold Tauri project inside `/origin/gui` within the monorepo
- [x] Implement dashboard view — list of running services with name, type (OCI/Wasm), status, CPU, RAM
- [x] Implement log streaming view — per-service live log tail with search/filter
- [x] Implement manifest editor — syntax-highlighted YAML/TOML editor with validation
- [x] Implement one-click start/stop/restart controls per service
- [x] Implement system resource panel — total CPU and RAM consumed by Origin runtime
- [x] Package for Windows (`.msi` / NSIS installer)
- [x] Package for macOS (`.dmg` with code signing placeholder)
- [x] Package for Linux (`.AppImage` and `.deb`)

---

## Product 2 — TPT Tether (State & Connection Proxy)

### Core Proxy (Rust + Tokio)

- [x] Design WIT definitions — WIT interface types for query, execute, transaction, key-value ops
- [x] Implement WASI Preview 2 component model bindings (expose WIT interface to Wasm callers)
- [x] Implement connection pool manager — configurable min/max pool size, idle timeout, acquire timeout
- [x] Implement Postgres driver (sqlx-based) — full query/exec/transaction support over the pool
- [x] Implement MySQL driver — same interface as Postgres driver
- [x] Implement Redis driver (redis-rs) — key-value ops (get, set, del, expire, pub/sub)
- [x] Implement TLS termination — mTLS between Tether and backend databases
- [x] Implement database authentication handler — username/password, env-var secrets, Vault integration stub
- [x] Implement connection health monitoring — ping loop, auto-evict broken connections, reconnect backoff
- [x] Write unit tests for pool manager, each driver, TLS layer
- [x] Write integration tests — Wasm test module ↔ Tether ↔ real Postgres/Redis (Docker Compose test env)

### Custom Wire-Protocol Drivers (replaces sqlx / redis-rs)

- [x] Implement `WireStream` enum (Plain/TLS) and `WireDriver` + `WireTransaction` traits in `drivers/wire.rs`
- [x] Implement PostgreSQL wire protocol driver — framing (FrontendMessage/BackendMessage), MD5 + SCRAM-SHA-256 auth handshake, Simple Query state machine
- [x] Implement Redis RESP3 wire protocol driver — encoder/parser, HELLO 3 negotiation, pipelining, pub/sub dispatch
- [x] Implement MySQL wire protocol skeleton — capability exchange, COM_QUERY (stub internals acceptable)
- [x] Rewire `pool.rs` to hold `Box<dyn WireDriver>` and add `DriverKind` enum
- [x] Rewire `wit.rs` to delegate to `Box<dyn WireDriver>` trait object
- [x] Remove `sqlx` and `redis` crate dependencies from `tether/proxy/Cargo.toml`

### Control Plane (Go)

- [x] Design configuration schema (YAML: backends, pool settings, routing rules)
- [x] Implement config API server (REST) — CRUD for backends and routing rules
- [x] Implement Kubernetes ConfigMap watcher — hot-reload routing config on ConfigMap change
- [x] Implement Consul KV integration — alternative config source for non-K8s environments
- [x] Write OpenAPI spec for the config API
- [x] Write end-to-end tests for control plane ↔ data plane config reload

---

## Product 3 — TPT Scope (Hybrid eBPF Observability)

### eBPF Collection Agent (Rust or Go)

- [x] Design eBPF probe architecture — map kernel tracepoints to OTel semantic conventions
- [x] Implement network traffic tracing probes (TCP connect/accept, UDP send/recv, bytes transferred)
- [x] Implement system call tracing (open, read, write, exec — with process/container context)
- [x] Implement Wasm runtime event tracing — hook Wasmtime events: compile time, instantiation latency, memory pages
- [x] Implement OpenTelemetry exporter — emit traces, metrics, and logs via OTLP (gRPC + HTTP)
- [x] Implement container-aware enrichment — attach container ID, image name, pod name to every span/metric
- [x] Write unit tests for each probe; write integration test that traces a real HTTP request end-to-end

### Backend Storage

- [x] Design ClickHouse schema — tables for traces (spans), metrics (time-series), logs
- [x] Implement ingestion pipeline — OTel Collector → ClickHouse sink (via `clickhouse-go` or HTTP API)
- [x] Configure retention policies — TTL-based expiry per data type (e.g., raw traces 7d, rolled-up metrics 90d)
- [x] Implement query API layer (Go) — REST endpoints for dashboard to fetch traces, metrics, and logs

### Custom OTLP Ingest Pipeline (replaces external OTel Collector)

- [x] Restructure `scope/backend/` as `cmd/backend` + `cmd/ingest` with shared `internal/` packages
- [x] Migrate `TraceRecord`, `MetricRecord`, `LogRecord` and DDL constants to `internal/schema/`
- [x] Implement fixed-capacity `RingBuffer[T]` (drop-oldest) and `Flusher[T]` in `internal/buffer/`
- [x] Implement `ClickHouseWriter` using `clickhouse-go/v2` native protocol (replace HTTP API calls)
- [x] Implement OTLP → internal record transform layer in `internal/transform/` (traces, metrics, logs)
- [x] Implement OTLP gRPC receiver (TraceService, MetricsService, LogsService) on port 4317
- [x] Implement OTLP/HTTP receiver (proto + JSON content negotiation) on port 4318
- [x] Add Prometheus `/metrics` endpoint with ingestion counters and buffer depth gauges

### Frontend Dashboard (React + TypeScript)

- [x] Scaffold React + TypeScript + Vite project in `/scope/dashboard`
- [x] Implement service dependency graph — Cytoscape or D3 force-directed graph of service call relationships
- [x] Implement trace waterfall view — span timeline for a single distributed trace
- [x] Implement metrics charts — latency (p50/p95/p99), throughput (req/s), error rate over time
- [x] Implement Wasm-specific metrics panel — compile time, instantiation latency, memory usage per module
- [x] Implement log viewer — filterable by service, severity, time range; full-text search
- [x] Write E2E tests (Playwright) for the golden-path trace view and graph render

---

## Product 4 — TPT Chisel (Automated Distiller & Migrator)

### Core Engine (Go)

- [x] Design the two-phase analysis pipeline — Phase 1: runtime trace; Phase 2: Wasm migration
- [x] Implement secure container sandbox runner — spin up target container in isolated network, mount eBPF probes
- [x] Implement eBPF file access tracer — record every `open()`, `read()`, `exec()` syscall during profiling run
- [x] Implement runtime dependency mapper — build an exact list of files, libs, and syscalls actually used
- [x] Implement distroless image builder — copy only used files into a minimal `scratch` or `distroless` base image
- [x] Implement SBOM generator — output CycloneDX or SPDX 2.3 format listing all included components
- [x] Integrate CVE scanner — scan the final image against a local Grype/Trivy database; fail on critical CVEs

### Wasm Migration Pipeline

- [x] Implement language detector — inspect compiled binary / source files to identify Rust, Go, C++ workloads
- [x] Integrate WASI SDK — invoke `wasi-sdk` toolchain for C/C++ cross-compilation to `wasm32-wasi`
- [x] Integrate Emscripten — alternative C/C++ → Wasm path for web-compatible output
- [x] Implement Rust wasm32-wasi pipeline — `cargo build --target wasm32-wasi`, strip, optimize with `wasm-opt`
- [x] Implement Go Wasm compilation pipeline — `GOOS=wasip1 GOARCH=wasm go build`, validate output
- [x] Implement output Wasm module validator — run module in Wasmtime sandbox, assert exit code and basic I/O

### AI / LLM Layer

- [x] Integrate local LLM via Ollama API (target: Llama 3 8B) — analyze source code for Wasm incompatibilities
- [x] Integrate cloud LLM via Claude API — same analysis task with higher quality output for complex cases
- [x] Write prompt templates for: dependency analysis, unsafe code detection, suggested refactoring paths
- [x] Implement structured output parsing — emit JSON list of incompatible symbols and suggested fixes
- [x] Add local/cloud toggle via `chisel.yaml` config (`ai.backend: local | cloud`)

### Custom LLM Client (replaces raw reqwest calls)

- [x] Restructure `chisel/core/src/ai.rs` into `ai/` module directory
- [x] Implement `LlmError` typed error enum (`RateLimit`, `AuthError`, `ContextLengthExceeded`, `Unavailable`, `ParseError`)
- [x] Implement unified `LlmProvider` trait (replaces split `LocalLlm`/`CloudLlm` traits)
- [x] Implement `RetryPolicy` with exponential backoff + jitter — no new dependencies
- [x] Implement `PromptCache` — SHA-256 keyed in-memory cache with configurable TTL
- [x] Migrate `OllamaClient` → `OllamaProvider` with streaming via `Response::chunk()`
- [x] Migrate `ClaudeClient` → `ClaudeProvider` with SSE streaming and `Retry-After` header parsing
- [x] Implement `OpenAiProvider` (configurable `base_url` for Azure / local proxies)
- [x] Update `AiOrchestrator` to use `Vec<Arc<dyn LlmProvider>>` with cache + retry

### CI/CD Plugins (distributable artifacts — NOT wired into this repo)

- [x] Build GitHub Actions action (`action.yml` + Docker/JS runner) — users add it to their own repos
- [x] Build GitLab CI component (`component.yml`) — same, distributable to user pipelines
- [x] Write example workflow files and integration docs for both platforms

---

## Product 5 — TPT Frontier (Wasm-Native Edge Service Mesh)

### Data Plane (Rust)

- [x] Design proxy architecture — listener, router, upstream pool, plugin chain
- [x] Implement HTTP/1.1 handler (Hyper)
- [x] Implement HTTP/2 handler (Hyper h2)
- [x] Implement async I/O event loop (Tokio runtime, tuned for high connection count)
- [x] Implement Wasm plugin loader (Wasmtime) — load `.wasm` plugin, expose host ABI
- [x] Implement hot-reload of Wasm plugins — swap plugin in-place with zero dropped connections
- [x] Implement routing rules engine — path prefix, header match, weighted round-robin
- [x] Implement load balancing algorithms — round-robin, least-connections, consistent hashing
- [x] Implement TLS termination — rustls-based, SNI routing, certificate auto-reload
- [x] Implement bundled JWT validation Wasm plugin — verify RS256/ES256 tokens; configurable JWKS endpoint
- [x] Implement bundled rate limiting Wasm plugin — sliding window, token bucket; configurable per-route
- [x] Write benchmarks (`criterion`) — target: >100k req/s on a single core for simple proxy path

### Plugin SDK

- [x] Design plugin host ABI — define the function signatures exposed to Wasm plugins
- [x] Implement Rust plugin SDK crate — safe wrappers around the host ABI, example plugin template
- [x] Implement TypeScript/AssemblyScript plugin SDK — same ABI, npm-publishable package
- [x] Write example plugins (JWT, rate limit, custom auth) in both Rust and AssemblyScript
- [x] Write plugin authoring documentation with build → test → hot-load walkthrough

### Control Plane (Go or Rust)

- [x] Implement xDS v3 API server (ADS — Aggregated Discovery Service, Envoy-compatible)
- [x] Implement Istio integration — consume Istio's xDS feed to configure Frontier as a sidecar proxy
- [x] Implement Consul Connect integration — register services and pull intentions via Consul API
- [x] Write E2E tests — real HTTP traffic through Frontier with a live Wasm plugin loaded

### Frontier-Native Control Plane (replaces broken xDS ADS + Consul Connect)

- [x] Define Protobuf schema for Frontier config types (`Route`, `Upstream`, `Plugin`, `TlsCert`, `ConfigSnapshot`) in `frontier/proto/frontier/v1/`
- [x] Implement in-memory config `Store` with `sync.RWMutex`, monotonic versioning, and fan-out pub/sub event bus
- [x] Implement `FrontierConfig` gRPC service — `WatchConfig` streaming (snapshot + diffs) and CRUD RPCs
- [x] Implement REST CRUD API using Go 1.22 `ServeMux` with `protojson` encode/decode
- [x] Implement atomic JSON config file pusher (`os.Rename`) to `/var/run/frontier/config.json` for Rust data plane
- [x] Implement Consul *catalog* sync via HTTP blocking queries (`?index=N&wait=30s`) — no Connect/mTLS
- [x] Implement optional xDS CDS+EDS adapter using correct `anypb.New()` packing (replaces broken `MarshalTo(nil)`)
- [x] Delete `frontier/control-plane/istio.go` (polls unsupported Istiod debug endpoints)
- [x] Add config-file watcher task to Rust proxy (`listener.rs`) for hot-reload on version change
- [x] Wrap `Router` and `UpstreamManager` in `Arc<RwLock<>>` to support atomic hot-reload

---

## Phase N — Cross-Cutting

- [x] Set up documentation site (`/docs`) — Docusaurus with per-product sections and API references
- [x] Write full flywheel integration test — Origin spins up app → Chisel distills image → Tether handles DB → Frontier routes traffic → Scope traces it all
- [x] Set up community infrastructure — GitHub Discussions categories, issue templates, Discord server (optional)
- [x] Commission security audit — static analysis (`cargo audit`, `govulncheck`), manual review of eBPF probes and Wasm sandboxing

---

## Phase 7 — Release Hardening

> **Progress:** 31 / 31 tasks complete

### Security Fixes

- [x] Fix JWT signature verification in `frontier/examples/jwt_validator.rs` — implement HMAC-SHA256 verify of `parts[2]`; reject tokens with invalid signature
- [x] Remove hardcoded `"my-secret-key"` from jwt_validator example; read secret from `JWT_SECRET` env var
- [x] Add `apiKeyMiddleware` to Frontier REST control plane (`frontier/control-plane/internal/rest/handler.go`) — constant-time compare against `FRONTIER_API_KEY` env var; 401 if missing/wrong
- [x] Check Tether control plane REST endpoints for same missing-auth gap; apply identical middleware if needed
- [x] Move ClickHouse password from `-clickhouse-pass` CLI flag to `CLICKHOUSE_PASS` env var in `scope/backend/cmd/ingest/main.go`
- [x] Replace `self.entries.lock().unwrap()` with `unwrap_or_else(|e| e.into_inner())` in `chisel/core/src/ai/cache.rs` (lines 27, 43, 47)
- [x] Implement health-tracking (`mark_healthy` / `mark_unhealthy`) in `RoundRobinBalancer` and `ConsistentHashBalancer` in `frontier/proxy/src/loadbalancer.rs`

### Backend Stubs

- [x] Add `tracing::warn!` before `bail!` in MySQL wire driver stub methods (`tether/proxy/src/drivers/wire_mysql.rs` lines 249–260)
- [x] Add `tracing::warn!` before `bail!` in PostgreSQL wire driver stub paths (`tether/proxy/src/drivers/wire_postgres.rs` lines 78, 749, 754, 758)

### Frontend — Scope Dashboard

- [x] Create `scope/dashboard/src/api/client.ts` — typed fetch wrapper reading `VITE_API_BASE_URL`; expose `getServices`, `getTraces`, `getMetrics`, `getLogs`, `getWasmMetrics`
- [x] Replace `MOCK_` data in `ServiceGraph.tsx` with `getServices()`; render graph with Cytoscape.js (`cola` layout, health-colored nodes)
- [x] Replace placeholder divs in `MetricsCharts.tsx` with Recharts `<LineChart>` / `<AreaChart>` fed by `getMetrics()`
- [x] Replace mock spans in `TraceWaterfall.tsx` with `getTraces()` data
- [x] Replace mock logs in `LogViewer.tsx` with `getLogs()` polling every 3 s; add SSE stream endpoint for real-time tail
- [x] Replace mock table in `WasmMetrics.tsx` with `getWasmMetrics()` data
- [x] Add top-level `ErrorBoundary` component and per-component error/retry states to Scope Dashboard
- [x] Add loading spinners while first fetch is in-flight to all Scope Dashboard components
- [x] Add `scope/dashboard/.env.example` documenting `VITE_API_BASE_URL`

### Frontend — Origin GUI

- [x] Create `origin/gui/src/api/tauriApi.ts` — typed Tauri `invoke()` wrappers for `listServices`, `startService`, `stopService`, `streamLogs`, `applyManifest`
- [x] Wire `Dashboard.tsx` to `listServices()` (poll every 5 s); connect Start/Stop buttons to `startService()`/`stopService()`
- [x] Wire `Logs.tsx` to Tauri `"log-event"` listener; add auto-scroll toggle
- [x] Replace string-search validation in `ManifestEditor.tsx` with `js-yaml` parse; add Save button calling `applyManifest()`; add Download .yaml export button
- [x] Add `ErrorBoundary` and loading/error states to Origin GUI components
- [x] Add `origin/gui/.env.example`

### UX Improvements

- [x] Add dark/light theme toggle to both frontends — CSS variables + `ThemeContext`, persisted to `localStorage`
- [x] Add keyboard shortcuts to Origin GUI (`S` = start, `X` = stop, `R` = restart, `L` = logs view)
- [x] Add one-click copy-to-clipboard icon on each log line in both log viewers
- [x] Add refresh-interval picker (5 s / 15 s / 30 s / manual) to Scope Dashboard, persisted to `localStorage`

### GitHub Release Files

- [x] Update `CONTRIBUTING.md` with dev environment setup, Rust/Go/Node prerequisites, PR conventions, and code-style tools (`rustfmt`, `gofmt`, `prettier`)
- [x] Create `CODE_OF_CONDUCT.md` — Contributor Covenant 2.1, attributed to TPT Solutions
- [x] Create `CHANGELOG.md` — initial `## [1.0.0] - 2026-06-30` entry summarising all five products
- [x] Create `.github/ISSUE_TEMPLATE/bug_report.md` and `feature_request.md`
- [x] Create `.github/PULL_REQUEST_TEMPLATE.md`
- [x] Polish `README.md` — add Apache 2.0 license badge, Quick Start section, and products table with one-line descriptions and sub-README links

### Custom Solution Improvements

- [x] Replace unbounded `HashMap` in `chisel/core/src/ai/cache.rs` with `lru::LruCache` (cap 10 000 entries); add `lru = "0.12"` to `chisel/core/Cargo.toml`; remove SHA-256 cache-key hashing in favour of plain string key
- [x] Fix `ConfigWatcher` in `frontier/proxy/src/listener.rs` to check `mtime` before re-reading and re-parsing the config file — eliminate unconditional reload on every poll tick
- [x] Replace `sha2::Sha256` with `ahash::AHasher` in `consistent_hash()` inside `frontier/proxy/src/loadbalancer.rs`; replace `rand_simple()` with `rand::thread_rng().gen::<u64>()`
- [x] Fix retry jitter in `chisel/core/src/ai/retry.rs`: replace `subsec_nanos() % ceiling` with `rand::thread_rng().gen_range(0..ceiling)`; add `rand` to `chisel/core/Cargo.toml`
- [x] Extract duplicated `retry-after` header parsing from `claude.rs` and `openai.rs` into a shared `fn extract_retry_after` in `retry.rs`
- [x] Add `ping()` method to `WireDriver` trait in `tether/proxy/src/drivers/wire.rs`; implement in each driver (PostgreSQL sync, MySQL `COM_PING`, Redis `PING`); call in `pool.rs` `acquire()` before returning a connection
- [x] Fix `stats()` in `tether/proxy/src/pool.rs` to report accurate idle count using an `AtomicUsize` counter incremented on release and decremented on acquire

---

## Phase 8 — Observability & Hardening

> **Progress:** 0 / 14 tasks complete

### Rate Limiting

- [ ] Add per-IP fixed-window rate-limiting middleware to `scope/backend/cmd/ingest` (OTLP HTTP receiver) — 1 000 req/min default, `SCOPE_INGEST_RATE_LIMIT` env override; 429 + `Retry-After` on breach
- [ ] Add per-IP rate-limiting middleware to `scope/backend/cmd/backend` (query API) — 200 req/min default, `SCOPE_QUERY_RATE_LIMIT` env override
- [ ] Add per-IP rate-limiting middleware to `frontier/control-plane` REST API — 300 req/min default, `FRONTIER_RATE_LIMIT` env override
- [ ] Add per-IP rate-limiting middleware to `tether/control-plane` REST API — 300 req/min default, `TETHER_RATE_LIMIT` env override

### Telemetry

- [ ] Add HTTP request metrics middleware to `scope/backend/cmd/backend` — expose `tpt_http_requests_total{method,path,status}`, `tpt_http_request_duration_seconds` histogram, and `tpt_http_inflight_requests` gauge on `/metrics`
- [ ] Add HTTP request metrics middleware to `scope/backend/cmd/ingest` — extend existing `prometheus.go` with the same three HTTP-layer metrics (distinct from the existing ingestion-count counters)
- [ ] Add lightweight Prometheus-format `/metrics` endpoint to `frontier/control-plane` — request count, latency histogram, in-flight gauge; no new Go module dependencies (hand-rolled text format + atomic counters)
- [ ] Add lightweight Prometheus-format `/metrics` endpoint to `tether/control-plane` — same approach as Frontier; expose on same port as REST API

### Dependency Reduction

- [ ] Replace `k8s.io/client-go` + `k8s.io/api` + `k8s.io/apimachinery` in `tether/control-plane` with a custom `net/http` ConfigMap watcher (~100 lines) — HTTP long-poll `GET /api/v1/namespaces/{ns}/configmaps/{name}?watch=true`; remove the three k8s packages from `go.mod`
- [ ] Replace `reqwest` in `frontier/proxy/Cargo.toml` with a direct Hyper 1.x client call in `frontier/proxy/src/plugins/jwt.rs` (`refresh_keys`) — proxy already imports Hyper; removes duplicate TLS stack and ~2 MB from the proxy binary
- [ ] Replace `#[async_trait]` macro in `tether/proxy` and `chisel/core` with native async-fn-in-trait syntax (stable since Rust 1.75) — remove `async-trait = "0.1"` from both `Cargo.toml` files
- [ ] Replace `parking_lot::RwLock` with `tokio::sync::RwLock` in async call sites within `frontier/proxy/src/plugins/jwt.rs` and `frontier/proxy/src/tls.rs` — eliminates blocking-lock risk on Tokio worker threads; pure-sync hot paths (`ratelimit.rs`) may keep `parking_lot::Mutex`

### Frontend Completeness

- [ ] Wire up `cytoscape` (already in `scope/dashboard/package.json`) in `ServiceGraph.tsx` — replace hand-written SVG grid with a Cytoscape `cose` force-directed layout; health-coloured nodes, arrowhead edges, zoom/pan
- [ ] Wire up `recharts` (already in `scope/dashboard/package.json`) in `MetricsCharts.tsx` — replace SVG `<polyline>` with Recharts `<AreaChart>` / `<LineChart>`; add `<Tooltip>` and `<Legend>`; group series by metric name
