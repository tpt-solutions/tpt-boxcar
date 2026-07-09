use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use uuid::Uuid;

use crate::analyzer::{AnalysisResult, Dependency, WasmCompatibility};
use crate::wasm_pipeline::{WasmModule, WasmModuleSignature, WasmPipeline};

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
    /// Which tool actually produced this scan — `"grype"` when the real
    /// binary was found on `PATH` and ran successfully, `"heuristic"` when
    /// it fell back to a pre-release-version-string heuristic instead.
    pub scanned_by: String,
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
    /// Which tool actually produced this document — `"syft"` when the real
    /// binary was found on `PATH` and ran successfully, `"heuristic"` when
    /// it fell back to scanning `Phase1Result::dependencies` directly.
    pub generated_by: String,
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
    pub wasm_signature: Option<WasmModuleSignature>,
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

    pub async fn distill(
        &self,
        analysis: &AnalysisResult,
        image_root: &Path,
    ) -> Result<DistilledImage> {
        info!("Distilling image: {}", analysis.phase1.image.name);

        let sbom = self
            .generate_sbom(image_root, &analysis.phase1.dependencies)
            .await?;
        let cve_scan = self.scan_cves(image_root, &sbom).await?;
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
            wasm_signature: None,
        };

        Ok(distilled)
    }

    /// Distills as `distill()` does, and additionally signs the given
    /// compiled `WasmModule`'s bytes with the key read from
    /// `CHISEL_SIGNING_KEY` (hex-encoded ed25519 secret key). Key
    /// management/KMS integration is out of scope — this is a BYO-key
    /// scheme, matching the "no PKI infra in this repo" constraint.
    pub async fn distill_and_sign_wasm(
        &self,
        analysis: &AnalysisResult,
        image_root: &Path,
        wasm_module: &WasmModule,
    ) -> Result<DistilledImage> {
        let mut distilled = self.distill(analysis, image_root).await?;
        let signing_key = Self::signing_key_from_env()?;
        let pipeline = WasmPipeline::new();
        distilled.wasm_signature = Some(pipeline.sign_module(wasm_module, &signing_key)?);
        Ok(distilled)
    }

    fn signing_key_from_env() -> Result<ed25519_dalek::SigningKey> {
        let hex_key = std::env::var("CHISEL_SIGNING_KEY").context(
            "CHISEL_SIGNING_KEY env var not set; required to sign a wasm migration artifact",
        )?;
        let bytes = hex::decode(hex_key.trim()).context("CHISEL_SIGNING_KEY is not valid hex")?;
        let key_bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("CHISEL_SIGNING_KEY must decode to 32 bytes"))?;
        Ok(ed25519_dalek::SigningKey::from_bytes(&key_bytes))
    }

    /// Generates a real CycloneDX SBOM via `syft` when it's on `PATH`,
    /// falling back to a synthesized document built directly from
    /// `Phase1Result::dependencies` (the pre-Phase-11 behavior) otherwise —
    /// `syft`/`grype` aren't a workspace dependency we can vendor, so this
    /// degrades honestly rather than faking a scan.
    async fn generate_sbom(
        &self,
        image_root: &Path,
        dependencies: &[Dependency],
    ) -> Result<SbomDocument> {
        match Self::run_syft(image_root, "cyclonedx-json").await {
            Ok(Some(raw)) => match Self::parse_syft_cyclonedx(&raw) {
                Ok(doc) => return Ok(doc),
                Err(e) => {
                    warn!("failed to parse syft output ({e:#}); falling back to heuristic SBOM")
                }
            },
            Ok(None) => info!("syft not found on PATH; falling back to heuristic SBOM generation"),
            Err(e) => {
                warn!("syft invocation failed ({e:#}); falling back to heuristic SBOM generation")
            }
        }
        Ok(Self::generate_sbom_heuristic(dependencies))
    }

    fn generate_sbom_heuristic(dependencies: &[Dependency]) -> SbomDocument {
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

        SbomDocument {
            format: SbomFormat::CycloneDx,
            spec_version: "1.5".to_string(),
            document_id: Uuid::new_v4().to_string(),
            version: "1.0".to_string(),
            created: Utc::now().to_rfc3339(),
            components,
            dependencies_graph: HashMap::new(),
            generated_by: "heuristic".to_string(),
        }
    }

    /// Parses `syft <target> -o cyclonedx-json`'s output. Syft's CycloneDX
    /// JSON is large; only the fields this SBOM struct actually models are
    /// read, via `serde_json::Value` navigation rather than a full
    /// `cyclonedx` schema crate (not a dependency here).
    fn parse_syft_cyclonedx(raw: &str) -> Result<SbomDocument> {
        let doc: Value = serde_json::from_str(raw).context("syft output is not valid JSON")?;

        let components: Vec<SbomComponent> = doc["components"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|c| {
                let name = c["name"].as_str().unwrap_or("unknown").to_string();
                let version = c["version"].as_str().unwrap_or("unknown").to_string();
                let purl = c["purl"].as_str().unwrap_or_default().to_string();
                let supplier = c["supplier"]["name"]
                    .as_str()
                    .or_else(|| c["publisher"].as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let license = c["licenses"]
                    .as_array()
                    .and_then(|licenses| licenses.first())
                    .and_then(|l| {
                        l["license"]["id"]
                            .as_str()
                            .or_else(|| l["license"]["name"].as_str())
                            .or_else(|| l["expression"].as_str())
                    })
                    .unwrap_or("unknown")
                    .to_string();
                let hash = c["hashes"]
                    .as_array()
                    .and_then(|hashes| hashes.iter().find(|h| h["alg"] == "SHA-256"))
                    .and_then(|h| h["content"].as_str())
                    .map(String::from)
                    .unwrap_or_else(|| {
                        let mut hasher = Sha256::new();
                        hasher.update(format!("{name}:{version}"));
                        hex::encode(hasher.finalize())
                    });

                SbomComponent {
                    name,
                    version,
                    supplier,
                    license,
                    purl,
                    hash,
                }
            })
            .collect();

        Ok(SbomDocument {
            format: SbomFormat::CycloneDx,
            spec_version: doc["specVersion"].as_str().unwrap_or("1.5").to_string(),
            document_id: doc["serialNumber"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            version: doc["version"]
                .as_u64()
                .map(|v| v.to_string())
                .unwrap_or_else(|| "1.0".to_string()),
            created: doc["metadata"]["timestamp"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| Utc::now().to_rfc3339()),
            components,
            dependencies_graph: HashMap::new(),
            generated_by: "syft".to_string(),
        })
    }

    pub async fn generate_spdx_sbom(
        &self,
        image_root: &Path,
        dependencies: &[Dependency],
    ) -> Result<SbomDocument> {
        match Self::run_syft(image_root, "spdx-json").await {
            Ok(Some(raw)) => match Self::parse_syft_cyclonedx(&raw) {
                Ok(mut doc) => {
                    doc.format = SbomFormat::Spdx;
                    doc.spec_version = "SPDX-2.3".to_string();
                    return Ok(doc);
                }
                Err(e) => warn!(
                    "failed to parse syft SPDX output ({e:#}); falling back to heuristic SBOM"
                ),
            },
            Ok(None) => info!("syft not found on PATH; falling back to heuristic SBOM generation"),
            Err(e) => {
                warn!("syft invocation failed ({e:#}); falling back to heuristic SBOM generation")
            }
        }
        let mut doc = Self::generate_sbom_heuristic(dependencies);
        doc.format = SbomFormat::Spdx;
        doc.spec_version = "SPDX-2.3".to_string();
        Ok(doc)
    }

    /// Runs `syft dir:<image_root> -o <output_format> -q` and returns its
    /// stdout, or `Ok(None)` if `syft` isn't installed (checked via `which`
    /// first so a missing binary isn't reported as an invocation error).
    async fn run_syft(image_root: &Path, output_format: &str) -> Result<Option<String>> {
        if which::which("syft").is_err() {
            return Ok(None);
        }
        let target = format!("dir:{}", image_root.display());
        let output = tokio::process::Command::new("syft")
            .arg(&target)
            .arg("-o")
            .arg(output_format)
            .arg("-q")
            .output()
            .await
            .context("failed to spawn `syft`")?;
        if !output.status.success() {
            anyhow::bail!(
                "syft exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
    }

    /// Runs a real CVE scan via `grype` against the same on-disk target
    /// `syft` scanned, falling back to the pre-release-version-string
    /// heuristic when `grype` isn't on `PATH` or fails.
    async fn scan_cves(&self, image_root: &Path, sbom: &SbomDocument) -> Result<CveScanResult> {
        match Self::run_grype(image_root).await {
            Ok(Some(raw)) => match Self::parse_grype_json(&raw, sbom.components.len()) {
                Ok(scan) => return Ok(scan),
                Err(e) => warn!(
                    "failed to parse grype output ({e:#}); falling back to heuristic CVE scan"
                ),
            },
            Ok(None) => info!("grype not found on PATH; falling back to heuristic CVE scan"),
            Err(e) => warn!("grype invocation failed ({e:#}); falling back to heuristic CVE scan"),
        }
        Ok(Self::scan_cves_heuristic(sbom))
    }

    fn scan_cves_heuristic(sbom: &SbomDocument) -> CveScanResult {
        let mut findings = Vec::new();
        let mut medium: usize = 0;

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

        CveScanResult {
            total_dependencies: sbom.components.len(),
            vulnerabilities_found,
            critical: 0,
            high: 0,
            medium,
            low: 0,
            findings,
            scanned_by: "heuristic".to_string(),
        }
    }

    /// Parses `grype dir:<target> -o json`'s output — a top-level
    /// `"matches"` array of `{vulnerability, artifact}` pairs.
    fn parse_grype_json(raw: &str, total_dependencies: usize) -> Result<CveScanResult> {
        let doc: Value = serde_json::from_str(raw).context("grype output is not valid JSON")?;

        let mut critical = 0;
        let mut high = 0;
        let mut medium = 0;
        let mut low = 0;

        let findings: Vec<CveFinding> = doc["matches"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                let vuln = &m["vulnerability"];
                let severity = match vuln["severity"].as_str().unwrap_or("Unknown") {
                    "Critical" => {
                        critical += 1;
                        CveSeverity::Critical
                    }
                    "High" => {
                        high += 1;
                        CveSeverity::High
                    }
                    "Medium" => {
                        medium += 1;
                        CveSeverity::Medium
                    }
                    "Low" | "Negligible" => {
                        low += 1;
                        CveSeverity::Low
                    }
                    _ => CveSeverity::Unknown,
                };
                let cvss_score = vuln["cvss"]
                    .as_array()
                    .and_then(|scores| scores.first())
                    .and_then(|s| s["metrics"]["baseScore"].as_f64());

                CveFinding {
                    id: vuln["id"].as_str().unwrap_or("unknown").to_string(),
                    severity,
                    package: m["artifact"]["name"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    version: m["artifact"]["version"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    fixed_in: vuln["fix"]["versions"]
                        .as_array()
                        .and_then(|v| v.first())
                        .and_then(|v| v.as_str())
                        .map(String::from),
                    description: vuln["description"].as_str().unwrap_or_default().to_string(),
                    cvss_score,
                }
            })
            .collect();

        Ok(CveScanResult {
            total_dependencies,
            vulnerabilities_found: findings.len(),
            critical,
            high,
            medium,
            low,
            findings,
            scanned_by: "grype".to_string(),
        })
    }

    /// Runs `grype dir:<image_root> -o json -q` and returns its stdout, or
    /// `Ok(None)` if `grype` isn't installed.
    async fn run_grype(image_root: &Path) -> Result<Option<String>> {
        if which::which("grype").is_err() {
            return Ok(None);
        }
        let target = format!("dir:{}", image_root.display());
        let output = tokio::process::Command::new("grype")
            .arg(&target)
            .arg("-o")
            .arg("json")
            .arg("-q")
            .output()
            .await
            .context("failed to spawn `grype`")?;
        if !output.status.success() {
            anyhow::bail!(
                "grype exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
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

        if let Some(sig) = &distilled.wasm_signature {
            lines.push(format!(
                "LABEL org.opencontainers.image.wasm.signature=\"{}\"",
                sig.signature
            ));
            lines.push(format!(
                "LABEL org.opencontainers.image.wasm.pubkey=\"{}\"",
                sig.public_key
            ));
        }

        lines.join("\n")
    }
}

impl Default for Distiller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod scan_tool_parsing_tests {
    use super::*;

    #[test]
    fn parses_real_syft_cyclonedx_output() {
        let raw = r#"{
            "specVersion": "1.5",
            "serialNumber": "urn:uuid:1234",
            "version": 1,
            "metadata": { "timestamp": "2026-01-01T00:00:00Z" },
            "components": [
                {
                    "name": "openssl",
                    "version": "3.0.2",
                    "purl": "pkg:deb/debian/openssl@3.0.2",
                    "supplier": { "name": "Debian" },
                    "licenses": [ { "license": { "id": "Apache-2.0" } } ],
                    "hashes": [ { "alg": "SHA-256", "content": "deadbeef" } ]
                }
            ]
        }"#;

        let doc = Distiller::parse_syft_cyclonedx(raw).expect("should parse");
        assert_eq!(doc.generated_by, "syft");
        assert_eq!(doc.spec_version, "1.5");
        assert_eq!(doc.components.len(), 1);
        let c = &doc.components[0];
        assert_eq!(c.name, "openssl");
        assert_eq!(c.version, "3.0.2");
        assert_eq!(c.license, "Apache-2.0");
        assert_eq!(c.hash, "deadbeef");
    }

    #[test]
    fn parses_real_grype_json_output_and_counts_severities() {
        let raw = r#"{
            "matches": [
                {
                    "vulnerability": {
                        "id": "CVE-2023-0001",
                        "severity": "Critical",
                        "description": "a bad bug",
                        "fix": { "versions": ["3.0.3"] },
                        "cvss": [ { "metrics": { "baseScore": 9.8 } } ]
                    },
                    "artifact": { "name": "openssl", "version": "3.0.2" }
                },
                {
                    "vulnerability": {
                        "id": "CVE-2023-0002",
                        "severity": "Low",
                        "description": "a minor bug"
                    },
                    "artifact": { "name": "zlib", "version": "1.2.11" }
                }
            ]
        }"#;

        let scan = Distiller::parse_grype_json(raw, 2).expect("should parse");
        assert_eq!(scan.scanned_by, "grype");
        assert_eq!(scan.total_dependencies, 2);
        assert_eq!(scan.vulnerabilities_found, 2);
        assert_eq!(scan.critical, 1);
        assert_eq!(scan.low, 1);
        assert_eq!(scan.findings[0].id, "CVE-2023-0001");
        assert_eq!(scan.findings[0].fixed_in, Some("3.0.3".to_string()));
        assert_eq!(scan.findings[0].cvss_score, Some(9.8));
    }

    #[test]
    fn heuristic_fallback_still_flags_prerelease_versions() {
        let deps = vec![Dependency {
            name: "libfoo".to_string(),
            version: "1.0.0-alpha".to_string(),
            source: "apt".to_string(),
            license: "MIT".to_string(),
            size_bytes: 100,
        }];
        let sbom = Distiller::generate_sbom_heuristic(&deps);
        assert_eq!(sbom.generated_by, "heuristic");
        let scan = Distiller::scan_cves_heuristic(&sbom);
        assert_eq!(scan.scanned_by, "heuristic");
        assert_eq!(scan.vulnerabilities_found, 1);
        assert_eq!(scan.medium, 1);
    }
}
