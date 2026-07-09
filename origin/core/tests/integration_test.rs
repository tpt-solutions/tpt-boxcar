use std::collections::HashMap;
use tpt_origin_core::dns::DnsResolver;
use tpt_origin_core::lifecycle::LifecycleManager;
use tpt_origin_core::manifest::{
    Manifest, OCIService, PortMapping, ProcessService, Service, VolumeMount, WasmService,
};
use tpt_origin_core::network::{NetworkConfig, NetworkManager};

fn sample_manifest(wasm_path: std::path::PathBuf) -> Manifest {
    let mut services = HashMap::new();
    services.insert(
        "api".to_string(),
        Service::OCI(OCIService {
            image: "node:20-alpine".to_string(),
            ports: vec![PortMapping {
                host: 3000,
                container: 3000,
                protocol: "tcp".to_string(),
            }],
            environment: HashMap::from([("NODE_ENV".to_string(), "production".to_string())]),
            volumes: vec![VolumeMount {
                source: "app-data".to_string(),
                target: "/data".to_string(),
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
        }),
    );
    services.insert(
        "transform".to_string(),
        Service::Wasm(WasmService {
            path: wasm_path,
            args: vec!["--workers".to_string(), "4".to_string()],
            ports: vec![],
            environment: HashMap::new(),
            memory_limit: Some("128m".to_string()),
            resources: None,
            depends_on: vec!["api".to_string()],
            expected_signature: None,
            trusted_public_key: None,
            restart_policy: Default::default(),
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
        }),
    );

    let mut networks = HashMap::new();
    networks.insert(
        "frontend".to_string(),
        tpt_origin_core::manifest::Network {
            driver: "bridge".to_string(),
        },
    );

    let mut volumes = HashMap::new();
    volumes.insert(
        "app-data".to_string(),
        tpt_origin_core::manifest::Volume {
            driver: Some("local".to_string()),
        },
    );

    Manifest {
        name: "integration-test".to_string(),
        version: "1.0.0".to_string(),
        services,
        networks,
        volumes,
        logging: None,
    }
}

#[test]
fn test_manifest_parse_oci_and_wasm_services() {
    let wasm_dir = std::env::temp_dir().join("tpt-integration-test");
    std::fs::create_dir_all(&wasm_dir).unwrap();
    let wasm_path = wasm_dir.join("transform.wasm");
    std::fs::write(&wasm_path, b"\0asm\x01\x00\x00\x00").unwrap();

    let manifest = sample_manifest(wasm_path.clone());
    assert_eq!(manifest.name, "integration-test");
    assert_eq!(manifest.services.len(), 2);

    match manifest.services.get("api").unwrap() {
        Service::OCI(oci) => {
            assert_eq!(oci.image, "node:20-alpine");
            assert_eq!(oci.ports.len(), 1);
            assert_eq!(oci.ports[0].host, 3000);
            assert!(oci.depends_on.is_empty());
            assert_eq!(oci.volumes.len(), 1);
            assert_eq!(oci.volumes[0].target, "/data");
        }
        _ => panic!("expected OCI service"),
    }

    match manifest.services.get("transform").unwrap() {
        Service::Wasm(wasm) => {
            assert_eq!(wasm.path, wasm_path);
            assert_eq!(wasm.args, vec!["--workers", "4"]);
            assert_eq!(wasm.memory_limit.as_deref(), Some("128m"));
            assert_eq!(wasm.depends_on, vec!["api"]);
        }
        _ => panic!("expected Wasm service"),
    }

    assert!(manifest.networks.contains_key("frontend"));
    assert!(manifest.volumes.contains_key("app-data"));
    let _ = std::fs::remove_dir_all(&wasm_dir);
}

#[test]
fn test_manifest_yaml_roundtrip() {
    let yaml = r#"
name: roundtrip
version: "2.0"
services:
  worker:
    type: oci
    image: redis:7-alpine
    ports:
      - host: 6379
        container: 6379
  proxy:
    type: wasm
    path: ./proxy.wasm
    depends_on:
      - worker
networks:
  backend:
    driver: overlay
volumes:
  cache: {}
"#;
    let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(manifest.name, "roundtrip");
    assert_eq!(manifest.services.len(), 2);
    assert!(manifest.networks.contains_key("backend"));

    let serialized = serde_yaml::to_string(&manifest).unwrap();
    let deserialized: Manifest = serde_yaml::from_str(&serialized).unwrap();
    assert_eq!(deserialized.name, manifest.name);
    assert_eq!(deserialized.services.len(), manifest.services.len());
}

#[tokio::test]
async fn test_lifecycle_manager_creation() {
    // Bind DNS to an ephemeral port: the standard mDNS port 5353 can
    // legitimately already be held by something else on the host (a system
    // resolver, a port-exclusion range, etc), and this test doesn't depend
    // on which port `up()` actually binds.
    std::env::set_var("ORIGIN_DNS_LISTEN_ADDR", "127.0.0.1:0");

    let wasm_dir = std::env::temp_dir().join("tpt-integration-lifecycle");
    std::fs::create_dir_all(&wasm_dir).unwrap();
    let wasm_path = wasm_dir.join("transform.wasm");
    std::fs::write(&wasm_path, b"\0asm\x01\x00\x00\x00").unwrap();

    // A dedicated manifest rather than `sample_manifest()`: that fixture's
    // "api" is deliberately `Service::OCI` so `test_manifest_parse_oci_and_wasm_services`
    // can validate OCI field parsing, but OCI services now require a real
    // Linux + containerd host (see runtime.rs). This test's job is
    // exercising `LifecycleManager::up`/`down` bookkeeping, not containerd,
    // so it uses a portable `Service::Process` instead.
    let mut services = HashMap::new();
    services.insert(
        "api".to_string(),
        Service::Process(ProcessService {
            command: if cfg!(windows) {
                vec![
                    "ping".to_string(),
                    "-n".to_string(),
                    "20".to_string(),
                    "127.0.0.1".to_string(),
                ]
            } else {
                vec!["sleep".to_string(), "20".to_string()]
            },
            ports: vec![],
            environment: HashMap::new(),
            working_dir: None,
            depends_on: vec![],
            resources: None,
            restart_policy: Default::default(),
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
        }),
    );
    services.insert(
        "transform".to_string(),
        Service::Wasm(WasmService {
            path: wasm_path,
            args: vec![],
            ports: vec![],
            environment: HashMap::new(),
            memory_limit: None,
            resources: None,
            depends_on: vec!["api".to_string()],
            expected_signature: None,
            trusted_public_key: None,
            restart_policy: Default::default(),
            env_file: None,
            secrets: None,
            security: None,
            logging: None,
        }),
    );
    let manifest = Manifest {
        name: "lifecycle-test".to_string(),
        version: String::new(),
        services,
        networks: HashMap::new(),
        volumes: HashMap::new(),
        logging: None,
    };
    let mut lm = LifecycleManager::new(&manifest);
    assert_eq!(lm.get_service_status("api").map(|s| &s.name), None);

    lm.up(&manifest).await.unwrap();

    let api_health = lm.get_service_status("api").expect("api health missing");
    assert_eq!(api_health.name, "api");

    let wasm_health = lm
        .get_service_status("transform")
        .expect("transform health missing");
    assert_eq!(wasm_health.name, "transform");

    let all = lm.list_services();
    assert_eq!(all.len(), 2);

    lm.down().await.unwrap();
    let _ = std::fs::remove_dir_all(&wasm_dir);
}

#[test]
fn test_dns_resolver_add_resolve_remove() {
    let mut dns = DnsResolver::new();
    assert!(dns.resolve("web").is_none());

    dns.add_entry("web", "10.0.0.2", Some(8080));
    let entry = dns.resolve("web").expect("web entry should resolve");
    assert_eq!(entry.ip, "10.0.0.2");
    assert_eq!(entry.port, Some(8080));
    assert_eq!(entry.name, "web");

    assert_eq!(dns.entries().len(), 1);

    dns.add_entry("db", "10.0.0.3", Some(5432));
    assert_eq!(dns.entries().len(), 2);

    dns.remove_entry("web");
    assert!(dns.resolve("web").is_none());
    assert_eq!(dns.entries().len(), 1);

    assert!(dns.resolve("db").is_some());
}

#[test]
fn test_dns_fqdn_handling() {
    let mut dns = DnsResolver::new();
    dns.add_entry("cache.local", "10.0.0.4", None);
    assert!(dns.resolve("cache.local").is_some());

    dns.add_entry("worker", "10.0.0.5", None);
    assert!(dns.resolve("worker").is_some());
    assert!(dns.resolve("worker.local").is_some());
}

/// Exercises the bookkeeping side of `NetworkManager` that must work
/// regardless of privilege level: real device creation (bridge on Linux,
/// `if_bridge` on macOS, Hyper-V internal switch on Windows) additionally
/// requires root/CAP_NET_ADMIN or an elevated process, so it isn't asserted
/// here — see `network_manager_creates_a_real_linux_bridge` for that.
#[tokio::test]
async fn test_network_manager_create_delete() {
    let mut net = NetworkManager::new();

    net.create_network(NetworkConfig {
        name: "app-net".to_string(),
        driver: "bridge".to_string(),
        subnet: Some("10.1.0.0/16".to_string()),
        gateway: Some("10.1.0.1".to_string()),
    })
    .await
    .unwrap();
    assert_eq!(net.network_gateway("app-net"), Some("10.1.0.1"));
    assert_eq!(net.network_driver("app-net"), Some("bridge"));

    let ip = net.connect_service("api", "app-net").await.unwrap();
    assert_eq!(net.assigned_ip("app-net", "api"), Some(ip.as_str()));
    net.disconnect_service("api", "app-net").await.unwrap();
    assert_eq!(net.assigned_ip("app-net", "api"), None);

    net.delete_network("app-net").await.unwrap();
}

/// Real end-to-end proof that `create_network` creates an actual Linux
/// bridge device (not just a fabricated name string): requires
/// root/CAP_NET_ADMIN and `iproute2`. Run manually with:
/// `sudo cargo test -p tpt-origin-core --test integration_test -- --ignored network_manager_creates_a_real_linux_bridge`
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore]
async fn network_manager_creates_a_real_linux_bridge() {
    let mut net = NetworkManager::new();
    net.create_network(NetworkConfig {
        name: "real-bridge-test".to_string(),
        driver: "bridge".to_string(),
        subnet: None,
        gateway: None,
    })
    .await
    .unwrap();

    let bridge = net
        .get_bridge_interface()
        .expect("real bridge device should have been created");
    let output = std::process::Command::new("ip")
        .args(["link", "show", bridge])
        .output()
        .expect("failed to run `ip link show`");
    assert!(
        output.status.success(),
        "bridge device {bridge} should really exist in the kernel"
    );

    net.delete_network("real-bridge-test").await.unwrap();
    let after = std::process::Command::new("ip")
        .args(["link", "show", bridge])
        .output()
        .expect("failed to run `ip link show`");
    assert!(
        !after.status.success(),
        "bridge device {bridge} should be gone after delete_network"
    );
}

#[tokio::test]
async fn test_network_manager_multiple_networks() {
    let mut net = NetworkManager::new();

    net.create_network(NetworkConfig {
        name: "frontend".to_string(),
        driver: "bridge".to_string(),
        subnet: None,
        gateway: None,
    })
    .await
    .unwrap();

    net.create_network(NetworkConfig {
        name: "backend".to_string(),
        driver: "bridge".to_string(),
        subnet: None,
        gateway: None,
    })
    .await
    .unwrap();

    net.delete_network("frontend").await.unwrap();
    net.delete_network("backend").await.unwrap();
}
