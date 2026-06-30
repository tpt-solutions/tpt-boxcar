# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-06-30

### Added
- **TPT Origin**: Unified local sandbox supporting OCI containers and Wasm modules via containerd and Wasmtime, with eBPF networking (Aya), local DNS resolution, and Tauri desktop GUI
- **TPT Tether**: State and connection proxy with WASI Preview 2 / WIT interfaces, hand-written wire-protocol drivers for PostgreSQL (SCRAM-SHA-256 auth, full query/exec/transaction), MySQL, and Redis (RESP3, pipelining, pub/sub)
- **TPT Scope**: Hybrid eBPF observability platform with custom OTLP ingest pipeline (gRPC + HTTP), ClickHouse storage, React dashboard with trace waterfall, service graph, metrics charts, and log viewer
- **TPT Chisel**: Automated image distiller and Wasm migrator with LLM analysis (Ollama local + Claude cloud), SBOM generation, CVE scanning via Grype/Trivy
- **TPT Frontier**: Wasm-native edge service mesh and API gateway with Hyper HTTP/1.1+2, hot-reloadable Wasm plugins via Wasmtime, Frontier-native control plane (gRPC + REST), and bundled JWT/rate-limiting plugins
