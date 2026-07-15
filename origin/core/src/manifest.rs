use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::result::Result;

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
    /// Top-level logging configuration applied to all services.
    /// Per-service `logging:` sections override these values.
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
    /// Top-level build configuration for services with `build:` references.
    #[serde(default)]
    pub configs: HashMap<String, ConfigDef>,
    /// Top-level secrets configuration.
    #[serde(default)]
    pub secrets: HashMap<String, SecretDef>,
}

/// Top-level config definition, analogous to Docker Compose's `configs:`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigDef {
    /// File path to the config content.
    pub file: Option<String>,
    /// Inline config content (alternative to `file`).
    pub content: Option<String>,
}

/// Top-level secret definition, analogous to Docker Compose's `secrets:`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretDef {
    /// File path to the secret content.
    pub file: Option<String>,
    /// Inline secret content (alternative to `file`).
    pub content: Option<String>,
    /// External secret name (reference to a pre-existing secret).
    pub external: Option<bool>,
    /// Name of the external secret.
    pub name: Option<String>,
}

/// Logging driver configuration, analogous to Docker's logging options.
/// Controls how a service's stdout/stderr is captured and where it goes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Logging driver: `"file"` (default, writes to a local log file with
    /// optional rotation), `"journald"` (forwards to systemd journal), or
    /// `"none"` (disables log capture).
    #[serde(default = "default_log_driver")]
    pub driver: String,
    /// Maximum size of a single log file before rotation is triggered.
    /// Accepts human-readable sizes: `"10m"`, `"1g"`, `"512k"`.
    /// `None` means unlimited (rotation only by file count).
    #[serde(default)]
    pub max_size: Option<String>,
    /// Number of rotated log files to keep. `Some(3)` means the main log
    /// file plus `.1`, `.2`, `.3` rotated copies. `None` or `Some(0)`
    /// disables rotation by count (only by `max_size`).
    #[serde(default)]
    pub max_file: Option<u32>,
}

fn default_log_driver() -> String {
    "file".to_string()
}

/// A file-mounted secret, analogous to Docker/K8s secret mounts.
/// The secret content is read from `source` on the host and mounted
/// into the container/process at `target`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMount {
    /// Logical name for the secret (used in logs/diagnostics).
    pub name: String,
    /// Host path to the secret file (e.g. `./secrets/db_password.txt`).
    /// Optional when using top-level `secrets:` definitions.
    #[serde(default)]
    pub source: Option<PathBuf>,
    /// Path inside the container/process where the secret is mounted.
    #[serde(default = "default_secret_target")]
    pub target: String,
    /// File permission mode inside the container (octal string, e.g. `"0400"`).
    /// Defaults to `0400` (read-only for owner).
    #[serde(default = "default_secret_mode")]
    pub mode: String,
    /// Reference to a top-level secret definition by name.
    #[serde(default)]
    pub name_ref: Option<String>,
}

fn default_secret_target() -> String {
    "/run/secrets".to_string()
}

fn default_secret_mode() -> String {
    "0400".to_string()
}

/// Build configuration for a service, analogous to Docker Compose's `build:`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BuildConfig {
    /// Simple form: just a path to the build context.
    Path(String),
    /// Detailed form with full build options.
    Details(BuildDetails),
}

/// Detailed build configuration, analogous to Docker Compose's `build:` object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildDetails {
    /// Path to the build context directory.
    pub context: String,
    /// Path to Dockerfile (relative to context, or absolute).
    #[serde(default)]
    pub dockerfile: Option<String>,
    /// Build arguments.
    #[serde(default)]
    pub args: HashMap<String, String>,
    /// Target stage in a multi-stage Dockerfile.
    #[serde(default)]
    pub target: Option<String>,
    /// Additional build tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Cache-from images.
    #[serde(default)]
    pub cache_from: Vec<String>,
    /// Cache-to destination.
    #[serde(default)]
    pub cache_to: Option<String>,
    /// Extra hosts to add to /etc/hosts during build.
    #[serde(default)]
    pub extra_hosts: HashMap<String, String>,
    /// Privileges configuration for build.
    #[serde(default)]
    pub privileged: Option<bool>,
    /// Platform to build for.
    #[serde(default)]
    pub platforms: Vec<String>,
    /// Labels to apply to the image.
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Shm_size for build.
    #[serde(default)]
    pub shm_size: Option<String>,
    /// Network mode during build.
    #[serde(default)]
    pub network: Option<String>,
    /// Target platform for the build.
    #[serde(default)]
    pub target_platform: Option<String>,
    /// Cache-from configuration.
    #[serde(default)]
    pub cache_from_config: Option<String>,
    /// Cache-to configuration.
    #[serde(default)]
    pub cache_to_config: Option<String>,
    /// Build isolation.
    #[serde(default)]
    pub isolation: Option<String>,
    /// Network during build.
    #[serde(default)]
    pub network_mode: Option<String>,
    /// Extra hosts during build.
    #[serde(default)]
    pub extra_hosts_config: Option<String>,
    /// Privileged during build.
    #[serde(default)]
    pub privileged_config: Option<bool>,
    /// Shm_size during build.
    #[serde(default)]
    pub shm_size_config: Option<String>,
    /// Labels during build.
    #[serde(default)]
    pub labels_config: Option<String>,
    /// Tags during build.
    #[serde(default)]
    pub tags_config: Option<Vec<String>>,
    /// Platforms during build.
    #[serde(default)]
    pub platforms_config: Option<Vec<String>>,
    /// Target during build.
    #[serde(default)]
    pub target_config: Option<String>,
    /// Cache-from during build.
    #[serde(default)]
    pub cache_from_config2: Option<String>,
    /// Cache-to during build.
    #[serde(default)]
    pub cache_to_config2: Option<String>,
}

/// Extends configuration for service inheritance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendsConfig {
    /// The base service to extend from.
    pub service: String,
    /// Optional manifest file containing the base service.
    #[serde(default)]
    pub file: Option<String>,
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
    /// Devices to pass through to the container (e.g. GPUs, TUN/TAP, etc.).
    #[serde(default)]
    pub devices: Vec<DeviceMapping>,
}

/// Device mapping for container passthrough, analogous to Docker's `--device`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMapping {
    /// Host device path (e.g. `/dev/nvidia0`).
    pub path_on_host: String,
    /// Device path inside the container (defaults to same as host).
    #[serde(default)]
    pub path_in_container: Option<String>,
    /// Device cgroup permissions (e.g. `"rwm"` for read/write/mknod).
    /// Defaults to `"rwm"`.
    #[serde(default = "default_device_permissions")]
    pub permissions: String,
}

fn default_device_permissions() -> String {
    "rwm".to_string()
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

    /// Per-service logging override, if any.
    pub fn logging(&self) -> &Option<LoggingConfig> {
        match self {
            Service::OCI(s) => &s.logging,
            Service::Wasm(s) => &s.logging,
            Service::Process(s) => &s.logging,
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

    /// Profiles this service belongs to. Empty means it runs in all profiles.
    pub fn profiles(&self) -> &[String] {
        match self {
            Service::OCI(s) => &s.profiles,
            Service::Wasm(s) => &s.profiles,
            Service::Process(s) => &s.profiles,
        }
    }

    /// Build configuration for OCI services.
    pub fn build(&self) -> Option<&BuildConfig> {
        match self {
            Service::OCI(s) => s.build.as_ref(),
            _ => None,
        }
    }

    /// Configs mounted into the service.
    pub fn configs(&self) -> &Option<Vec<ConfigMount>> {
        match self {
            Service::OCI(s) => &s.configs,
            Service::Wasm(s) => &s.configs,
            Service::Process(s) => &s.configs,
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
    /// Per-service logging override. When `None`, the manifest-level
    /// `logging:` config (or built-in defaults) is used.
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
    /// Health check configuration for process services.
    #[serde(default)]
    pub healthcheck: Option<HealthCheck>,
    /// Profiles this service belongs to. Empty means it runs in all profiles.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// Configs mounted into the service.
    #[serde(default)]
    pub configs: Option<Vec<ConfigMount>>,
    /// Extends configuration for service inheritance.
    #[serde(default)]
    pub extends: Option<ExtendsConfig>,
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
    /// Per-service logging override.
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
    /// Profiles this service belongs to. Empty means it runs in all profiles.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// Build configuration for the image.
    #[serde(default)]
    pub build: Option<BuildConfig>,
    /// Configs mounted into the service.
    #[serde(default)]
    pub configs: Option<Vec<ConfigMount>>,
    /// Extends configuration for service inheritance.
    #[serde(default)]
    pub extends: Option<ExtendsConfig>,
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
    /// Per-service logging override.
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
    /// Health check configuration for Wasm services.
    #[serde(default)]
    pub healthcheck: Option<HealthCheck>,
    /// Profiles this service belongs to. Empty means it runs in all profiles.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// Configs mounted into the service.
    #[serde(default)]
    pub configs: Option<Vec<ConfigMount>>,
    /// Extends configuration for service inheritance.
    #[serde(default)]
    pub extends: Option<ExtendsConfig>,
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
    /// Mount type: `bind` (default, bind mount), `tmpfs` (in-memory filesystem),
    /// or `volume` (named volume).
    #[serde(default = "default_mount_type")]
    pub mount_type: MountType,
    /// Options for tmpfs mounts (e.g., "size=100m,mode=755").
    #[serde(default)]
    pub tmpfs_options: Option<String>,
}

fn default_mount_type() -> MountType {
    MountType::Bind
}

/// Mount type for volume mounts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MountType {
    /// Bind mount (default). Mounts a host path into the container.
    #[default]
    Bind,
    /// In-memory filesystem. Creates a tmpfs mount in the container.
    Tmpfs,
    /// Named volume. Uses a managed directory under the volumes base path.
    Volume,
}

/// Config mount for a service, analogous to Docker Compose's `configs:`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigMount {
    /// Simple form: just the config name.
    Name(String),
    /// Detailed form with source and target.
    Details(ConfigMountDetails),
}

/// Detailed config mount configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigMountDetails {
    /// Name of the top-level config to mount.
    pub source: String,
    /// Path inside the container where the config is mounted.
    #[serde(default = "default_config_target")]
    pub target: String,
    /// File permission mode inside the container.
    #[serde(default = "default_config_mode")]
    pub mode: u32,
}

fn default_config_target() -> String {
    String::new()
}

fn default_config_mode() -> u32 {
    0o444
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    /// The health check command to execute (for `check_type: exec`).
    /// Optional: not needed for HTTP/TCP checks.
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_retries")]
    pub retries: u32,
    /// Type of health check probe. Defaults to `exec` for backward
    /// compatibility with existing manifests that only use `command`.
    #[serde(default)]
    pub check_type: CheckType,
    /// Port to probe for `http` and `tcp` check types. For `exec` checks,
    /// this field is ignored.
    #[serde(default)]
    pub port: Option<u16>,
    /// URL path for `http` checks (e.g. `/health`). Defaults to `/`.
    #[serde(default)]
    pub path: Option<String>,
}

/// Health check probe type, analogous to Docker/K8s liveness probe types.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckType {
    /// Execute a command inside the container/process. The command must
    /// exit 0 to be considered healthy. This is the default and matches
    /// existing `command`-only healthcheck behavior.
    #[default]
    Exec,
    /// HTTP GET request to `http://127.0.0.1:<port><path>`. Healthy if
    /// the response status is 2xx.
    Http,
    /// TCP socket connection to `127.0.0.1:<port>`. Healthy if the
    /// connection succeeds.
    Tcp,
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
    /// IPv4 `/24` subnet override (e.g. `"10.88.4.0/24"`); auto-allocated
    /// when omitted.
    #[serde(default)]
    pub subnet: Option<String>,
    /// Gateway address override; defaults to the subnet's `.1`.
    #[serde(default)]
    pub gateway: Option<String>,
    /// VXLAN Network Identifier for `driver: overlay` — required for
    /// multi-host networking, ignored by other drivers.
    #[serde(default)]
    pub vni: Option<u32>,
    /// Remote host IPs to peer with over VXLAN unicast head-end replication
    /// for `driver: overlay` (each peer host must list the others' IPs and
    /// use the same `vni`). Ignored by other drivers.
    #[serde(default)]
    pub peers: Vec<String>,
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

/// Interpolates `${VAR}` and `${VAR:-default}` patterns in a string value.
/// Uses environment variables and an optional overrides map. The overrides
/// map takes precedence over environment variables.
pub fn interpolate_env(
    value: &str,
    env: &HashMap<String, String>,
    overrides: &HashMap<String, String>,
) -> String {
    let mut result = value.to_string();
    let mut pos = 0;

    while pos < result.len() {
        if let Some(start) = result[pos..].find("${") {
            let start = pos + start;
            if let Some(end) = result[start + 2..].find('}') {
                let end = start + 2 + end;
                let var_expr = &result[start + 2..end];

                // Parse the variable name and optional default
                let (var_name, default_value) = if let Some(colon_pos) = var_expr.find(":-") {
                    let name = &var_expr[..colon_pos];
                    let default = &var_expr[colon_pos + 2..];
                    (name, Some(default.to_string()))
                } else {
                    (var_expr, None)
                };

                // Look up the value: overrides first, then env
                let resolved = overrides
                    .get(var_name)
                    .or_else(|| env.get(var_name))
                    .or(default_value.as_ref())
                    .cloned()
                    .unwrap_or_default();

                result = format!("{}{}{}", &result[..start], resolved, &result[end + 1..]);
                pos = start + resolved.len();
            } else {
                pos = start + 2;
            }
        } else {
            break;
        }
    }

    result
}

/// Resolves `extends:` references by merging base service configuration
/// into the target service. Returns a new manifest with resolved services.
pub fn resolve_extends(manifest: &Manifest) -> Result<Manifest, String> {
    let mut resolved = manifest.clone();

    // Build a map of service names to check for circular dependencies
    let mut visited = std::collections::HashSet::new();
    let mut resolving = std::collections::HashSet::new();

    for (name, service) in &manifest.services {
        resolve_extends_recursive(
            name,
            service,
            &mut resolved.services,
            &manifest.services,
            &mut visited,
            &mut resolving,
        )?;
    }

    Ok(resolved)
}

fn resolve_extends_recursive(
    name: &str,
    service: &Service,
    target: &mut HashMap<String, Service>,
    source: &HashMap<String, Service>,
    visited: &mut std::collections::HashSet<String>,
    resolving: &mut std::collections::HashSet<String>,
) -> Result<(), String> {
    let extends = match service {
        Service::OCI(s) => s.extends.as_ref(),
        Service::Wasm(s) => s.extends.as_ref(),
        Service::Process(s) => s.extends.as_ref(),
    };

    let Some(extends_config) = extends else {
        return Ok(());
    };

    let base_name = &extends_config.service;

    // Check for circular dependencies
    if resolving.contains(name) {
        return Err(format!(
            "circular extends dependency detected: {name} -> {base_name}"
        ));
    }

    if visited.contains(name) {
        return Ok(());
    }

    resolving.insert(name.to_string());

    // Get the base service
    let base_service = if let Some(file) = &extends_config.file {
        // Load from external file
        let content = std::fs::read_to_string(file)
            .map_err(|e| format!("failed to read extends file '{file}': {e}"))?;
        let base_manifest: Manifest = serde_yaml::from_str(&content)
            .map_err(|e| format!("failed to parse extends file '{file}': {e}"))?;
        base_manifest
            .services
            .get(base_name)
            .cloned()
            .ok_or_else(|| {
                format!("base service '{base_name}' not found in extends file '{file}'")
            })?
    } else {
        source
            .get(base_name)
            .cloned()
            .ok_or_else(|| format!("base service '{base_name}' not found"))?
    };

    // Resolve the base service recursively
    resolve_extends_recursive(base_name, &base_service, target, source, visited, resolving)?;

    // Merge the base service into the current service
    match (&service, &base_service) {
        (Service::OCI(oci), Service::OCI(base_oci)) => {
            let mut merged = base_oci.clone();
            // Override fields from the child service
            if !oci.image.is_empty() {
                merged.image = oci.image.clone();
            }
            if !oci.ports.is_empty() {
                merged.ports = oci.ports.clone();
            }
            if !oci.environment.is_empty() {
                // Merge environments: base underlay, child overlay
                for (k, v) in &oci.environment {
                    merged.environment.insert(k.clone(), v.clone());
                }
            }
            if !oci.volumes.is_empty() {
                merged.volumes = oci.volumes.clone();
            }
            if oci.command.is_some() {
                merged.command = oci.command.clone();
            }
            if !oci.depends_on.is_empty() {
                merged.depends_on = oci.depends_on.clone();
            }
            if oci.healthcheck.is_some() {
                merged.healthcheck = oci.healthcheck.clone();
            }
            if oci.resources.is_some() {
                merged.resources = oci.resources.clone();
            }
            if oci.restart_policy != RestartPolicy::Never {
                merged.restart_policy = oci.restart_policy;
            }
            if oci.env_file.is_some() {
                merged.env_file = oci.env_file.clone();
            }
            if oci.secrets.is_some() {
                merged.secrets = oci.secrets.clone();
            }
            if oci.security.is_some() {
                merged.security = oci.security.clone();
            }
            if oci.logging.is_some() {
                merged.logging = oci.logging.clone();
            }
            if !oci.profiles.is_empty() {
                merged.profiles = oci.profiles.clone();
            }
            if oci.build.is_some() {
                merged.build = oci.build.clone();
            }
            if oci.configs.is_some() {
                merged.configs = oci.configs.clone();
            }
            target.insert(name.to_string(), Service::OCI(merged));
        }
        (Service::Wasm(wasm), Service::Wasm(base_wasm)) => {
            let mut merged = base_wasm.clone();
            if !wasm.path.as_os_str().is_empty() {
                merged.path = wasm.path.clone();
            }
            if !wasm.args.is_empty() {
                merged.args = wasm.args.clone();
            }
            if !wasm.ports.is_empty() {
                merged.ports = wasm.ports.clone();
            }
            if !wasm.environment.is_empty() {
                for (k, v) in &wasm.environment {
                    merged.environment.insert(k.clone(), v.clone());
                }
            }
            if wasm.memory_limit.is_some() {
                merged.memory_limit = wasm.memory_limit.clone();
            }
            if wasm.resources.is_some() {
                merged.resources = wasm.resources.clone();
            }
            if !wasm.depends_on.is_empty() {
                merged.depends_on = wasm.depends_on.clone();
            }
            if wasm.restart_policy != RestartPolicy::Never {
                merged.restart_policy = wasm.restart_policy;
            }
            if wasm.env_file.is_some() {
                merged.env_file = wasm.env_file.clone();
            }
            if wasm.secrets.is_some() {
                merged.secrets = wasm.secrets.clone();
            }
            if wasm.security.is_some() {
                merged.security = wasm.security.clone();
            }
            if wasm.logging.is_some() {
                merged.logging = wasm.logging.clone();
            }
            if wasm.healthcheck.is_some() {
                merged.healthcheck = wasm.healthcheck.clone();
            }
            if !wasm.profiles.is_empty() {
                merged.profiles = wasm.profiles.clone();
            }
            if wasm.configs.is_some() {
                merged.configs = wasm.configs.clone();
            }
            target.insert(name.to_string(), Service::Wasm(merged));
        }
        (Service::Process(process), Service::Wasm(base_wasm)) => {
            let mut merged = base_wasm.clone();
            if !process.command.is_empty() {
                // Process service extending Wasm service: convert command to args
                merged.args = process.command.clone();
            }
            if !process.ports.is_empty() {
                merged.ports = process.ports.clone();
            }
            if !process.environment.is_empty() {
                for (k, v) in &process.environment {
                    merged.environment.insert(k.clone(), v.clone());
                }
            }
            if process.resources.is_some() {
                merged.resources = process.resources.clone();
            }
            if !process.depends_on.is_empty() {
                merged.depends_on = process.depends_on.clone();
            }
            if process.restart_policy != RestartPolicy::Never {
                merged.restart_policy = process.restart_policy;
            }
            if process.env_file.is_some() {
                merged.env_file = process.env_file.clone();
            }
            if process.secrets.is_some() {
                merged.secrets = process.secrets.clone();
            }
            if process.security.is_some() {
                merged.security = process.security.clone();
            }
            if process.logging.is_some() {
                merged.logging = process.logging.clone();
            }
            if process.healthcheck.is_some() {
                merged.healthcheck = process.healthcheck.clone();
            }
            if !process.profiles.is_empty() {
                merged.profiles = process.profiles.clone();
            }
            if process.configs.is_some() {
                merged.configs = process.configs.clone();
            }
            target.insert(name.to_string(), Service::Wasm(merged));
        }
        _ => {
            return Err(format!(
                "cannot extend service of different type: {name} extends {base_name}"
            ));
        }
    }

    visited.insert(name.to_string());
    resolving.remove(name);

    Ok(())
}

/// Filters services based on active profiles. Returns a new manifest
/// containing only services that match the given profiles (or have no
/// profile restrictions).
pub fn filter_by_profiles(manifest: &Manifest, active_profiles: &[String]) -> Manifest {
    let mut filtered = manifest.clone();
    filtered.services.retain(|_name, service| {
        let profiles = service.profiles();
        profiles.is_empty() || profiles.iter().any(|p| active_profiles.contains(p))
    });
    filtered
}
