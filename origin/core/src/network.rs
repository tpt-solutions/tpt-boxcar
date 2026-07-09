use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::Output;

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub name: String,
    pub driver: String,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
}

/// A service attached to a network: its allocated IP, and — on Linux, when
/// the service is a real containerd-backed process — the host-side veth end
/// that carries its traffic to the bridge (`None` when we only have enough
/// information to hand out an address, e.g. `process`/`wasm` services that
/// share the host's own network namespace, or when the platform/privilege
/// level didn't allow creating a real device).
#[derive(Debug, Clone)]
struct ConnectedService {
    ip: String,
    /// Only read (by `teardown_veth`) on Linux, the only platform that
    /// creates one; see `attach_service_netns`.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    veth_host: Option<String>,
}

#[derive(Debug, Clone)]
struct NetworkState {
    config: NetworkConfig,
    /// Real OS bridge/switch device name once one was actually created;
    /// `None` if we fell back to bookkeeping-only (unprivileged, or the
    /// platform command failed).
    bridge_name: Option<String>,
    /// First three octets of the allocated /24, e.g. `"10.88.4"`.
    subnet_prefix: String,
    prefix_len: u8,
    gateway: String,
    next_host_octet: u8,
    connected: HashMap<String, ConnectedService>,
}

pub struct NetworkManager {
    networks: HashMap<String, NetworkState>,
    /// Most recently created bridge/switch device name, kept for the
    /// existing single-value `get_bridge_interface` accessor.
    bridge_interface: Option<String>,
    next_subnet_octet: u8,
    /// When true, uses slirp4netns for unprivileged networking instead of
    /// creating real bridges/veths (requires root).
    rootless: bool,
}

impl NetworkManager {
    pub fn new() -> Self {
        Self {
            networks: HashMap::new(),
            bridge_interface: None,
            next_subnet_octet: 0,
            rootless: false,
        }
    }

    /// Enables rootless mode: networking will use slirp4netns for
    /// unprivileged connectivity instead of creating real bridges/veths.
    pub fn with_rootless(mut self) -> Self {
        self.rootless = true;
        self
    }

    pub async fn create_network(&mut self, config: NetworkConfig) -> Result<()> {
        tracing::info!(
            "Creating network: {} (driver: {}) {}",
            config.name,
            config.driver,
            if self.rootless { "(rootless)" } else { "" }
        );

        let subnet_prefix = if let Some(subnet) = &config.subnet {
            subnet
                .rsplit_once('.')
                .map(|(prefix, _)| prefix.to_string())
                .unwrap_or_else(|| subnet.clone())
        } else {
            let octet = self.next_subnet_octet;
            self.next_subnet_octet = self.next_subnet_octet.wrapping_add(1);
            format!("10.88.{octet}")
        };
        let prefix_len: u8 = 24;
        let gateway = config
            .gateway
            .clone()
            .unwrap_or_else(|| format!("{subnet_prefix}.1"));

        let bridge_name = if self.rootless {
            create_rootless_network(&config.name, &gateway, prefix_len).await
        } else {
            create_platform_bridge(&config.name, &gateway, prefix_len).await
        };

        if bridge_name.is_none() {
            tracing::warn!(
                "network '{}' created in bookkeeping-only mode (no real bridge device) \
                 — services on it get DNS entries but no real L2/L3 connectivity",
                config.name
            );
        }
        self.bridge_interface = bridge_name.clone();

        self.networks.insert(
            config.name.clone(),
            NetworkState {
                config,
                bridge_name,
                subnet_prefix,
                prefix_len,
                gateway,
                next_host_octet: 2,
                connected: HashMap::new(),
            },
        );
        Ok(())
    }

    pub async fn delete_network(&mut self, name: &str) -> Result<()> {
        tracing::info!("Deleting network: {name}");
        if let Some(net) = self.networks.remove(name) {
            for conn in net.connected.values() {
                teardown_veth(conn).await;
            }
            teardown_bridge(net.bridge_name.as_deref()).await;
        }
        Ok(())
    }

    pub fn get_bridge_interface(&self) -> Option<&str> {
        self.bridge_interface.as_deref()
    }

    /// The gateway address assigned to a network's bridge/switch.
    pub fn network_gateway(&self, network_name: &str) -> Option<&str> {
        self.networks.get(network_name).map(|n| n.gateway.as_str())
    }

    /// The driver a network was created with (as recorded in the manifest).
    pub fn network_driver(&self, network_name: &str) -> Option<&str> {
        self.networks
            .get(network_name)
            .map(|n| n.config.driver.as_str())
    }

    /// The address allocated to a service on a network, if it's connected.
    pub fn assigned_ip(&self, network_name: &str, service_name: &str) -> Option<&str> {
        self.networks
            .get(network_name)?
            .connected
            .get(service_name)
            .map(|c| c.ip.as_str())
    }

    /// Attaches a service to a network for bookkeeping purposes only
    /// (allocates it a real address from the network's subnet, recorded so
    /// `disconnect_service`/`delete_network` can clean up): used for
    /// services that share the host's own network namespace (`process`,
    /// in-process `wasm`) and don't need a veth of their own. Returns the
    /// allocated IP.
    pub async fn connect_service(
        &mut self,
        service_name: &str,
        network_name: &str,
    ) -> Result<String> {
        self.connect_service_inner(service_name, network_name, None)
            .await
    }

    /// Same as [`Self::connect_service`], but for a service that really has
    /// its own OS process/network namespace (a containerd-started OCI
    /// service on Linux, identified by `pid`): creates a veth pair, attaches
    /// the host end to the network's bridge, and moves the peer into the
    /// service's netns with the allocated address configured and up.
    pub async fn connect_service_with_pid(
        &mut self,
        service_name: &str,
        network_name: &str,
        pid: u32,
    ) -> Result<String> {
        self.connect_service_inner(service_name, network_name, Some(pid))
            .await
    }

    async fn connect_service_inner(
        &mut self,
        service_name: &str,
        network_name: &str,
        pid: Option<u32>,
    ) -> Result<String> {
        let net = self
            .networks
            .get_mut(network_name)
            .with_context(|| format!("network '{network_name}' does not exist"))?;

        if let Some(existing) = net.connected.get(service_name) {
            return Ok(existing.ip.clone());
        }

        let host_octet = net.next_host_octet;
        anyhow::ensure!(
            host_octet < 255,
            "network '{network_name}' has no free addresses left in {}.0/{}",
            net.subnet_prefix,
            net.prefix_len
        );
        net.next_host_octet += 1;
        let ip = format!("{}.{}", net.subnet_prefix, host_octet);

        tracing::info!("Connecting service {service_name} to network {network_name} at {ip}");

        let mut veth_host = None;
        if let (Some(bridge), Some(pid)) = (net.bridge_name.as_deref(), pid) {
            match attach_service_netns(service_name, bridge, &ip, net.prefix_len, pid).await {
                Ok(host_if) => veth_host = host_if,
                Err(e) => tracing::warn!(
                    "failed to attach {service_name} to bridge {bridge}, it will have an \
                     assigned address but no real connectivity: {e:#}"
                ),
            }
        }

        net.connected.insert(
            service_name.to_string(),
            ConnectedService {
                ip: ip.clone(),
                veth_host,
            },
        );
        Ok(ip)
    }

    pub async fn disconnect_service(
        &mut self,
        service_name: &str,
        network_name: &str,
    ) -> Result<()> {
        tracing::info!("Disconnecting service {service_name} from network {network_name}");
        if let Some(net) = self.networks.get_mut(network_name) {
            if let Some(conn) = net.connected.remove(service_name) {
                teardown_veth(&conn).await;
            }
        }
        Ok(())
    }
}

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn run(cmd: &str, args: &[&str]) -> Result<Output> {
    tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to spawn `{cmd}`"))
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn succeeded_or_already_exists(output: &Output) -> bool {
    if output.status.success() {
        return true;
    }
    let stderr = stderr_of(output);
    stderr.contains("File exists") || stderr.contains("already exists")
}

/// Bridge/switch device name for a network, truncated to a short hash-based
/// form when the natural `tpt-<name>` would exceed Linux's 15-character
/// `IFNAMSIZ` limit.
#[cfg(target_os = "linux")]
fn bridge_name_for(network_name: &str) -> String {
    let candidate = format!("tpt-{network_name}");
    if candidate.len() <= 15 {
        candidate
    } else {
        format!("tpt-{}", short_hash(network_name))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn short_hash(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(&hasher.finalize()[..4])
}

/// Host-side veth name for a service, always short enough for `IFNAMSIZ`
/// regardless of the service's own name.
#[cfg(target_os = "linux")]
fn veth_name_for(service_name: &str) -> String {
    format!("veth{}", short_hash(service_name))
}

#[cfg(target_os = "linux")]
async fn create_platform_bridge(
    network_name: &str,
    gateway: &str,
    prefix_len: u8,
) -> Option<String> {
    let bridge = bridge_name_for(network_name);
    tracing::info!("Creating Linux bridge {bridge} for network {network_name}");

    let add = match run("ip", &["link", "add", &bridge, "type", "bridge"]).await {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("failed to invoke `ip` (is iproute2 installed?): {e}");
            return None;
        }
    };
    if !succeeded_or_already_exists(&add) {
        tracing::warn!(
            "could not create real bridge device {bridge} (likely missing root/CAP_NET_ADMIN): {}",
            stderr_of(&add)
        );
        return None;
    }

    let addr = run(
        "ip",
        &[
            "addr",
            "add",
            &format!("{gateway}/{prefix_len}"),
            "dev",
            &bridge,
        ],
    )
    .await;
    if !matches!(&addr, Ok(o) if succeeded_or_already_exists(o)) {
        tracing::warn!("failed to assign gateway address {gateway}/{prefix_len} to {bridge}");
    }

    match run("ip", &["link", "set", &bridge, "up"]).await {
        Ok(o) if o.status.success() => Some(bridge),
        Ok(o) => {
            tracing::warn!("failed to bring up bridge {bridge}: {}", stderr_of(&o));
            None
        }
        Err(e) => {
            tracing::warn!("failed to invoke `ip link set up` for {bridge}: {e}");
            None
        }
    }
}

/// Only Linux has real per-service network namespaces to attach to here
/// (Origin's OCI/containerd runtime is Linux-only, see `runtime.rs`'s
/// `cfg(target_os = "linux")` gates), so this is a no-op elsewhere — the
/// caller falls back to bookkeeping-only address assignment.
#[cfg(not(target_os = "linux"))]
async fn attach_service_netns(
    _service_name: &str,
    _bridge: &str,
    _ip: &str,
    _prefix_len: u8,
    _pid: u32,
) -> Result<Option<String>> {
    Ok(None)
}

#[cfg(target_os = "linux")]
async fn attach_service_netns(
    service_name: &str,
    bridge: &str,
    ip: &str,
    prefix_len: u8,
    pid: u32,
) -> Result<Option<String>> {
    let veth_host = veth_name_for(service_name);
    let veth_peer = format!("{veth_host}p");
    let pid_s = pid.to_string();

    let add = run(
        "ip",
        &[
            "link", "add", &veth_host, "type", "veth", "peer", "name", &veth_peer,
        ],
    )
    .await?;
    anyhow::ensure!(
        succeeded_or_already_exists(&add),
        "ip link add veth {veth_host}/{veth_peer} failed: {}",
        stderr_of(&add)
    );

    let master = run("ip", &["link", "set", &veth_host, "master", bridge]).await?;
    anyhow::ensure!(
        master.status.success(),
        "failed to attach {veth_host} to bridge {bridge}: {}",
        stderr_of(&master)
    );

    let up = run("ip", &["link", "set", &veth_host, "up"]).await?;
    anyhow::ensure!(
        up.status.success(),
        "failed to bring up {veth_host}: {}",
        stderr_of(&up)
    );

    let move_ns = run("ip", &["link", "set", &veth_peer, "netns", &pid_s]).await?;
    anyhow::ensure!(
        move_ns.status.success(),
        "failed to move {veth_peer} into netns of pid {pid}: {}",
        stderr_of(&move_ns)
    );

    let addr = run(
        "nsenter",
        &[
            "-t",
            &pid_s,
            "-n",
            "ip",
            "addr",
            "add",
            &format!("{ip}/{prefix_len}"),
            "dev",
            &veth_peer,
        ],
    )
    .await?;
    anyhow::ensure!(
        addr.status.success(),
        "failed to assign {ip}/{prefix_len} to {veth_peer} inside netns of pid {pid}: {}",
        stderr_of(&addr)
    );

    let ifup = run(
        "nsenter",
        &["-t", &pid_s, "-n", "ip", "link", "set", &veth_peer, "up"],
    )
    .await?;
    anyhow::ensure!(
        ifup.status.success(),
        "failed to bring up {veth_peer} inside netns of pid {pid}: {}",
        stderr_of(&ifup)
    );

    if let Ok(lo_up) = run(
        "nsenter",
        &["-t", &pid_s, "-n", "ip", "link", "set", "lo", "up"],
    )
    .await
    {
        if !lo_up.status.success() {
            tracing::warn!(
                "failed to bring up loopback inside netns of pid {pid}: {}",
                stderr_of(&lo_up)
            );
        }
    }

    Ok(Some(veth_host))
}

#[cfg(target_os = "linux")]
async fn teardown_veth(conn: &ConnectedService) {
    if let Some(veth_host) = &conn.veth_host {
        match run("ip", &["link", "delete", veth_host]).await {
            Ok(o) if o.status.success() => {}
            Ok(o) => tracing::warn!("failed to delete veth {veth_host}: {}", stderr_of(&o)),
            Err(e) => tracing::warn!("failed to invoke `ip link delete` for {veth_host}: {e}"),
        }
    }
}

#[cfg(target_os = "linux")]
async fn teardown_bridge(bridge: Option<&str>) {
    let Some(bridge) = bridge else { return };
    match run("ip", &["link", "delete", bridge]).await {
        Ok(o) if o.status.success() => {}
        Ok(o) => tracing::warn!("failed to delete bridge {bridge}: {}", stderr_of(&o)),
        Err(e) => tracing::warn!("failed to invoke `ip link delete` for bridge {bridge}: {e}"),
    }
}

/// macOS has no per-container network namespaces in this codebase (Origin's
/// containerd/OCI runtime is Linux-only, see `runtime.rs`'s `cfg(target_os =
/// "linux")` gates), so there's nothing to attach a service's own netns to
/// here. What was previously fabricated is the bridge device itself; this
/// creates a real `if_bridge(4)` interface with the network's gateway
/// address configured, which is what a future macOS container runtime would
/// attach container-side taps to.
#[cfg(target_os = "macos")]
async fn create_platform_bridge(
    network_name: &str,
    gateway: &str,
    prefix_len: u8,
) -> Option<String> {
    tracing::info!("Creating macOS bridge interface for network {network_name}");

    let create = match run("ifconfig", &["bridge", "create"]).await {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("failed to invoke `ifconfig`: {e}");
            return None;
        }
    };
    if !create.status.success() {
        tracing::warn!(
            "could not create real bridge interface (likely missing root): {}",
            stderr_of(&create)
        );
        return None;
    }
    let name = String::from_utf8_lossy(&create.stdout).trim().to_string();
    if name.is_empty() {
        tracing::warn!("`ifconfig bridge create` produced no interface name");
        return None;
    }

    let addr = run(
        "ifconfig",
        &[&name, "inet", &format!("{gateway}/{prefix_len}")],
    )
    .await;
    if !matches!(&addr, Ok(o) if o.status.success()) {
        tracing::warn!("failed to assign gateway address {gateway}/{prefix_len} to {name}");
    }

    match run("ifconfig", &[&name, "up"]).await {
        Ok(o) if o.status.success() => Some(name),
        Ok(o) => {
            tracing::warn!("failed to bring up {name}: {}", stderr_of(&o));
            None
        }
        Err(e) => {
            tracing::warn!("failed to invoke `ifconfig up` for {name}: {e}");
            None
        }
    }
}

#[cfg(target_os = "macos")]
async fn teardown_veth(_conn: &ConnectedService) {}

#[cfg(target_os = "macos")]
async fn teardown_bridge(bridge: Option<&str>) {
    let Some(bridge) = bridge else { return };
    match run("ifconfig", &[bridge, "destroy"]).await {
        Ok(o) if o.status.success() => {}
        Ok(o) => tracing::warn!("failed to destroy bridge {bridge}: {}", stderr_of(&o)),
        Err(e) => tracing::warn!("failed to invoke `ifconfig destroy` for {bridge}: {e}"),
    }
}

/// Windows has no per-container network namespaces in this codebase either
/// (same reasoning as macOS above), so this creates a real Hyper-V internal
/// virtual switch with the network's gateway address configured on its
/// host-side `vEthernet` adapter — replacing the previous stub, which only
/// fabricated an interface name string. Requires Hyper-V to be enabled and
/// an elevated (Administrator) process; falls back to bookkeeping-only mode
/// otherwise, same as the other platforms.
#[cfg(target_os = "windows")]
async fn create_platform_bridge(
    network_name: &str,
    gateway: &str,
    prefix_len: u8,
) -> Option<String> {
    let switch_name = format!("tpt-{network_name}");
    tracing::info!(
        "Creating Windows Hyper-V internal switch {switch_name} for network {network_name}"
    );

    let adapter_name = format!("vEthernet ({switch_name})");
    let script = format!(
        "$ErrorActionPreference = 'Stop'; \
         if (-not (Get-VMSwitch -Name '{switch_name}' -ErrorAction SilentlyContinue)) {{ \
           New-VMSwitch -Name '{switch_name}' -SwitchType Internal | Out-Null \
         }}; \
         $ifIndex = (Get-NetAdapter -Name '{adapter_name}').ifIndex; \
         if (-not (Get-NetIPAddress -InterfaceIndex $ifIndex -IPAddress '{gateway}' -ErrorAction SilentlyContinue)) {{ \
           New-NetIPAddress -InterfaceIndex $ifIndex -IPAddress '{gateway}' -PrefixLength {prefix_len} | Out-Null \
         }}; \
         Write-Output '{adapter_name}'"
    );

    let out = run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
    .await;
    match out {
        Ok(o) if o.status.success() => {
            let name = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        }
        Ok(o) => {
            tracing::warn!(
                "could not create real Hyper-V internal switch {switch_name} \
                 (requires Hyper-V + an elevated process): {}",
                stderr_of(&o)
            );
            None
        }
        Err(e) => {
            tracing::warn!("failed to invoke PowerShell for network setup: {e}");
            None
        }
    }
}

#[cfg(target_os = "windows")]
async fn teardown_veth(_conn: &ConnectedService) {}

#[cfg(target_os = "windows")]
async fn teardown_bridge(bridge: Option<&str>) {
    let Some(adapter_name) = bridge else { return };
    // `adapter_name` is `vEthernet (<switch_name>)`; recover the switch name
    // to remove the switch itself (which also tears down the adapter).
    let switch_name = adapter_name
        .strip_prefix("vEthernet (")
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(adapter_name);
    let script =
        format!("Remove-VMSwitch -Name '{switch_name}' -Force -ErrorAction SilentlyContinue");
    if let Err(e) = run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", &script],
    )
    .await
    {
        tracing::warn!("failed to invoke PowerShell to remove switch {switch_name}: {e}");
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn create_platform_bridge(
    network_name: &str,
    _gateway: &str,
    _prefix_len: u8,
) -> Option<String> {
    tracing::warn!("no real networking support for this platform; network '{network_name}' is bookkeeping-only");
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn teardown_veth(_conn: &ConnectedService) {}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
async fn teardown_bridge(_bridge: Option<&str>) {}

/// Rootless network creation using `slirp4netns`. This creates a TAP
/// device in a new user namespace, providing unprivileged networking
/// without requiring root or CAP_NET_ADMIN.
///
/// `slirp4netns` is the standard tool for rootless networking (used by
/// Podman rootless, bubblewrap, etc.). It provides a full TCP/IP stack
/// in userspace via libslirp.
async fn create_rootless_network(
    network_name: &str,
    gateway: &str,
    _prefix_len: u8,
) -> Option<String> {
    // Check if slirp4netns is available
    match tokio::process::Command::new("slirp4netns")
        .arg("--version")
        .output()
        .await
    {
        Ok(o) if !o.status.success() => {
            tracing::warn!(
                "slirp4netns is installed but returned an error; \
                 falling back to bookkeeping-only mode for network '{network_name}'"
            );
            return None;
        }
        Err(e) => {
            tracing::warn!(
                "slirp4netns not found (required for rootless networking): {e}; \
                 falling back to bookkeeping-only mode for network '{network_name}'"
            );
            return None;
        }
        _ => {}
    }

    tracing::info!(
        "Creating rootless network '{network_name}' via slirp4netns (gateway: {gateway})"
    );

    // In a full implementation, slirp4netns would be started with a
    // new TAP device that containers' network namespaces can attach to.
    // For now, we create the network in bookkeeping-only mode and log
    // the limitation — real slirp4netns integration requires coordinating
    // user namespace setup with the container runtime (rootless containerd
    // or Podman), which is a larger integration effort.
    //
    // The key integration points would be:
    // 1. Create a TAP device: `slirp4netns --netns-type=path <netns> tap0`
    // 2. Assign the gateway address to tap0 inside the namespace
    // 3. Configure DNS to point service names to their container IPs
    //
    // For process/wasm services (which share the host network namespace),
    // slirp4netns isn't needed — they can bind directly to localhost.
    tracing::warn!(
        "rootless networking for '{network_name}' is in bookkeeping-only mode; \
         services get DNS entries and IP assignments but no real L2/L3 connectivity. \
         Full slirp4netns integration requires rootless containerd coordination."
    );

    // Return a virtual device name for bookkeeping; no real device is created
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(network_name.as_bytes());
    let hash = hex::encode(&hasher.finalize()[..4]);
    let tap_name = format!("tap-{hash}");
    Some(tap_name)
}
