---
sidebar_position: 2
title: Multi-Arch-by-Default Benchmark
---

# Multi-Arch-by-Default Benchmark

Phase 10 backlog item: "Document/benchmark true multi-arch-by-default — Wasm
modules run unmodified on ARM/x86 without multi-arch image builds."

## Why there's no code fix here

Wasm is architecture-independent by construction: a `.wasm` binary targets
the WebAssembly virtual ISA, not a CPU ISA, and `wasmtime` JIT-compiles it to
the host's native code at load time. A search of `origin/`, `frontier/`, and
`chisel/core/src` confirms there is no `cfg(target_arch = ...)` conditional
compilation anywhere in this repo — the only architecture-adjacent
conditionals are `cfg(target_os = ...)` in `origin/core/src/network.rs`,
which branch on OS-specific *networking* APIs, not CPU architecture. This
item is a documentation and verification exercise, not a bug fix.

## The burden this replaces

To run a container image on both ARM64 and x86_64 hosts today, teams
typically build a multi-arch OCI image:

```dockerfile
# Two separate builds, one per target architecture, joined by a manifest list
FROM --platform=$BUILDPLATFORM rust:1 AS build
ARG TARGETPLATFORM
RUN cargo build --release --target $(...)
```

```bash
docker buildx build --platform linux/amd64,linux/arm64 -t myapp:latest --push .
```

This means: two separate compiles, two separate pushed image layers, and a
manifest list tying them together — one build pipeline per architecture you
support. A single compiled `.wasm` module has none of this: `cargo build
--target wasm32-wasi` produces one artifact that runs unmodified on any host
`wasmtime` supports, regardless of the host's CPU architecture.

## Methodology

1. Build one `.wasm` fixture once (`wasm32-wasi` target, per
   `rust-toolchain.toml` — the same fixture used by the Slice 0 and Slice 4
   benchmarks in `origin/core/src/runtime.rs`'s tests and
   `origin/core/benches/coldstart_bench.rs`).
2. Run it via Origin's real `start_wasm` path (`tpt origin up`, or directly
   via the `wasmtime` CLI) on an x86_64 host and on an ARM64 host (e.g. a
   cloud ARM instance or Apple Silicon Mac).
3. Confirm the two runs produce byte-identical output.
4. Record instantiate+run latency on each host — both should be fast (low
   milliseconds), though absolute numbers will differ by host/CPU
   generation, not by architecture-specific code paths in this repo.

Use `scripts/multiarch-check.sh <fixture.wasm>` to automate steps 2–4 on each
host — it prints the host arch, a sha256 of the fixture (to guarantee both
runs used the identical bytes), the module's output, and elapsed time.

## Results

| Host | Arch | Fixture sha256 (truncated) | Output | Elapsed |
|---|---|---|---|---|
| _fill in: e.g. AWS Graviton3_ | arm64 | `<paste>` | `<paste>` | `<paste>` ms |
| _fill in: e.g. dev workstation_ | x86_64 | `<paste>` | `<paste>` | `<paste>` ms |

The two `Fixture sha256` values must match (same bytes shipped to both
hosts) and the two `Output` values must match byte-for-byte — that's the
actual claim being verified: one build artifact, unmodified, on two CPU
architectures. Timings are expected to be in the same rough ballpark (both
low-single-digit-to-low-double-digit milliseconds); this benchmark is about
correctness-across-architectures, not a performance comparison between the
architectures themselves.

## Reproducing

```bash
# On each target host:
./scripts/multiarch-check.sh origin/core/tests/fixtures/hello.wasm
```

No CI check enforces this — CI runners are typically single-architecture,
so cross-arch verification is deliberately a manual exercise using the
script above. Reuse the exact same fixture file (don't rebuild it per host)
so the comparison is meaningful.
