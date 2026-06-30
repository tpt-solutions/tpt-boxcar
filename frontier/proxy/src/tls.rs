use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use tokio_rustls::TlsAcceptor as RustlsTlsAcceptor;
use tracing::{debug, info};

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
    pub ca_path: Option<PathBuf>,
    pub sni_certs: HashMap<String, TlsCertPair>,
}

#[derive(Debug, Clone)]
pub struct TlsCertPair {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

#[derive(Clone)]
pub struct TlsAcceptor {
    inner: RustlsTlsAcceptor,
    cert_paths: Arc<RwLock<HashMap<String, TlsCertPair>>>,
    default_cert: TlsCertPair,
}

impl TlsAcceptor {
    pub fn new(config: TlsConfig) -> Result<Self> {
        let (certs, key) = load_cert_and_key(&config.cert_path, &config.key_path)?;

        let mut server_config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .context("failed to create TLS server config")?;

        server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        let server_config = Arc::new(server_config);
        let inner = RustlsTlsAcceptor::from(server_config);

        info!("TLS configured with cert: {}", config.cert_path.display());

        Ok(Self {
            inner,
            cert_paths: Arc::new(RwLock::new(config.sni_certs)),
            default_cert: TlsCertPair {
                cert_path: config.cert_path,
                key_path: config.key_path,
            },
        })
    }

    pub async fn accept<IO>(&self, stream: IO) -> Result<tokio_rustls::server::TlsStream<IO>>
    where
        IO: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        self.inner.accept(stream).await.context("TLS handshake failed")
    }

    pub fn reload_certs(&self) -> Result<()> {
        let _ = load_cert_and_key(&self.default_cert.cert_path, &self.default_cert.key_path)?;
        info!("TLS certificate validation passed for reload");
        Ok(())
    }

    pub fn add_sni_cert(&self, sni_name: String, cert_pair: TlsCertPair) -> Result<()> {
        let mut certs = self.cert_paths.write();
        certs.insert(sni_name.clone(), cert_pair);
        debug!("added SNI cert for: {}", sni_name);
        Ok(())
    }

    pub fn remove_sni_cert(&self, sni_name: &str) -> Result<()> {
        let mut certs = self.cert_paths.write();
        certs.remove(sni_name)
            .context(format!("SNI cert not found: {}", sni_name))?;
        debug!("removed SNI cert for: {}", sni_name);
        Ok(())
    }

    pub fn list_sni_certs(&self) -> Vec<String> {
        let certs = self.cert_paths.read();
        certs.keys().cloned().collect()
    }
}

fn load_cert_and_key(cert_path: &Path, key_path: &Path) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let cert_file = File::open(cert_path)
        .context(format!("failed to open cert: {}", cert_path.display()))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_reader)
        .collect::<Result<Vec<_>, _>>()
        .context("failed to parse certs")?;

    if certs.is_empty() {
        return Err(anyhow::anyhow!("no certificates found in {}", cert_path.display()));
    }

    let key_file = File::open(key_path)
        .context(format!("failed to open key: {}", key_path.display()))?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("failed to parse private key")?
        .context("no private key found")?;

    info!("loaded {} cert(s) from {}", certs.len(), cert_path.display());
    Ok((certs, key))
}
