use async_trait::async_trait;
use reqwest::Client;
use tracing::info;

use super::error::LlmError;
use super::retry::extract_retry_after;
use super::traits::LlmProvider;

pub struct ClaudeProvider {
    client: Client,
    api_key: String,
    model: String,
}

impl ClaudeProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for ClaudeProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        info!("claude api request to model: {}", self.model);

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 4096,
            "stream": true,
            "messages": [{
                "role": "user",
                "content": prompt
            }]
        });

        let resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Network(format!("failed to send request: {e}")))?;

        let status = resp.status();
        if !status.is_success() {
            if status.as_u16() == 429 {
                let retry_after = extract_retry_after(resp.headers())
                    .map(|d| d.as_secs());
                return Err(LlmError::RateLimit { retry_after });
            }
            if status.as_u16() == 401 {
                let text = resp
                    .text()
                    .await
                    .unwrap_or_else(|_| "unauthorized".into());
                return Err(LlmError::AuthError(text));
            }
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".into());
            return Err(LlmError::Unavailable(format!(
                "claude returned {status}: {text}"
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
            return Err(LlmError::ParseError(
                "empty response from claude".into(),
            ));
        }

        Ok(content)
    }

    fn name(&self) -> &str {
        &self.model
    }
}
