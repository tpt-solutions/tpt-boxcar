use tpt_origin_core::lifecycle::topological_waves;
use tpt_origin_core::manifest::Manifest;

/// `boxcar.yaml` at the repo root is the cross-product manifest consumed by
/// `tpt up`/`tpt down` (origin/cli). It's a plain Origin manifest, so it
/// must parse with the same `Manifest` type and respect the documented
/// Tether -> Origin -> Frontier dependency order via `depends_on`.
#[test]
fn boxcar_yaml_parses_and_orders_tether_before_origin_before_frontier() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../boxcar.yaml");
    let content = std::fs::read_to_string(path).expect("boxcar.yaml should exist at repo root");
    let manifest: Manifest = serde_yaml::from_str(&content).expect("boxcar.yaml should parse as a valid Manifest");

    assert!(manifest.services.contains_key("tether-control-plane"));
    assert!(manifest.services.contains_key("origin-workload"));
    assert!(manifest.services.contains_key("frontier-control-plane"));

    let waves = topological_waves(&manifest).expect("boxcar.yaml's depends_on graph should be acyclic");

    let wave_of = |name: &str| {
        waves
            .iter()
            .position(|wave| wave.iter().any(|s| s == name))
            .unwrap_or_else(|| panic!("service '{name}' missing from topological_waves output"))
    };

    let tether_wave = wave_of("tether-control-plane");
    let origin_wave = wave_of("origin-workload");
    let frontier_wave = wave_of("frontier-control-plane");

    assert!(
        tether_wave < origin_wave,
        "tether-control-plane must start before origin-workload"
    );
    assert!(
        origin_wave < frontier_wave,
        "origin-workload must start before frontier-control-plane"
    );
}
