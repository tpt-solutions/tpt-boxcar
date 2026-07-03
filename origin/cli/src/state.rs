use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const STATE_FILE: &str = ".tpt-origin-state.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    pub name: String,
    pub service_type: String,
    /// PID of the real OS process backing this service, if any (only
    /// `type: process` services spawn one today).
    #[serde(default)]
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentState {
    pub manifest_name: String,
    pub pid: u32,
    pub started_at: String,
    pub services: Vec<ServiceState>,
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE)
}

pub fn write(dir: &Path, state: &EnvironmentState) -> anyhow::Result<()> {
    let content = serde_json::to_string_pretty(state)?;
    std::fs::write(state_path(dir), content)?;
    Ok(())
}

pub fn read(dir: &Path) -> anyhow::Result<Option<EnvironmentState>> {
    let path = state_path(dir);
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&content)?))
}

pub fn clear(dir: &Path) -> anyhow::Result<()> {
    let path = state_path(dir);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Best-effort termination of a previously recorded `tpt origin up` process.
/// Returns true if a terminate command was issued (not a guarantee the
/// process has actually exited yet).
pub fn terminate(pid: u32) -> bool {
    let status = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()
    } else {
        std::process::Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
    };
    matches!(status, Ok(s) if s.success())
}
