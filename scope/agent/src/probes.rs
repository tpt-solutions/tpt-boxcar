use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("probe not attached: {0}")]
    NotAttached(String),
    #[error("invalid probe configuration: {0}")]
    InvalidConfig(String),
    #[error("probe event parse error: {0}")]
    ParseError(String),
}

#[derive(Debug, Clone)]
pub enum ProbeCategory {
    Network,
    Syscall,
    WasmRuntime,
}

#[derive(Debug, Clone)]
pub enum ProbeState {
    Detached,
    Attached,
    Paused,
}

#[derive(Debug, Clone)]
pub struct ProbeEvent {
    pub timestamp: u64,
    pub category: ProbeCategory,
    pub data: EventData,
    pub pid: u32,
    pub tid: u32,
    pub comm: String,
}

#[derive(Debug, Clone)]
pub enum EventData {
    Network(NetworkEvent),
    Syscall(SyscallEvent),
    WasmRuntime(WasmEvent),
}

#[derive(Debug, Clone)]
pub struct NetworkEvent {
    pub event_type: NetworkEventType,
    pub src_addr: std::net::IpAddr,
    pub dst_addr: std::net::IpAddr,
    pub src_port: u16,
    pub dst_port: u16,
    pub bytes: u64,
    pub proto: TransportProtocol,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEventType {
    TcpConnect,
    TcpAccept,
    TcpClose,
    UdpSend,
    UdpRecv,
    BytesTransferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportProtocol {
    Tcp,
    Udp,
}

#[derive(Debug, Clone)]
pub struct SyscallEvent {
    pub event_type: SyscallEventType,
    pub syscall_nr: i64,
    pub path: Option<String>,
    pub fd: Option<i32>,
    pub bytes_rw: Option<u64>,
    pub exit_code: i64,
    pub latency: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyscallEventType {
    Open,
    Read,
    Write,
    Exec,
}

#[derive(Debug, Clone)]
pub struct WasmEvent {
    pub event_type: WasmEventType,
    pub module_name: String,
    pub instance_id: u64,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WasmEventType {
    CompileStart,
    CompileEnd,
    Instantiate,
    MemoryGrow,
}

#[derive(Debug, Clone)]
pub struct CompileEvent {
    pub module_name: String,
    pub compile_time: Duration,
    pub wasm_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct InstantiateEvent {
    pub module_name: String,
    pub instance_id: u64,
    pub instantiation_latency: Duration,
}

#[derive(Debug, Clone)]
pub struct MemoryEvent {
    pub instance_id: u64,
    pub pages_delta: i32,
    pub current_pages: u32,
}

pub trait Probe: Send + Sync {
    fn name(&self) -> &str;
    fn category(&self) -> ProbeCategory;
    fn state(&self) -> ProbeState;
    fn attach(&mut self) -> Result<(), ProbeError>;
    fn detach(&mut self) -> Result<(), ProbeError>;
    fn pause(&mut self) -> Result<(), ProbeError>;
    fn resume(&mut self) -> Result<(), ProbeError>;
    fn poll_event(&self) -> Option<ProbeEvent>;
}

pub struct ProbeManager {
    probes: HashMap<String, Box<dyn Probe>>,
}

impl ProbeManager {
    pub fn new() -> Self {
        Self {
            probes: HashMap::new(),
        }
    }

    pub fn register(&mut self, probe: Box<dyn Probe>) {
        let name = probe.name().to_string();
        self.probes.insert(name, probe);
    }

    pub fn attach_all(&mut self) -> Result<(), ProbeError> {
        for probe in self.probes.values_mut() {
            probe.attach()?;
        }
        Ok(())
    }

    pub fn detach_all(&mut self) -> Result<(), ProbeError> {
        for probe in self.probes.values_mut() {
            probe.detach()?;
        }
        Ok(())
    }

    pub fn list_probes(&self) -> Vec<(&str, ProbeCategory, ProbeState)> {
        self.probes
            .values()
            .map(|p| (p.name(), p.category(), p.state()))
            .collect()
    }
}

pub struct NetworkProbe {
    name: String,
    state: ProbeState,
    events: Vec<ProbeEvent>,
}

impl NetworkProbe {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            state: ProbeState::Detached,
            events: Vec::new(),
        }
    }
}

impl Probe for NetworkProbe {
    fn name(&self) -> &str {
        &self.name
    }

    fn category(&self) -> ProbeCategory {
        ProbeCategory::Network
    }

    fn state(&self) -> ProbeState {
        self.state.clone()
    }

    fn attach(&mut self) -> Result<(), ProbeError> {
        tracing::info!(probe = %self.name, "attaching network probe");
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn detach(&mut self) -> Result<(), ProbeError> {
        tracing::info!(probe = %self.name, "detaching network probe");
        self.state = ProbeState::Detached;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Paused;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn poll_event(&self) -> Option<ProbeEvent> {
        self.events.first().cloned()
    }
}

pub struct SyscallProbe {
    name: String,
    state: ProbeState,
    events: Vec<ProbeEvent>,
    syscalls: Vec<SyscallEventType>,
}

impl SyscallProbe {
    pub fn new(name: &str, syscalls: Vec<SyscallEventType>) -> Self {
        Self {
            name: name.to_string(),
            state: ProbeState::Detached,
            events: Vec::new(),
            syscalls,
        }
    }
}

impl Probe for SyscallProbe {
    fn name(&self) -> &str {
        &self.name
    }

    fn category(&self) -> ProbeCategory {
        ProbeCategory::Syscall
    }

    fn state(&self) -> ProbeState {
        self.state.clone()
    }

    fn attach(&mut self) -> Result<(), ProbeError> {
        tracing::info!(
            probe = %self.name,
            syscalls = ?self.syscalls,
            "attaching syscall probe"
        );
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn detach(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Detached;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Paused;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn poll_event(&self) -> Option<ProbeEvent> {
        self.events.first().cloned()
    }
}

pub struct WasmProbe {
    name: String,
    state: ProbeState,
    events: Vec<ProbeEvent>,
}

impl WasmProbe {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            state: ProbeState::Detached,
            events: Vec::new(),
        }
    }
}

impl Probe for WasmProbe {
    fn name(&self) -> &str {
        &self.name
    }

    fn category(&self) -> ProbeCategory {
        ProbeCategory::WasmRuntime
    }

    fn state(&self) -> ProbeState {
        self.state.clone()
    }

    fn attach(&mut self) -> Result<(), ProbeError> {
        tracing::info!(probe = %self.name, "attaching wasm probe");
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn detach(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Detached;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Paused;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn poll_event(&self) -> Option<ProbeEvent> {
        self.events.first().cloned()
    }
}
