//! Compiles `scope-ebpf-probe` (the `#![no_std]` kernel-side crate) to a
//! `bpfel-unknown-none` ELF object via `aya-build`, and embeds it for
//! `src/lib.rs` to load with `aya::include_bytes_aligned!`.
//!
//! `aya-build` shells out to a separate `cargo build` for the probe crate
//! using *its own* `rust-toolchain.toml` (nightly + `build-std`), so this
//! only works on Linux with a nightly toolchain installed — same
//! precondition as every other `aya`-based project. On non-Linux hosts (or
//! without the `ebpf` feature enabled on `tpt-scope-agent`), this crate and
//! its `build.rs` are never built at all.

use std::path::PathBuf;

use aya_build::cargo_metadata;

fn main() {
    let cargo_metadata::Metadata { packages, .. } = cargo_metadata::MetadataCommand::new()
        .no_deps()
        .manifest_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../probe/Cargo.toml"))
        .exec()
        .expect("failed to run `cargo metadata` on scope-ebpf-probe");

    let probe_package = packages
        .into_iter()
        .find(|p| p.name == "scope-ebpf-probe")
        .expect("scope-ebpf-probe package not found by cargo metadata");

    // Re-run this build script if the probe source changes, not just when
    // this crate's own sources change (the default cargo behavior).
    let probe_dir: PathBuf = probe_package
        .manifest_path
        .parent()
        .expect("probe manifest has no parent dir")
        .into();
    println!("cargo:rerun-if-changed={}", probe_dir.join("src").display());
    println!(
        "cargo:rerun-if-changed={}",
        probe_dir.join("Cargo.toml").display()
    );

    aya_build::build_ebpf([probe_package]).expect("failed to build scope-ebpf-probe");
}
