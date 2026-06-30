use async_trait::async_trait;

use super::error::LlmError;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError>;
    fn name(&self) -> &str;
}
