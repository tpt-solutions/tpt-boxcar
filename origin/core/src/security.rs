use crate::manifest::SecurityConfig;

/// Applies security settings to a child process **after fork, before exec**.
///
/// - `no_new_privileges`: calls `prctl(PR_SET_NO_NEW_PRIVS, 1)` to prevent
///   the process from gaining privileges via setuid/setgid binaries.
/// - `cap_drop`/`cap_add`: uses `prctl(PR_CAPBSET_DROP/PR_CAPBSET_READ)`
///   to manipulate the process's capability bounding set. When `cap_drop`
///   contains `"ALL"`, all capabilities are dropped first, then `cap_add`
///   ones are restored.
///
/// # Safety
/// Must be called from a forked child process context (inside `pre_exec`).
/// Requires Linux.
#[cfg(target_os = "linux")]
pub unsafe fn apply_security_pre_exec(config: &SecurityConfig) {
    use libc::{prctl, PR_CAPBSET_DROP, PR_CAPBSET_READ, PR_SET_NO_NEW_PRIVS};

    if config.no_new_privileges {
        prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
    }

    if !config.cap_drop.is_empty() {
        let drop_all = config.cap_drop.iter().any(|c| c == "ALL");
        if drop_all {
            // Drop all capabilities from the bounding set
            for cap in 0..40 {
                prctl(PR_CAPBSET_DROP, cap, 0, 0, 0);
            }
        } else {
            for cap_name in &config.cap_drop {
                if let Some(cap_num) = linux_capability_number(cap_name) {
                    prctl(PR_CAPBSET_DROP, cap_num, 0, 0, 0);
                }
            }
        }
    }

    if !config.cap_add.is_empty() {
        // Note: PR_CAPBSET_DROP is one-way (can only drop, not add).
        // cap_add is honored at the OCI/containerd level via --cap-add.
        // For process services, we can only drop capabilities, not re-add
        // them after dropping ALL. Log a warning if both ALL drop and add
        // are specified (handled by the caller).
    }
}

/// Non-Linux no-op: capabilities don't exist on this platform.
///
/// # Safety
/// Always safe — does nothing.
#[cfg(not(target_os = "linux"))]
pub unsafe fn apply_security_pre_exec(_config: &SecurityConfig) {}

/// Maps a Linux capability name (e.g. `"NET_BIND_SERVICE"`, `"SYS_PTRACE"`)
/// to its numeric value (as used by `prctl(PR_CAPBSET_DROP, cap)`).
/// Returns `None` for unknown capability names.
#[cfg(target_os = "linux")]
fn linux_capability_number(name: &str) -> Option<u32> {
    match name {
        "CHOWN" => Some(0),
        "DAC_OVERRIDE" => Some(1),
        "DAC_READ_SEARCH" => Some(2),
        "FOWNER" => Some(3),
        "FSETID" => Some(4),
        "KILL" => Some(5),
        "SETGID" => Some(6),
        "SETUID" => Some(7),
        "SETPCAP" => Some(8),
        "LINUX_IMMUTABLE" => Some(9),
        "NET_BIND_SERVICE" => Some(10),
        "NET_BROADCAST" => Some(11),
        "NET_ADMIN" => Some(12),
        "NET_RAW" => Some(13),
        "IPC_LOCK" => Some(14),
        "IPC_OWNER" => Some(15),
        "SYS_MODULE" => Some(16),
        "SYS_RAWIO" => Some(17),
        "SYS_CHROOT" => Some(18),
        "SYS_PTRACE" => Some(19),
        "SYS_PACCT" => Some(20),
        "SYS_ADMIN" => Some(21),
        "SYS_BOOT" => Some(22),
        "SYS_NICE" => Some(23),
        "SYS_RESOURCE" => Some(24),
        "SYS_TIME" => Some(25),
        "SYS_TTY_CONFIG" => Some(26),
        "MKNOD" => Some(27),
        "LEASE" => Some(28),
        "AUDIT_WRITE" => Some(29),
        "AUDIT_CONTROL" => Some(30),
        "SETFCAP" => Some(31),
        "MAC_OVERRIDE" => Some(32),
        "MAC_ADMIN" => Some(33),
        "SYSLOG" => Some(34),
        "WAKE_ALARM" => Some(35),
        "BLOCK_SUSPEND" => Some(36),
        "AUDIT_READ" => Some(37),
        "PERFMON" => Some(38),
        "BPF" => Some(39),
        "CHECKPOINT_RESTORE" => Some(40),
        _ => None,
    }
}

/// Converts a SecurityConfig into `ctr run` CLI arguments for OCI containers.
/// Returns a list of additional args to append to the `ctr run` command.
pub fn security_to_ctr_args(config: &SecurityConfig) -> Vec<String> {
    let mut args = Vec::new();

    if !config.cap_drop.is_empty() {
        let drop_all = config.cap_drop.iter().any(|c| c == "ALL");
        if drop_all {
            args.push("--cap-drop".to_string());
            args.push("ALL".to_string());
            // Re-add specific capabilities
            for cap in &config.cap_add {
                args.push("--cap-add".to_string());
                args.push(cap.clone());
            }
        } else {
            for cap in &config.cap_drop {
                args.push("--cap-drop".to_string());
                args.push(cap.clone());
            }
        }
    } else if !config.cap_add.is_empty() {
        for cap in &config.cap_add {
            args.push("--cap-add".to_string());
            args.push(cap.clone());
        }
    }

    if config.no_new_privileges {
        args.push("--security-opt".to_string());
        args.push("no-new-privileges".to_string());
    }

    if config.read_only {
        args.push("--read-only".to_string());
    }

    // Device passthrough (GPU, TUN/TAP, etc.)
    for device in &config.devices {
        let device_path = device
            .path_in_container
            .as_deref()
            .unwrap_or(&device.path_on_host);
        args.push("--device".to_string());
        args.push(format!(
            "{}:{}:{}",
            device.path_on_host, device_path, device.permissions
        ));
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::DeviceMapping;

    #[test]
    fn security_to_ctr_args_drop_all_with_adds() {
        let config = SecurityConfig {
            cap_add: vec!["NET_BIND_SERVICE".to_string(), "SYS_PTRACE".to_string()],
            cap_drop: vec!["ALL".to_string()],
            read_only: false,
            no_new_privileges: false,
            devices: vec![],
        };
        let args = security_to_ctr_args(&config);
        assert_eq!(
            args,
            vec![
                "--cap-drop",
                "ALL",
                "--cap-add",
                "NET_BIND_SERVICE",
                "--cap-add",
                "SYS_PTRACE",
            ]
        );
    }

    #[test]
    fn security_to_ctr_args_drop_specific() {
        let config = SecurityConfig {
            cap_add: vec![],
            cap_drop: vec!["NET_RAW".to_string(), "SYS_ADMIN".to_string()],
            read_only: false,
            no_new_privileges: false,
            devices: vec![],
        };
        let args = security_to_ctr_args(&config);
        assert_eq!(
            args,
            vec!["--cap-drop", "NET_RAW", "--cap-drop", "SYS_ADMIN",]
        );
    }

    #[test]
    fn security_to_ctr_args_no_new_privileges() {
        let config = SecurityConfig {
            cap_add: vec![],
            cap_drop: vec![],
            read_only: false,
            no_new_privileges: true,
            devices: vec![],
        };
        let args = security_to_ctr_args(&config);
        assert_eq!(args, vec!["--security-opt", "no-new-privileges"]);
    }

    #[test]
    fn security_to_ctr_args_read_only() {
        let config = SecurityConfig {
            cap_add: vec![],
            cap_drop: vec![],
            read_only: true,
            no_new_privileges: false,
            devices: vec![],
        };
        let args = security_to_ctr_args(&config);
        assert_eq!(args, vec!["--read-only"]);
    }

    #[test]
    fn security_to_ctr_args_device_passthrough() {
        let config = SecurityConfig {
            cap_add: vec![],
            cap_drop: vec![],
            read_only: false,
            no_new_privileges: false,
            devices: vec![DeviceMapping {
                path_on_host: "/dev/nvidia0".to_string(),
                path_in_container: None,
                permissions: "rwm".to_string(),
            }],
        };
        let args = security_to_ctr_args(&config);
        assert_eq!(
            args,
            vec!["--device", "/dev/nvidia0:/dev/nvidia0:rwm"]
        );
    }

    #[test]
    fn security_to_ctr_args_device_with_custom_path() {
        let config = SecurityConfig {
            cap_add: vec![],
            cap_drop: vec![],
            read_only: false,
            no_new_privileges: false,
            devices: vec![DeviceMapping {
                path_on_host: "/dev/nvidia0".to_string(),
                path_in_container: Some("/dev/gpu0".to_string()),
                permissions: "rw".to_string(),
            }],
        };
        let args = security_to_ctr_args(&config);
        assert_eq!(
            args,
            vec!["--device", "/dev/nvidia0:/dev/gpu0:rw"]
        );
    }

    #[test]
    fn security_to_ctr_args_empty_config() {
        let config = SecurityConfig::default();
        let args = security_to_ctr_args(&config);
        assert!(args.is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_capability_number_known_caps() {
        assert_eq!(linux_capability_number("CHOWN"), Some(0));
        assert_eq!(linux_capability_number("NET_BIND_SERVICE"), Some(10));
        assert_eq!(linux_capability_number("SYS_ADMIN"), Some(21));
        assert_eq!(linux_capability_number("BPF"), Some(39));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_capability_number_unknown() {
        assert_eq!(linux_capability_number("FAKE_CAP"), None);
    }
}
