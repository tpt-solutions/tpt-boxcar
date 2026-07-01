use anyhow::Result;
use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, Request};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::json;
use tracing::info;

use super::error::LlmError;
use super::retry::extract_retry_after;
use super::traits::LlmProvider;

#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

impl LlmProvider for OpenAiProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        info!("openai api request to model: {}", self.model);

        let body = json!({
            "model": self.model,
            "stream": true,
            "messages": [{
                "role": "user",
                "content": prompt
            }]
        });

        let url = format!("{}/v1/chat/completions", self.base_url);

        let https = HttpsConnector::with_native_roots();
        let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new())
            .build(https);

        let req = Request::builder()
            .method("POST")
            .uri(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .body(Full::new(Bytes::from(body.to_string())))
            .map_err(|e| LlmError::Network(format!("failed to build request: {e}")))?;

        let resp = client.request(req).await.map_err(|e| {
            LlmError::Network(format!("failed to fetch response: {e}"))
        })?;

        let status = resp.status();
        if !status.is_success() {
            if status.as_u16() == 429 {
                let retry_after = extract_retry_after(resp.headers())
                    .map(|d| d.as_secs());
                return Err(LlmError::RateLimit { retry_after });
            }
            if status.as_u16() == 401 {
                return Err(LlmError::AuthError("unauthorized".into()));
            }
            let text = resp
                .into_body()
                .collect()
                .await
                .map_err(|e| LlmError::Network(format!("failed to read error body: {e}")))?
                .to_bytes();
            let text = String::from_utf8_lossy(&text);
            return Err(LlmError::Unavailable(format!(
                "openai returned {status}: {text}"
            )));
        }

        let mut content = String::new();
        let mut buffer = String::new();

        let mut resp = resp;
        while let Some(next) = resp.frame().await {
            let frame = next.map_err(|e| LlmError::Network(format!("stream read error: {e}")))?;
            if let Some(data) = frame.into_data().ok() {
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
                    if let Some(delta) = parsed["choices"][0]["delta"]["content"].as_str() {
                        content.push_str(delta);
                    }
                }
            }
        }

        if content.is_empty() {
            return Err(LlmError::ParseError(
                "empty response from openai".into(),
            ));
        }

        Ok(content)
    }

    fn name(&self) -> &str {
        &self.model
    }
}