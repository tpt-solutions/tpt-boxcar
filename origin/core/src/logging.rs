//! In-process log rotation for service stdout/stderr capture.
//!
//! When a service's `logging:` config uses `driver: "file"`, Origin writes
//! the service's output through a [`LogRotation`] wrapper that enforces
//! `max_size` and `max_file` limits — rotating the log file when either
//! threshold is exceeded. This gives bounded disk usage for long-running
//! services without relying on an external log rotation daemon.

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::manifest::LoggingConfig;

/// Directory where service log files are written. Mirrors the old
/// `containerd::logs_dir` but is now available on all platforms so
/// process/wasm services can also capture logs here. Overridable via
/// `ORIGIN_LOGS_DIR`.
pub fn logs_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("ORIGIN_LOGS_DIR").unwrap_or_else(|_| "/var/lib/tpt-boxcar/logs".to_string()),
    )
}

/// Path of the captured combined stdout/stderr log for service `id`.
pub fn log_path(id: &str) -> PathBuf {
    logs_dir().join(format!("{id}.log"))
}

/// Parses a human-readable size string (e.g. `"10m"`, `"1g"`, `"512k"`)
/// into bytes. Returns `None` if the input is `None` or empty.
pub fn parse_size_bytes(s: &str) -> Option<u64> {
    let s = s.trim().to_lowercase();
    let (num_part, multiplier) = if let Some(rest) = s.strip_suffix("g") {
        (rest, 1024 * 1024 * 1024)
    } else if let Some(rest) = s.strip_suffix("m") {
        (rest, 1024 * 1024)
    } else if let Some(rest) = s.strip_suffix("k") {
        (rest, 1024)
    } else if let Some(rest) = s.strip_suffix("b") {
        (rest, 1)
    } else {
        (s.as_str(), 1)
    };
    let num: f64 = num_part.parse().ok()?;
    Some((num * multiplier as f64) as u64)
}

/// Resolves the effective logging configuration for a service by merging
/// the per-service override (if any) with the manifest-level default.
pub fn resolve_logging_config(
    service_logging: &Option<LoggingConfig>,
    manifest_logging: &Option<LoggingConfig>,
) -> LoggingConfig {
    let base = manifest_logging.clone().unwrap_or_default();
    match service_logging {
        Some(override_cfg) => LoggingConfig {
            driver: if override_cfg.driver == default_log_driver() {
                base.driver
            } else {
                override_cfg.driver.clone()
            },
            max_size: override_cfg.max_size.clone().or(base.max_size),
            max_file: override_cfg.max_file.or(base.max_file),
        },
        None => base,
    }
}

fn default_log_driver() -> String {
    "file".to_string()
}

/// A log file with rotation support. Writes go through an internal
/// `BufWriter` that flushes to the underlying file. When the file exceeds
/// `max_size` bytes (or after every write if size limits are disabled),
/// the rotation check triggers — renaming old copies and starting a fresh
/// file.
pub struct LogRotation {
    path: PathBuf,
    writer: BufWriter<File>,
    current_size: u64,
    max_size_bytes: Option<u64>,
    max_file: u32,
}

impl LogRotation {
    /// Opens (or creates) the log file at `path` and configures rotation
    /// according to the resolved `LoggingConfig`.
    pub fn open(path: impl Into<PathBuf>, config: &LoggingConfig) -> Result<Self> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create log directory {}", parent.display()))?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open log file {}", path.display()))?;

        let current_size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let max_size_bytes = config.max_size.as_deref().and_then(parse_size_bytes);
        let max_file = config.max_file.unwrap_or(3);

        Ok(Self {
            path,
            writer: BufWriter::new(file),
            current_size,
            max_size_bytes,
            max_file,
        })
    }

    /// Returns a reference to the underlying file, useful for cloning
    /// the file handle (e.g. to pass to a child process's stdout/stderr).
    pub fn get_ref(&self) -> &File {
        self.writer.get_ref()
    }

    /// Writes data to the log file, triggering rotation if the size
    /// threshold is exceeded.
    pub fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.writer.write_all(buf)?;
        self.current_size += buf.len() as u64;

        if self.should_rotate() {
            self.rotate()?;
        }
        Ok(())
    }

    /// Flushes any buffered data to disk.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    fn should_rotate(&self) -> bool {
        if let Some(max) = self.max_size_bytes {
            if max > 0 && self.current_size >= max {
                return true;
            }
        }
        false
    }

    /// Rotates log files: renames `service.log.N` → `service.log.N+1` for
    /// each existing copy (dropping the oldest if at `max_file` depth),
    /// then renames the current file to `service.log.1` and creates a
    /// fresh `service.log`.
    fn rotate(&mut self) -> io::Result<()> {
        // Flush and close the current file handle.
        self.writer.flush()?;

        let base = &self.path;
        let stem = base
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("service");
        let parent = base.parent().unwrap_or(Path::new("."));

        // Drop oldest file if we're at the rotation limit.
        if self.max_file > 0 {
            let oldest = parent.join(format!("{stem}.{}", self.max_file));
            if oldest.exists() {
                let _ = std::fs::remove_file(&oldest);
            }
        }

        // Shift existing rotated files: .N-1 → .N
        for i in (1..self.max_file).rev() {
            let src = parent.join(format!("{stem}.{i}"));
            let dst = parent.join(format!("{stem}.{}", i + 1));
            if src.exists() {
                let _ = std::fs::rename(&src, &dst);
            }
        }

        // Rotate current file to .1
        let rotated = parent.join(format!("{stem}.1"));
        let _ = std::fs::rename(base, &rotated);

        // Create fresh log file.
        let file = File::create(base)?;
        self.writer = BufWriter::new(file);
        self.current_size = 0;

        Ok(())
    }
}

impl Write for LogRotation {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_bytes_various() {
        assert_eq!(parse_size_bytes("10m"), Some(10 * 1024 * 1024));
        assert_eq!(parse_size_bytes("1g"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size_bytes("512k"), Some(512 * 1024));
        assert_eq!(parse_size_bytes("100"), Some(100));
        assert_eq!(parse_size_bytes("1.5m"), Some((1.5 * 1024.0 * 1024.0) as u64));
        assert_eq!(parse_size_bytes(""), None);
    }

    #[test]
    fn resolve_logging_config_per_service_overrides() {
        let manifest_cfg = Some(LoggingConfig {
            driver: "file".to_string(),
            max_size: Some("10m".to_string()),
            max_file: Some(3),
        });
        let service_cfg = Some(LoggingConfig {
            driver: "file".to_string(),
            max_size: Some("50m".to_string()),
            max_file: None, // inherit from manifest
        });
        let resolved = resolve_logging_config(&service_cfg, &manifest_cfg);
        assert_eq!(resolved.max_size, Some("50m".to_string()));
        assert_eq!(resolved.max_file, Some(3));
    }

    #[test]
    fn log_rotation_rotates_at_max_size() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        let config = LoggingConfig {
            driver: "file".to_string(),
            max_size: Some("1k".to_string()),
            max_file: Some(2),
        };

        let mut rot = LogRotation::open(&log_path, &config).unwrap();

        // Write enough to exceed 1k.
        let data = vec![b'x'; 600];
        rot.write_all(&data).unwrap();
        assert!(log_path.exists());

        // Write more to trigger rotation.
        rot.write_all(&data).unwrap();
        let rotated1 = dir.path().join("test.log.1");
        assert!(rotated1.exists(), "should have created test.log.1");
    }

    #[test]
    fn log_rotation_drops_oldest_at_max_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        let config = LoggingConfig {
            driver: "file".to_string(),
            max_size: Some("500b".to_string()),
            max_file: Some(2),
        };

        let mut rot = LogRotation::open(&log_path, &config).unwrap();
        let data = vec![b'y'; 300];

        // First write: no rotation yet.
        rot.write_all(&data).unwrap();
        // Second write: triggers rotation → test.log.1 created.
        rot.write_all(&data).unwrap();
        // Third write: triggers rotation → test.log.1 → test.log.2, new .1
        rot.write_all(&data).unwrap();

        let rotated2 = dir.path().join("test.log.2");
        assert!(rotated2.exists(), "should have created test.log.2");
        // max_file is 2, so .3 should NOT exist.
        let rotated3 = dir.path().join("test.log.3");
        assert!(!rotated3.exists(), "should not create beyond max_file");
    }
}
