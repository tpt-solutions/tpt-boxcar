---
sidebar_position: 1
title: Scale-to-Zero Density Benchmark
---

# Scale-to-Zero Density Benchmark

Phase 10 backlog item: "Document and benchmark true scale-to-zero density — Wasm
cold-start (sub-ms–few-ms) vs container cold-start, enabling serverless-like
packing without a serverless platform."

## Methodology

`origin/core/benches/coldstart_bench.rs` (run via `cargo bench -p tpt-origin-core
--bench coldstart_bench`) measures two cold-start paths with
[criterion](https://docs.rs/criterion):

- **`wasm_cold_start`** — a fresh `wasmtime::Engine` is created per iteration
  (no engine reuse), then the module is compiled, instantiated, and its
  `_start` entry point is called via `tpt_origin_core::runtime::instantiate_and_run`
  — the same function Origin's `RuntimeManager::start_wasm` uses in production.
  This captures the full cost of going from zero to a running instance, not
  just steady-state invocation.
- **`process_cold_start`** — spawns a trivial native process (`cmd /C exit 0`
  on Windows, `true` on Unix) and waits for it to exit.

### Caveat: this is not a real containerd benchmark

`origin/core` has no containerd integration (see the Phase 10 Slice 0 scope
note in the codebase) — there is no real OCI container runtime in this repo
to benchmark against. A bare process spawn is the closest available proxy
for "container cold-start" without standing up containerd/runc
infrastructure. Treat `process_cold_start` as a **lower bound** on real
container cold-start cost (a real container additionally pays for namespace
setup, cgroup creation, and image layer mounting, none of which a bare
process spawn incurs) — the true wasm-vs-container density gap is at least
as large as what's measured here, likely larger.

## Results

Measured on a Windows 11 development machine (x86_64), release profile,
10-sample criterion runs:

| Path | Time (median) |
|---|---|
| `wasm_cold_start` (fresh engine + compile + instantiate + call) | ~315 µs |
| `process_cold_start` (`cmd /C exit 0`) | ~26 ms |

That's roughly **80×** faster for the Wasm path in this environment. Some of
that gap is Windows-specific — `cmd.exe` has unusually high spawn overhead
compared to a native Unix `fork`/`exec` of a minimal binary — so treat the
absolute multiplier as environment-dependent, not universal. Re-run the
benchmark on your own target platform (`cargo bench -p tpt-origin-core
--bench coldstart_bench`) before citing a specific number; the qualitative
conclusion (Wasm cold-start is at least one to two orders of magnitude
cheaper than spawning a new OS-level unit of isolation) is expected to hold
across platforms, since it reflects the fundamental difference between
loading a pre-validated Wasm module into an existing process versus asking
the OS kernel to create a new one.

## Density estimate

At ~315 µs per cold start, a single core could theoretically instantiate
roughly 3,000 fresh Wasm modules per second (upper bound; ignores contention
and real workload cost after `_start` returns). At ~26 ms per process spawn,
the same core manages roughly 38 processes per second. This is the practical
basis for the "scale-to-zero" pitch: a Wasm-native platform can afford to
tear an idle service down to zero instances and pay a cold-start cost on the
next request that's imperceptible compared to typical network round-trip
latency, in a way that's much harder to justify for container cold-starts.

## Reproducing

```bash
cargo bench -p tpt-origin-core --bench coldstart_bench
```

HTML reports (with distribution plots) are written to
`target/criterion/wasm_cold_start` and `target/criterion/process_cold_start`.
