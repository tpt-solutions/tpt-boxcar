use std::collections::HashMap;

use anyhow::{Context, Result};
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
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

        let body = reqwest::get(&self.config.jwks_url)
            .await
            .context("failed to fetch JWKS")?
            .text()
            .await
            .context("failed to read JWKS response")?;

        let jwks: Jwks = serde_json::from_str(&body)
            .context("failed to parse JWKS")?;

        let mut keys = self.keys.write();
        keys.clear();

        for jwk in &jwks.keys {
            match self.jwk_to_decoding_key(jwk) {
                Ok(key) => {
                    let kid = jwk.kid.clone().unwrap_or_else(|| jwk.alg.clone());
                    keys.insert(kid, key);
                    debug!("loaded JWK: kid={}", jwk.kid.as_deref().unwrap_or("default"));
                }
                Err(e) => {
                    warn!("failed to convert JWK: {}", e);
                }
            }
        }

        info!("loaded {} keys from JWKS", keys.len());
        Ok(())
    }

    fn jwk_to_decoding_key(&self, jwk: &Jwk) -> Result<DecodingKey> {
        match jwk.alg.as_str() {
            "RS256" | "RS384" | "RS512" => {
                let n = jwk.n.as_ref().context("missing 'n' parameter")?;
                let e = jwk.e.as_ref().context("missing 'e' parameter")?;
                DecodingKey::from_rsa_components(n, e).context("invalid RSA key")
            }
            "ES256" | "ES384" | "ES512" => {
                let x = jwk.x.as_ref().context("missing 'x' parameter")?;
                let y = jwk.y.as_ref().context("missing 'y' parameter")?;
                let mut key_data = Vec::with_capacity(65);
                key_data.push(0x04);
                key_data.extend_from_slice(&base64url_decode(x)?);
                key_data.extend_from_slice(&base64url_decode(y)?);
                Ok(DecodingKey::from_ec_der(&key_data))
            }
            alg => Err(anyhow::anyhow!("unsupported algorithm: {}", alg)),
        }
    }

    pub fn validate_token(&self, token: &str) -> Result<JwtClaims> {
        let header = decode_header(token)
            .context("failed to decode JWT header")?;

        let kid = header.kid.unwrap_or_else(|| {
            format!("{:?}", header.alg)
        });

        let keys = self.keys.read();
        let key = keys.get(&kid)
            .or_else(|| keys.values().next())
            .context("no matching key found")?;

        let mut validation = Validation::new(header.alg);

        if let Some(ref issuer) = self.config.issuer {
            validation.set_issuer(&[issuer.as_str()]);
        }

        if let Some(ref audience) = self.config.audience {
            validation.set_audience(&[audience.as_str()]);
        }

        let token_data = decode::<JwtClaims>(token, key, &validation)
            .context("JWT validation failed")?;

        Ok(token_data.claims)
    }

    pub fn config(&self) -> &JwtConfig {
        &self.config
    }

    pub fn key_count(&self) -> usize {
        self.keys.read().len()
    }
}

fn base64url_decode(input: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    engine.decode(input).context("base64url decode failed")
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
        assert_eq!(validator.key_count(), 0);
    }
}
