//! Minimal end-to-end wiring of the real `tcp_connect()` kprobe
//! (`EbpfNetworkProbe`, `scope/ebpf/`) into the existing OTLP export
//! pipeline (`otel::OtelExporter`). Demonstrates the full path the Phase 11
//! eBPF work was for: real kernel events -> enrichment -> OTLP spans.
//!
//! Requires Linux, root (or `CAP_BPF`+`CAP_PERFMON`), and this crate built
//! with `--features ebpf` — a nightly toolchain is only needed to *compile*
//! the kernel object (see `scope/ebpf/loader/build.rs`), not to run this
//! binary once built.
//!
//! Run: `cargo run -p tpt-scope-agent --features ebpf --example ebpf_tcp_connect_daemon`

#[cfg(all(target_os = "linux", feature = "ebpf"))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use tpt_scope_agent::probes::Probe;

    tracing_subscriber::fmt::init();

    let otel_config = tpt_scope_agent::otel::OtelConfig::default();
    let exporter = tpt_scope_agent::otel::OtelExporter::new(&otel_config)?;
    let mut enrichment = tpt_scope_agent::enrichment::EnrichmentProvider::new();

    let mut probe = tpt_scope_agent::probes::EbpfNetworkProbe::new("tcp-connect");
    probe
        .attach()
        .map_err(|e| anyhow::anyhow!("failed to attach tcp_connect kprobe: {e}"))?;

    tracing::info!("tcp_connect kprobe attached, streaming connect events to OTLP");

    loop {
        for event in probe.drain_events() {
            let enrichment_data = enrichment.enrich_with_fallback(event.pid);
            exporter.emit_event(&event, Some(&enrichment_data));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}

#[cfg(not(all(target_os = "linux", feature = "ebpf")))]
fn main() {
    eprintln!(
        "ebpf_tcp_connect_daemon requires Linux and `--features ebpf` \
         (see scope/ebpf/README or scope/agent/Cargo.toml's [features] section)"
    );
    std::process::exit(1);
}
