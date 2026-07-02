use anyhow::Result;
use http_body_util::{BodyExt, Full};
use hyper::{body::Bytes, Request};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::json;
use tracing::info;

use super::error::LlmError;
use super::traits::LlmProvider;

#[derive(Debug, Clone)]
pub struct OllamaProvider {
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
        }
    }
}

impl LlmProvider for OllamaProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        info!("ollama request to model: {}", self.model);

        let body = json!({
            "model": self.model,
            "prompt": prompt,
            "stream": true
        });

        let url = format!("{}/api/generate", self.base_url);

        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|e| LlmError::Network(format!("failed to load native roots: {e}")))?
            .https_only()
            .enable_http1()
            .build();
        let client: Client<_, Full<Bytes>> = Client::builder(TokioExecutor::new())
            .build(https);

        let req = Request::builder()
            .method("POST")
            .uri(&url)
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body.to_string())))
            .map_err(|e| LlmError::Network(format!("failed to build request: {e}")))?;

        let resp = client.request(req).await.map_err(|e| {
            LlmError::Network(format!("failed to fetch response: {e}"))
        })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp
                .into_body()
                .collect()
                .await
                .map_err(|e| LlmError::Network(format!("failed to read error body: {e}")))?
                .to_bytes();
            let text = String::from_utf8_lossy(&text);
            return Err(LlmError::Unavailable(format!(
                "ollama returned {status}: {text}"
            )));
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

                if line.is_empty() {
                    continue;
                }

                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(text) = parsed["response"].as_str() {
                        content.push_str(text);
                    }
                }
            }
        }

        if content.is_empty() {
            return Err(LlmError::ParseError(
                "empty response from ollama".into(),
            ));
        }

        Ok(content)
    }

    fn name(&self) -> &str {
        &self.model
    }
}