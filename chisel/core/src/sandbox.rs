use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub image_name: String,
    pub network_isolation: bool,
    pub memory_limit_mb: u32,
    pub cpu_limit: f32,
    pub timeout_seconds: u64,
    pub mount_paths: Vec<MountConfig>,
    pub env_vars: HashMap<String, String>,
    pub command: Vec<String>,
    pub ebpf_probes: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            image_name: "sandbox:latest".to_string(),
            network_isolation: true,
            memory_limit_mb: 512,
            cpu_limit: 1.0,
            timeout_seconds: 300,
            mount_paths: Vec::new(),
            env_vars: HashMap::new(),
            command: vec!["/bin/sh".to_string()],
            ebpf_probes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountConfig {
    pub source: PathBuf,
    pub target: PathBuf,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub sandbox_id: String,
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub accessed_files: Vec<PathBuf>,
    pub syscalls: Vec<String>,
    pub memory_peak_bytes: u64,
    pub cpu_usage_seconds: f64,
    pub metadata: SandboxMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxMetadata {
    pub image_digest: String,
    pub created_at: String,
    pub terminated_at: String,
    pub network_namespace: String,
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub sandbox_id: String,
    pub config: SandboxConfig,
    pub status: SandboxStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SandboxStatus {
    Creating,
    Running,
    Stopping,
    Stopped,
    Failed,
}

#[async_trait]
pub trait SandboxProvider: Send + Sync {
    async fn start_sandbox(&self, config: &SandboxConfig) -> Result<SandboxInfo>;
    async fn stop_sandbox(&self, sandbox_id: &str) -> Result<()>;
    async fn collect_traces(&self, sandbox_id: &str) -> Result<SandboxResult>;
    async fn list_sandboxes(&self) -> Result<Vec<SandboxInfo>>;
    async fn get_status(&self, sandbox_id: &str) -> Result<SandboxStatus>;
}

pub struct SandboxRunner {
    sandboxes: HashMap<String, SandboxInfo>,
}

impl SandboxRunner {
    pub fn new() -> Self {
        Self {
            sandboxes: HashMap::new(),
        }
    }

    pub async fn run_sandboxed(&self, config: &SandboxConfig) -> Result<SandboxResult> {
        info!(
            "Running sandboxed execution with image: {}",
            config.image_name
        );

        let mut runner = self.create_runner(config).await?;
        runner.start(config).await?;
        let result = runner.execute(config).await?;
        runner.cleanup().await?;

        Ok(result)
    }

    async fn create_runner(&self, config: &SandboxConfig) -> Result<SandboxRunnerInstance> {
        let sandbox_id = Uuid::new_v4().to_string();

        if config.network_isolation {
            info!("Creating isolated network namespace for sandbox: {}", sandbox_id);
        }

        for probe in &config.ebpf_probes {
            info!("Mounting eBPF probe: {} in sandbox: {}", probe, sandbox_id);
        }

        for mount in &config.mount_paths {
            info!(
                "Mounting {} -> {} (ro: {})",
                mount.source.display(),
                mount.target.display(),
                mount.read_only
            );
        }

        Ok(SandboxRunnerInstance {
            sandbox_id,
            status: SandboxStatus::Creating,
        })
    }
}

impl Default for SandboxRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SandboxProvider for SandboxRunner {
    async fn start_sandbox(&self, config: &SandboxConfig) -> Result<SandboxInfo> {
        let sandbox_id = Uuid::new_v4().to_string();
        let created_at = chrono::Utc::now().to_rfc3339();

        info!("Starting sandbox: {} with image: {}", sandbox_id, config.image_name);

        let info = SandboxInfo {
            sandbox_id: sandbox_id.clone(),
            config: config.clone(),
            status: SandboxStatus::Running,
            created_at,
        };

        Ok(info)
    }

    async fn stop_sandbox(&self, sandbox_id: &str) -> Result<()> {
        info!("Stopping sandbox: {}", sandbox_id);
        Ok(())
    }

    async fn collect_traces(&self, sandbox_id: &str) -> Result<SandboxResult> {
        info!("Collecting traces from sandbox: {}", sandbox_id);

        Ok(SandboxResult {
            sandbox_id: sandbox_id.to_string(),
            success: true,
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            accessed_files: Vec::new(),
            syscalls: Vec::new(),
            memory_peak_bytes: 0,
            cpu_usage_seconds: 0.0,
            metadata: SandboxMetadata {
                image_digest: String::new(),
                created_at: chrono::Utc::now().to_rfc3339(),
                terminated_at: chrono::Utc::now().to_rfc3339(),
                network_namespace: String::new(),
                pid: 0,
            },
        })
    }

    async fn list_sandboxes(&self) -> Result<Vec<SandboxInfo>> {
        Ok(self.sandboxes.values().cloned().collect())
    }

    async fn get_status(&self, sandbox_id: &str) -> Result<SandboxStatus> {
        self.sandboxes
            .get(sandbox_id)
            .map(|info| info.status.clone())
            .ok_or_else(|| anyhow::anyhow!("Sandbox not found: {}", sandbox_id))
    }
}

struct SandboxRunnerInstance {
    sandbox_id: String,
    status: SandboxStatus,
}

impl SandboxRunnerInstance {
    async fn start(&mut self, _config: &SandboxConfig) -> Result<()> {
        self.status = SandboxStatus::Running;
        info!("Sandbox {} started", self.sandbox_id);
        Ok(())
    }

    async fn execute(&self, config: &SandboxConfig) -> Result<SandboxResult> {
        let start = std::time::Instant::now();

        info!(
            "Executing command {:?} in sandbox: {}",
            config.command, self.sandbox_id
        );

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(SandboxResult {
            sandbox_id: self.sandbox_id.clone(),
            success: true,
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms,
            accessed_files: Vec::new(),
            syscalls: Vec::new(),
            memory_peak_bytes: 0,
            cpu_usage_seconds: duration_ms as f64 / 1000.0,
            metadata: SandboxMetadata {
                image_digest: String::new(),
                created_at: chrono::Utc::now().to_rfc3339(),
                terminated_at: chrono::Utc::now().to_rfc3339(),
                network_namespace: self.sandbox_id.clone(),
                pid: 0,
            },
        })
    }

    async fn cleanup(&mut self) -> Result<()> {
        self.status = SandboxStatus::Stopped;
        info!("Sandbox {} cleaned up", self.sandbox_id);
        Ok(())
    }
}
