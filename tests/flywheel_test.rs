use std::collections::HashMap;
use std::time::Duration;

use tpt_origin_core::dns::DnsResolver;
use tpt_origin_core::lifecycle::LifecycleManager;
use tpt_origin_core::manifest::{
    Manifest, OCIService, PortMapping, Service, VolumeMount, WasmService,
};
use tpt_origin_core::network::{NetworkConfig, NetworkManager};
use tpt_origin_core::runtime::ServiceStatus;

use tpt_scope_agent::enrichment::EnrichmentProvider;
use tpt_scope_agent::probes::{
    EventData, NetworkEvent, NetworkEventType, ProbeCategory, ProbeEvent, ProbeManager,
    ProbeState, SyscallEvent, SyscallEventType, TransportProtocol, WasmEvent, WasmEventType,
    NetworkProbe, SyscallProbe, WasmProbe,
};

fn flywheel_manifest(wasm_path: std::path::PathBuf) -> Manifest {
    let mut services = HashMap::new();

    services.insert(
        "db".to_string(),
        Service::OCI(OCIService {
            image: "postgres:16-alpine".to_string(),
            ports: vec![PortMapping {
                host: 5432,
                container: 5432,
                protocol: "tcp".to_string(),
            }],
            environment: HashMap::from([(
                "POSTGRES_PASSWORD".to_string(),
                "test".to_string(),
            )]),
            volumes: vec![VolumeMount {
                source: "pgdata".to_string(),
                target: "/var/lib/postgresql/data".to_string(),
                read_only: false,
            }],
            command: None,
            depends_on: vec![],
            healthcheck: None,
            resources: None,
            restart_policy: Default::default(),
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
            profiles: vec![],
            build: None,
            configs: None,
            extends: None,
        }),
    );

    services.insert(
        "api".to_string(),
        Service::Wasm(WasmService {
            path: wasm_path,
            args: vec!["--port".to_string(), "8080".to_string()],
            ports: vec![],
            environment: HashMap::from([(
                "DATABASE_URL".to_string(),
                "postgres://test@db/app".to_string(),
            )]),
            memory_limit: Some("256m".to_string()),
            resources: None,
            depends_on: vec!["db".to_string()],
            expected_signature: None,
            trusted_public_key: None,
            restart_policy: Default::default(),
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
            healthcheck: None,
            profiles: vec![],
            configs: None,
            extends: None,
        }),
    );

    services.insert(
        "proxy".to_string(),
        Service::OCI(OCIService {
            image: "nginx:alpine".to_string(),
            ports: vec![PortMapping {
                host: 443,
                container: 443,
                protocol: "tcp".to_string(),
            }],
            environment: HashMap::new(),
            volumes: vec![],
            command: None,
            depends_on: vec!["api".to_string()],
            healthcheck: None,
            resources: None,
            restart_policy: Default::default(),
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
            profiles: vec![],
            build: None,
            configs: None,
            extends: None,
        }),
    );

    let mut networks = HashMap::new();
    networks.insert(
        "internal".to_string(),
        tpt_origin_core::manifest::Network {
            driver: "bridge".to_string(),
            subnet: None,
            gateway: None,
            vni: None,
            peers: vec![],
        },
    );

    let mut volumes = HashMap::new();
    volumes.insert(
        "pgdata".to_string(),
        tpt_origin_core::manifest::Volume {
            driver: Some("local".to_string()),
        },
    );

    Manifest {
        name: "flywheel-e2e".to_string(),
        version: "1.0.0".to_string(),
        services,
        networks,
        volumes,
        logging: None,
        configs: HashMap::new(),
        secrets: HashMap::new(),
    }
}

#[tokio::test]
async fn test_flywheel_origin_to_scope() {
    let wasm_dir = std::env::temp_dir().join("tpt-flywheel-test");
    std::fs::create_dir_all(&wasm_dir).unwrap();
    let wasm_path = wasm_dir.join("api.wasm");
    std::fs::write(&wasm_path, b"\0asm\x01\x00\x00\x00").unwrap();

    // --- Origin: lifecycle management ---
    let manifest = flywheel_manifest(wasm_path.clone());
    let mut lifecycle = LifecycleManager::new(&manifest);

    lifecycle.up(&manifest).await.unwrap();

    assert!(lifecycle.get_service_status("db").is_some());
    assert!(lifecycle.get_service_status("api").is_some());
    assert!(lifecycle.get_service_status("proxy").is_some());

    for (name, health) in lifecycle.list_services() {
        assert_eq!(health.status, ServiceStatus::Running, "service {name} should be running");
    }

    // --- Origin: DNS resolution ---
    let mut dns = DnsResolver::new();
    for name in manifest.services.keys() {
        let ip = match name.as_str() {
            "db" => "10.0.0.2",
            "api" => "10.0.0.3",
            "proxy" => "10.0.0.4",
            _ => "10.0.0.5",
        };
        dns.add_entry(name, ip, Some(8080));
    }
    assert!(dns.resolve("db").is_some());
    assert!(dns.resolve("api").is_some());
    assert!(dns.resolve("proxy").is_some());
    assert_eq!(dns.entries().len(), 3);

    // --- Origin: network management ---
    let mut network = NetworkManager::new();
    for (name, net_cfg) in &manifest.networks {
        network
            .create_network(NetworkConfig {
                name: name.clone(),
                driver: net_cfg.driver.clone(),
                subnet: None,
                gateway: None,
            })
            .await
            .unwrap();
    }
    assert!(network.get_bridge_interface().is_some());

    // --- Scope: probe pipeline ---
    let mut probe_manager = ProbeManager::new();
    probe_manager.register(Box::new(NetworkProbe::new("origin-net")));
    probe_manager.register(Box::new(SyscallProbe::new(
        "origin-sys",
        vec![SyscallEventType::Read, SyscallEventType::Write],
    )));
    probe_manager.register(Box::new(WasmProbe::new("origin-wasm")));

    probe_manager.attach_all().unwrap();

    let probes = probe_manager.list_probes();
    assert_eq!(probes.len(), 3);
    for (_, _, state) in &probes {
        assert!(matches!(state, ProbeState::Attached));
    }

    // --- Scope: event ingestion simulation ---
    let mut events: Vec<ProbeEvent> = Vec::new();

    events.push(ProbeEvent {
        timestamp: 1700000000,
        category: ProbeCategory::Network,
        data: EventData::Network(NetworkEvent {
            event_type: NetworkEventType::TcpConnect,
            src_addr: "10.0.0.3".parse().unwrap(),
            dst_addr: "10.0.0.2".parse().unwrap(),
            src_port: 45678,
            dst_port: 5432,
            bytes: 0,
            proto: TransportProtocol::Tcp,
        }),
        pid: 1001,
        tid: 1001,
        comm: "api".to_string(),
    });

    events.push(ProbeEvent {
        timestamp: 1700000001,
        category: ProbeCategory::WasmRuntime,
        data: EventData::WasmRuntime(WasmEvent {
            event_type: WasmEventType::Instantiate,
            module_name: "api".to_string(),
            instance_id: 1,
            duration: Duration::from_millis(12),
        }),
        pid: 1002,
        tid: 1002,
        comm: "wasm-runtime".to_string(),
    });

    events.push(ProbeEvent {
        timestamp: 1700000002,
        category: ProbeCategory::Syscall,
        data: EventData::Syscall(SyscallEvent {
            event_type: SyscallEventType::Read,
            syscall_nr: 63,
            path: Some("/var/lib/postgresql/data".to_string()),
            fd: Some(4),
            bytes_rw: Some(4096),
            exit_code: 0,
            latency: Duration::from_micros(150),
        }),
        pid: 1001,
        tid: 1001,
        comm: "api".to_string(),
    });

    assert_eq!(events.len(), 3);

    // --- Scope: enrichment pipeline ---
    let mut provider = EnrichmentProvider::new();
    for event in &events {
        let enriched = provider.enrich_with_fallback(event.pid);
        assert_eq!(enriched.pid, event.pid);
    }

    // --- Scope: teardown ---
    probe_manager.detach_all().unwrap();
    for (_, _, state) in probe_manager.list_probes() {
        assert!(matches!(state, ProbeState::Detached));
    }

    lifecycle.down().await.unwrap();
    for (_, health) in lifecycle.list_services() {
        assert_eq!(health.status, ServiceStatus::Running);
    }

    let _ = std::fs::remove_dir_all(&wasm_dir);
}

#[tokio::test]
async fn test_flywright_dns_network_roundtrip() {
    let wasm_dir = std::env::temp_dir().join("tpt-flywheel-test-2");
    std::fs::create_dir_all(&wasm_dir).unwrap();
    let wasm_path = wasm_dir.join("api.wasm");
    std::fs::write(&wasm_path, b"\0asm\x01\x00\x00\x00").unwrap();

    let manifest = flywheel_manifest(wasm_path);

    let mut network = NetworkManager::new();
    for (name, net_cfg) in &manifest.networks {
        network
            .create_network(NetworkConfig {
                name: name.clone(),
                driver: net_cfg.driver.clone(),
                subnet: None,
                gateway: None,
            })
            .await
            .unwrap();
    }

    let mut dns = DnsResolver::new();
    let mut ip_counter = 2u8;
    for name in manifest.services.keys() {
        let ip = format!("10.0.0.{ip_counter}");
        dns.add_entry(name, &ip, Some(8080));
        ip_counter += 1;
    }

    for (name, _) in &manifest.services {
        let entry = dns.resolve(name).unwrap();
        assert!(!entry.ip.is_empty());
    }

    for name in manifest.services.keys() {
        network.connect_service(name, "internal").await.unwrap();
    }

    for name in manifest.services.keys() {
        network.disconnect_service(name, "internal").await.unwrap();
    }

    dns.remove_entry("proxy");
    assert!(dns.resolve("proxy").is_none());
    assert_eq!(dns.entries().len(), 2);

    let _ = std::fs::remove_dir_all(&wasm_dir);
}

#[test]
fn test_flywheel_event_category_dispatch() {
    let events = vec![
        ProbeEvent {
            timestamp: 0,
            category: ProbeCategory::Network,
            data: EventData::Network(NetworkEvent {
                event_type: NetworkEventType::TcpAccept,
                src_addr: "0.0.0.0".parse().unwrap(),
                dst_addr: "127.0.0.1".parse().unwrap(),
                src_port: 80,
                dst_port: 9000,
                bytes: 1024,
                proto: TransportProtocol::Tcp,
            }),
            pid: 1,
            tid: 1,
            comm: "proxy".to_string(),
        },
        ProbeEvent {
            timestamp: 1,
            category: ProbeCategory::Syscall,
            data: EventData::Syscall(SyscallEvent {
                event_type: SyscallEventType::Open,
                syscall_nr: 257,
                path: Some("/etc/hosts".to_string()),
                fd: None,
                bytes_rw: None,
                exit_code: 0,
                latency: Duration::from_micros(50),
            }),
            pid: 2,
            tid: 2,
            comm: "api".to_string(),
        },
        ProbeEvent {
            timestamp: 2,
            category: ProbeCategory::WasmRuntime,
            data: EventData::WasmRuntime(WasmEvent {
                event_type: WasmEventType::CompileEnd,
                module_name: "api".to_string(),
                instance_id: 0,
                duration: Duration::from_millis(200),
            }),
            pid: 2,
            tid: 2,
            comm: "wasm-compile".to_string(),
        },
    ];

    let mut counts = HashMap::new();
    for event in &events {
        *counts.entry(format!("{:?}", event.category)).or_insert(0) += 1;
    }

    assert_eq!(counts.get("Network"), Some(&1));
    assert_eq!(counts.get("Syscall"), Some(&1));
    assert_eq!(counts.get("WasmRuntime"), Some(&1));
}
