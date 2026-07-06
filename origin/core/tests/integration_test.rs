use std::collections::HashMap;
use tpt_origin_core::dns::DnsResolver;
use tpt_origin_core::lifecycle::LifecycleManager;
use tpt_origin_core::manifest::{
    Manifest, OCIService, PortMapping, Service, VolumeMount, WasmService,
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
        }),
    );
    services.insert(
        "transform".to_string(),
        Service::Wasm(WasmService {
            path: wasm_path,
            args: vec!["--workers".to_string(), "4".to_string()],
            environment: HashMap::new(),
            memory_limit: Some("128m".to_string()),
            depends_on: vec!["api".to_string()],
            expected_signature: None,
            trusted_public_key: None,
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
    let wasm_dir = std::env::temp_dir().join("tpt-integration-lifecycle");
    std::fs::create_dir_all(&wasm_dir).unwrap();
    let wasm_path = wasm_dir.join("transform.wasm");
    std::fs::write(&wasm_path, b"\0asm\x01\x00\x00\x00").unwrap();

    let manifest = sample_manifest(wasm_path);
    let mut lm = LifecycleManager::new(&manifest);
    assert_eq!(
        lm.get_service_status("api").map(|s| &s.name),
        None
    );

    lm.up(&manifest).await.unwrap();

    let api_health = lm.get_service_status("api").expect("api health missing");
    assert_eq!(api_health.name, "api");

    let wasm_health = lm.get_service_status("transform").expect("transform health missing");
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
    assert!(net.get_bridge_interface().is_some());

    net.connect_service("api", "app-net").await.unwrap();
    net.disconnect_service("api", "app-net").await.unwrap();

    net.delete_network("app-net").await.unwrap();
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
