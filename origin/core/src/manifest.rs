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

/// A file-mounted secret, analogous to Docker/K8s secret mounts.
/// The secret content is read from `source` on the host and mounted
/// into the container/process at `target`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMount {
    /// Logical name for the secret (used in logs/diagnostics).
    pub name: String,
    /// Host path to the secret file (e.g. `./secrets/db_password.txt`).
    pub source: PathBuf,
    /// Path inside the container/process where the secret is mounted.
    pub target: String,
    /// File permission mode inside the container (octal string, e.g. `"0400"`).
    /// Defaults to `0400` (read-only for owner).
    #[serde(default = "default_secret_mode")]
    pub mode: String,
}

fn default_secret_mode() -> String {
    "0400".to_string()
}

/// Security configuration for a service, analogous to Docker's
/// `--cap-add`/`--cap-drop`, `--read-only`, and `--security-opt`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Linux capabilities to add (e.g. `["NET_BIND_SERVICE", "SYS_PTRACE"]`).
    #[serde(default)]
    pub cap_add: Vec<String>,
    /// Linux capabilities to drop. When non-empty, ALL capabilities are
    /// dropped first, then `cap_add` ones are restored.
    #[serde(default)]
    pub cap_drop: Vec<String>,
    /// Mount the container's root filesystem as read-only.
    #[serde(default)]
    pub read_only: bool,
    /// Prevent the process from gaining new privileges via setuid/setgid
    /// binaries (equivalent to Docker's `--security-opt no-new-privileges`).
    #[serde(default)]
    pub no_new_privileges: bool,
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

    pub fn ports(&self) -> &[PortMapping] {
        match self {
            Service::OCI(s) => &s.ports,
            Service::Wasm(s) => &s.ports,
            Service::Process(s) => &s.ports,
        }
    }

    /// Path(s) to `.env` files to load environment variables from.
    pub fn env_file(&self) -> &Option<Vec<String>> {
        match self {
            Service::OCI(s) => &s.env_file,
            Service::Wasm(s) => &s.env_file,
            Service::Process(s) => &s.env_file,
        }
    }

    /// Secret mounts for this service.
    pub fn secrets(&self) -> &Option<Vec<SecretMount>> {
        match self {
            Service::OCI(s) => &s.secrets,
            Service::Wasm(s) => &s.secrets,
            Service::Process(s) => &s.secrets,
        }
    }

    /// Security configuration for this service.
    pub fn security(&self) -> &Option<SecurityConfig> {
        match self {
            Service::OCI(s) => &s.security,
            Service::Wasm(s) => &s.security,
            Service::Process(s) => &s.security,
        }
    }

    /// Mutable reference to the environment HashMap, so callers can merge
    /// env_file variables before starting the service.
    pub fn environment_mut(&mut self) -> &mut HashMap<String, String> {
        match self {
            Service::OCI(s) => &mut s.environment,
            Service::Wasm(s) => &mut s.environment,
            Service::Process(s) => &mut s.environment,
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
    pub ports: Vec<PortMapping>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub resources: Option<ResourceLimits>,
    #[serde(default)]
    pub restart_policy: RestartPolicy,
    /// Path(s) to `.env` files. Variables are loaded in order (last wins)
    /// and merged with `environment` (explicit `environment:` takes precedence).
    #[serde(default)]
    pub env_file: Option<Vec<String>>,
    /// Secrets mounted as files into the container/process.
    #[serde(default)]
    pub secrets: Option<Vec<SecretMount>>,
    /// Security hardening: capabilities, read-only rootfs, no_new_privileges.
    #[serde(default)]
    pub security: Option<SecurityConfig>,
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
    #[serde(default)]
    pub env_file: Option<Vec<String>>,
    #[serde(default)]
    pub secrets: Option<Vec<SecretMount>>,
    #[serde(default)]
    pub security: Option<SecurityConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmService {
    pub path: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub ports: Vec<PortMapping>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[serde(default)]
    pub memory_limit: Option<String>,
    #[serde(default)]
    pub resources: Option<ResourceLimits>,
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
    #[serde(default)]
    pub env_file: Option<Vec<String>>,
    #[serde(default)]
    pub secrets: Option<Vec<SecretMount>>,
    #[serde(default)]
    pub security: Option<SecurityConfig>,
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

    #[test]
    fn test_parse_env_file_and_secrets() {
        let yaml = r#"
name: secrets-app
services:
  api:
    type: oci
    image: myapp:latest
    env_file:
      - .env
      - .env.local
    secrets:
      - name: db_password
        source: ./secrets/db_password.txt
        target: /run/secrets/db_password
        mode: "0400"
    security:
      cap_drop:
        - ALL
      cap_add:
        - NET_BIND_SERVICE
      read_only: true
      no_new_privileges: true
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        let api = match manifest.services.get("api").unwrap() {
            Service::OCI(s) => s,
            _ => panic!("expected OCI service"),
        };
        assert_eq!(
            api.env_file,
            Some(vec![".env".to_string(), ".env.local".to_string()])
        );
        let secrets = api.secrets.as_ref().unwrap();
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].name, "db_password");
        assert_eq!(secrets[0].target, "/run/secrets/db_password");
        assert_eq!(secrets[0].mode, "0400");

        let security = api.security.as_ref().unwrap();
        assert_eq!(security.cap_drop, vec!["ALL"]);
        assert_eq!(security.cap_add, vec!["NET_BIND_SERVICE"]);
        assert!(security.read_only);
        assert!(security.no_new_privileges);
    }

    #[test]
    fn test_parse_process_service_with_security() {
        let yaml = r#"
name: process-app
services:
  worker:
    type: process
    command: ["./worker"]
    security:
      cap_drop:
        - ALL
      no_new_privileges: true
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        let worker = match manifest.services.get("worker").unwrap() {
            Service::Process(s) => s,
            _ => panic!("expected Process service"),
        };
        let security = worker.security.as_ref().unwrap();
        assert_eq!(security.cap_drop, vec!["ALL"]);
        assert!(security.no_new_privileges);
    }

    #[test]
    fn test_parse_wasm_service_with_env_file() {
        let yaml = r#"
name: wasm-app
services:
  handler:
    type: wasm
    path: ./handler.wasm
    env_file:
      - secrets.env
    secrets:
      - name: api_key
        source: ./keys/api_key.txt
        target: /tmp/api_key
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        let handler = match manifest.services.get("handler").unwrap() {
            Service::Wasm(s) => s,
            _ => panic!("expected Wasm service"),
        };
        assert_eq!(handler.env_file, Some(vec!["secrets.env".to_string()]));
        assert!(handler.secrets.is_some());
    }

    #[test]
    fn test_service_accessor_methods() {
        let yaml = r#"
name: accessor-test
services:
  api:
    type: process
    command: ["./api"]
    env_file:
      - .env
    secrets:
      - name: key
        source: ./key.txt
        target: /key
    security:
      read_only: true
"#;
        let manifest: Manifest = serde_yaml::from_str(yaml).unwrap();
        let api = manifest.services.get("api").unwrap();
        assert!(api.env_file().is_some());
        assert!(api.secrets().is_some());
        assert!(api.security().is_some());
    }
}
