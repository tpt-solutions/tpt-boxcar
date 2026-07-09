use anyhow::Result;
use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, Request};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::json;
use tracing::info;

use super::error::LlmError;
use super::retry::extract_retry_after;
use super::traits::LlmProvider;

#[derive(Debug, Clone)]
pub struct ClaudeProvider {
    api_key: String,
    model: String,
}

impl ClaudeProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

impl LlmProvider for ClaudeProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        info!("claude api request to model: {}", self.model);

        let body = json!({
            "model": self.model,
            "max_tokens": 4096,
            "stream": true,
            "messages": [{
                "role": "user",
                "content": prompt
            }]
        });

        let url = "https://api.anthropic.com/v1/messages";

        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|e| LlmError::Network(format!("failed to load native roots: {e}")))?
            .https_only()
            .enable_http1()
            .build();
        let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new()).build(https);

        let req = Request::builder()
            .method("POST")
            .uri(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body.to_string())))
            .map_err(|e| LlmError::Network(format!("failed to build request: {e}")))?;

        let resp = client
            .request(req)
            .await
            .map_err(|e| LlmError::Network(format!("failed to fetch response: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            if status.as_u16() == 429 {
                let retry_after = extract_retry_after(resp.headers()).map(|d| d.as_secs());
                return Err(LlmError::RateLimit { retry_after });
            }
            if status.as_u16() == 401 {
                return Err(LlmError::AuthError("unauthorized".into()));
            }
            return Err(LlmError::Unavailable(format!("claude returned {status}")));
        }

        let mut content = String::new();
        let mut buffer = String::new();

        let mut resp = resp;
        while let Some(next) = resp.frame().await {
            let frame = next.map_err(|e| LlmError::Network(format!("stream read error: {e}")))?;
            if let Ok(data) = frame.into_data() {
                buffer.push_str(&String::from_utf8_lossy(&data));
            }

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.is_empty() || !line.starts_with("data: ") {
                    continue;
                }

                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }

                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                    let event_type = parsed["type"].as_str().unwrap_or("");
                    if event_type == "content_block_delta" {
                        if let Some(text) = parsed["delta"]["text"].as_str() {
                            content.push_str(text);
                        }
                    }
                }
            }
        }

        if content.is_empty() {
            return Err(LlmError::ParseError("empty response from claude".into()));
        }

        Ok(content)
    }

    fn name(&self) -> &str {
        &self.model
    }
}
