//! Userspace loader for the `scope-ebpf-probe` kprobe: loads the compiled
//! eBPF object (embedded at build time by `build.rs` via `aya-build`),
//! attaches it to the kernel's `tcp_connect()`, and drains the resulting
//! `ConnectEvent`s off a ring buffer from a background thread so callers
//! can poll them without needing an async runtime.
//!
//! Linux-only, and requires `CAP_BPF`+`CAP_PERFMON` (or root) to attach the
//! kprobe — `TcpConnectProbe::attach()` surfaces that as a normal `Result`
//! rather than panicking, so callers (e.g. `tpt-scope-agent`) can fall back
//! to the in-memory stub probe when unprivileged.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use aya::maps::RingBuf;
use aya::programs::KProbe;
use aya::Ebpf;

pub use scope_ebpf_common::ConnectEvent;
pub use scope_ebpf_common::SyscallEvent;

#[derive(Debug, thiserror::Error)]
pub enum EbpfError {
    #[error("failed to load eBPF object: {0}")]
    Load(#[from] aya::EbpfError),
    #[error("failed to load eBPF program: {0}")]
    Program(#[from] aya::programs::ProgramError),
    #[error("CONNECT_EVENTS map missing or wrong type: {0}")]
    Map(String),
}

/// A single UTF-8 (lossy) decode of `ConnectEvent::comm`, for callers that
/// don't want to depend on this crate's raw byte representation.
pub fn comm_string(event: &ConnectEvent) -> String {
    String::from_utf8_lossy(&event.comm[..event.comm_len()]).into_owned()
}

/// Lossy UTF-8 decode of a `SyscallEvent`'s comm field.
pub fn syscall_comm_string(event: &SyscallEvent) -> String {
    String::from_utf8_lossy(&event.comm[..event.comm_len()]).into_owned()
}

/// Attached `tcp_connect()` kprobe plus its background ring-buffer reader.
pub struct TcpConnectProbe {
    // Kept alive for as long as the probe is attached — dropping `Ebpf`
    // detaches the kprobe and tears down its maps.
    _ebpf: Ebpf,
    events: Arc<Mutex<VecDeque<ConnectEvent>>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl TcpConnectProbe {
    /// Loads and attaches the compiled probe. Requires Linux + sufficient
    /// privilege (root, or `CAP_BPF`+`CAP_PERFMON`) — returns `Err` rather
    /// than panicking if either precondition isn't met, so callers can
    /// degrade gracefully.
    pub fn attach() -> Result<Self, EbpfError> {
        let mut ebpf = Ebpf::load(aya::include_bytes_aligned!(concat!(
            env!("OUT_DIR"),
            "/scope-ebpf-probe"
        )))?;

        let program: &mut KProbe = ebpf
            .program_mut("tcp_connect")
            .expect("tcp_connect program missing from compiled probe object")
            .try_into()?;
        program.load()?;
        program.attach("tcp_connect", 0)?;

        let ring_buf_map = ebpf
            .take_map("CONNECT_EVENTS")
            .ok_or_else(|| EbpfError::Map("CONNECT_EVENTS map not found".to_string()))?;
        let ring_buf = RingBuf::try_from(ring_buf_map)
            .map_err(|e| EbpfError::Map(format!("CONNECT_EVENTS is not a ring buffer: {e}")))?;

        let events: Arc<Mutex<VecDeque<ConnectEvent>>> = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));

        let reader = spawn_reader(ring_buf, Arc::clone(&events), Arc::clone(&stop));

        Ok(Self {
            _ebpf: ebpf,
            events,
            stop,
            reader: Some(reader),
        })
    }

    /// Pops the oldest captured event, if any (non-blocking).
    pub fn poll_event(&self) -> Option<ConnectEvent> {
        self.events.lock().expect("events mutex poisoned").pop_front()
    }

    /// Removes and returns every event captured since the last drain.
    pub fn drain_events(&self) -> Vec<ConnectEvent> {
        let mut guard = self.events.lock().expect("events mutex poisoned");
        guard.drain(..).collect()
    }
}

impl Drop for TcpConnectProbe {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}

/// Polls the ring buffer on a dedicated OS thread and pushes decoded events
/// into the shared queue. Uses a short sleep between empty polls rather
/// than an epoll wait on the map's fd — simpler, and connect-rate events
/// are not latency-sensitive enough to justify the extra plumbing here.
fn spawn_reader(
    mut ring_buf: RingBuf<aya::maps::MapData>,
    events: Arc<Mutex<VecDeque<ConnectEvent>>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            let mut drained_any = false;
            while let Some(item) = ring_buf.next() {
                drained_any = true;
                if item.len() < ConnectEvent::LEN {
                    tracing::warn!(
                        len = item.len(),
                        expected = ConnectEvent::LEN,
                        "short CONNECT_EVENTS record, dropping"
                    );
                    continue;
                }
                let event = unsafe { ConnectEvent::from_bytes(&item) };
                events.lock().expect("events mutex poisoned").push_back(event);
            }
            if !drained_any {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Syscall probe loader — mirrors `TcpConnectProbe` but drains the
// `SYSCALL_EVENTS` ring buffer and attaches the three syscall kprobes.
// ---------------------------------------------------------------------------

/// Attached syscall kprobes (openat, read, write) plus their shared
/// background ring-buffer reader.
pub struct SyscallProbeLoader {
    _ebpf: Ebpf,
    events: Arc<Mutex<VecDeque<SyscallEvent>>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

/// Mapping from our BPF program names to the real kernel function names
/// they attach to.  x86_64 only — arm64 uses `__arm64_sys_*` prefixes.
const SYSCALL_KPROBE_TARGETS: &[(&str, &str)] = &[
    ("sys_openat", "__x64_sys_openat"),
    ("sys_read", "__x64_sys_read"),
    ("sys_write", "__x64_sys_write"),
];

impl SyscallProbeLoader {
    /// Loads the compiled probe object (same ELF as `TcpConnectProbe` — the
    /// kernel only activates the programs that are actually attached) and
    /// attaches all three syscall kprobes.  Requires Linux + root (or
    /// `CAP_BPF`+`CAP_PERFMON`).
    pub fn attach() -> Result<Self, EbpfError> {
        let mut ebpf = Ebpf::load(aya::include_bytes_aligned!(concat!(
            env!("OUT_DIR"),
            "/scope-ebpf-probe"
        )))?;

        for &(prog_name, kernel_fn) in SYSCALL_KPROBE_TARGETS {
            let program: &mut KProbe = ebpf
                .program_mut(prog_name)
                .unwrap_or_else(|| {
                    panic!("{prog_name} program missing from compiled probe object")
                })
                .try_into()?;
            program.load()?;
            program.attach(kernel_fn, 0)?;
        }

        let ring_buf_map = ebpf
            .take_map("SYSCALL_EVENTS")
            .ok_or_else(|| EbpfError::Map("SYSCALL_EVENTS map not found".to_string()))?;
        let ring_buf = RingBuf::try_from(ring_buf_map)
            .map_err(|e| EbpfError::Map(format!("SYSCALL_EVENTS is not a ring buffer: {e}")))?;

        let events: Arc<Mutex<VecDeque<SyscallEvent>>> = Arc::new(Mutex::new(VecDeque::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reader = spawn_syscall_reader(ring_buf, Arc::clone(&events), Arc::clone(&stop));

        Ok(Self {
            _ebpf: ebpf,
            events,
            stop,
            reader: Some(reader),
        })
    }

    pub fn poll_event(&self) -> Option<SyscallEvent> {
        self.events.lock().expect("events mutex poisoned").pop_front()
    }

    pub fn drain_events(&self) -> Vec<SyscallEvent> {
        let mut guard = self.events.lock().expect("events mutex poisoned");
        guard.drain(..).collect()
    }
}

impl Drop for SyscallProbeLoader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
    }
}

fn spawn_syscall_reader(
    mut ring_buf: RingBuf<aya::maps::MapData>,
    events: Arc<Mutex<VecDeque<SyscallEvent>>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            let mut drained_any = false;
            while let Some(item) = ring_buf.next() {
                drained_any = true;
                if item.len() < SyscallEvent::LEN {
                    tracing::warn!(
                        len = item.len(),
                        expected = SyscallEvent::LEN,
                        "short SYSCALL_EVENTS record, dropping"
                    );
                    continue;
                }
                let event = unsafe { SyscallEvent::from_bytes(&item) };
                events.lock().expect("events mutex poisoned").push_back(event);
            }
            if !drained_any {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    })
}
