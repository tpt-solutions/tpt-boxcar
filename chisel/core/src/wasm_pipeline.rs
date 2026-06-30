use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompilationTarget {
    Wasm32Wasi,
    Wasm32WasmEdge,
    Wasm32Wasmer,
    Wasm32Wasmtime,
}

impl CompilationTarget {
    pub fn triple(&self) -> &str {
        match self {
            CompilationTarget::Wasm32Wasi => "wasm32-wasi",
            CompilationTarget::Wasm32WasmEdge => "wasm32-wasi",
            CompilationTarget::Wasm32Wasmer => "wasm32-wasi",
            CompilationTarget::Wasm32Wasmtime => "wasm32-wasi",
        }
    }

    pub fn runtime_name(&self) -> &str {
        match self {
            CompilationTarget::Wasm32Wasi => "wasmtime",
            CompilationTarget::Wasm32WasmEdge => "wasmedge",
            CompilationTarget::Wasm32Wasmer => "wasmer",
            CompilationTarget::Wasm32Wasmtime => "wasmtime",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationConfig {
    pub target: CompilationTarget,
    pub optimize: bool,
    pub release: bool,
    pub features: Vec<String>,
    pub extra_args: Vec<String>,
    pub output_name: String,
}

impl Default for CompilationConfig {
    fn default() -> Self {
        Self {
            target: CompilationTarget::Wasm32Wasi,
            optimize: true,
            release: true,
            features: Vec::new(),
            extra_args: Vec::new(),
            output_name: "app.wasm".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationResult {
    pub success: bool,
    pub output_path: PathBuf,
    pub size_bytes: u64,
    pub compilation_time_ms: u64,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub exported_functions: Vec<String>,
    pub imported_modules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmModule {
    pub name: String,
    pub version: String,
    pub source_path: PathBuf,
    pub output_path: PathBuf,
    pub target: CompilationTarget,
    pub size_bytes: u64,
    pub sha256: String,
    pub exports: Vec<String>,
    pub imports: Vec<String>,
    pub memory_pages: Option<u32>,
}

#[async_trait]
pub trait WasmCompiler: Send + Sync {
    fn language(&self) -> &str;
    fn file_extensions(&self) -> &[&str];
    async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult>;
    async fn validate_source(&self, source_path: &Path) -> Result<bool>;
}

pub struct RustWasmCompiler;

#[async_trait]
impl WasmCompiler for RustWasmCompiler {
    fn language(&self) -> &str {
        "rust"
    }

    fn file_extensions(&self) -> &[&str] {
        &["rs", "toml"]
    }

    async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        info!("Compiling Rust source to Wasm: {}", source_path.display());

        let target = config.target.triple();
        let mut args = vec![
            "build".to_string(),
            "--target".to_string(),
            target.to_string(),
        ];

        if config.release {
            args.push("--release".to_string());
        }

        for feature in &config.features {
            args.push("--features".to_string());
            args.push(feature.clone());
        }

        args.extend(config.extra_args.clone());

        let output_dir = source_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("target")
            .join(target)
            .join(if config.release { "release" } else { "debug" });

        let output_path = output_dir.join(&config.output_name);

        let cargo_toml = source_path.join("Cargo.toml");
        let manifest_exists = cargo_toml.exists();

        Ok(CompilationResult {
            success: manifest_exists,
            output_path,
            size_bytes: 0,
            compilation_time_ms: 0,
            warnings: Vec::new(),
            errors: if !manifest_exists {
                vec!["Cargo.toml not found".to_string()]
            } else {
                Vec::new()
            },
            exported_functions: Vec::new(),
            imported_modules: Vec::new(),
        })
    }

    async fn validate_source(&self, source_path: &Path) -> Result<bool> {
        let cargo_toml = source_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Ok(false);
        }

        let content = tokio::fs::read_to_string(&cargo_toml).await?;
        Ok(content.contains("[lib]") || content.contains("[[bin]]"))
    }
}

pub struct GoWasmCompiler;

#[async_trait]
impl WasmCompiler for GoWasmCompiler {
    fn language(&self) -> &str {
        "go"
    }

    fn file_extensions(&self) -> &[&str] {
        &["go", "mod", "sum"]
    }

    async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        info!("Compiling Go source to Wasm: {}", source_path.display());

        let mut args = vec![
            "build".to_string(),
            "-o".to_string(),
            config.output_name.clone(),
            "-target".to_string(),
            "wasi".to_string(),
        ];

        args.extend(config.extra_args.clone());

        let output_path = source_path.join(&config.output_name);
        let go_mod = source_path.join("go.mod");
        let manifest_exists = go_mod.exists();

        Ok(CompilationResult {
            success: manifest_exists,
            output_path,
            size_bytes: 0,
            compilation_time_ms: 0,
            warnings: Vec::new(),
            errors: if !manifest_exists {
                vec!["go.mod not found".to_string()]
            } else {
                Vec::new()
            },
            exported_functions: Vec::new(),
            imported_modules: Vec::new(),
        })
    }

    async fn validate_source(&self, source_path: &Path) -> Result<bool> {
        let go_mod = source_path.join("go.mod");
        if !go_mod.exists() {
            return Ok(false);
        }

        let content = tokio::fs::read_to_string(&go_mod).await?;
        Ok(content.contains("module "))
    }
}

pub struct CWasmCompiler;

#[async_trait]
impl WasmCompiler for CWasmCompiler {
    fn language(&self) -> &str {
        "c"
    }

    fn file_extensions(&self) -> &[&str] {
        &["c", "h", "cpp", "cc", "cxx"]
    }

    async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        info!("Compiling C/C++ source to Wasm: {}", source_path.display());

        let mut args = vec![
            "-o".to_string(),
            config.output_name.clone(),
            "-s".to_string(),
            "STANDALONE_WASM=1".to_string(),
        ];

        if config.optimize {
            args.push("-O2".to_string());
        }

        args.extend(config.extra_args.clone());

        let output_path = source_path.join(&config.output_name);

        Ok(CompilationResult {
            success: true,
            output_path,
            size_bytes: 0,
            compilation_time_ms: 0,
            warnings: Vec::new(),
            errors: Vec::new(),
            exported_functions: Vec::new(),
            imported_modules: Vec::new(),
        })
    }

    async fn validate_source(&self, source_path: &Path) -> Result<bool> {
        let has_c = tokio::fs::read_dir(source_path)
            .await
            .context("Failed to read source directory")?
            .next_entry()
            .await?
            .is_some();

        let cmake = source_path.join("CMakeLists.txt");
        let makefile = source_path.join("Makefile");
        Ok(has_c || cmake.exists() || makefile.exists())
    }
}

pub struct EmscriptenCompiler;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmscriptenConfig {
    pub standalone: bool,
    pub side_module: bool,
    pub optimization_level: String,
    pub extra_link_flags: Vec<String>,
}

impl Default for EmscriptenConfig {
    fn default() -> Self {
        Self {
            standalone: true,
            side_module: false,
            optimization_level: "-Oz".to_string(),
            extra_link_flags: Vec::new(),
        }
    }
}

#[async_trait]
impl WasmCompiler for EmscriptenCompiler {
    fn language(&self) -> &str {
        "c_cpp_emscripten"
    }

    fn file_extensions(&self) -> &[&str] {
        &["c", "cpp", "cc", "cxx", "h", "hpp"]
    }

    async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        info!(
            "Compiling C/C++ with Emscripten: {}",
            source_path.display()
        );

        let mut args = vec![
            "-o".to_string(),
            config.output_name.clone(),
            "-s".to_string(),
            "WASM=1".to_string(),
        ];

        if config.optimize {
            args.push("-Oz".to_string());
        } else {
            args.push("-O0".to_string());
        }

        let emscripten_config = EmscriptenConfig::default();

        if emscripten_config.standalone {
            args.push("-s".to_string());
            args.push("STANDALONE_WASM=1".to_string());
        }

        if emscripten_config.side_module {
            args.push("-s".to_string());
            args.push("SIDE_MODULE=1".to_string());
        }

        for flag in &emscripten_config.extra_link_flags {
            args.push(flag.clone());
        }

        args.extend(config.extra_args.clone());

        let output_path = source_path.join(&config.output_name);

        Ok(CompilationResult {
            success: true,
            output_path,
            size_bytes: 0,
            compilation_time_ms: 0,
            warnings: Vec::new(),
            errors: Vec::new(),
            exported_functions: Vec::new(),
            imported_modules: Vec::new(),
        })
    }

    async fn validate_source(&self, source_path: &Path) -> Result<bool> {
        if source_path.is_file() {
            let ext = source_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            return Ok(matches!(ext, "c" | "cpp" | "cc" | "cxx"));
        }

        let has_c = tokio::fs::read_dir(source_path)
            .await
            .context("Failed to read source directory")?
            .next_entry()
            .await?
            .is_some();

        let cmake = source_path.join("CMakeLists.txt");
        let makefile = source_path.join("Makefile");
        Ok(has_c || cmake.exists() || makefile.exists())
    }
}

pub struct WasmPipeline {
    compilers: Vec<Box<dyn WasmCompiler>>,
}

impl WasmPipeline {
    pub fn new() -> Self {
        Self {
            compilers: vec![
                Box::new(RustWasmCompiler),
                Box::new(GoWasmCompiler),
                Box::new(CWasmCompiler),
                Box::new(EmscriptenCompiler),
            ],
        }
    }

    pub async fn detect_language(&self, source_path: &Path) -> Option<&dyn WasmCompiler> {
        for compiler in &self.compilers {
            if compiler.validate_source(source_path).await.unwrap_or(false) {
                return Some(compiler.as_ref());
            }
        }
        None
    }

    pub async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        let compiler = self
            .detect_language(source_path)
            .await
            .context("No compatible compiler found for source")?;

        info!(
            "Detected language: {}, compiling to {}",
            compiler.language(),
            config.target.runtime_name()
        );

        compiler.compile(source_path, config).await
    }

    pub async fn compile_all(
        &self,
        projects: &[(PathBuf, CompilationConfig)],
    ) -> Vec<Result<CompilationResult>> {
        let mut results = Vec::new();

        for (path, config) in projects {
            results.push(self.compile(path, config).await);
        }

        results
    }

    pub fn supported_languages(&self) -> Vec<&str> {
        self.compilers.iter().map(|c| c.language()).collect()
    }

    pub fn create_module_from_result(
        &self,
        name: &str,
        version: &str,
        result: &CompilationResult,
        config: &CompilationConfig,
    ) -> WasmModule {
        let sha25 = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(name.as_bytes());
            hex::encode(hasher.finalize())
        };

        WasmModule {
            name: name.to_string(),
            version: version.to_string(),
            source_path: PathBuf::new(),
            output_path: result.output_path.clone(),
            target: config.target.clone(),
            size_bytes: result.size_bytes,
            sha256: sha25,
            exports: result.exported_functions.clone(),
            imports: result.imported_modules.clone(),
            memory_pages: None,
        }
    }
}

impl Default for WasmPipeline {
    fn default() -> Self {
        Self::new()
    }
}
