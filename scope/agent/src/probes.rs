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
    WasmInvocation(WasmInvocationEvent),
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

/// A captured Wasm invocation's inputs (module, args, env, and the wasm
/// module's content digest), recorded by `WasmProbe::record_invocation` so
/// it can be replayed offline later. Captured via a direct in-process call
/// from Origin's `start_wasm`, not a kernel-level eBPF hook — real eBPF
/// uprobe capture on `wasmtime::Instance::call` is a larger follow-up. Kept
/// as a distinct `EventData` variant rather than folded into `WasmEvent`
/// since it carries a different shape (args/env/digest, not a duration).
#[derive(Debug, Clone)]
pub struct WasmInvocationEvent {
    pub module_name: String,
    pub function: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub wasm_sha256: String,
    pub timestamp_ns: u64,
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

impl Default for ProbeManager {
    fn default() -> Self {
        Self::new()
    }
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

    /// Records a captured Wasm invocation as a real probe event. This is
    /// the first source of real content for `WasmProbe` — `attach`/
    /// `poll_event` are otherwise stubs with no kernel-level tracing wired
    /// up yet.
    pub fn record_invocation(&mut self, event: WasmInvocationEvent) {
        let pid = std::process::id();
        self.events.push(ProbeEvent {
            timestamp: event.timestamp_ns,
            category: ProbeCategory::WasmRuntime,
            data: EventData::WasmInvocation(event),
            pid,
            tid: 0,
            comm: self.name.clone(),
        });
    }

    /// Removes and returns all events recorded so far.
    pub fn drain_events(&mut self) -> Vec<ProbeEvent> {
        std::mem::take(&mut self.events)
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

/// Wraps a real `tcp_connect()` kprobe (`scope-ebpf-loader`), translating
/// its `ConnectEvent`s into the same `ProbeEvent`/`NetworkEvent` shape the
/// rest of this crate (and `otel.rs`) already knows how to emit. Only
/// available with `--features ebpf` on Linux — see `scope/ebpf/` and the
/// `[features] ebpf` entry in this crate's `Cargo.toml` for why that's
/// off by default (needs a nightly toolchain to compile the kernel-side
/// object, and only runs on Linux at all).
#[cfg(all(target_os = "linux", feature = "ebpf"))]
pub struct EbpfNetworkProbe {
    name: String,
    state: ProbeState,
    inner: Option<scope_ebpf_loader::TcpConnectProbe>,
}

#[cfg(all(target_os = "linux", feature = "ebpf"))]
impl EbpfNetworkProbe {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            state: ProbeState::Detached,
            inner: None,
        }
    }

    fn to_probe_event(event: scope_ebpf_loader::ConnectEvent) -> ProbeEvent {
        let src_addr = std::net::IpAddr::from(std::net::Ipv4Addr::from(event.saddr.to_ne_bytes()));
        let dst_addr = std::net::IpAddr::from(std::net::Ipv4Addr::from(event.daddr.to_ne_bytes()));
        ProbeEvent {
            timestamp: event.timestamp_ns,
            category: ProbeCategory::Network,
            data: EventData::Network(NetworkEvent {
                event_type: NetworkEventType::TcpConnect,
                src_addr,
                dst_addr,
                src_port: event.sport,
                dst_port: event.dport,
                bytes: 0,
                proto: TransportProtocol::Tcp,
            }),
            pid: event.pid,
            tid: event.tid,
            comm: scope_ebpf_loader::comm_string(&event),
        }
    }

    /// Removes and returns every event captured since the last drain,
    /// already converted to `ProbeEvent`s ready for `OtelExporter::emit_event`.
    pub fn drain_events(&self) -> Vec<ProbeEvent> {
        match &self.inner {
            Some(probe) => probe
                .drain_events()
                .into_iter()
                .map(Self::to_probe_event)
                .collect(),
            None => Vec::new(),
        }
    }
}

#[cfg(all(target_os = "linux", feature = "ebpf"))]
impl Probe for EbpfNetworkProbe {
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
        tracing::info!(probe = %self.name, "attaching real tcp_connect kprobe");
        let probe = scope_ebpf_loader::TcpConnectProbe::attach().map_err(|e| {
            ProbeError::InvalidConfig(format!("failed to attach tcp_connect kprobe: {e}"))
        })?;
        self.inner = Some(probe);
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn detach(&mut self) -> Result<(), ProbeError> {
        self.inner = None;
        self.state = ProbeState::Detached;
        Ok(())
    }

    fn pause(&mut self) -> Result<(), ProbeError> {
        // The kprobe keeps capturing while "paused" (there's no cheap
        // kernel-side pause primitive here) — poll_event/drain_events just
        // stop being called by the manager loop meanwhile, so events queue
        // up in the ring buffer reader thread until resumed or detached.
        self.state = ProbeState::Paused;
        Ok(())
    }

    fn resume(&mut self) -> Result<(), ProbeError> {
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn poll_event(&self) -> Option<ProbeEvent> {
        self.inner
            .as_ref()
            .and_then(|probe| probe.poll_event())
            .map(Self::to_probe_event)
    }
}

/// Wraps the real syscall kprobes (`openat`, `read`, `write`) via
/// `scope-ebpf-loader`'s `SyscallProbeLoader`, translating its
/// `SyscallEvent`s into the `ProbeEvent`/`SyscallEvent` shape that
/// `otel.rs` already handles. Only available with `--features ebpf` on
/// Linux — same caveats as `EbpfNetworkProbe`.
#[cfg(all(target_os = "linux", feature = "ebpf"))]
pub struct EbpfSyscallProbe {
    name: String,
    state: ProbeState,
    inner: Option<scope_ebpf_loader::SyscallProbeLoader>,
    filter: Vec<SyscallEventType>,
}

#[cfg(all(target_os = "linux", feature = "ebpf"))]
impl EbpfSyscallProbe {
    pub fn new(name: &str, filter: Vec<SyscallEventType>) -> Self {
        Self {
            name: name.to_string(),
            state: ProbeState::Detached,
            inner: None,
            filter,
        }
    }

    fn to_probe_event(event: scope_ebpf_loader::SyscallEvent) -> Option<ProbeEvent> {
        let event_type = match event.event_type {
            0 => SyscallEventType::Open,
            1 => SyscallEventType::Read,
            2 => SyscallEventType::Write,
            _ => return None,
        };

        Some(ProbeEvent {
            timestamp: event.timestamp_ns,
            category: ProbeCategory::Syscall,
            data: EventData::Syscall(crate::probes::SyscallEvent {
                event_type,
                syscall_nr: event.syscall_nr,
                path: None,
                fd: Some(event.arg_fd as i32),
                bytes_rw: Some(event.arg_bytes),
                exit_code: 0,
                latency: std::time::Duration::ZERO,
            }),
            pid: event.pid,
            tid: event.tid,
            comm: scope_ebpf_loader::syscall_comm_string(&event),
        })
    }

    fn matches_filter(event: &ProbeEvent, filter: &[SyscallEventType]) -> bool {
        if filter.is_empty() {
            return true;
        }
        match &event.data {
            EventData::Syscall(s) => filter.contains(&s.event_type),
            _ => false,
        }
    }

    /// Removes and returns every event captured since the last drain,
    /// filtered by the configured syscall types and converted to
    /// `ProbeEvent`s ready for `OtelExporter::emit_event`.
    pub fn drain_events(&self) -> Vec<ProbeEvent> {
        match &self.inner {
            Some(probe) => probe
                .drain_events()
                .into_iter()
                .filter_map(Self::to_probe_event)
                .filter(|e| Self::matches_filter(e, &self.filter))
                .collect(),
            None => Vec::new(),
        }
    }
}

#[cfg(all(target_os = "linux", feature = "ebpf"))]
impl Probe for EbpfSyscallProbe {
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
            filter = ?self.filter,
            "attaching real syscall kprobes (openat, read, write)"
        );
        let probe = scope_ebpf_loader::SyscallProbeLoader::attach().map_err(|e| {
            ProbeError::InvalidConfig(format!("failed to attach syscall kprobes: {e}"))
        })?;
        self.inner = Some(probe);
        self.state = ProbeState::Attached;
        Ok(())
    }

    fn detach(&mut self) -> Result<(), ProbeError> {
        self.inner = None;
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
        self.inner
            .as_ref()
            .and_then(|probe| probe.poll_event())
            .and_then(Self::to_probe_event)
            .filter(|e| Self::matches_filter(e, &self.filter))
    }
}

#[cfg(test)]
mod wasm_probe_tests {
    use super::*;

    #[test]
    fn record_invocation_round_trips_through_poll_event() {
        let mut probe = WasmProbe::new("origin-wasm");
        let event = WasmInvocationEvent {
            module_name: "api".to_string(),
            function: "_start".to_string(),
            args: vec!["--port".to_string(), "8080".to_string()],
            env: HashMap::from([("DB_HOST".to_string(), "db".to_string())]),
            wasm_sha256: "deadbeef".to_string(),
            timestamp_ns: 42,
        };

        probe.record_invocation(event.clone());
        let polled = probe.poll_event().expect("should have a recorded event");

        assert_eq!(polled.timestamp, 42);
        assert!(matches!(polled.category, ProbeCategory::WasmRuntime));
        match polled.data {
            EventData::WasmInvocation(captured) => {
                assert_eq!(captured.module_name, "api");
                assert_eq!(captured.function, "_start");
                assert_eq!(captured.args, event.args);
                assert_eq!(captured.env, event.env);
                assert_eq!(captured.wasm_sha256, "deadbeef");
            }
            other => panic!("expected WasmInvocation event, got {other:?}"),
        }
    }
}
