use async_trait::async_trait;
use reqwest::Client;
use tracing::info;

use super::error::LlmError;
use super::retry::extract_retry_after;
use super::traits::LlmProvider;

pub struct OpenAiProvider {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        info!("openai api request to model: {}", self.model);

        let body = serde_json::json!({
            "model": self.model,
            "stream": true,
            "messages": [{
                "role": "user",
                "content": prompt
            }]
        });

        let url = format!("{}/v1/chat/completions", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
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
                "openai returned {status}: {text}"
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
