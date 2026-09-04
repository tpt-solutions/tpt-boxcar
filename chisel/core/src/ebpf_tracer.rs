use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SyscallType {
    Open,
    Openat,
    Read,
    Write,
    Close,
    Execve,
    Execveat,
}

impl SyscallType {
    pub fn as_str(&self) -> &str {
        match self {
            SyscallType::Open => "open",
            SyscallType::Openat => "openat",
            SyscallType::Read => "read",
            SyscallType::Write => "write",
            SyscallType::Close => "close",
            SyscallType::Execve => "execve",
            SyscallType::Execveat => "execveat",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraceEvent {
    Open {
        path: PathBuf,
        flags: u32,
        timestamp_ns: u64,
    },
    Read {
        path: PathBuf,
        bytes_read: u64,
        offset: u64,
        timestamp_ns: u64,
    },
    Exec {
        path: PathBuf,
        args: Vec<String>,
        env_keys: Vec<String>,
        timestamp_ns: u64,
    },
}

impl TraceEvent {
    pub fn path(&self) -> &PathBuf {
        match self {
            TraceEvent::Open { path, .. } => path,
            TraceEvent::Read { path, .. } => path,
            TraceEvent::Exec { path, .. } => path,
        }
    }

    pub fn timestamp_ns(&self) -> u64 {
        match self {
            TraceEvent::Open { timestamp_ns, .. }
            | TraceEvent::Read { timestamp_ns, .. }
            | TraceEvent::Exec { timestamp_ns, .. } => *timestamp_ns,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAccessTrace {
    pub trace_id: String,
    pub process_id: u32,
    pub process_name: String,
    pub file_paths: Vec<PathBuf>,
    pub read_bytes: u64,
    pub write_bytes: u64,
    pub exec_paths: Vec<PathBuf>,
    pub events: Vec<TraceEvent>,
    pub start_timestamp_ns: u64,
    pub end_timestamp_ns: u64,
    pub total_syscalls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfig {
    pub target_pid: Option<u32>,
    pub filter_paths: Vec<PathBuf>,
    pub exclude_paths: Vec<PathBuf>,
    pub syscalls: Vec<SyscallType>,
    pub max_events: usize,
    pub buffer_size_pages: u32,
    pub include_args: bool,
    pub include_env: bool,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            target_pid: None,
            filter_paths: Vec::new(),
            exclude_paths: vec![
                PathBuf::from("/proc"),
                PathBuf::from("/sys"),
                PathBuf::from("/dev"),
            ],
            syscalls: vec![
                SyscallType::Open,
                SyscallType::Openat,
                SyscallType::Read,
                SyscallType::Execve,
            ],
            max_events: 100_000,
            buffer_size_pages: 256,
            include_args: true,
            include_env: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EbpfStats {
    pub events_captured: u64,
    pub events_dropped: u64,
    pub buffer_usage_percent: f64,
    pub active_probes: u32,
    pub cpu_time_ns: u64,
}

#[allow(async_fn_in_trait)]
pub trait TracerProvider {
    fn start_tracing(
        &self,
        config: &TraceConfig,
    ) -> impl std::future::Future<Output = Result<String>> + Send;
    fn stop_tracing(
        &self,
        trace_id: &str,
    ) -> impl std::future::Future<Output = Result<FileAccessTrace>> + Send;
    fn get_stats(
        &self,
        trace_id: &str,
    ) -> impl std::future::Future<Output = Result<EbpfStats>> + Send;
}

pub struct EbpfTracer {
    #[allow(dead_code)]
    active_traces: HashMap<String, TraceConfig>,
}

impl EbpfTracer {
    pub fn new() -> Self {
        Self {
            active_traces: HashMap::new(),
        }
    }

    pub async fn attach_to_process(&self, pid: u32, _config: &TraceConfig) -> Result<()> {
        info!("Attaching eBPF probes to process: {}", pid);
        Ok(())
    }

    pub async fn record_event(&self, event: TraceEvent) -> Result<()> {
        let path = event.path();
        let ts = event.timestamp_ns();
        info!("Recorded event at {:?} (ts: {})", path, ts);
        Ok(())
    }

    pub fn build_trace(&self, trace_id: &str, config: &TraceConfig) -> Result<FileAccessTrace> {
        Ok(FileAccessTrace {
            trace_id: trace_id.to_string(),
            process_id: config.target_pid.unwrap_or(0),
            process_name: String::new(),
            file_paths: Vec::new(),
            read_bytes: 0,
            write_bytes: 0,
            exec_paths: Vec::new(),
            events: Vec::new(),
            start_timestamp_ns: 0,
            end_timestamp_ns: 0,
            total_syscalls: 0,
        })
    }
}

impl Default for EbpfTracer {
    fn default() -> Self {
        Self::new()
    }
}

impl TracerProvider for EbpfTracer {
    async fn start_tracing(&self, config: &TraceConfig) -> Result<String> {
        let trace_id = uuid::Uuid::new_v4().to_string();
        info!(
            "Starting eBPF trace: {} with config: {:?}",
            trace_id, config
        );
        Ok(trace_id)
    }

    async fn stop_tracing(&self, trace_id: &str) -> Result<FileAccessTrace> {
        info!("Stopping eBPF trace: {}", trace_id);
        Ok(FileAccessTrace {
            trace_id: trace_id.to_string(),
            process_id: 0,
            process_name: String::new(),
            file_paths: Vec::new(),
            read_bytes: 0,
            write_bytes: 0,
            exec_paths: Vec::new(),
            events: Vec::new(),
            start_timestamp_ns: 0,
            end_timestamp_ns: 0,
            total_syscalls: 0,
        })
    }

    async fn get_stats(&self, _trace_id: &str) -> Result<EbpfStats> {
        Ok(EbpfStats {
            events_captured: 0,
            events_dropped: 0,
            buffer_usage_percent: 0.0,
            active_probes: 0,
            cpu_time_ns: 0,
        })
    }
}
