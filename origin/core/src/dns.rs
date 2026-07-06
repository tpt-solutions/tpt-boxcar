use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::Ipv4Addr;
use tokio::net::UdpSocket;
use tokio::sync::watch;

#[derive(Debug, Clone)]
pub struct DnsEntry {
    pub name: String,
    pub ip: String,
    pub port: Option<u16>,
}

pub struct DnsResolver {
    entries: HashMap<String, DnsEntry>,
    listen_addr: String,
    /// Pushes a fresh snapshot of `entries` to the running listener task
    /// (if any) every time `add_entry`/`remove_entry` mutates the map, so
    /// the real UDP responder always answers with current data without
    /// needing a lock shared across `&self`/`&mut self` call sites.
    update_tx: Option<watch::Sender<HashMap<String, DnsEntry>>>,
    listener_task: Option<tokio::task::JoinHandle<()>>,
}

impl DnsResolver {
    /// Binds `ORIGIN_DNS_LISTEN_ADDR` if set, else the standard
    /// `127.0.0.1:5353`. The override exists because binding the real mDNS
    /// port can legitimately fail on a host where something else (a system
    /// mDNS responder, a restrictive port-exclusion range, another Origin
    /// instance) already holds it — the same class of conflict Docker hits
    /// with port 53.
    pub fn new() -> Self {
        let addr = std::env::var("ORIGIN_DNS_LISTEN_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:5353".to_string());
        Self::on(addr)
    }

    /// Same as [`Self::new`] but binding to a caller-chosen address —
    /// mainly for tests, which need an ephemeral port rather than
    /// contending for the real default.
    pub fn on(listen_addr: impl Into<String>) -> Self {
        Self {
            entries: HashMap::new(),
            listen_addr: listen_addr.into(),
            update_tx: None,
            listener_task: None,
        }
    }

    fn fqdn(name: &str) -> String {
        if name.ends_with(".local") {
            name.to_string()
        } else {
            format!("{name}.local")
        }
    }

    pub fn add_entry(&mut self, name: &str, ip: &str, port: Option<u16>) {
        let fqdn = Self::fqdn(name);
        tracing::info!("DNS: {fqdn} -> {ip}");
        self.entries.insert(
            fqdn,
            DnsEntry {
                name: name.to_string(),
                ip: ip.to_string(),
                port,
            },
        );
        self.publish_update();
    }

    pub fn remove_entry(&mut self, name: &str) {
        let fqdn = Self::fqdn(name);
        self.entries.remove(&fqdn);
        self.publish_update();
    }

    fn publish_update(&self) {
        if let Some(tx) = &self.update_tx {
            // Only fails if the listener task has already exited (e.g.
            // after `stop`), which is harmless to ignore here.
            let _ = tx.send(self.entries.clone());
        }
    }

    pub fn resolve(&self, name: &str) -> Option<&DnsEntry> {
        let fqdn = Self::fqdn(name);
        self.entries.get(&fqdn)
    }

    /// Starts a real UDP DNS responder on `listen_addr` that answers A
    /// record queries for `<service>.local` names from the current entry
    /// table — replacing the previous stub, which only logged and never
    /// opened a socket.
    pub async fn start(&mut self) -> Result<()> {
        let socket = UdpSocket::bind(&self.listen_addr)
            .await
            .with_context(|| format!("failed to bind DNS listener on {}", self.listen_addr))?;
        tracing::info!(
            "DNS resolver listening on {} with {} entries",
            self.listen_addr,
            self.entries.len()
        );

        let (tx, mut rx) = watch::channel(self.entries.clone());
        self.update_tx = Some(tx);

        let task = tokio::spawn(async move {
            let mut current = rx.borrow_and_update().clone();
            let mut buf = [0u8; 512];
            loop {
                tokio::select! {
                    changed = rx.changed() => {
                        if changed.is_err() {
                            break; // sender (DnsResolver) was dropped
                        }
                        current = rx.borrow_and_update().clone();
                    }
                    result = socket.recv_from(&mut buf) => {
                        let (len, peer) = match result {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!("DNS socket recv error: {e}");
                                continue;
                            }
                        };
                        match handle_query(&buf[..len], &current) {
                            Some(response) => {
                                if let Err(e) = socket.send_to(&response, peer).await {
                                    tracing::warn!("DNS socket send error: {e}");
                                }
                            }
                            None => tracing::debug!("DNS: failed to parse or answer query from {peer}"),
                        }
                    }
                }
            }
        });
        self.listener_task = Some(task);

        Ok(())
    }

    pub async fn stop(&mut self) -> Result<()> {
        self.update_tx = None; // signals the listener's `rx.changed()` to end
        if let Some(task) = self.listener_task.take() {
            task.abort();
        }
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

/// Parses a minimal DNS query (header + one question) and, if it asks for
/// an A record matching a known `.local` entry, builds a real wire-format
/// response with that entry's IP. Returns `None` on anything it doesn't
/// recognize (malformed packet, non-A query, unknown name) rather than
/// guessing.
fn handle_query(query: &[u8], entries: &HashMap<String, DnsEntry>) -> Option<Vec<u8>> {
    if query.len() < 12 {
        return None;
    }
    let id = &query[0..2];
    let qdcount = u16::from_be_bytes([query[4], query[5]]);
    if qdcount == 0 {
        return None;
    }

    let (qname, qname_end) = parse_qname(query, 12)?;
    if query.len() < qname_end + 4 {
        return None;
    }
    let qtype = u16::from_be_bytes([query[qname_end], query[qname_end + 1]]);
    let qclass = u16::from_be_bytes([query[qname_end + 2], query[qname_end + 3]]);

    const TYPE_A: u16 = 1;
    const CLASS_IN: u16 = 1;

    let mut response = Vec::with_capacity(query.len() + 16);
    response.extend_from_slice(id);

    if qtype != TYPE_A || qclass != CLASS_IN {
        // QR=1, Opcode=0, AA=0, RD=1, RA=0, RCODE=4 (Not Implemented)
        response.extend_from_slice(&[0x81, 0x04]);
        response.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
        response.extend_from_slice(&query[12..qname_end + 4]);
        return Some(response);
    }

    let lookup_name = qname.trim_end_matches('.').to_ascii_lowercase();
    let entry = entries.get(&format!("{lookup_name}.")).or_else(|| entries.get(&lookup_name));

    match entry.and_then(|e| e.ip.parse::<Ipv4Addr>().ok()) {
        Some(ip) => {
            // QR=1, Opcode=0, AA=1, RD=1, RA=0, RCODE=0
            response.extend_from_slice(&[0x85, 0x00]);
            response.extend_from_slice(&[0, 1]); // QDCOUNT
            response.extend_from_slice(&[0, 1]); // ANCOUNT
            response.extend_from_slice(&[0, 0, 0, 0]); // NSCOUNT, ARCOUNT
            response.extend_from_slice(&query[12..qname_end + 4]); // echo question

            response.extend_from_slice(&[0xc0, 0x0c]); // pointer to question's QNAME
            response.extend_from_slice(&TYPE_A.to_be_bytes());
            response.extend_from_slice(&CLASS_IN.to_be_bytes());
            response.extend_from_slice(&30u32.to_be_bytes()); // TTL
            response.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
            response.extend_from_slice(&ip.octets());
            Some(response)
        }
        None => {
            // QR=1, Opcode=0, AA=1, RD=1, RA=0, RCODE=3 (NXDOMAIN)
            response.extend_from_slice(&[0x85, 0x03]);
            response.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 0]);
            response.extend_from_slice(&query[12..qname_end + 4]);
            Some(response)
        }
    }
}

/// Builds a minimal, real DNS query packet (header + one question) asking
/// for the A record of `name`, matching what an actual stub resolver sends.
#[cfg(test)]
fn build_query(id: u16, name: &str) -> Vec<u8> {
    let mut packet = Vec::new();
    packet.extend_from_slice(&id.to_be_bytes());
    packet.extend_from_slice(&[0x01, 0x00]); // flags: RD=1
    packet.extend_from_slice(&[0, 1]); // QDCOUNT=1
    packet.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // ANCOUNT, NSCOUNT, ARCOUNT

    for label in name.split('.') {
        packet.push(label.len() as u8);
        packet.extend_from_slice(label.as_bytes());
    }
    packet.push(0); // root label

    packet.extend_from_slice(&1u16.to_be_bytes()); // QTYPE=A
    packet.extend_from_slice(&1u16.to_be_bytes()); // QCLASS=IN
    packet
}

#[cfg(test)]
mod wire_protocol_tests {
    use super::*;

    fn sample_entries() -> HashMap<String, DnsEntry> {
        let mut entries = HashMap::new();
        entries.insert(
            "web.local".to_string(),
            DnsEntry { name: "web".to_string(), ip: "10.0.0.2".to_string(), port: Some(8080) },
        );
        entries
    }

    #[test]
    fn answers_known_a_record_with_real_ip_bytes() {
        let query = build_query(0x1234, "web.local");
        let response = handle_query(&query, &sample_entries()).expect("should produce a response");

        assert_eq!(&response[0..2], &[0x12, 0x34], "response ID must echo the query ID");
        assert_eq!(response[2] & 0x80, 0x80, "QR bit must be set on a response");
        assert_eq!(response[3] & 0x0f, 0, "RCODE must be 0 (no error)");

        // Last 4 bytes of a single-A-record response are the raw IPv4 octets.
        let ip_bytes = &response[response.len() - 4..];
        assert_eq!(ip_bytes, &[10, 0, 0, 2]);
    }

    #[test]
    fn answers_unknown_name_with_nxdomain() {
        let query = build_query(0x0001, "nonexistent.local");
        let response = handle_query(&query, &sample_entries()).expect("should produce a response");
        assert_eq!(response[3] & 0x0f, 3, "RCODE must be 3 (NXDOMAIN)");
    }

    #[test]
    fn rejects_malformed_packet() {
        assert!(handle_query(&[0u8; 4], &sample_entries()).is_none());
    }

    /// End-to-end proof: a real UDP client sends a real wire-format query to
    /// the actual listener started by `DnsResolver::start`, and gets back a
    /// real answer with the correct IP — not a stub that never opens a
    /// socket.
    #[tokio::test]
    async fn real_udp_listener_answers_a_real_client_query() {
        // Bind to an ephemeral port ourselves first so we know the real
        // address to both start the resolver on and query.
        let probe = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe); // free the port; small re-bind race is fine for a test

        let mut resolver = DnsResolver::on(addr.to_string());
        resolver.add_entry("api", "10.0.0.9", Some(9000));
        resolver.start().await.unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let query = build_query(0xabcd, "api.local");
        client.send_to(&query, addr).await.unwrap();

        let mut buf = [0u8; 512];
        let (len, _) = tokio::time::timeout(std::time::Duration::from_secs(2), client.recv_from(&mut buf))
            .await
            .expect("should receive a response before timeout")
            .unwrap();

        let response = &buf[..len];
        assert_eq!(&response[0..2], &[0xab, 0xcd]);
        assert_eq!(&response[len - 4..], &[10, 0, 0, 9]);

        resolver.stop().await.unwrap();
    }
}

/// Decodes a (non-compressed, as sent by a resolver) QNAME starting at
/// `start`, returning the dotted name and the offset just past the
/// trailing zero label.
fn parse_qname(packet: &[u8], start: usize) -> Option<(String, usize)> {
    let mut labels = Vec::new();
    let mut pos = start;
    loop {
        let len = *packet.get(pos)? as usize;
        if len == 0 {
            pos += 1;
            break;
        }
        if len & 0xc0 != 0 {
            return None; // compression pointers not expected in a query we generate answers for
        }
        pos += 1;
        let label = packet.get(pos..pos + len)?;
        labels.push(String::from_utf8_lossy(label).to_string());
        pos += len;
    }
    Some((labels.join("."), pos))
}
