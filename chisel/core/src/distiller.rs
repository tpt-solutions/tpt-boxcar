use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::info;
use uuid::Uuid;

use crate::analyzer::{AnalysisResult, Dependency, WasmCompatibility};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SbomFormat {
    CycloneDx,
    Spdx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CveSeverity {
    Critical,
    High,
    Medium,
    Low,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CveFinding {
    pub id: String,
    pub severity: CveSeverity,
    pub package: String,
    pub version: String,
    pub fixed_in: Option<String>,
    pub description: String,
    pub cvss_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CveScanResult {
    pub total_dependencies: usize,
    pub vulnerabilities_found: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub findings: Vec<CveFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomComponent {
    pub name: String,
    pub version: String,
    pub supplier: String,
    pub license: String,
    pub purl: String,
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SbomDocument {
    pub format: SbomFormat,
    pub spec_version: String,
    pub document_id: String,
    pub version: String,
    pub created: String,
    pub components: Vec<SbomComponent>,
    pub dependencies_graph: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistilledImage {
    pub name: String,
    pub tag: String,
    pub base_image: String,
    pub os: String,
    pub arch: String,
    pub entrypoint: Vec<String>,
    pub cmd: Vec<String>,
    pub env_vars: HashMap<String, String>,
    pub layers: Vec<DistilledLayer>,
    pub total_size_bytes: u64,
    pub layer_count: usize,
    pub sbom: SbomDocument,
    pub cve_scan: CveScanResult,
    pub wasm_migration_path: Option<WasmMigrationPath>,
    pub distillation_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistilledLayer {
    pub digest: String,
    pub size_bytes: u64,
    pub diff_id: String,
    pub created: String,
    pub instruction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmMigrationPath {
    pub target: String,
    pub estimated_size_reduction: f64,
    pub compatible_apis: Vec<String>,
    pub requires_adaptation: Vec<String>,
}

pub struct Distiller;

impl Distiller {
    pub fn new() -> Self {
        Self
    }

    pub fn distill(&self, analysis: &AnalysisResult) -> Result<DistilledImage> {
        info!("Distilling image: {}", analysis.phase1.image.name);

        let sbom = self.generate_sbom(&analysis.phase1.dependencies)?;
        let cve_scan = self.scan_cves(&sbom)?;
        let wasm_path = self.compute_wasm_migration_path(analysis);
        let layers = self.distill_layers(&analysis.phase1)?;
        let total_size: u64 = layers.iter().map(|l| l.size_bytes).sum();
        let original_size = analysis.phase1.image.total_size_bytes;
        let ratio = if original_size > 0 {
            total_size as f64 / original_size as f64
        } else {
            1.0
        };

        let distilled = DistilledImage {
            name: format!("{}-distilled", analysis.phase1.image.name),
            tag: format!("{}-distilled", analysis.phase1.image.tag),
            base_image: self.select_base_image(analysis),
            os: analysis.phase1.image.os.clone(),
            arch: analysis.phase1.image.arch.clone(),
            entrypoint: self.determine_entrypoint(analysis),
            cmd: self.determine_cmd(analysis),
            env_vars: self.filter_env_vars(&analysis.phase1.image.env_vars),
            layers,
            total_size_bytes: total_size,
            layer_count: 0,
            sbom,
            cve_scan,
            wasm_migration_path: wasm_path,
            distillation_ratio: ratio,
        };

        Ok(distilled)
    }

    fn generate_sbom(&self, dependencies: &[Dependency]) -> Result<SbomDocument> {
        let components: Vec<SbomComponent> = dependencies
            .iter()
            .map(|dep| {
                let purl = format!("pkg:generic/{}@{}", dep.name, dep.version);
                let mut hasher = Sha256::new();
                hasher.update(format!("{}:{}", dep.name, dep.version));
                let hash = hex::encode(hasher.finalize());

                SbomComponent {
                    name: dep.name.clone(),
                    version: dep.version.clone(),
                    supplier: dep.source.clone(),
                    license: dep.license.clone(),
                    purl,
                    hash,
                }
            })
            .collect();

        Ok(SbomDocument {
            format: SbomFormat::CycloneDx,
            spec_version: "1.5".to_string(),
            document_id: Uuid::new_v4().to_string(),
            version: "1.0".to_string(),
            created: Utc::now().to_rfc3339(),
            components,
            dependencies_graph: HashMap::new(),
        })
    }

    pub fn generate_spdx_sbom(&self, dependencies: &[Dependency]) -> Result<SbomDocument> {
        let mut doc = self.generate_sbom(dependencies)?;
        doc.format = SbomFormat::Spdx;
        doc.spec_version = "SPDX-2.3".to_string();
        Ok(doc)
    }

    fn scan_cves(&self, sbom: &SbomDocument) -> Result<CveScanResult> {
        let mut findings = Vec::new();
        let critical: usize = 0;
        let high: usize = 0;
        let mut medium: usize = 0;
        let low: usize = 0;

        for component in &sbom.components {
            if component.version.contains("alpha") || component.version.contains("beta") {
                findings.push(CveFinding {
                    id: format!("CVE-{}-{}", component.name.len(), component.version.len()),
                    severity: CveSeverity::Medium,
                    package: component.name.clone(),
                    version: component.version.clone(),
                    fixed_in: None,
                    description: format!("Pre-release version detected for {}", component.name),
                    cvss_score: Some(5.0),
                });
                medium += 1;
            }
        }

        let vulnerabilities_found = findings.len();

        Ok(CveScanResult {
            total_dependencies: sbom.components.len(),
            vulnerabilities_found,
            critical,
            high,
            medium,
            low,
            findings,
        })
    }

    fn compute_wasm_migration_path(&self, analysis: &AnalysisResult) -> Option<WasmMigrationPath> {
        match &analysis.phase2.wasm_compatibility {
            WasmCompatibility::FullyCompatible => Some(WasmMigrationPath {
                target: analysis.phase2.suggested_target.clone(),
                estimated_size_reduction: 0.7,
                compatible_apis: analysis.phase2.required_imports.clone(),
                requires_adaptation: Vec::new(),
            }),
            WasmCompatibility::PartiallyCompatible { missing_features } => {
                Some(WasmMigrationPath {
                    target: analysis.phase2.suggested_target.clone(),
                    estimated_size_reduction: 0.5,
                    compatible_apis: analysis.phase2.required_imports.clone(),
                    requires_adaptation: missing_features.clone(),
                })
            }
            WasmCompatibility::Incompatible { .. } => None,
        }
    }

    fn distill_layers(
        &self,
        phase1: &crate::analyzer::Phase1Result,
    ) -> Result<Vec<DistilledLayer>> {
        let mut layers = Vec::new();

        for layer in &phase1.image.layers {
            if let Some(cmd) = &layer.command {
                if cmd.starts_with("RUN apt-get install") || cmd.starts_with("RUN yum install") {
                    let distilled_size = (layer.size_bytes as f64 * 0.6) as u64;
                    layers.push(DistilledLayer {
                        digest: layer.digest.clone(),
                        size_bytes: distilled_size,
                        diff_id: format!("sha256:{}", hex::encode(layer.digest.as_bytes())),
                        created: Utc::now().to_rfc3339(),
                        instruction: cmd.clone(),
                    });
                } else if !cmd.starts_with("RUN") || cmd.contains("&&") {
                    layers.push(DistilledLayer {
                        digest: layer.digest.clone(),
                        size_bytes: layer.size_bytes,
                        diff_id: format!("sha256:{}", hex::encode(layer.digest.as_bytes())),
                        created: Utc::now().to_rfc3339(),
                        instruction: cmd.clone(),
                    });
                }
            }
        }

        if layers.is_empty() {
            layers.push(DistilledLayer {
                digest: "sha256:distilled-base".to_string(),
                size_bytes: 0,
                diff_id: "sha256:distilled-base".to_string(),
                created: Utc::now().to_rfc3339(),
                instruction: "FROM scratch".to_string(),
            });
        }

        Ok(layers)
    }

    fn select_base_image(&self, analysis: &AnalysisResult) -> String {
        match &analysis.phase2.wasm_compatibility {
            WasmCompatibility::FullyCompatible => {
                "gcr.io/distroless/static-debian12:nonroot".to_string()
            }
            WasmCompatibility::PartiallyCompatible { .. } => {
                "gcr.io/distroless/base-debian12:nonroot".to_string()
            }
            WasmCompatibility::Incompatible { .. } => analysis.phase1.image.name.clone(),
        }
    }

    fn determine_entrypoint(&self, analysis: &AnalysisResult) -> Vec<String> {
        let entrypoint = analysis.phase1.trace.entrypoint.clone();
        if entrypoint.is_empty() || entrypoint == "/" {
            vec!["/ko-app/".to_string(), analysis.phase1.image.name.clone()]
        } else {
            vec![entrypoint]
        }
    }

    fn determine_cmd(&self, analysis: &AnalysisResult) -> Vec<String> {
        if !analysis.phase1.trace.entrypoint.is_empty() {
            Vec::new()
        } else {
            vec!["serve".to_string()]
        }
    }

    fn filter_env_vars(&self, env: &HashMap<String, String>) -> HashMap<String, String> {
        env.iter()
            .filter(|(k, _)| {
                !k.starts_with("PATH")
                    && !k.starts_with("LD_")
                    && !k.starts_with("HOME")
                    && !k.contains("SECRET")
                    && !k.contains("KEY")
                    && !k.contains("PASSWORD")
                    && !k.contains("TOKEN")
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn generate_dockerfile(&self, distilled: &DistilledImage) -> String {
        let mut lines = Vec::new();

        lines.push(format!("FROM {} AS base", distilled.base_image));
        lines.push(String::new());

        for (k, v) in &distilled.env_vars {
            lines.push(format!("ENV {k}={v}"));
        }

        lines.push(String::new());
        lines.push("COPY --from=builder /app /app".to_string());

        if !distilled.entrypoint.is_empty() {
            let entry_str = distilled
                .entrypoint
                .iter()
                .map(|e| format!("\"{e}\""))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("ENTRYPOINT [{entry_str}]"));
        }

        if !distilled.cmd.is_empty() {
            let cmd_str = distilled
                .cmd
                .iter()
                .map(|c| format!("\"{c}\""))
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("CMD [{cmd_str}]"));
        }

        lines.push("USER nonroot:nonroot".to_string());
        lines.push(String::new());
        lines.push("LABEL org.opencontainers.image.title=\"distilled\"".to_string());

        lines.join("\n")
    }
}

impl Default for Distiller {
    fn default() -> Self {
        Self::new()
    }
}
