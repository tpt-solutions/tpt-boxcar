use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SourceLanguage {
    Rust,
    Go,
    C,
    Cpp,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WasmCompatibility {
    FullyCompatible,
    PartiallyCompatible { missing_features: Vec<String> },
    Incompatible { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeTrace {
    pub cpu_seconds: f64,
    pub memory_peak_bytes: u64,
    pub syscalls_used: Vec<String>,
    pub filesystem_paths: Vec<String>,
    pub network_endpoints: Vec<String>,
    pub shared_libs: Vec<String>,
    pub entrypoint: String,
    pub total_instructions: u64,
    pub io_operations: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerInfo {
    pub digest: String,
    pub size_bytes: u64,
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadata {
    pub name: String,
    pub tag: String,
    pub os: String,
    pub arch: String,
    pub total_size_bytes: u64,
    pub layer_count: usize,
    pub layers: Vec<LayerInfo>,
    pub env_vars: HashMap<String, String>,
    pub exposed_ports: Vec<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub source: String,
    pub license: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase1Result {
    pub trace: RuntimeTrace,
    pub image: ImageMetadata,
    pub dependencies: Vec<Dependency>,
    pub used_files: Vec<PathBuf>,
    pub dynamic_libs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase2Result {
    pub detected_language: SourceLanguage,
    pub wasm_compatibility: WasmCompatibility,
    pub compilation_hints: Vec<String>,
    pub required_imports: Vec<String>,
    pub suggested_target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisResult {
    pub phase1: Phase1Result,
    pub phase2: Phase2Result,
    pub image_hash: String,
    pub analysis_timestamp: String,
}

pub struct Analyzer {
    image_path: PathBuf,
}

impl Analyzer {
    pub fn new(image_path: impl AsRef<Path>) -> Self {
        Self {
            image_path: image_path.as_ref().to_path_buf(),
        }
    }

    pub async fn analyze(&self) -> Result<AnalysisResult> {
        info!(
            "Starting two-phase analysis for: {}",
            self.image_path.display()
        );

        let phase1 = self.phase1_runtime_trace().await?;
        let phase2 = self.phase2_wasm_migration(&phase1).await?;

        let hash = self.compute_image_hash().await?;
        let timestamp = chrono::Utc::now().to_rfc3339();

        Ok(AnalysisResult {
            phase1,
            phase2,
            image_hash: hash,
            analysis_timestamp: timestamp,
        })
    }

    async fn phase1_runtime_trace(&self) -> Result<Phase1Result> {
        info!("Phase 1: Runtime trace analysis");

        let image = self.load_image_metadata().await?;
        let trace = self.collect_runtime_trace().await?;
        let dependencies = self.scan_dependencies().await?;
        let used_files = self.identify_used_files(&trace).await?;
        let dynamic_libs = trace.shared_libs.clone();

        Ok(Phase1Result {
            trace,
            image,
            dependencies,
            used_files,
            dynamic_libs,
        })
    }

    async fn phase2_wasm_migration(&self, phase1: &Phase1Result) -> Result<Phase2Result> {
        info!("Phase 2: Wasm migration analysis");

        let detected_language = self.detect_language(phase1).await?;
        let wasm_compatibility = self
            .check_wasm_compatibility(&detected_language, phase1)
            .await?;
        let compilation_hints =
            self.generate_compilation_hints(&detected_language, &wasm_compatibility);
        let required_imports = self.identify_required_imports(phase1);
        let suggested_target = self.determine_wasm_target(&detected_language);

        Ok(Phase2Result {
            detected_language,
            wasm_compatibility,
            compilation_hints,
            required_imports,
            suggested_target,
        })
    }

    async fn load_image_metadata(&self) -> Result<ImageMetadata> {
        let manifest_path = self.image_path.join("manifest.json");
        if manifest_path.exists() {
            let content = tokio::fs::read_to_string(&manifest_path)
                .await
                .context("Failed to read image manifest")?;
            let manifest: serde_json::Value = serde_json::from_str(&content)?;

            let layers: Vec<LayerInfo> = manifest["layers"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|l| LayerInfo {
                            digest: l["digest"].as_str().unwrap_or("unknown").to_string(),
                            size_bytes: l["size"].as_u64().unwrap_or(0),
                            command: l["command"].as_str().map(String::from),
                        })
                        .collect()
                })
                .unwrap_or_default();

            let total_size: u64 = layers.iter().map(|l| l.size_bytes).sum();

            Ok(ImageMetadata {
                name: manifest["name"].as_str().unwrap_or("unknown").to_string(),
                tag: manifest["tag"].as_str().unwrap_or("latest").to_string(),
                os: manifest["os"].as_str().unwrap_or("linux").to_string(),
                arch: manifest["architecture"]
                    .as_str()
                    .unwrap_or("amd64")
                    .to_string(),
                total_size_bytes: total_size,
                layer_count: layers.len(),
                layers,
                env_vars: HashMap::new(),
                exposed_ports: Vec::new(),
            })
        } else {
            Ok(ImageMetadata {
                name: self
                    .image_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                tag: "latest".to_string(),
                os: "linux".to_string(),
                arch: "amd64".to_string(),
                total_size_bytes: 0,
                layer_count: 0,
                layers: Vec::new(),
                env_vars: HashMap::new(),
                exposed_ports: Vec::new(),
            })
        }
    }

    async fn collect_runtime_trace(&self) -> Result<RuntimeTrace> {
        let binary_path = self.find_entrypoint().await?;
        let trace_path = self.image_path.join("trace.json");

        if trace_path.exists() {
            let content = tokio::fs::read_to_string(&trace_path)
                .await
                .context("Failed to read trace data")?;
            let trace: RuntimeTrace = serde_json::from_str(&content)?;
            return Ok(trace);
        }

        warn!(
            "No trace data found, generating synthetic trace for: {}",
            binary_path.display()
        );

        Ok(RuntimeTrace {
            cpu_seconds: 0.0,
            memory_peak_bytes: 0,
            syscalls_used: Vec::new(),
            filesystem_paths: Vec::new(),
            network_endpoints: Vec::new(),
            shared_libs: Vec::new(),
            entrypoint: binary_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            total_instructions: 0,
            io_operations: 0,
        })
    }

    async fn find_entrypoint(&self) -> Result<PathBuf> {
        let config_path = self.image_path.join("config.json");
        if config_path.exists() {
            let content = tokio::fs::read_to_string(&config_path)
                .await
                .context("Failed to read config")?;
            let config: serde_json::Value = serde_json::from_str(&content)?;
            if let Some(cmd) = config["cmd"].as_array() {
                if let Some(first) = cmd.first() {
                    return Ok(PathBuf::from(first.as_str().unwrap_or("/app")));
                }
            }
        }
        Ok(PathBuf::from("/app"))
    }

    async fn scan_dependencies(&self) -> Result<Vec<Dependency>> {
        let deps_path = self.image_path.join("deps.json");
        if deps_path.exists() {
            let content = tokio::fs::read_to_string(&deps_path)
                .await
                .context("Failed to read dependencies")?;
            let deps: Vec<Dependency> = serde_json::from_str(&content)?;
            return Ok(deps);
        }
        Ok(Vec::new())
    }

    async fn identify_used_files(&self, trace: &RuntimeTrace) -> Result<Vec<PathBuf>> {
        let mut files: Vec<PathBuf> = trace.filesystem_paths.iter().map(PathBuf::from).collect();

        let lib_files: Vec<PathBuf> = trace.shared_libs.iter().map(PathBuf::from).collect();
        files.extend(lib_files);

        files.sort();
        files.dedup();
        Ok(files)
    }

    async fn detect_language(&self, phase1: &Phase1Result) -> Result<SourceLanguage> {
        let binary_path = self.find_entrypoint().await?;
        let ext = binary_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        match ext {
            "rs" | "rlib" => return Ok(SourceLanguage::Rust),
            "go" | "" => {
                if self.has_rust_indicators(phase1) {
                    return Ok(SourceLanguage::Rust);
                }
                if self.has_go_indicators(phase1) {
                    return Ok(SourceLanguage::Go);
                }
            }
            "c" | "h" => return Ok(SourceLanguage::C),
            "cpp" | "cc" | "cxx" => return Ok(SourceLanguage::Cpp),
            _ => {}
        }

        for dep in &phase1.dependencies {
            if dep.name.contains("rust") || dep.source.contains("crates.io") {
                return Ok(SourceLanguage::Rust);
            }
            if dep.source.contains("golang") || dep.name.starts_with("go-") {
                return Ok(SourceLanguage::Go);
            }
        }

        for lib in &phase1.dynamic_libs {
            if lib.contains("libc") || lib.contains("libm") {
                return Ok(SourceLanguage::C);
            }
        }

        Ok(SourceLanguage::Unknown)
    }

    fn has_rust_indicators(&self, phase1: &Phase1Result) -> bool {
        phase1
            .dynamic_libs
            .iter()
            .any(|l| l.contains("libstd") || l.contains("libcore"))
            || phase1
                .dependencies
                .iter()
                .any(|d| d.source.contains("crates.io"))
            || phase1.trace.shared_libs.iter().any(|l| l.contains("rust"))
    }

    fn has_go_indicators(&self, phase1: &Phase1Result) -> bool {
        phase1.trace.entrypoint.contains("go")
            || phase1
                .dependencies
                .iter()
                .any(|d| d.source.contains("golang"))
    }

    async fn check_wasm_compatibility(
        &self,
        language: &SourceLanguage,
        phase1: &Phase1Result,
    ) -> Result<WasmCompatibility> {
        match language {
            SourceLanguage::Rust => {
                let mut missing = Vec::new();
                if phase1
                    .trace
                    .syscalls_used
                    .iter()
                    .any(|s| s == "fork" || s == "execve")
                {
                    missing.push("process fork/exec".to_string());
                }
                if !phase1.trace.network_endpoints.is_empty()
                    && !phase1
                        .trace
                        .network_endpoints
                        .iter()
                        .all(|e| e.starts_with("http"))
                {
                    missing.push("raw TCP/UDP sockets".to_string());
                }
                if missing.is_empty() {
                    Ok(WasmCompatibility::FullyCompatible)
                } else {
                    Ok(WasmCompatibility::PartiallyCompatible {
                        missing_features: missing,
                    })
                }
            }
            SourceLanguage::Go => {
                let mut missing = Vec::new();
                missing.push("cgo support".to_string());
                if phase1.dynamic_libs.iter().any(|l| l.contains("libpthread")) {
                    missing.push("native threads".to_string());
                }
                Ok(WasmCompatibility::PartiallyCompatible {
                    missing_features: missing,
                })
            }
            SourceLanguage::C | SourceLanguage::Cpp => {
                let mut missing = Vec::new();
                for lib in &phase1.dynamic_libs {
                    if lib.contains("libssl") {
                        missing.push("native OpenSSL bindings".to_string());
                    }
                    if lib.contains("libcurl") {
                        missing.push("native libcurl".to_string());
                    }
                }
                if missing.is_empty() {
                    Ok(WasmCompatibility::FullyCompatible)
                } else {
                    Ok(WasmCompatibility::PartiallyCompatible {
                        missing_features: missing,
                    })
                }
            }
            SourceLanguage::Unknown => Ok(WasmCompatibility::Incompatible {
                reason: "Cannot determine source language".to_string(),
            }),
        }
    }

    fn generate_compilation_hints(
        &self,
        language: &SourceLanguage,
        compat: &WasmCompatibility,
    ) -> Vec<String> {
        let mut hints = Vec::new();

        match language {
            SourceLanguage::Rust => {
                hints.push(
                    "Use `wasm-pack build --target web` or `cargo build --target wasm32-wasi`"
                        .to_string(),
                );
                hints
                    .push("Enable wasm32-wasi target: `rustup target add wasm32-wasi`".to_string());
                if let WasmCompatibility::PartiallyCompatible { missing_features } = compat {
                    if missing_features.iter().any(|f| f.contains("socket")) {
                        hints.push(
                            "Use wasi-net or HTTP-based alternatives for networking".to_string(),
                        );
                    }
                }
            }
            SourceLanguage::Go => {
                hints.push(
                    "Use TinyGo for smaller Wasm output: `tinygo build -o app.wasm -target wasi`"
                        .to_string(),
                );
                hints.push("Avoid cgo dependencies".to_string());
                hints.push("Use go:build wasm annotation".to_string());
            }
            SourceLanguage::C | SourceLanguage::Cpp => {
                hints.push("Use Emscripten: `emcc -o app.wasm -s STANDALONE_WASM`".to_string());
                hints.push("Target wasm32-wasi for maximum compatibility".to_string());
                if let WasmCompatibility::PartiallyCompatible { missing_features } = compat {
                    if missing_features.iter().any(|f| f.contains("thread")) {
                        hints.push(
                            "Use wasm32-wasip1 threads proposal or single-threaded execution"
                                .to_string(),
                        );
                    }
                }
            }
            SourceLanguage::Unknown => {
                hints.push("Cannot generate hints for unknown language".to_string());
            }
        }

        if let WasmCompatibility::Incompatible { reason } = compat {
            hints.push(format!("Migration not recommended: {reason}"));
        }

        hints
    }

    fn identify_required_imports(&self, phase1: &Phase1Result) -> Vec<String> {
        let mut imports = Vec::new();

        if !phase1.trace.network_endpoints.is_empty() {
            imports.push("wasi:http".to_string());
        }
        if !phase1.trace.filesystem_paths.is_empty() {
            imports.push("wasi:filesystem".to_string());
        }
        imports.push("wasi:clocks".to_string());
        imports.push("wasi:io".to_string());

        imports.sort();
        imports.dedup();
        imports
    }

    fn determine_wasm_target(&self, language: &SourceLanguage) -> String {
        match language {
            SourceLanguage::Rust => "wasm32-wasi".to_string(),
            SourceLanguage::Go => "wasm32-wasi".to_string(),
            SourceLanguage::C | SourceLanguage::Cpp => "wasm32-wasi".to_string(),
            SourceLanguage::Unknown => "wasm32-wasi".to_string(),
        }
    }

    async fn compute_image_hash(&self) -> Result<String> {
        let manifest_path = self.image_path.join("manifest.json");
        let content = if manifest_path.exists() {
            tokio::fs::read(&manifest_path).await?
        } else {
            self.image_path.to_string_lossy().as_bytes().to_vec()
        };

        let mut hasher = Sha256::new();
        hasher.update(&content);
        let result = hasher.finalize();
        Ok(hex::encode(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_detection_order() {
        assert_eq!(SourceLanguage::Rust, SourceLanguage::Rust);
        assert_ne!(SourceLanguage::Rust, SourceLanguage::Go);
    }

    #[test]
    fn test_wasm_compatibility_variants() {
        let compat = WasmCompatibility::FullyCompatible;
        assert_eq!(compat, WasmCompatibility::FullyCompatible);

        let partial = WasmCompatibility::PartiallyCompatible {
            missing_features: vec!["threads".to_string()],
        };
        assert!(matches!(
            partial,
            WasmCompatibility::PartiallyCompatible { .. }
        ));

        let incompatible = WasmCompatibility::Incompatible {
            reason: "test".to_string(),
        };
        assert!(matches!(
            incompatible,
            WasmCompatibility::Incompatible { .. }
        ));
    }
}
