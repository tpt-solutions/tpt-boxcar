use std::collections::HashMap;

use anyhow::Result;
use http_body_util::{BodyExt, Empty};
use hyper::{Request, StatusCode};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtConfig {
    pub jwks_url: String,
    pub allowed_algorithms: Vec<String>,
    pub header_name: String,
    pub issuer: Option<String>,
    pub audience: Option<String>,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            jwks_url: String::new(),
            allowed_algorithms: vec!["RS256".into(), "ES256".into()],
            header_name: "authorization".into(),
            issuer: None,
            audience: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    pub alg: String,
    pub kid: Option<String>,
    pub n: Option<String>,
    pub e: Option<String>,
    pub crv: Option<String>,
    pub x: Option<String>,
    pub y: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub aud: Option<String>,
    pub exp: Option<u64>,
    pub iat: Option<u64>,
    pub nbf: Option<u64>,
    pub jti: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

pub struct JwtValidator {
    config: JwtConfig,
    keys: RwLock<HashMap<String, DecodingKey>>,
}

impl JwtValidator {
    pub fn new(config: JwtConfig) -> Self {
        Self {
            config,
            keys: RwLock::new(HashMap::new()),
        }
    }

    pub async fn refresh_keys(&self) -> Result<()> {
        info!("refreshing JWKS from {}", self.config.jwks_url);

        let url = self
            .config
            .jwks_url
            .parse::<hyper::Uri>()
            .map_err(|e| anyhow::anyhow!("invalid JWKS URL: {e}"))?;

        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|e| anyhow::anyhow!("failed to load native roots: {e}"))?
            .https_only()
            .enable_http1()
            .build();
        let client: Client<_, Empty<hyper::body::Bytes>> =
            Client::builder(TokioExecutor::new()).build(https);

        let req = Request::builder()
            .method("GET")
            .uri(&url)
            .header("Accept", "application/json")
            .body(Empty::new())
            .map_err(|e| anyhow::anyhow!("failed to build request: {e}"))?;

        let resp = client
            .request(req)
            .await
            .map_err(|e| anyhow::anyhow!("failed to fetch JWKS: {e}"))?;

        if resp.status() != StatusCode::OK {
            return Err(anyhow::anyhow!(
                "JWKS request returned status {}",
                resp.status()
            ));
        }

        let body = resp
            .into_body()
            .collect()
            .await
            .map_err(|e| anyhow::anyhow!("failed to read response body: {e}"))?
            .to_bytes();
        let body_str = String::from_utf8_lossy(&body);

        let jwks: Jwks = serde_json::from_str(&body_str)
            .map_err(|e| anyhow::anyhow!("failed to parse JWKS: {e}"))?;

        let mut keys = self.keys.write().await;
        keys.clear();

        for jwk in &jwks.keys {
            match self.jwk_to_decoding_key(jwk) {
                Ok(key) => {
                    let kid = jwk.kid.clone().unwrap_or_else(|| jwk.alg.clone());
                    keys.insert(kid, key);
                    debug!(
                        "loaded JWK: kid={}",
                        jwk.kid.as_deref().unwrap_or("default")
                    );
                }
                Err(e) => {
                    warn!("failed to convert JWK: {}", e);
                }
            }
        }

        info!("loaded {} keys from JWKS", keys.len());
        Ok(())
    }

    fn jwk_to_decoding_key(&self, jwk: &Jwk) -> anyhow::Result<DecodingKey> {
        match jwk.alg.as_str() {
            "RS256" | "RS384" | "RS512" => {
                let n = jwk
                    .n
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing 'n' parameter"))?;
                let e = jwk
                    .e
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing 'e' parameter"))?;
                DecodingKey::from_rsa_components(n, e)
                    .map_err(|e| anyhow::anyhow!("invalid RSA key: {e}"))
            }
            "ES256" | "ES384" | "ES512" => {
                let x = jwk
                    .x
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing 'x' parameter"))?;
                let y = jwk
                    .y
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("missing 'y' parameter"))?;
                let mut key_data = Vec::with_capacity(65);
                key_data.push(0x04);
                key_data.extend_from_slice(&base64url_decode(x)?);
                key_data.extend_from_slice(&base64url_decode(y)?);
                Ok(DecodingKey::from_ec_der(&key_data))
            }
            alg => Err(anyhow::anyhow!("unsupported algorithm: {}", alg)),
        }
    }

    pub async fn validate_token(&self, token: &str) -> Result<JwtClaims> {
        let header = decode_header(token)
            .map_err(|e| anyhow::anyhow!("failed to decode JWT header: {e}"))?;

        let kid = header
            .kid
            .clone()
            .unwrap_or_else(|| format!("{:?}", header.alg));

        let keys = self.keys.read().await;
        let key = keys
            .get(&kid)
            .or_else(|| keys.values().next())
            .ok_or_else(|| anyhow::anyhow!("no matching key found"))?
            .clone();
        drop(keys);

        let mut validation = Validation::new(header.alg);

        if let Some(ref issuer) = self.config.issuer {
            validation.set_issuer(&[issuer.as_str()]);
        }

        if let Some(ref audience) = self.config.audience {
            validation.set_audience(&[audience.as_str()]);
        }

        let token_data = decode::<JwtClaims>(token, &key, &validation)
            .map_err(|e| anyhow::anyhow!("JWT validation failed: {e}"))?;

        Ok(token_data.claims)
    }

    pub fn config(&self) -> &JwtConfig {
        &self.config
    }

    pub async fn key_count(&self) -> usize {
        self.keys.read().await.len()
    }
}

fn base64url_decode(input: &str) -> anyhow::Result<Vec<u8>> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    engine
        .decode(input)
        .map_err(|e| anyhow::anyhow!("base64url decode failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_config_default() {
        let config = JwtConfig::default();
        assert!(config.allowed_algorithms.contains(&"RS256".to_string()));
        assert!(config.allowed_algorithms.contains(&"ES256".to_string()));
        assert_eq!(config.header_name, "authorization");
    }

    #[test]
    fn test_jwt_validator_new() {
        let config = JwtConfig::default();
        let validator = JwtValidator::new(config);
    }
}
