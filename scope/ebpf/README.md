# scope/ebpf

Real kernel-side probes backing `tpt-scope-agent`'s `EbpfNetworkProbe` and
`EbpfSyscallProbe` (`scope/agent/src/probes.rs`), replacing the in-memory
state-machine stubs with actual kprobes via [`aya`](https://aya-rs.dev):
- `tcp_connect()` — captures IPv4 connect events (4-tuple)
- `openat`, `read`, `write` — captures syscall entry events (fd, dirfd)

Deliberately **not** a member of the root repo Cargo workspace — see the
comment in [`Cargo.toml`](./Cargo.toml). Building any of this requires
Linux; the `probe` crate additionally requires a nightly toolchain (pinned
in `probe/rust-toolchain.toml`) to target `bpfel-unknown-none`.

## Layout

- `common/` — `#![no_std]`, alloc-free `ConnectEvent` and `SyscallEvent`
  structs shared byte-for-byte between the kernel probes and the userspace
  loader.
- `probe/` — the `#![no_std] #![no_main]` eBPF programs: kprobes on
  `tcp_connect()`, `__x64_sys_openat`, `__x64_sys_read`, and
  `__x64_sys_write`. Each writes to its own `RingBuf` map (`CONNECT_EVENTS`
  or `SYSCALL_EVENTS`). Not part of this directory's own inner workspace
  (`exclude`d) — built as a separate `cargo` invocation by
  `loader/build.rs` via `aya-build`.
- `loader/` — userspace: embeds the compiled probe object, attaches the
  selected kprobes, and drains each ring buffer from a dedicated background
  thread into a plain queue. `TcpConnectProbe` handles network events;
  `SyscallProbeLoader` handles syscall events.

## Building

```bash
# from repo root, Linux only:
cd scope/ebpf/loader
cargo build   # aya-build cross-compiles probe/ as part of this
```

`tpt-scope-agent` only pulls this in when built with `--features ebpf`
(off by default, see that crate's `Cargo.toml`) — a plain
`cargo build --workspace` from the repo root never touches this directory.

## Known limitations

- **x86_64 only**: syscall kprobes attach to `__x64_sys_*` functions.
  arm64 uses `__arm64_sys_*` prefixes — a follow-up.
- IPv4 only; `AF_INET6` connections are silently skipped (`probe/src/main.rs`).
- `sport` (local port) is always `0` — the kernel hasn't picked it yet at
  the point `tcp_connect()` fires.
- Syscall probes are **entry-only** (no kretprobes yet): `arg_bytes` is
  always 0, `exit_code` is 0, and `latency` is `Duration::ZERO`. Follow-up:
  kretprobes on `sys_openat` return to capture the actual fd and return code.
- `openat` path resolution is deferred — only `dirfd` is captured. Full
  filename requires `bpf_probe_read_user_str` or `/proc/<pid>/fd` lookup.
- The `sock_common` field offsets (`SKC_DADDR_OFFSET` etc. in
  `probe/src/main.rs`) are hardcoded to the layout bcc's/bpftrace's
  `tcpconnect` tools have relied on for years, not read via BTF/CO-RE — if
  a kernel ever changes that historically-stable layout, decoding breaks
  silently (family check fails closed and drops the event, it won't panic).
- Ring buffer draining polls every 5ms on a dedicated thread rather than
  blocking on the map's fd via epoll — simpler, adequate for connect-rate
  events, but not maximally efficient.
- **Unverified**: this environment (Windows) cannot compile or run eBPF/BPF
  code at all, let alone attach a live kprobe. Written directly against the
  `aya`/`aya-ebpf`/`aya-build` public APIs from documentation; a real Linux
  run (`cargo build` in `loader/`, then the `ebpf_tcp_connect_daemon`
  example in `scope/agent/examples/`) is the outstanding verification step.
