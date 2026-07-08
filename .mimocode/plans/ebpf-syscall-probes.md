# Implementation Plan: Real eBPF Syscall Probes

## Architecture Summary

Add kprobes for `__x64_sys_openat`, `__x64_sys_read`, and `__x64_sys_write` to the existing eBPF probe pipeline. Events flow through the same path as `tcp_connect()`: kernel probe → ring buffer → loader → agent wrapper → `ProbeEvent` → `OtelExporter`.

## Design Decisions

### 1. Kprobe targets: x86_64-specific kprobes

**Decision**: Use `__x64_sys_openat`, `__x64_sys_read`, `__x64_sys_write` — x86_64-specific kprobe targets.

**Rationale**:
- Raw tracepoints (`tracepoint:syscalls/sys_enter_openat`) are more portable but require `aya` tracepoint program types which add complexity (fentry vs kprobe semantics differ)
- The existing `tcp_connect` probe already uses kprobes, so we're consistent
- x86_64 covers all current TPT Boxcar deployment targets
- Document the x86_64 limitation clearly; arm64 support is a follow-up

### 2. Ring buffer strategy: single `SYSCALL_EVENTS` buffer with type discriminator

**Decision**: One `RingBuf` named `SYSCALL_EVENTS` with a `SyscallEvent` struct tagged by `syscall_type` enum.

**Rationale**:
- Single buffer = one reader thread, simpler loader
- Type discriminator lets one loader drain all syscall events
- 256 KiB buffer with ~160-byte events = ~1600 events capacity, adequate for normal load
- If a specific syscall fires too fast, events drop (same tradeoff as `CONNECT_EVENTS`)

### 3. Event struct: fixed-size 160 bytes with `#[repr(C)]`

**Decision**: All fields fixed-size. Paths truncated to 64 bytes in-kernel. Fields unused by a given syscall zeroed.

**Rationale**:
- Ring buffer records must be fixed-size for `reserve::<T>()`
- `bpf_probe_read_kernel_str` writes into a fixed buffer (truncate at 64 bytes, no NUL termination needed — `comm_len()` pattern handles that)
- No allocation or variable-length data in kernel or common crate

### 4. Architecture: x86_64-only, documented

**Decision**: x86_64-only for the initial implementation. Add a compile-time `#[cfg(target_arch = "x86_64")]` gate on the three new kprobe functions. The rest of the struct/loader/agent code is arch-agnostic.

### 5. Loader: separate `SyscallProbeLoader`

**Decision**: New `SyscallProbeLoader` struct in `scope-ebpf-loader`, parallel to `TcpConnectProbe`.

**Rationale**:
- Keeps syscall concerns isolated from network probe
- The agent can attach/detach independently
- Follows existing pattern exactly (separate struct, separate reader thread)

---

## File-by-File Changes

### 1. `scope/ebpf/common/src/lib.rs` — Add `SyscallEvent`

Add the shared struct alongside `ConnectEvent`. Must be `#![no_std]`, `#[repr(C)]`, all `Copy`.

```rust
/// Syscall type discriminator — fits in a `u8` for compact ring buffer layout.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallType {
    OpenAt = 0,
    Read = 1,
    Write = 2,
}

/// A captured syscall event (openat/read/write), written by the kernel-side
/// probe and read by the userspace loader. Fixed-size `#[repr(C)]` for ring
/// buffer compatibility — fields unused by a given syscall are zeroed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SyscallEvent {
    pub timestamp_ns: u64,
    pub pid: u32,
    pub tid: u32,
    pub syscall_type: u8,       // SyscallType as u8
    pub _pad0: [u8; 3],        // alignment padding
    pub syscall_nr: i64,
    pub arg0: i64,              // openat: dirfd, read/write: fd
    pub arg1: i64,              // openat: flags, read/write: buf (not useful in-kernel, zeroed)
    pub arg2: i64,              // openat: mode
    pub ret: i64,               // return value (filled on kretprobe, 0 on kprobe entry)
    pub bytes_rw: u64,          // read/write byte count (from ret on success, 0 on entry)
    pub path: [u8; 64],         // openat: filename (truncated), read/write: zeroed
    pub comm: [u8; 16],         // current->comm, NUL-padded
}

impl SyscallEvent {
    pub const LEN: usize = core::mem::size_of::<SyscallEvent>(); // 160 bytes

    pub unsafe fn from_bytes(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() >= Self::LEN);
        core::ptr::read_unaligned(bytes.as_ptr() as *const SyscallEvent)
    }

    pub fn comm_len(&self) -> usize {
        self.comm.iter().position(|&b| b == 0).unwrap_or(self.comm.len())
    }

    pub fn path_len(&self) -> usize {
        self.path.iter().position(|&b| b == 0).unwrap_or(self.path.len())
    }
}
```

**Size verification**: `8 + 4 + 4 + 1 + 3 + 8 + 8 + 8 + 8 + 8 + 8 + 64 + 16 = 160 bytes`.

### 2. `scope/ebpf/probe/src/main.rs` — Add three kprobes + ring buffer

Add to the existing probe file, which already has `tcp_connect`. The new code:

- A `SYSCALL_EVENTS` `RingBuf` (256 KiB, same as `CONNECT_EVENTS`)
- Three kprobe entry functions: `sys_openat`, `sys_read`, `sys_write`
- A shared `try_syscall_event()` helper that fills `SyscallEvent` from `ProbeContext`
- `#[cfg(target_arch = "x86_64")]` gate on the kprobe functions

**Key implementation details**:

For `sys_openat(ctx: ProbeContext)`:
- `ctx.arg(0)` = `dirfd` (i32, cast to i64)
- `ctx.arg(1)` = `filename` (*const u8) — read with `bpf_probe_read_kernel_str` into `path` field
- `ctx.arg(2)` = `flags` (i64)
- `ctx.arg(3)` = `mode` (i64)

For `sys_read(ctx: ProbeContext)` and `sys_write(ctx: ProbeContext)`:
- `ctx.arg(0)` = `fd` (i32, cast to i64)
- `ctx.arg(1)` = `buf` (usize) — not useful in-kernel, zeroed
- `ctx.arg(2)` = `count` (usize) — stored as `bytes_rw` (best-effort, actual bytes returned is in ret)

**Note on kretprobes**: The initial implementation captures syscall **entry** only. Return values and actual byte counts would require kretprobes (a follow-up). For now, `ret` is zero on entry and `bytes_rw` reflects the requested count (not actual). This is acceptable for the OTel pipeline — the latency and call-pattern data is still valuable.

**Map reservation**: Same pattern as `CONNECT_EVENTS` — `SYSCALL_EVENTS.reserve::<SyscallEvent>(0)`, write, submit. Drop on full.

### 3. `scope/ebpf/loader/src/lib.rs` — Add `SyscallProbeLoader`

New struct alongside `TcpConnectProbe`:

```rust
/// Attached syscall kprobes (openat/read/write) plus their background
/// ring-buffer reader. Shares the same architecture as `TcpConnectProbe`.
pub struct SyscallProbeLoader {
    _ebpf: Ebpf,
    events: Arc<Mutex<VecDeque<SyscallEvent>>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl SyscallProbeLoader {
    pub fn attach() -> Result<Self, EbpfError> {
        // Same `aya::include_bytes_aligned!` embed as TcpConnectProbe
        // Loads "sys_openat", "sys_read", "sys_write" programs
        // Attaches each kprobe to its target symbol
        // Takes "SYSCALL_EVENTS" map, spawns reader thread
    }

    pub fn poll_event(&self) -> Option<SyscallEvent> { ... }
    pub fn drain_events(&self) -> Vec<SyscallEvent> { ... }
}
```

**Important**: Both `TcpConnectProbe` and `SyscallProbeLoader` call `Ebpf::load()` with the **same** embedded ELF object. The eBPF object contains all four programs (`tcp_connect` + three syscall probes) and both ring buffers (`CONNECT_EVENTS` + `SYSCALL_EVENTS`). Each loader takes ownership of the `Ebpf` instance, so they can't coexist in the same process with separate `Ebpf::load()` calls — but since the agent only needs one instance, this is fine. Actually, **correction**: each loader holds its own `Ebpf` instance. This means the probe object is loaded **twice** (once for network, once for syscalls). This is acceptable — the kernel allows multiple BPF programs on the same kprobe, and the memory overhead is minimal (two copies of the same ELF). If this becomes a concern, a shared `Ebpf` instance could be refactored later.

**Alternative**: A single `EbpfLoader` struct that loads once and exposes both `TcpConnectProbe` and `SyscallProbe` halves. This is cleaner but requires refactoring the existing `TcpConnectProbe` API. The plan preserves the existing API and adds a parallel loader.

### 4. `scope/agent/src/probes.rs` — Add `EbpfSyscallProbe` wrapper

New struct gated behind `#[cfg(all(target_os = "linux", feature = "ebpf"))]`, parallel to `EbpfNetworkProbe`:

```rust
#[cfg(all(target_os = "linux", feature = "ebpf"))]
pub struct EbpfSyscallProbe {
    name: String,
    state: ProbeState,
    inner: Option<scope_ebpf_loader::SyscallProbeLoader>,
}

#[cfg(all(target_os = "linux", feature = "ebpf"))]
impl EbpfSyscallProbe {
    pub fn new(name: &str) -> Self { ... }

    fn to_probe_event(event: scope_ebpf_loader::SyscallEvent) -> ProbeEvent {
        // Maps SyscallType → SyscallEventType
        // Converts path from [u8; 64] → Option<String> (None if all zeros)
        // Builds ProbeEvent with EventData::Syscall(SyscallEvent { ... })
    }

    pub fn drain_events(&self) -> Vec<ProbeEvent> { ... }
}

#[cfg(all(target_os = "linux", feature = "ebpf"))]
impl Probe for EbpfSyscallProbe { ... }
```

The stub `SyscallProbe` (line 252) remains untouched for non-Linux / non-ebpf builds.

### 5. `scope/agent/src/otel.rs` — No changes needed

The existing `emit_syscall_trace()` already handles `EventData::Syscall` with the correct attributes (`syscall.nr`, `file.path`, `file.fd`, `io.bytes`) and metrics (`syscall.count`, `syscall.duration`, `syscall.open.count`, `syscall.read.bytes`, `syscall.write.bytes`). No modifications required.

---

## Struct Layout Diagram

```
SyscallEvent (160 bytes, #[repr(C)])
┌──────────────────────────────────────────────────┐
│ timestamp_ns      u64         8 bytes            │
│ pid               u32         4 bytes            │
│ tid               u32         4 bytes            │
│ syscall_type      u8          1 byte             │
│ _pad0             [u8; 3]     3 bytes            │
│ syscall_nr        i64         8 bytes            │
│ arg0              i64         8 bytes            │
│ arg1              i64         8 bytes            │
│ arg2              i64         8 bytes            │
│ ret               i64         8 bytes            │
│ bytes_rw          u64         8 bytes            │
│ path              [u8; 64]    64 bytes           │
│ comm              [u8; 16]    16 bytes           │
└──────────────────────────────────────────────────┘
Total: 160 bytes
Capacity in 256 KiB ring buffer: ~1,638 events
```

## Data Flow

```
Kernel                          Userspace
──────                          ─────────
tcp_connect() kprobe ─────┐
                          ├──→ CONNECT_EVENTS RingBuf ──→ TcpConnectProbe ──→ EbpfNetworkProbe
sys_openat() kprobe ──────┤                                     │
sys_read() kprobe ────────┤    SYSCALL_EVENTS RingBuf ──→ SyscallProbeLoader ──→ EbpfSyscallProbe
sys_write() kprobe ───────┘                                     │
                                                                ↓
                                                    ProbeEvent { category: Syscall, data: SyscallEvent }
                                                                │
                                                                ↓
                                                    OtelExporter::emit_syscall_trace()
                                                        → OTel span + SyscallMetrics
```

## Implementation Order

1. **`scope/ebpf/common/src/lib.rs`** — Add `SyscallType` enum and `SyscallEvent` struct. Zero risk, no behavioral change.
2. **`scope/ebpf/probe/src/main.rs`** — Add `SYSCALL_EVENTS` map and three kprobe entry functions. This is the core work.
3. **`scope/ebpf/loader/src/lib.rs`** — Add `SyscallProbeLoader`. Depends on step 2 (same ELF object).
4. **`scope/agent/src/probes.rs`** — Add `EbpfSyscallProbe`. Depends on step 3.
5. **Verification** — Compile, test, manual validation.

## Test Strategy

### Unit tests (can run on any platform)

- `scope/ebpf/common`: Add tests verifying `SyscallEvent::LEN == 160`, `from_bytes` round-trip, `comm_len` and `path_len` edge cases (all zeros, no zeros, partial NUL).
- `scope/agent/src/probes.rs`: Add test for `EbpfSyscallProbe::to_probe_event` conversion with synthetic `SyscallEvent` data.

### Integration tests (Linux only, needs `CAP_BPF`)

- `scope/ebpf/loader`: Add test (marked `#[ignore]`, run in CI with `--ignored`) that calls `SyscallProbeLoader::attach()`, performs a known I/O operation (write to a temp file), and asserts at least one `SyscallEvent` appears in `drain_events()`.
- `scope/agent`: Add test that `EbpfSyscallProbe` + `OtelExporter::emit_event` produces a span with correct attributes when given a synthetic `ProbeEvent`.

### Manual verification steps

1. `cargo build -p scope-ebpf-common` — verify shared types compile for both targets
2. On Linux: `cargo build -p scope-ebpf-loader` — verify probe object compiles and loads
3. On Linux with `--features ebpf`: `cargo build -p tpt-scope-agent` — verify agent compiles with real eBPF support
4. Run the agent, trigger I/O (e.g., `cat /dev/null`), verify OTel spans appear in the collector
5. Verify `SyscallMetrics` counters increment in the OTel metrics pipeline

### CI considerations

- The eBPF probe compilation requires nightly + `bpfel-unknown-none` target, which is only set up in the Linux CI job
- The `ebpf` feature test should be a separate CI step (same as existing containerd tests)
- Non-Linux CI should verify the stub `SyscallProbe` still compiles (no feature gate)

## Follow-up Work (not in this PR)

- **kretprobes**: Capture return values (`ret`) and actual byte counts. Requires pairing entry/exit events per-tid.
- **arm64 support**: Add `#[cfg(target_arch = "aarch64")]` kprobe targets (`__arm64_sys_openat`, etc.)
- **Path deduplication**: Use a BPF hash map to store unique paths, reference by ID in the ring buffer event (saves ring buffer space).
- **Ring buffer sizing**: Tune based on observed syscall rates in production.
- **Syscall filtering**: Allow the agent to configure which syscalls are traced (reduce noise).
