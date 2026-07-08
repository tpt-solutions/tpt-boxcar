use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub services: HashMap<String, Service>,
    #[serde(default)]
    pub networks: HashMap<String, Network>,
    #[serde(default)]
    pub volumes: HashMap<String, Volume>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Service {
    #[serde(rename = "oci")]
    OCI(OCIService),
    #[serde(rename = "wasm")]
    Wasm(WasmService),
    #[serde(rename = "process")]
    Process(ProcessService),
}

impl Service {
    /// Names of services this one depends on, used to order startup so a
    /// service isn't started before what it needs is already up.
    pub fn depends_on(&self) -> &[String] {
        match self {
            Service::OCI(s) => &s.depends_on,
            Service::Wasm(s) => &s.depends_on,
            Service::Process(s) => &s.depends_on,
        }
    }

    pub fn restart_policy(&self) -> RestartPolicy {
        match self {
            Service::OCI(s) => s.restart_policy,
            Service::Wasm(s) => s.restart_policy,
            Service::Process(s) => s.restart_policy,
        }
    }
}

/// Whether `LifecycleManager::reap_and_restart` should bring a service back
/// up after it exits on its own (crash, or — for `OnFailure` — any nonzero
/// exit). Mirrors Docker's `--restart` policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    #[default]
    Never,
    OnFailure,
    Always,
}

/// A native process service: runs a pre-built binary directly (no
/// containerd/Wasmtime involved). Useful for driving already-compiled
/// control-plane binaries (e.g. the Go control planes) from an Origin
/// manifest without requiring real OCI/Wasm runtime integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessService {
    pub command: Vec<String>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub restart_policy: RestartPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OCIService {
    pub image: String,
    #[serde(default)]
    pub ports: Vec<PortMapping>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[serde(default)]
    pub volumes: Vec<VolumeMount>,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub healthcheck: Option<HealthCheck>,
    #[serde(default)]
    pub resources: Option<ResourceLimits>,
    #[serde(default)]
    pub restart_policy: RestartPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmService {
    pub path: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[serde(default)]
    pub memory_limit: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Hex-encoded ed25519 signature over the module's compiled wasm bytes,
    /// produced by Chisel's `WasmPipeline::sign_module`. If set alongside
    /// `trusted_public_key`, Origin verifies it before instantiation and
    /// refuses to load on mismatch. If either is unset, the module loads
    /// unverified (backward compatible with existing manifests).
    #[serde(default)]
    pub expected_signature: Option<String>,
    #[serde(default)]
    pub trusted_public_key: Option<String>,
    #[serde(default)]
    pub restart_policy: RestartPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortMapping {
    pub host: u16,
    pub container: u16,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "tcp".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolumeMount {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub command: Vec<String>,
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_retries")]
    pub retries: u32,
}

fn default_interval() -> u64 {
    30
}
fn default_timeout() -> u64 {
    5
}
fn default_retries() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub cpu: Option<String>,
    pub memory: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Network {
    #[serde(default = "default_bridge")]
    pub driver: String,
}

fn default_bridge() -> String {
    "bridge".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Volume {
    pub driver: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_manifest() {
        let yaml = r#"
name: my-app
services:
  db:
    type: oci
    image: postgres:16
  api:
    type: wasm
    path: ./target/api.wasm
    depends_on:
      - db
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.name, "my-app");
        assert_eq!(manifest.services.len(), 2);
    }

    #[test]
    fn test_parse_full_manifest() {
        let yaml = r#"
name: full-app
version: "1.0"
services:
  db:
    type: oci
    image: postgres:16
    ports:
      - host: 5432
        container: 5432
    environment:
      POSTGRES_PASSWORD: secret
    volumes:
      - source: pgdata
        target: /var/lib/postgresql/data
    healthcheck:
      command: ["pg_isready"]
      interval_secs: 10
      timeout_secs: 5
      retries: 5
    resources:
      cpu: "1.0"
      memory: "512m"
  api:
    type: wasm
    path: ./target/api.wasm
    args: ["--port", "8080"]
    environment:
      DB_HOST: db
    memory_limit: "256m"
    depends_on:
      - db
networks:
  default:
    driver: bridge
volumes:
  pgdata: {}
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(manifest.services.len(), 2);
        assert!(manifest.networks.contains_key("default"));
        assert!(manifest.volumes.contains_key("pgdata"));
    }
}
