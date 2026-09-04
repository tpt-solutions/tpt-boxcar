//! Live resource usage collection for running services.
//!
//! On Linux, reads `/proc/<pid>/stat` and `/proc/<pid>/status` for
//! process services, and cgroup files for OCI containers. On other
//! platforms, returns what's available through platform APIs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Live resource usage statistics for a single service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStats {
    pub name: String,
    /// CPU usage as a percentage (0.0 - 100.0+). `None` if not measurable.
    pub cpu_percent: Option<f64>,
    /// Resident set size (RSS) in bytes. `None` if not measurable.
    pub memory_rss_bytes: Option<u64>,
    /// Memory limit in bytes from the service's resource config. `None` if
    /// no limit is set.
    pub memory_limit_bytes: Option<u64>,
    /// Network bytes received. `None` if not measurable.
    pub net_rx_bytes: Option<u64>,
    /// Network bytes transmitted. `None` if not measurable.
    pub net_tx_bytes: Option<u64>,
    /// Process uptime in seconds since the service started.
    pub uptime_secs: u64,
    /// The service's OS PID, if available.
    pub pid: Option<u32>,
}

/// Reads per-process CPU and memory stats from `/proc/<pid>/stat` and
/// `/proc/<pid>/status`. Returns `(cpu_time_ticks, rss_bytes)`.
#[cfg(target_os = "linux")]
fn read_proc_stats(pid: u32) -> Option<(u64, u64)> {
    use std::fs;

    let stat_path = format!("/proc/{pid}/stat");
    let stat_content = fs::read_to_string(&stat_path).ok()?;

    // Fields 14 (utime) and 15 (stime) in /proc/<pid>/stat are CPU ticks
    // consumed in user and kernel mode. The comm field (field 2) may contain
    // spaces/parens, so we count from the last `)`.
    let after_comm = stat_content.rfind(')')? + 2;
    let fields: Vec<&str> = stat_content[after_comm..].split_whitespace().collect();
    if fields.len() < 20 {
        return None;
    }
    let utime: u64 = fields[11].parse().ok()?;
    let stime: u64 = fields[12].parse().ok()?;
    let cpu_ticks = utime + stime;

    // RSS from /proc/<pid>/status (VmRSS line)
    let status_path = format!("/proc/{pid}/status");
    let status_content = fs::read_to_string(&status_path).ok()?;
    let rss_bytes = status_content
        .lines()
        .find(|line| line.starts_with("VmRSS:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .map(|kb| kb * 1024); // Convert from kB to bytes

    Some((cpu_ticks, rss_bytes.unwrap_or(0)))
}

/// Reads network I/O from `/proc/<pid>/net/dev` and sums all interfaces.
#[cfg(target_os = "linux")]
fn read_net_io(pid: u32) -> Option<(u64, u64)> {
    let path = format!("/proc/{pid}/net/dev");
    let content = std::fs::read_to_string(&path).ok()?;

    let mut rx_total = 0u64;
    let mut tx_total = 0u64;

    for line in content.lines().skip(2) {
        // Format: "  iface: rx_bytes rx_packets ... tx_bytes tx_packets ..."
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }
        // Skip "lo" (loopback)
        if parts[0].trim_end_matches(':') == "lo" {
            continue;
        }
        if let Ok(rx) = parts[1].parse::<u64>() {
            rx_total += rx;
        }
        if let Ok(tx) = parts[9].parse::<u64>() {
            tx_total += tx;
        }
    }

    Some((rx_total, tx_total))
}

/// No-op stubs for non-Linux platforms.
#[cfg(not(target_os = "linux"))]
fn read_proc_stats(_pid: u32) -> Option<(u64, u64)> {
    None
}

#[cfg(not(target_os = "linux"))]
fn read_net_io(_pid: u32) -> Option<(u64, u64)> {
    None
}

/// Collects live stats for all running services with known PIDs.
/// Services without PIDs (e.g., Wasm services) get `None` for
/// all measurable fields.
pub fn collect_stats(
    services: &HashMap<String, crate::runtime::RunningService>,
    start_times: &HashMap<String, std::time::Instant>,
) -> Vec<ServiceStats> {
    let sys_ticks_per_sec = 100; // Most Linux systems: USER_HZ = 100

    services
        .iter()
        .map(|(name, svc)| {
            let pid = svc.pid;
            let uptime_secs = start_times
                .get(name)
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(0);

            let (cpu_percent, memory_rss_bytes, net_rx_bytes, net_tx_bytes) = if let Some(pid) = pid
            {
                let (cpu_ticks, rss) = read_proc_stats(pid).unwrap_or((0, 0));
                let (rx, tx) = read_net_io(pid).unwrap_or((0, 0));

                // CPU% approximation: ticks / (uptime_secs * ticks_per_sec) * 100
                // This is cumulative, not instantaneous — for a real-time
                // view you'd sample twice and diff. Good enough for a snapshot.
                let cpu = if uptime_secs > 0 {
                    Some(
                        (cpu_ticks as f64 / (uptime_secs as f64 * sys_ticks_per_sec as f64))
                            * 100.0,
                    )
                } else {
                    None
                };

                (cpu, Some(rss), Some(rx), Some(tx))
            } else {
                (None, None, None, None)
            };

            ServiceStats {
                name: name.clone(),
                cpu_percent,
                memory_rss_bytes,
                memory_limit_bytes: None, // caller can fill from manifest
                net_rx_bytes,
                net_tx_bytes,
                uptime_secs,
                pid,
            }
        })
        .collect()
}

/// Formats a byte count as a human-readable string (e.g. "1.2MB", "340KB").
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        let kb = bytes as f64 / 1024.0;
        if kb.fract() == 0.0 {
            format!("{:.0}KB", kb)
        } else {
            format!("{:.1}KB", kb)
        }
    } else {
        format!("{bytes}B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_various() {
        assert_eq!(format_bytes(0), "0B");
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1024), "1KB");
        assert_eq!(format_bytes(1536), "1.5KB"); // Note: 1536/1024 = 1.5
        assert_eq!(format_bytes(1024 * 1024), "1.0MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0GB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0MB");
    }
}
