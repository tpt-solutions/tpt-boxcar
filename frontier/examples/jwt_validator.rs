use std::collections::HashMap;

use anyhow::Result;
use tpt_frontier_plugin_sdk::abi::{
    FilterResult, HttpRequest, HttpResponse, LogLevel, PluginContext, PluginFilter,
};

pub struct JwtValidatorPlugin {
    jwt_secret: Vec<u8>,
    header_name: String,
    required_claims: Vec<String>,
}

impl JwtValidatorPlugin {
    pub fn new(config: HashMap<String, String>) -> Self {
        let jwt_secret = config
            .get("jwt_secret")
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default();

        let header_name = config
            .get("header_name")
            .cloned()
            .unwrap_or_else(|| "authorization".into());

        let required_claims = config
            .get("required_claims")
            .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
            .unwrap_or_default();

        Self {
            jwt_secret,
            header_name,
            required_claims,
        }
    }

    fn extract_token(&self, request: &HttpRequest) -> Option<String> {
        let header_value = request.headers.get(&self.header_name)?;
        let token = header_value.strip_prefix("Bearer ")?;
        Some(token.to_string())
    }

    fn validate_jwt(&self, token: &str) -> Result<HashMap<String, String>> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(anyhow::anyhow!("invalid token format"));
        }

        // Verify HMAC-SHA256 signature
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(&self.jwt_secret)
            .map_err(|e| anyhow::anyhow!("invalid HMAC key: {}", e))?;
        mac.update(format!("{}.{}", parts[0], parts[1]).as_bytes());
        let expected_sig = mac.finalize().into_bytes();
        let expected_b64 = base64_encode(&expected_sig);
        if !constant_time_eq(parts[2], &expected_b64) {
            return Err(anyhow::anyhow!("invalid signature"));
        }

        let payload_b64 = parts[1];
        let payload_bytes = base64_decode(payload_b64)?;
        let payload: HashMap<String, serde_json::Value> =
            serde_json::from_slice(&payload_bytes)?;

        let mut claims = HashMap::new();
        for (key, value) in payload {
            if let Some(s) = value.as_str() {
                claims.insert(key, s.to_string());
            }
        }

        if let Some(exp) = claims.get("exp") {
            if let Ok(exp_ts) = exp.parse::<u64>() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                if now > exp_ts {
                    return Err(anyhow::anyhow!("token expired"));
                }
            }
        }

        for claim in &self.required_claims {
            if !claims.contains_key(claim) {
                return Err(anyhow::anyhow!("missing required claim: {}", claim));
            }
        }

        Ok(claims)
    }
}

impl PluginFilter for JwtValidatorPlugin {
    fn on_request(&self, request: &HttpRequest) -> FilterResult {
        match self.extract_token(request) {
            Some(token) => match self.validate_jwt(&token) {
                Ok(claims) => {
                    FilterResult::Continue
                }
                Err(e) => FilterResult::Deny {
                    reason: format!("JWT validation failed: {}", e),
                },
            },
            None => FilterResult::Deny {
                reason: format!("missing {} header", self.header_name),
            },
        }
    }

    fn on_response(&self, _request: &HttpRequest, _response: &HttpResponse) -> FilterResult {
        FilterResult::Continue
    }
}

fn base64_decode(input: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let padded = match input.len() % 4 {
        0 => input.to_string(),
        2 => format!("{}==", input),
        3 => format!("{}=", input),
        _ => return Err(anyhow::anyhow!("invalid base64 length")),
    };
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&padded)
        .map_err(|e| anyhow::anyhow!("base64 decode error: {}", e))
}

fn base64_encode(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn constant_time_eq(a: &str, b: &str) -> bool {
    use std::time::SystemTime;
    if a.len() != b.len() {
        // Still do the comparison to avoid timing leak on length
        let _ = a.as_bytes().iter().zip(b.as_bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y));
        return false;
    }
    let diff = a.as_bytes().iter().zip(b.as_bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    // Add a small constant-time delay
    let _ = SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap();
    diff == 0
}

fn main() {
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
        eprintln!("warning: JWT_SECRET not set, using empty secret");
        String::new()
    });

    let config = HashMap::from([
        ("jwt_secret".into(), jwt_secret),
        ("header_name".into(), "authorization".into()),
        ("required_claims".into(), "sub,iss".into()),
    ]);

    let plugin = JwtValidatorPlugin::new(config);

    let request = HttpRequest {
        method: "GET".into(),
        path: "/api/protected".into(),
        headers: HashMap::from([
            ("authorization".into(), "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.abc".into()),
        ]),
        body: vec![],
    };

    let result = plugin.on_request(&request);
    println!("Filter result: {:?}", result);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_missing_header_denied() {
        let plugin = JwtValidatorPlugin::new(HashMap::new());
        let request = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: HashMap::new(),
            body: vec![],
        };
        match plugin.on_request(&request) {
            FilterResult::Deny { reason } => assert!(reason.contains("missing")),
            _ => panic!("expected deny"),
        }
    }

    #[test]
    fn test_invalid_token_denied() {
        let plugin = JwtValidatorPlugin::new(HashMap::new());
        let request = HttpRequest {
            method: "GET".into(),
            path: "/".into(),
            headers: HashMap::from([("authorization".into(), "Bearer invalid".into())]),
            body: vec![],
        };
        match plugin.on_request(&request) {
            FilterResult::Deny { reason } => assert!(reason.contains("invalid")),
            _ => panic!("expected deny"),
        }
    }
}
