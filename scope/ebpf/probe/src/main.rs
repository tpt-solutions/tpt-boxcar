#![no_std]
#![no_main]

use aya_ebpf::{
    helpers::{bpf_ktime_get_ns, bpf_probe_read_kernel},
    macros::{kprobe, map},
    maps::RingBuf,
    programs::ProbeContext,
};
use scope_ebpf_common::{ConnectEvent, SyscallEvent};

/// Holds captured `tcp_connect()` events until the userspace loader drains
/// them. 256 KiB is generous headroom for connect-rate bursts without
/// risking `ENOSPC` drops under normal load; sized in kernel pages like the
/// rest of the aya ecosystem expects.
#[map]
static CONNECT_EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0);

/// Ring buffer for syscall entry events (openat, read, write). Separate from
/// `CONNECT_EVENTS` to keep the two event streams independent — a burst of
/// connects doesn't starve syscall visibility and vice versa.
#[map]
static SYSCALL_EVENTS: RingBuf = RingBuf::with_byte_size(256 * 1024, 0);

// Event type discriminators matching `SyscallEvent::event_type`.
const SYSCALL_OPENAT: u8 = 0;
const SYSCALL_READ: u8 = 1;
const SYSCALL_WRITE: u8 = 2;

// Offsets into `struct sock_common` (embedded at the start of `struct
// sock`, which is `tcp_connect(struct sock *sk)`'s sole argument). This is
// the same fixed layout bcc's/bpftrace's `tcpconnect` tools have relied on
// for years — `skc_daddr`/`skc_rcv_saddr`/`skc_dport`/`skc_family` sit at
// the front of `sock_common` and haven't moved across kernel versions in
// practice, unlike the rest of `struct sock`, which does churn. IPv6 is not
// decoded (`skc_family != AF_INET` events are dropped).
const SKC_DADDR_OFFSET: usize = 0;
const SKC_RCV_SADDR_OFFSET: usize = 4;
const SKC_DPORT_OFFSET: usize = 12;
const SKC_FAMILY_OFFSET: usize = 16;
const AF_INET: u16 = 2;

#[kprobe]
pub fn tcp_connect(ctx: ProbeContext) -> u32 {
    match try_tcp_connect(ctx) {
        Ok(()) => 0,
        Err(ret) => ret as u32,
    }
}

fn try_tcp_connect(ctx: ProbeContext) -> Result<(), i64> {
    let sk: *const u8 = ctx.arg(0).ok_or(1i64)?;

    let family: u16 =
        unsafe { bpf_probe_read_kernel(sk.add(SKC_FAMILY_OFFSET) as *const u16) }?;
    if family != AF_INET {
        // IPv6 (or an unexpected family) — not decoded yet, skip cleanly.
        return Ok(());
    }

    let daddr: u32 = unsafe { bpf_probe_read_kernel(sk.add(SKC_DADDR_OFFSET) as *const u32) }?;
    let saddr: u32 =
        unsafe { bpf_probe_read_kernel(sk.add(SKC_RCV_SADDR_OFFSET) as *const u32) }?;
    let dport_be: u16 =
        unsafe { bpf_probe_read_kernel(sk.add(SKC_DPORT_OFFSET) as *const u16) }?;
    let dport = u16::from_be(dport_be);

    let pid_tgid = aya_ebpf::helpers::bpf_get_current_pid_tgid();
    let pid = (pid_tgid >> 32) as u32;
    let tid = pid_tgid as u32;

    let comm = aya_ebpf::helpers::bpf_get_current_comm().unwrap_or([0u8; 16]);
    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    let event = ConnectEvent {
        timestamp_ns,
        pid,
        tid,
        saddr,
        daddr,
        // The local (source) port isn't resolved yet at `tcp_connect()` time
        // for an outbound connect (the kernel picks it after this hook
        // runs), so it's left zeroed rather than reporting a stale/garbage
        // value.
        sport: 0,
        dport,
        comm,
    };

    if let Some(mut entry) = CONNECT_EVENTS.reserve::<ConnectEvent>(0) {
        entry.write(event);
        entry.submit(0);
    }
    // A full ring buffer just means this connect event is dropped, not a
    // program error — the probe keeps running either way.

    Ok(())
}

// ---------------------------------------------------------------------------
// Syscall kprobes — entry-only probes on `openat`, `read`, `write`.
//
// These attach to the x86_64 kernel functions `__x64_sys_openat`,
// `__x64_sys_read`, and `__x64_sys_write` respectively.  The kernel
// passes a `pt_regs*` as the first argument; the aya `ProbeContext` gives
// us `ctx.arg(0..N)` which reads the register-backed syscall arguments
// directly (arg0 = rdi, arg1 = rsi, arg2 = rdx on x86_64).
//
// `openat(dirfd, filename, flags, mode)` — we capture `dirfd` (arg0).
// `read(fd, buf, count)` / `write(fd, buf, count)` — we capture `fd` (arg0).
// Full path resolution and byte counts require kretprobes (follow-up).
// ---------------------------------------------------------------------------

#[kprobe]
pub fn sys_openat(ctx: ProbeContext) -> u32 {
    match try_syscall_entry(ctx, SYSCALL_OPENAT) {
        Ok(()) => 0,
        Err(ret) => ret as u32,
    }
}

#[kprobe]
pub fn sys_read(ctx: ProbeContext) -> u32 {
    match try_syscall_entry(ctx, SYSCALL_READ) {
        Ok(()) => 0,
        Err(ret) => ret as u32,
    }
}

#[kprobe]
pub fn sys_write(ctx: ProbeContext) -> u32 {
    match try_syscall_entry(ctx, SYSCALL_WRITE) {
        Ok(()) => 0,
        Err(ret) => ret as u32,
    }
}

fn try_syscall_entry(ctx: ProbeContext, event_type: u8) -> Result<(), i64> {
    let pid_tgid = aya_ebpf::helpers::bpf_get_current_pid_tgid();
    let pid = (pid_tgid >> 32) as u32;
    let tid = pid_tgid as u32;

    let comm = aya_ebpf::helpers::bpf_get_current_comm().unwrap_or([0u8; 16]);
    let timestamp_ns = unsafe { bpf_ktime_get_ns() };

    // Syscall arguments live in `pt_regs` at the arch-specific register
    // slots.  For kprobes on x86_64 syscall entry points, aya exposes them
    // as `ctx.arg(0)`, `ctx.arg(1)`, ... mapping to rdi, rsi, rdx.
    let arg0: i64 = ctx.arg(0).unwrap_or(0);

    let event = SyscallEvent {
        timestamp_ns,
        pid,
        tid,
        syscall_nr: 0,
        arg_fd: arg0,
        arg_bytes: 0,
        comm,
        event_type,
        _pad: [0; 7],
    };

    if let Some(mut entry) = SYSCALL_EVENTS.reserve::<SyscallEvent>(0) {
        entry.write(event);
        entry.submit(0);
    }

    Ok(())
}

// `tcp_connect` and the syscall probes are plain (GPL-only) kprobe targets,
// not GPL-tainted tracepoint helpers, but the kernel still refuses to load
// *any* BPF program without a declared license section, so this is required
// regardless.
// without a declared license section, so this is required regardless.
#[no_mangle]
#[link_section = "license"]
pub static LICENSE: [u8; 4] = *b"GPL\0";

#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
