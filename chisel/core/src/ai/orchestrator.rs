use std::time::Duration;

use anyhow::{Context, Result};
use tracing::warn;

use crate::analyzer::{AnalysisResult, WasmCompatibility};
use crate::distiller::DistilledImage;

use super::cache::PromptCache;
use super::error::LlmError;
use super::prompt_library::{PromptLibrary, ResponseFormat};
use super::retry::RetryPolicy;
use super::traits::LlmProvider;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmRequest {
    pub prompt: String,
    pub system: Option<String>,
    pub model: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub finish_reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DistillationPlan {
    pub steps: Vec<DistillationStep>,
    pub estimated_size_reduction: f64,
    pub risk_assessment: String,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DistillationStep {
    pub order: usize,
    pub action: String,
    pub description: String,
    pub impact: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationPlan {
    pub source_language: String,
    pub target_platform: String,
    pub phases: Vec<MigrationPhase>,
    pub estimated_effort: String,
    pub risk_factors: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MigrationPhase {
    pub name: String,
    pub tasks: Vec<String>,
    pub estimated_hours: f64,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SecurityAudit {
    pub findings: Vec<SecurityFinding>,
    pub overall_risk: String,
    pub compliance_notes: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SecurityFinding {
    pub severity: String,
    pub category: String,
    pub description: String,
    pub remediation: String,
}

/// Enum-based provider to avoid dyn compatibility issues with async fn in trait.
#[derive(Debug, Clone)]
pub enum LlmProviderEnum {
    OpenAi(super::openai::OpenAiProvider),
    Claude(super::claude::ClaudeProvider),
    Ollama(super::ollama::OllamaProvider),
}

impl LlmProviderEnum {
    pub fn name(&self) -> &str {
        match self {
            LlmProviderEnum::OpenAi(p) => p.name(),
            LlmProviderEnum::Claude(p) => p.name(),
            LlmProviderEnum::Ollama(p) => p.name(),
        }
    }

    pub async fn complete(&self, prompt: &str) -> Result<String, LlmError> {
        match self {
            LlmProviderEnum::OpenAi(p) => p.complete(prompt).await,
            LlmProviderEnum::Claude(p) => p.complete(prompt).await,
            LlmProviderEnum::Ollama(p) => p.complete(prompt).await,
        }
    }
}

pub struct AiOrchestrator {
    providers: Vec<LlmProviderEnum>,
    retry_policy: RetryPolicy,
    cache: PromptCache,
}

impl AiOrchestrator {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            retry_policy: RetryPolicy::new(3, Duration::from_millis(500)),
            cache: PromptCache::new(Duration::from_secs(300)),
        }
    }

    pub fn with_provider(mut self, provider: LlmProviderEnum) -> Self {
        self.providers.push(provider);
        self
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache = PromptCache::new(ttl);
        self
    }

    async fn call_llm(&self, prompt: &str) -> Result<String, LlmError> {
        if let Some(cached) = self.cache.get(prompt) {
            return Ok(cached);
        }

        let result = self
            .retry_policy
            .execute(|| async {
                let mut last_err = None;
                for provider in &self.providers {
                    match provider.complete(prompt).await {
                        Ok(text) => return Ok(text),
                        Err(e) => {
                            warn!(
                                "provider '{}' failed: {e}",
                                provider.name()
                            );
                            last_err = Some(e);
                        }
                    }
                }
                Err(last_err.unwrap_or_else(|| {
                    LlmError::Unavailable("no providers configured".into())
                }))
            })
            .await?;

        self.cache.insert(prompt, &result);
        Ok(result)
    }

    pub async fn generate_distillation_plan(
        &self,
        analysis: &AnalysisResult,
    ) -> Result<DistillationPlan> {
        let template = PromptLibrary::distillation_plan();
        let mut vars = std::collections::HashMap::new();
        vars.insert("image_name".to_string(), analysis.phase1.image.name.clone());
        vars.insert("image_tag".to_string(), analysis.phase1.image.tag.clone());
        vars.insert(
            "total_size".to_string(),
            analysis.phase1.image.total_size_bytes.to_string(),
        );
        vars.insert(
            "layer_count".to_string(),
            analysis.phase1.image.layer_count.to_string(),
        );
        vars.insert(
            "dependency_count".to_string(),
            analysis.phase1.dependencies.len().to_string(),
        );

        let prompt = PromptLibrary::fill_template(&template, &vars);
        let response = self.call_llm(&prompt).await.map_err(|e| {
            anyhow::anyhow!("LLM call failed: {e}")
        })?;
        let plan: DistillationPlan = serde_json::from_str(&response)
            .context("failed to parse distillation plan")?;
        Ok(plan)
    }

    pub async fn generate_migration_plan(
        &self,
        analysis: &AnalysisResult,
    ) -> Result<MigrationPlan> {
        let template = PromptLibrary::wasm_migration_plan();
        let missing_features = match &analysis.phase2.wasm_compatibility {
            WasmCompatibility::FullyCompatible => "none".to_string(),
            WasmCompatibility::PartiallyCompatible { missing_features } => {
                missing_features.join(", ")
            }
            WasmCompatibility::Incompatible { reason } => reason.clone(),
        };

        let mut vars = std::collections::HashMap::new();
        vars.insert(
            "language".to_string(),
            format!("{:?}", analysis.phase2.detected_language),
        );
        vars.insert(
            "compatibility".to_string(),
            format!("{:?}", analysis.phase2.wasm_compatibility),
        );
        vars.insert("missing_features".to_string(), missing_features);
        vars.insert(
            "required_imports".to_string(),
            analysis.phase2.required_imports.join(", "),
        );

        let prompt = PromptLibrary::fill_template(&template, &vars);
        let response = self.call_llm(&prompt).await.map_err(|e| {
            anyhow::anyhow!("LLM call failed: {e}")
        })?;
        let plan: MigrationPlan = serde_json::from_str(&response)
            .context("failed to parse migration plan")?;
        Ok(plan)
    }

    pub async fn generate_security_audit(
        &self,
        distilled: &DistilledImage,
    ) -> Result<SecurityAudit> {
        let template = PromptLibrary::security_audit();
        let mut vars = std::collections::HashMap::new();
        vars.insert(
            "sbom_format".to_string(),
            format!("{:?}", distilled.sbom.format),
        );
        vars.insert(
            "total_deps".to_string(),
            distilled.cve_scan.total_dependencies.to_string(),
        );
        vars.insert(
            "vuln_count".to_string(),
            distilled.cve_scan.vulnerabilities_found.to_string(),
        );
        vars.insert(
            "critical".to_string(),
            distilled.cve_scan.critical.to_string(),
        );
        vars.insert("high".to_string(), distilled.cve_scan.high.to_string());

        let prompt = PromptLibrary::fill_template(&template, &vars);
        let response = self.call_llm(&prompt).await.map_err(|e| {
            anyhow::anyhow!("LLM call failed: {e}")
        })?;
        let audit: SecurityAudit = serde_json::from_str(&response)
            .context("failed to parse security audit")?;
        Ok(audit)
    }
}

impl Default for AiOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}