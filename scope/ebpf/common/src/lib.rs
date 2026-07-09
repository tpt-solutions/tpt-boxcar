#![no_std]

/// A captured IPv4 `tcp_connect()` call, as written into the ring buffer by
/// the kernel-side probe and read back verbatim by the userspace loader.
///
/// `#[repr(C)]` + all-`Copy`/fixed-size fields so the byte layout is stable
/// across the kernel (`bpfel-unknown-none`) and userspace (host) builds of
/// this crate — no padding-sensitive types, no pointers.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ConnectEvent {
    pub timestamp_ns: u64,
    pub pid: u32,
    pub tid: u32,
    /// Raw 4 bytes of `skc_rcv_saddr`/`skc_daddr` as read from kernel
    /// memory, reinterpreted as a native-endian `u32` by
    /// `bpf_probe_read_kernel`. To recover the actual dotted-quad byte
    /// order, take `.to_ne_bytes()` on the *reading* host (this round-trips
    /// the original memory bytes regardless of host endianness — do not
    /// use `.to_be_bytes()`/`from_be`, those would reverse it).
    pub saddr: u32,
    pub daddr: u32,
    /// Unresolved — `tcp_connect()` fires before the kernel picks the local
    /// port, so this is always `0`.
    pub sport: u16,
    /// Already byte-swapped to host-native order by the probe (unlike
    /// `saddr`/`daddr`), so this is a plain, directly usable port number.
    pub dport: u16,
    /// `current->comm`, NUL-padded, not necessarily NUL-terminated if the
    /// full 16 bytes are used.
    pub comm: [u8; 16],
}

impl ConnectEvent {
    pub const LEN: usize = core::mem::size_of::<ConnectEvent>();

    /// Reinterprets a raw ring-buffer record as a `ConnectEvent`. Used on
    /// the userspace side, where `aya::maps::RingBuf` hands back `&[u8]`
    /// rather than a typed value directly (the same struct is also written
    /// with `write::<ConnectEvent>()` by the eBPF side, so the byte layout
    /// always matches by construction).
    ///
    /// # Safety
    /// `bytes` must be at least `ConnectEvent::LEN` bytes, written by the
    /// probe side of this same crate version.
    pub unsafe fn from_bytes(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() >= Self::LEN);
        core::ptr::read_unaligned(bytes.as_ptr() as *const ConnectEvent)
    }

    /// Byte length of `comm` before the first NUL (or the full 16 bytes if
    /// unterminated). No `alloc` dependency here — this crate is included
    /// as-is by the `no_std`, no-heap `probe` crate; string conversion
    /// happens on the userspace (`loader`) side, which has `std`.
    pub fn comm_len(&self) -> usize {
        self.comm
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.comm.len())
    }
}

/// A captured syscall entry event (openat, read, write), written into a
/// separate ring buffer by the kernel-side probe and drained by the
/// userspace `SyscallProbeLoader`.
///
/// Same `#[repr(C)]` + `Copy` constraints as `ConnectEvent` — the struct
/// crosses the kernel/userspace boundary byte-for-byte.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct SyscallEvent {
    pub timestamp_ns: u64,
    pub pid: u32,
    pub tid: u32,
    /// Syscall number (e.g. `__NR_openat`). Not filled by the entry-only
    /// kprobe — reserved for future use with raw tracepoints.
    pub syscall_nr: i64,
    /// For `openat`: the `dirfd` argument (AT_FDCWD = -100).
    /// For `read`/`write`: the fd number.
    pub arg_fd: i64,
    /// Bytes transferred. Always `0` on entry-only probes — a future
    /// kretprobe will capture the actual return value.
    pub arg_bytes: u64,
    /// `current->comm`, NUL-padded, not necessarily NUL-terminated.
    pub comm: [u8; 16],
    /// Event type discriminator: `0` = openat, `1` = read, `2` = write.
    pub event_type: u8,
    /// Alignment padding — keeps the struct at a round 64 bytes and
    /// guarantees identical layout across kernel and userspace builds.
    _pad: [u8; 7],
}

impl SyscallEvent {
    pub const LEN: usize = core::mem::size_of::<SyscallEvent>();

    pub unsafe fn from_bytes(bytes: &[u8]) -> Self {
        debug_assert!(bytes.len() >= Self::LEN);
        core::ptr::read_unaligned(bytes.as_ptr() as *const SyscallEvent)
    }

    pub fn comm_len(&self) -> usize {
        self.comm
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.comm.len())
    }
}
