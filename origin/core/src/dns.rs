use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct DnsEntry {
    pub name: String,
    pub ip: String,
    pub port: Option<u16>,
}

pub struct DnsResolver {
    entries: HashMap<String, DnsEntry>,
    listen_addr: String,
}

impl DnsResolver {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            listen_addr: "127.0.0.1:5353".to_string(),
        }
    }

    pub fn add_entry(&mut self, name: &str, ip: &str, port: Option<u16>) {
        let fqdn = if name.ends_with(".local") {
            name.to_string()
        } else {
            format!("{name}.local")
        };

        tracing::info!("DNS: {fqdn} -> {ip}");
        self.entries.insert(
            fqdn,
            DnsEntry {
                name: name.to_string(),
                ip: ip.to_string(),
                port,
            },
        );
    }

    pub fn remove_entry(&mut self, name: &str) {
        let fqdn = if name.ends_with(".local") {
            name.to_string()
        } else {
            format!("{name}.local")
        };
        self.entries.remove(&fqdn);
    }

    pub fn resolve(&self, name: &str) -> Option<&DnsEntry> {
        let fqdn = if name.ends_with(".local") {
            name.to_string()
        } else {
            format!("{name}.local")
        };
        self.entries.get(&fqdn)
    }

    pub async fn start(&self) -> Result<()> {
        tracing::info!(
            "DNS resolver listening on {} with {} entries",
            self.listen_addr,
            self.entries.len()
        );
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        tracing::info!("DNS resolver stopped");
        Ok(())
    }

    pub fn entries(&self) -> &HashMap<String, DnsEntry> {
        &self.entries
    }
}

impl Default for DnsResolver {
    fn default() -> Self {
        Self::new()
    }
}
