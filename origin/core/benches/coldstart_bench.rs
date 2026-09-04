//! Compares Wasm cold-start (fresh `wasmtime::Engine` + compile + instantiate
//! + call `_start`) against a native process spawn, as a proxy for the
//!   "scale-to-zero density" claim: how many isolated units of work can be
//!   started per second per core.
//!
//! `process_cold_start` is NOT a real containerd/OCI cold-start measurement
//! — there's no containerd integration in this repo to benchmark against
//! (see Phase 10 Slice 0's scope note). A bare process spawn is the closest
//! available proxy without standing up real container infrastructure;
//! treat it as a lower bound on container cold-start cost, not an exact one.

use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::HashMap;
use tpt_origin_core::runtime::instantiate_and_run;

/// A minimal WASI-CLI-shaped module: exports `_start` and does nothing,
/// matching the fixture used in `runtime.rs`'s own unit tests.
fn hello_wasm_bytes() -> Vec<u8> {
    let wat = r#"
        (module
            (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
            (memory (export "memory") 1)
            (func $_start (export "_start"))
        )
    "#;
    wat::parse_str(wat).expect("valid wat fixture")
}

fn bench_wasm_cold_start(c: &mut Criterion) {
    let wasm_bytes = hello_wasm_bytes();
    let args: Vec<String> = vec![];
    let env: HashMap<String, String> = HashMap::new();

    c.bench_function("wasm_cold_start", |b| {
        b.iter(|| {
            instantiate_and_run(&wasm_bytes, &args, &env, None).expect("instantiate_and_run");
        })
    });
}

fn bench_process_cold_start(c: &mut Criterion) {
    c.bench_function("process_cold_start", |b| {
        b.iter(|| {
            let status = if cfg!(target_os = "windows") {
                std::process::Command::new("cmd")
                    .args(["/C", "exit", "0"])
                    .status()
            } else {
                std::process::Command::new("true").status()
            }
            .expect("spawn trivial process");
            assert!(status.success());
        })
    });
}

criterion_group!(benches, bench_wasm_cold_start, bench_process_cold_start);
criterion_main!(benches);
