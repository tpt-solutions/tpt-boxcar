use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Request;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::sync::{watch, RwLock};
use tracing::{error, info, warn};

use crate::loadbalancer::BackendEndpoint;
use crate::router::{RouteRule, Router};
use crate::tls::TlsAcceptor;
use std::collections::HashMap;
use std::net::SocketAddr as StdSocketAddr;

pub struct ListenerConfig {
    pub addr: SocketAddr,
    pub tls_config: Option<TlsAcceptor>,
}

pub struct ProxyService {
    pub router: Arc<RwLock<Router>>,
}

impl ProxyService {
    pub fn new(router: Arc<RwLock<Router>>) -> Self {
        Self { router }
    }

    pub async fn serve(
        &self,
        config: ListenerConfig,
        mut shutdown: watch::Receiver<bool>,
    ) -> Result<()> {
        let listener = TcpListener::bind(config.addr)
            .await
            .context("failed to bind listener")?;
        info!("listening on {}", config.addr);

        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, remote_addr)) => {
                            let router = self.router.clone();
                            let tls = config.tls_config.clone();
                            tokio::spawn(async move {
                                if let Some(tls) = tls {
                                    match tls.accept(stream).await {
                                        Ok(tls_stream) => {
                                            let io = TokioIo::new(tls_stream);
                                            if let Err(e) = serve_connection(io, router).await {
                                                warn!("TLS connection error from {}: {}", remote_addr, e);
                                            }
                                        }
                                        Err(e) => {
                                            warn!("TLS handshake failed from {}: {}", remote_addr, e);
                                        }
                                    }
                                } else {
                                    let io = TokioIo::new(stream);
                                    if let Err(e) = serve_connection(io, router).await {
                                        warn!("connection error from {}: {}", remote_addr, e);
                                    }
                                }
                            });
                        }
                        Err(e) => {
                            warn!("accept error: {}", e);
                        }
                    }
                }
                _ = shutdown.changed() => {
                    info!("shutdown signal received");
                    break;
                }
            }
        }
        Ok(())
    }
}

async fn serve_connection<I>(io: TokioIo<I>, router: Arc<RwLock<Router>>) -> Result<()>
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let service = service_fn(move |req: Request<hyper::body::Incoming>| {
        let router = router.clone();
        async move {
            let r = router.read().await;
            r.route(req).await
        }
    });

    http1::Builder::new()
        .serve_connection(io, service)
        .with_upgrades()
        .await
        .context("HTTP/1.1 connection error")?;

    Ok(())
}

pub async fn start_listeners(
    configs: Vec<ListenerConfig>,
    router: Arc<RwLock<Router>>,
    shutdown: watch::Receiver<bool>,
) -> Result<()> {
    let mut handles = Vec::new();

    for config in configs {
        let proxy = ProxyService::new(router.clone());
        let shutdown_rx = shutdown.clone();
        let handle = tokio::spawn(async move { proxy.serve(config, shutdown_rx).await });
        handles.push(handle);
    }

    for handle in handles {
        handle.await??;
    }

    Ok(())
}

// --- Config File Watcher ---

/// ConfigSnapshot mirrors the JSON written by the control-plane config pusher.
#[derive(Debug, serde::Deserialize)]
struct ConfigSnapshot {
    #[allow(dead_code)]
    version: i64,
    routes: Option<HashMap<String, RouteSnapshot>>,
    upstreams: Option<HashMap<String, UpstreamSnapshot>>,
}

#[derive(Debug, serde::Deserialize)]
struct RouteSnapshot {
    #[serde(default)]
    prefix: Option<String>,
    cluster: String,
    #[serde(default)]
    headers: Option<HashMap<String, String>>,
    #[serde(default)]
    priority: u32,
    #[serde(default)]
    strip_prefix: bool,
}

#[derive(Debug, serde::Deserialize)]
struct UpstreamSnapshot {
    name: String,
    #[serde(default)]
    endpoints: Vec<String>,
}

/// ConfigWatcher polls a JSON config file and hot-reloads the Router on version change.
pub struct ConfigWatcher {
    path: PathBuf,
    router: Arc<RwLock<Router>>,
    poll_interval_ms: u64,
    last_mtime: std::sync::Mutex<Option<std::time::SystemTime>>,
}

impl ConfigWatcher {
    pub fn new(
        path: impl Into<PathBuf>,
        router: Arc<RwLock<Router>>,
        poll_interval_ms: u64,
    ) -> Self {
        Self {
            path: path.into(),
            router,
            poll_interval_ms,
            last_mtime: std::sync::Mutex::new(None),
        }
    }

    /// Start the watcher loop. Runs until the shutdown signal fires.
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) {
        info!(
            "config watcher started: path={:?}, poll={}ms",
            self.path, self.poll_interval_ms
        );

        // Load initial config if file exists
        if self.path.exists() {
            if let Err(e) = self.reload().await {
                warn!("initial config load failed: {}", e);
            }
        }

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(self.poll_interval_ms)) => {
                    if self.path.exists() {
                        if let Err(e) = self.reload().await {
                            error!("config reload failed: {}", e);
                        }
                    }
                }
                _ = shutdown.changed() => {
                    info!("config watcher stopped");
                    return;
                }
            }
        }
    }

    /// Read the config file and rebuild the router, skipping if mtime unchanged.
    async fn reload(&self) -> Result<()> {
        let metadata = tokio::fs::metadata(&self.path)
            .await
            .with_context(|| format!("stat config file {:?}", self.path))?;
        let mtime = metadata.modified().context("failed to read file mtime")?;

        {
            let mut last = self.last_mtime.lock().unwrap();
            if *last == Some(mtime) {
                return Ok(());
            }
            *last = Some(mtime);
        }

        let data = tokio::fs::read(&self.path)
            .await
            .with_context(|| format!("read config file {:?}", self.path))?;

        let snapshot: ConfigSnapshot =
            serde_json::from_slice(&data).context("parse config JSON")?;

        let new_router = self.build_router(snapshot)?;

        {
            let mut w = self.router.write().await;
            *w = new_router;
        }

        info!("config reloaded successfully");
        Ok(())
    }

    fn build_router(&self, snap: ConfigSnapshot) -> Result<Router> {
        let mut router = Router::new();

        // Build upstreams (load balancers) from snapshot
        let upstreams = snap.upstreams.unwrap_or_default();
        for us in upstreams.values() {
            let endpoints: Vec<BackendEndpoint> = us
                .endpoints
                .iter()
                .filter_map(|ep| {
                    let addr: StdSocketAddr = ep.parse().ok()?;
                    Some(BackendEndpoint {
                        addr,
                        weight: 1,
                        tags: HashMap::new(),
                    })
                })
                .collect();

            let lb = Arc::new(crate::loadbalancer::RoundRobinBalancer::new(endpoints));
            router.register_upstream(us.name.clone(), lb);
        }

        // Build route rules from snapshot
        let routes = snap.routes.unwrap_or_default();
        for rs in routes.values() {
            let rule = RouteRule {
                path_prefix: rs.prefix.clone(),
                headers: rs.headers.clone().unwrap_or_default(),
                upstream: rs.cluster.clone(),
                priority: rs.priority,
                strip_prefix: rs.strip_prefix,
            };
            router.add_rule(rule);
        }

        Ok(router)
    }
}
