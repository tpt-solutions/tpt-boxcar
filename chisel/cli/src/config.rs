use std::path::Path;

use serde::Deserialize;
use tpt_chisel_core::ai::{AiOrchestrator, ClaudeProvider, LlmProviderEnum, OllamaProvider, OpenAiProvider};

#[derive(Debug, Deserialize)]
pub struct ChiselConfig {
    pub ai: AiConfig,
}

#[derive(Debug, Deserialize)]
pub struct AiConfig {
    pub backend: String,
    pub local: Option<LocalAiConfig>,
    pub cloud: Option<CloudAiConfig>,
}

#[derive(Debug, Deserialize)]
pub struct LocalAiConfig {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct CloudAiConfig {
    pub provider: String,
    pub model: String,
    pub api_key_env: String,
    #[serde(default)]
    pub base_url: Option<String>,
}

pub fn load(path: &Path) -> anyhow::Result<ChiselConfig> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read {}: {e}", path.display()))?;
    let config: ChiselConfig = serde_yaml::from_str(&content)?;
    Ok(config)
}

/// Builds an `AiOrchestrator` with a single provider selected either by the
/// explicit `backend_override` ("local" | "cloud") or, if absent, the
/// config file's `ai.backend` value.
pub fn build_orchestrator(
    config: &ChiselConfig,
    backend_override: Option<&str>,
) -> anyhow::Result<AiOrchestrator> {
    let backend = backend_override.unwrap_or(&config.ai.backend);

    let provider = match backend {
        "local" => {
            let local = config
                .ai
                .local
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("chisel.yaml has no ai.local section"))?;
            match local.provider.as_str() {
                "ollama" => LlmProviderEnum::Ollama(OllamaProvider::new(&local.endpoint, &local.model)),
                other => anyhow::bail!("unsupported local AI provider: {other}"),
            }
        }
        "cloud" => {
            let cloud = config
                .ai
                .cloud
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("chisel.yaml has no ai.cloud section"))?;
            let api_key = std::env::var(&cloud.api_key_env).map_err(|_| {
                anyhow::anyhow!(
                    "environment variable {} is not set (required for cloud AI backend)",
                    cloud.api_key_env
                )
            })?;
            match cloud.provider.as_str() {
                "claude" => LlmProviderEnum::Claude(ClaudeProvider::new(&api_key, &cloud.model)),
                "openai" => {
                    // OpenAiProvider appends "/v1/chat/completions" itself, so
                    // base_url must NOT already include a "/v1" suffix.
                    let base_url = cloud
                        .base_url
                        .as_deref()
                        .unwrap_or("https://api.openai.com");
                    LlmProviderEnum::OpenAi(OpenAiProvider::new(base_url, &api_key, &cloud.model))
                }
                "openrouter" => {
                    // OpenRouter exposes an OpenAI-compatible chat completions
                    // API, so it reuses OpenAiProvider. Model IDs are
                    // OpenRouter's own namespaced form, e.g.
                    // "anthropic/claude-3.5-sonnet" or "openai/gpt-4o".
                    let base_url = cloud
                        .base_url
                        .as_deref()
                        .unwrap_or("https://openrouter.ai/api");
                    LlmProviderEnum::OpenAi(OpenAiProvider::new(base_url, &api_key, &cloud.model))
                }
                other => anyhow::bail!("unsupported cloud AI provider: {other}"),
            }
        }
        other => anyhow::bail!("unknown AI backend: {other} (expected \"local\" or \"cloud\")"),
    };

    Ok(AiOrchestrator::new().with_provider(provider))
}
