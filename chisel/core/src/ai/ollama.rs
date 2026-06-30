use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::Client;
use tracing::info;

use super::error::LlmError;
use super::traits::LlmProvider;

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        info!("ollama request to model: {}", self.model);

        let mut body = HashMap::new();
        body.insert("model", self.model.clone());
        body.insert("prompt", prompt.to_string());
        body.insert("stream", "true".to_string());

        let url = format!("{}/api/generate", self.base_url);

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Network(format!("failed to send request: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(LlmError::Unavailable(format!(
                "ollama returned {status}: {text}"
            )));
        }

        let mut content = String::new();
        let mut buffer = String::new();

        let mut resp = resp;
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| LlmError::Network(format!("stream read error: {e}")))?
        {
            buffer.push_str(&String::from_utf8_lossy(&chunk));

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
