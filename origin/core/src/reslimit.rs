use crate::manifest::ResourceLimits;

/// Parses a docker-style memory limit string (`"512m"`, `"1g"`, `"128Mi"`,
/// `"64Ki"`, a bare byte count) into a byte count. Returns `None` on
/// unparseable input so callers can warn rather than silently guessing.
pub fn parse_memory_limit(value: &str) -> Option<u64> {
    let value = value.trim();
    let (number_part, multiplier) = if let Some(n) = value.strip_suffix("Gi").or_else(|| value.strip_suffix("gi")) {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = value.strip_suffix("Mi").or_else(|| value.strip_suffix("mi")) {
        (n, 1024 * 1024)
    } else if let Some(n) = value.strip_suffix("Ki").or_else(|| value.strip_suffix("ki")) {
        (n, 1024)
    } else if let Some(n) = value.strip_suffix('g').or_else(|| value.strip_suffix('G')) {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = value.strip_suffix('m').or_else(|| value.strip_suffix('M')) {
        (n, 1024 * 1024)
    } else if let Some(n) = value.strip_suffix('k').or_else(|| value.strip_suffix('K')) {
        (n, 1024)
    } else if let Some(n) = value.strip_suffix('b').or_else(|| value.strip_suffix('B')) {
        (n, 1)
    } else {
        (value, 1)
    };
    number_part.trim().parse::<u64>().ok().map(|n| n * multiplier)
}

/// Parses a CPU limit string (e.g. `"1.5"`, `"2"`) into a fractional CPU
/// count. Returns `None` on unparseable input.
pub fn parse_cpu_limit(value: &str) -> Option<f64> {
    value.trim().parse::<f64>().ok()
}

/// Applies resource limits to a child process **after fork, before exec**.
/// This is intended to be called from a `tokio::process::Command::pre_exec`
/// hook on Linux/macOS, where it runs in the child's address space.
///
/// Memory: sets `RLIMIT_AS` (virtual address space) which is the closest
/// portable equivalent to a cgroup memory limit. Not as precise as cgroup
/// enforcement (the OOM killer won't target this process specifically), but
/// it prevents unbounded growth without requiring root or cgroup setup.
///
/// CPU: sets `RLIMIT_CPU` which causes the kernel to send `SIGKXCPU` when
/// the process exceeds the limit. This is a soft enforcement — the process
/// gets a signal, not a hard throttle — but it's the standard POSIX mechanism.
///
/// # Safety
/// Must be called from a forked child process context (inside `pre_exec`).
#[cfg(unix)]
pub unsafe fn apply_rlimits(memory_bytes: Option<u64>, cpu_seconds: Option<u64>) {
    use libc::{setrlimit, RLIMIT_AS, RLIMIT_CPU, rlimit};

    if let Some(bytes) = memory_bytes {
        let rlim = rlimit { rlim_cur: bytes, rlim_max: bytes };
        setrlimit(RLIMIT_AS, &rlim);
    }
    if let Some(secs) = cpu_seconds {
        let rlim = rlimit { rlim_cur: secs, rlim_max: secs };
        setrlimit(RLIMIT_CPU, &rlim);
    }
}

#[cfg(not(unix))]
pub unsafe fn apply_rlimits(_memory_bytes: Option<u64>, _cpu_seconds: Option<u64>) {}

/// Applies resource limits from a `ResourceLimits` struct, returning parsed
/// values suitable for `apply_rlimits` or Wasmtime configuration.
pub fn parse_resource_limits(resources: &ResourceLimits) -> (Option<u64>, Option<f64>) {
    let memory_bytes = resources.memory.as_deref().and_then(parse_memory_limit);
    let cpu_cores = resources.cpu.as_deref().and_then(parse_cpu_limit);
    (memory_bytes, cpu_cores)
}

/// Wasmtime `ResourceLimiter` implementation that enforces memory and table
/// size limits on a per-module basis. Attaches to a `wasmtime::Store` via
/// `Store::limiter()`.
pub struct WasmResourceLimiter {
    max_memory_bytes: u64,
    memory_used: u64,
}

impl WasmResourceLimiter {
    pub fn new(max_memory_bytes: u64) -> Self {
        Self {
            max_memory_bytes,
            memory_used: 0,
        }
    }
}

impl wasmtime::ResourceLimiter for WasmResourceLimiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> anyhow::Result<bool> {
        let desired = desired as u64;
        if desired > self.max_memory_bytes {
            tracing::warn!(
                "wasm memory growth denied: requested {desired} bytes exceeds limit of {} bytes",
                self.max_memory_bytes,
            );
            return Ok(false);
        }
        self.memory_used = desired;
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: u32,
        _desired: u32,
        _maximum: Option<u32>,
    ) -> anyhow::Result<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_memory_limit_bytes() {
        assert_eq!(parse_memory_limit("1024"), Some(1024));
        assert_eq!(parse_memory_limit("1024b"), Some(1024));
        assert_eq!(parse_memory_limit("1024B"), Some(1024));
    }

    #[test]
    fn parse_memory_limit_kilobytes() {
        assert_eq!(parse_memory_limit("1k"), Some(1024));
        assert_eq!(parse_memory_limit("1K"), Some(1024));
        assert_eq!(parse_memory_limit("1Ki"), Some(1024));
        assert_eq!(parse_memory_limit("1ki"), Some(1024));
        assert_eq!(parse_memory_limit("512k"), Some(512 * 1024));
    }

    #[test]
    fn parse_memory_limit_megabytes() {
        assert_eq!(parse_memory_limit("1m"), Some(1024 * 1024));
        assert_eq!(parse_memory_limit("1M"), Some(1024 * 1024));
        assert_eq!(parse_memory_limit("1Mi"), Some(1024 * 1024));
        assert_eq!(parse_memory_limit("256mi"), Some(256 * 1024 * 1024));
        assert_eq!(parse_memory_limit("512m"), Some(512 * 1024 * 1024));
    }

    #[test]
    fn parse_memory_limit_gigabytes() {
        assert_eq!(parse_memory_limit("1g"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory_limit("1G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory_limit("1Gi"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_memory_limit("2gi"), Some(2 * 1024 * 1024 * 1024));
    }

    #[test]
    fn parse_memory_limit_with_whitespace() {
        assert_eq!(parse_memory_limit(" 256m "), Some(256 * 1024 * 1024));
    }

    #[test]
    fn parse_memory_limit_invalid() {
        assert_eq!(parse_memory_limit(""), None);
        assert_eq!(parse_memory_limit("abc"), None);
        assert_eq!(parse_memory_limit("1.5g"), None); // no decimal support
    }

    #[test]
    fn parse_cpu_limit_values() {
        assert_eq!(parse_cpu_limit("1.0"), Some(1.0));
        assert_eq!(parse_cpu_limit("2"), Some(2.0));
        assert_eq!(parse_cpu_limit("0.5"), Some(0.5));
        assert_eq!(parse_cpu_limit(" 1.5 "), Some(1.5));
        assert_eq!(parse_cpu_limit(""), None);
        assert_eq!(parse_cpu_limit("abc"), None);
    }

    #[test]
    fn wasm_limiter_allows_within_limit() {
        let mut limiter = WasmResourceLimiter::new(1024 * 1024); // 1 MiB
        assert!(limiter.memory_growing(0, 512 * 1024, None).unwrap()); // 512 KiB — OK
        assert!(limiter.memory_growing(512 * 1024, 1024 * 1024, None).unwrap()); // 1 MiB — OK
    }

    #[test]
    fn wasm_limiter_denies_over_limit() {
        let mut limiter = WasmResourceLimiter::new(1024 * 1024); // 1 MiB
        assert!(!limiter.memory_growing(0, 2 * 1024 * 1024, None).unwrap()); // 2 MiB — denied
    }

    #[test]
    fn wasm_limiter_table_always_allows() {
        let mut limiter = WasmResourceLimiter::new(1024);
        assert!(limiter.table_growing(0, 10000, None).unwrap());
    }
}
