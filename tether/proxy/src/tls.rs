use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ClientConfig, RootCertStore};
use serde::{Deserialize, Serialize};
use tokio_rustls::TlsConnector;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ca_cert_path: Option<PathBuf>,
    #[serde(default)]
    pub client_cert_path: Option<PathBuf>,
    #[serde(default)]
    pub client_key_path: Option<PathBuf>,
    #[serde(default = "default_server_name")]
    pub server_name: String,
}

fn default_server_name() -> String {
    "localhost".into()
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            server_name: default_server_name(),
        }
    }
}

impl TlsConfig {
    pub fn is_mtls(&self) -> bool {
        self.client_cert_path.is_some() && self.client_key_path.is_some()
    }

    pub fn build_client_config(&self) -> Result<Option<Arc<ClientConfig>>> {
        if !self.enabled {
            return Ok(None);
        }

        let mut root_store = RootCertStore::empty();

        if let Some(ca_path) = &self.ca_cert_path {
            let ca_certs = load_certs(ca_path)?;
            for cert in ca_certs {
                root_store
                    .add(cert)
                    .context("failed to add CA cert to root store")?;
            }
            debug!(count = root_store.len(), "loaded CA certificates");
        } else {
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        }

        let config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();

        Ok(Some(Arc::new(config)))
    }

    pub fn build_tls_connector(&self) -> Result<Option<TlsConnector>> {
        match self.build_client_config()? {
            Some(config) => Ok(Some(TlsConnector::from(config))),
            None => Ok(None),
        }
    }
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file =
        File::open(path).with_context(|| format!("failed to open cert: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("failed to parse certificates")?;
    info!(
        count = certs.len(),
        path = %path.display(),
        "loaded certificates"
    );
    Ok(certs)
}

#[allow(dead_code)]
fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file =
        File::open(path).with_context(|| format!("failed to open key: {}", path.display()))?;
    let mut reader = BufReader::new(file);

    for item in rustls_pemfile::read_all(&mut reader) {
        match item.context("failed to parse key")? {
            rustls_pemfile::Item::Pkcs1Key(key) => {
                return Ok(PrivateKeyDer::Pkcs1(key.into()));
            }
            rustls_pemfile::Item::Pkcs8Key(key) => {
                return Ok(PrivateKeyDer::Pkcs8(key.into()));
            }
            rustls_pemfile::Item::Sec1Key(key) => {
                return Ok(PrivateKeyDer::Sec1(key.into()));
            }
            _ => continue,
        }
    }

    anyhow::bail!("no private key found in {}", path.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_config_default() {
        let config = TlsConfig::default();
        assert!(!config.enabled);
        assert!(config.ca_cert_path.is_none());
        assert!(config.client_cert_path.is_none());
        assert!(config.client_key_path.is_none());
        assert_eq!(config.server_name, "localhost");
    }

    #[test]
    fn test_tls_config_not_mtls_by_default() {
        let config = TlsConfig::default();
        assert!(!config.is_mtls());
    }

    #[test]
    fn test_tls_config_mtls_when_certs_present() {
        let config = TlsConfig {
            enabled: true,
            client_cert_path: Some(PathBuf::from("/path/to/cert.pem")),
            client_key_path: Some(PathBuf::from("/path/to/key.pem")),
            ..Default::default()
        };
        assert!(config.is_mtls());
    }

    #[test]
    fn test_build_client_config_when_disabled() {
        let config = TlsConfig::default();
        let result = config.build_client_config().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_tls_config_serde_roundtrip() {
        let config = TlsConfig {
            enabled: true,
            server_name: "db.example.com".into(),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: TlsConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.enabled);
        assert_eq!(deserialized.server_name, "db.example.com");
    }
}
