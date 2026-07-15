use anyhow::Result;

use super::error::LlmError;

/// Native async fn in trait (stable since Rust 1.75).
/// Note: This trait cannot be used as a trait object (dyn LlmProvider) without async_trait.
/// For trait object support, use the LlmProvider enum in mod.rs.
#[allow(async_fn_in_trait)]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, prompt: &str) -> Result<String, LlmError>;
    fn name(&self) -> &str;
}
