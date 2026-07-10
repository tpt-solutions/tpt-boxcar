use anyhow::{Context, Result};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};

use crate::manifest::PortMapping;

/// A single host→container port mapping proxy: listens on `host:port` and
/// forwards every connection to `container_ip:container_port`. Holds two
/// listener tasks — IPv4 (`0.0.0.0`) and, best-effort, IPv6 (`[::]`) — so a
/// published port is reachable over either family, matching how Docker's
/// userland proxy binds dual-stack.
struct PortProxy {
    _handle: tokio::task::JoinHandle<()>,
    _handle6: Option<tokio::task::JoinHandle<()>>,
}

/// Manages TCP proxies for all port mappings across every service in the
/// environment. Each mapping is identified by `(service_name, host_port)`.
pub struct PortMapper {
    proxies: HashMap<(String, u16), PortProxy>,
}

impl PortMapper {
    pub fn new() -> Self {
        Self {
            proxies: HashMap::new(),
        }
    }

    /// Starts a TCP proxy for every `PortMapping` of the named service,
    /// forwarding `host:port` → `container_ip:container_port`.
    ///
    /// Already-running mappings for the same `(service, host_port)` are
    /// stopped first so a restart can re-bind without port conflicts.
    pub async fn apply(
        &mut self,
        service_name: &str,
        mappings: &[PortMapping],
        container_ip: &str,
    ) -> Result<()> {
        for mapping in mappings {
            let host_port = mapping.host;
            let key = (service_name.to_string(), host_port);
            self.stop_proxy(&key);

            let host_addr = format!("0.0.0.0:{host_port}");
            let target_addr = format!("{}:{}", container_ip, mapping.container);

            let listener = match TcpListener::bind(&host_addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!("port mapping {host_port} → {target_addr} failed to bind: {e}");
                    continue;
                }
            };

            tracing::info!(
                "port mapping: {service_name} {}:{host_port} → {target_addr}",
                mapping.protocol,
            );
            let handle =
                spawn_accept_loop(listener, host_addr, service_name.to_string(), host_port, target_addr.clone());

            // IPv6 is best-effort: on hosts without IPv6, or where
            // `net.ipv6.bindv6only` already makes the v4 bind above cover
            // v6-mapped traffic, this simply fails to bind and is skipped —
            // the v4 mapping above still works either way.
            let host_addr6 = format!("[::]:{host_port}");
            let handle6 = match TcpListener::bind(&host_addr6).await {
                Ok(l) => Some(spawn_accept_loop(
                    l,
                    host_addr6,
                    service_name.to_string(),
                    host_port,
                    target_addr,
                )),
                Err(e) => {
                    tracing::debug!("port mapping {host_port} IPv6 bind skipped: {e}");
                    None
                }
            };

            self.proxies.insert(
                key,
                PortProxy {
                    _handle: handle,
                    _handle6: handle6,
                },
            );
        }
        Ok(())
    }

    /// Stops all proxies for the named service.
    pub fn stop_service(&mut self, service_name: &str) {
        let keys: Vec<_> = self
            .proxies
            .keys()
            .filter(|(svc, _)| svc == service_name)
            .cloned()
            .collect();
        for key in &keys {
            self.stop_proxy(key);
        }
    }

    /// Stops all proxies for every service.
    pub fn stop_all(&mut self) {
        self.proxies.clear();
    }

    fn stop_proxy(&mut self, key: &(String, u16)) {
        if let Some(proxy) = self.proxies.remove(key) {
            proxy._handle.abort();
        }
    }
}

impl Default for PortMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawns the accept→forward loop for a single bound listener (either the
/// IPv4 or IPv6 side of a port mapping).
fn spawn_accept_loop(
    listener: TcpListener,
    host_addr: String,
    svc: String,
    host_port: u16,
    target_addr: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((client_stream, peer)) => {
                    let target = target_addr.clone();
                    let svc = svc.clone();
                    tokio::spawn(async move {
                        if let Err(e) = proxy_connection(client_stream, &target).await {
                            tracing::trace!(
                                "proxy {svc}:{host_port}→{target} connection from {peer} ended: {e}",
                            );
                        }
                    });
                }
                Err(e) => {
                    tracing::warn!("accept error on {host_addr}: {e}");
                    break;
                }
            }
        }
    })
}

/// Forwards bytes bidirectionally between `client` and `target_addr`.
async fn proxy_connection(mut client: TcpStream, target_addr: &str) -> Result<()> {
    let mut upstream = TcpStream::connect(target_addr)
        .await
        .with_context(|| format!("connect to {target_addr} failed"))?;

    tokio::io::copy_bidirectional(&mut client, &mut upstream)
        .await
        .ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn proxy_forwards_bytes() {
        // Start a fake "upstream" server that echoes back what it receives.
        let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_addr = upstream.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if let Ok((mut stream, _)) = upstream.accept().await {
                    tokio::spawn(async move {
                        let mut buf = [0u8; 1024];
                        loop {
                            let n = match stream.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    });
                }
            }
        });

        // Start a port proxy pointing at the fake upstream.
        let proxy_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        drop(proxy_listener); // free the port for the proxy

        let mut mapper = PortMapper::new();
        let mapping = PortMapping {
            host: proxy_port,
            container: upstream_addr.port(),
            protocol: "tcp".to_string(),
        };
        mapper
            .apply("test-svc", &[mapping], "127.0.0.1")
            .await
            .unwrap();

        // Connect to the proxy and send data; expect it echoed back.
        let mut client = TcpStream::connect(format!("127.0.0.1:{proxy_port}"))
            .await
            .unwrap();
        client.write_all(b"hello").await.unwrap();
        client.shutdown().await.unwrap();

        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        assert_eq!(response, b"hello");

        mapper.stop_all();
    }
}
