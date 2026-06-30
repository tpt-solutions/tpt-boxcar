use std::net::IpAddr;

use tpt_scope_agent::enrichment::{
    parse_container_id_from_path, EnrichmentData, EnrichmentProvider,
};
use tpt_scope_agent::probes::{
    EventData, NetworkEvent, NetworkEventType, Probe, ProbeCategory, ProbeEvent,
    ProbeManager, ProbeState, SyscallEventType, SyscallProbe, TransportProtocol,
    WasmProbe, NetworkProbe,
};

#[test]
fn test_network_probe_lifecycle() {
    let mut probe = NetworkProbe::new("net-trace");
    assert_eq!(probe.name(), "net-trace");
    assert!(matches!(probe.category(), ProbeCategory::Network));
    assert!(matches!(probe.state(), ProbeState::Detached));

    probe.attach().unwrap();
    assert!(matches!(probe.state(), ProbeState::Attached));

    probe.pause().unwrap();
    assert!(matches!(probe.state(), ProbeState::Paused));

    probe.resume().unwrap();
    assert!(matches!(probe.state(), ProbeState::Attached));

    probe.detach().unwrap();
    assert!(matches!(probe.state(), ProbeState::Detached));
}

#[test]
fn test_syscall_probe_configuration() {
    let syscalls = vec![SyscallEventType::Open, SyscallEventType::Read];
    let mut probe = SyscallProbe::new("fs-trace", syscalls);
    assert_eq!(probe.name(), "fs-trace");
    assert!(matches!(probe.category(), ProbeCategory::Syscall));
    assert!(matches!(probe.state(), ProbeState::Detached));

    probe.attach().unwrap();
    assert!(matches!(probe.state(), ProbeState::Attached));
    assert!(probe.poll_event().is_none());

    probe.detach().unwrap();
    assert!(matches!(probe.state(), ProbeState::Detached));
}

#[test]
fn test_wasm_probe_lifecycle() {
    let mut probe = WasmProbe::new("wasm-trace");
    assert_eq!(probe.name(), "wasm-trace");
    assert!(matches!(probe.category(), ProbeCategory::WasmRuntime));

    probe.attach().unwrap();
    assert!(matches!(probe.state(), ProbeState::Attached));

    probe.pause().unwrap();
    assert!(matches!(probe.state(), ProbeState::Paused));

    probe.resume().unwrap();
    assert!(matches!(probe.state(), ProbeState::Attached));

    probe.detach().unwrap();
    assert!(matches!(probe.state(), ProbeState::Detached));
}

#[test]
fn test_probe_manager_register_and_list() {
    let mut manager = ProbeManager::new();
    assert!(manager.list_probes().is_empty());

    manager.register(Box::new(NetworkProbe::new("net-1")));
    manager.register(Box::new(SyscallProbe::new(
        "sys-1",
        vec![SyscallEventType::Read],
    )));
    manager.register(Box::new(WasmProbe::new("wasm-1")));

    let probes = manager.list_probes();
    assert_eq!(probes.len(), 3);

    let names: Vec<&str> = probes.iter().map(|(n, _, _)| *n).collect();
    assert!(names.contains(&"net-1"));
    assert!(names.contains(&"sys-1"));
    assert!(names.contains(&"wasm-1"));
}

#[test]
fn test_probe_manager_attach_detach_all() {
    let mut manager = ProbeManager::new();
    manager.register(Box::new(NetworkProbe::new("net-1")));
    manager.register(Box::new(WasmProbe::new("wasm-1")));

    manager.attach_all().unwrap();
    for (_, cat, state) in manager.list_probes() {
        assert!(matches!(state, ProbeState::Attached), "probe {:?} not attached", cat);
    }

    manager.detach_all().unwrap();
    for (_, cat, state) in manager.list_probes() {
        assert!(matches!(state, ProbeState::Detached), "probe {:?} not detached", cat);
    }
}

#[test]
fn test_probe_event_construction() {
    let event = ProbeEvent {
        timestamp: 1700000000,
        category: ProbeCategory::Network,
        data: EventData::Network(NetworkEvent {
            event_type: NetworkEventType::TcpConnect,
            src_addr: "127.0.0.1".parse::<IpAddr>().unwrap(),
            dst_addr: "10.0.0.1".parse::<IpAddr>().unwrap(),
            src_port: 45678,
            dst_port: 8080,
            bytes: 0,
            proto: TransportProtocol::Tcp,
        }),
        pid: 1234,
        tid: 1234,
        comm: "curl".to_string(),
    };

    assert_eq!(event.pid, 1234);
    match &event.data {
        EventData::Network(net) => {
            assert_eq!(net.event_type, NetworkEventType::TcpConnect);
            assert_eq!(net.src_port, 45678);
            assert_eq!(net.dst_port, 8080);
        }
        _ => panic!("expected network event"),
    }
}

#[test]
fn test_parse_container_id_from_cgroup_path() {
    assert_eq!(
        parse_container_id_from_path(
            "/kubepods/burstable/pod123/abc123def456"
        ),
        Some("abc123def456".to_string())
    );

    assert_eq!(
        parse_container_id_from_path(
            "0::/kubepods/besteffort/pod-abc123/container/abcdef01234567890"
        ),
        Some("abcdef01234567890".to_string())
    );

    assert_eq!(parse_container_id_from_path("/user.slice/user-1000.slice"), None);
}

#[test]
fn test_enrichment_data_default() {
    let data = EnrichmentData::default();
    assert!(data.container_id.is_empty());
    assert!(data.pod_name.is_empty());
    assert_eq!(data.pid, 0);
    assert!(data.labels.is_empty());
}

#[test]
fn test_enrichment_provider_cache() {
    let mut provider = EnrichmentProvider::new();

    let data = provider.enrich_with_fallback(99999);
    assert_eq!(data.pid, 99999);
    assert!(data.container_id.starts_with("pid-"));

    let data2 = provider.enrich_with_fallback(99999);
    assert_eq!(data2.container_id, data.container_id);

    provider.invalidate(99999);
    let data3 = provider.enrich_with_fallback(99999);
    assert_eq!(data3.pid, 99999);

    provider.clear_cache();
}

#[test]
fn test_enrichment_image_name_from_id() {
    use tpt_scope_agent::enrichment::image_name_from_id;
    let name = image_name_from_id("aabbccddeeff00112233");
    assert_eq!(name, "container-aabbccddeeff");
}
