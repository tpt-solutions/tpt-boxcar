use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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
        // Rust renamed the `wasm32-wasi` target to `wasm32-wasip1`; all
        // four logical targets currently compile through the same rustc
        // target since WasmEdge/Wasmer/Wasmtime all accept WASI Preview 1
        // modules.
        match self {
            CompilationTarget::Wasm32Wasi => "wasm32-wasip1",
            CompilationTarget::Wasm32WasmEdge => "wasm32-wasip1",
            CompilationTarget::Wasm32Wasmer => "wasm32-wasip1",
            CompilationTarget::Wasm32Wasmtime => "wasm32-wasip1",
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
    pub signature: Option<WasmModuleSignature>,
}

/// An ed25519 signature over a module's compiled wasm bytes, produced by
/// `WasmPipeline::sign_module` at build time and checked by Origin at load
/// time. There's no PKI/KMS infrastructure elsewhere in the repo, so this is
/// a self-contained BYO-key scheme rather than a full Sigstore integration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmModuleSignature {
    pub signature: String,
    pub public_key: String,
    pub sha256: String,
}

/// Enum-based compiler to avoid dyn compatibility issues with async fn in trait
#[derive(Debug, Clone)]
pub enum WasmCompiler {
    Rust(RustWasmCompiler),
    Go(GoWasmCompiler),
    C(CWasmCompiler),
    Emscripten(EmscriptenCompiler),
}

impl WasmCompiler {
    pub fn language(&self) -> &str {
        match self {
            WasmCompiler::Rust(c) => c.language(),
            WasmCompiler::Go(c) => c.language(),
            WasmCompiler::C(c) => c.language(),
            WasmCompiler::Emscripten(c) => c.language(),
        }
    }

    pub fn file_extensions(&self) -> &[&str] {
        match self {
            WasmCompiler::Rust(c) => c.file_extensions(),
            WasmCompiler::Go(c) => c.file_extensions(),
            WasmCompiler::C(c) => c.file_extensions(),
            WasmCompiler::Emscripten(c) => c.file_extensions(),
        }
    }

    pub async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        match self {
            WasmCompiler::Rust(c) => c.compile(source_path, config).await,
            WasmCompiler::Go(c) => c.compile(source_path, config).await,
            WasmCompiler::C(c) => c.compile(source_path, config).await,
            WasmCompiler::Emscripten(c) => c.compile(source_path, config).await,
        }
    }

    pub async fn validate_source(&self, source_path: &Path) -> Result<bool> {
        match self {
            WasmCompiler::Rust(c) => c.validate_source(source_path).await,
            WasmCompiler::Go(c) => c.validate_source(source_path).await,
            WasmCompiler::C(c) => c.validate_source(source_path).await,
            WasmCompiler::Emscripten(c) => c.validate_source(source_path).await,
        }
    }
}

/// Runs `cargo metadata --no-deps` in `dir` and returns the parsed JSON, used
/// by both `RustWasmCompiler` and `parse_wasm_exports_imports`'s caller to
/// find the real `target_directory` and binary target name rather than
/// guessing a path (guessing breaks for workspace members, which share a
/// target dir at the workspace root rather than under the crate itself).
async fn cargo_metadata(dir: &Path) -> Result<serde_json::Value> {
    let output = tokio::process::Command::new("cargo")
        .args(["metadata", "--format-version=1", "--no-deps"])
        .current_dir(dir)
        .output()
        .await
        .context("failed to spawn `cargo metadata`")?;
    anyhow::ensure!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).context("failed to parse cargo metadata JSON")
}

/// Locates the real `[[bin]]` target name for `dir`'s package, needed
/// because Cargo uses the bin target name (not necessarily the package
/// name) as the compiled artifact's filename.
fn find_bin_target_name(metadata: &serde_json::Value) -> Result<String> {
    let packages = metadata["packages"]
        .as_array()
        .context("cargo metadata missing 'packages'")?;
    let package = packages
        .first()
        .context("cargo metadata returned no packages for this directory")?;
    let targets = package["targets"]
        .as_array()
        .context("package has no 'targets'")?;
    targets
        .iter()
        .find(|t| {
            t["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|k| k == "bin"))
        })
        .and_then(|t| t["name"].as_str())
        .map(|s| s.to_string())
        .context("no [[bin]] target found — chisel only compiles binary crates to wasm today")
}

/// Real introspection of a compiled `.wasm` module's export/import sections
/// via `wasmparser`, replacing what used to be hardcoded empty vectors.
async fn parse_wasm_exports_imports(path: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let bytes = tokio::fs::read(path)
        .await
        .with_context(|| format!("failed to read compiled wasm module: {}", path.display()))?;

    let mut exported_functions = Vec::new();
    let mut imported_modules = Vec::new();

    for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
        match payload.context("failed to parse wasm module structure")? {
            wasmparser::Payload::ExportSection(reader) => {
                for export in reader {
                    exported_functions
                        .push(export.context("malformed export entry")?.name.to_string());
                }
            }
            wasmparser::Payload::ImportSection(reader) => {
                for import in reader {
                    imported_modules
                        .push(import.context("malformed import entry")?.module.to_string());
                }
            }
            _ => {}
        }
    }

    imported_modules.sort();
    imported_modules.dedup();
    Ok((exported_functions, imported_modules))
}

#[derive(Debug, Clone)]
pub struct RustWasmCompiler;

impl RustWasmCompiler {
    pub fn language(&self) -> &str {
        "rust"
    }

    pub fn file_extensions(&self) -> &[&str] {
        &["rs", "toml"]
    }

    pub async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        info!("Compiling Rust source to Wasm: {}", source_path.display());

        let cargo_toml = source_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Ok(CompilationResult {
                success: false,
                output_path: PathBuf::new(),
                size_bytes: 0,
                compilation_time_ms: 0,
                warnings: Vec::new(),
                errors: vec!["Cargo.toml not found".to_string()],
                exported_functions: Vec::new(),
                imported_modules: Vec::new(),
            });
        }

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

        let start = std::time::Instant::now();
        let output = tokio::process::Command::new("cargo")
            .args(&args)
            .current_dir(source_path)
            .output()
            .await
            .context("failed to spawn `cargo build`")?;
        let compilation_time_ms = start.elapsed().as_millis() as u64;

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let warnings: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("warning:"))
            .map(String::from)
            .collect();
        let errors: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("error"))
            .map(String::from)
            .collect();

        if !output.status.success() {
            return Ok(CompilationResult {
                success: false,
                output_path: PathBuf::new(),
                size_bytes: 0,
                compilation_time_ms,
                warnings,
                errors,
                exported_functions: Vec::new(),
                imported_modules: Vec::new(),
            });
        }

        let metadata = cargo_metadata(source_path).await?;
        let target_directory = metadata["target_directory"]
            .as_str()
            .context("cargo metadata missing 'target_directory'")?;
        let bin_name = find_bin_target_name(&metadata)?;
        let profile = if config.release { "release" } else { "debug" };
        let output_path = PathBuf::from(target_directory)
            .join(target)
            .join(profile)
            .join(format!("{bin_name}.wasm"));

        anyhow::ensure!(
            output_path.exists(),
            "cargo build succeeded but no .wasm binary was found at {}",
            output_path.display()
        );
        let size_bytes = tokio::fs::metadata(&output_path).await?.len();
        let (exported_functions, imported_modules) =
            parse_wasm_exports_imports(&output_path).await?;

        Ok(CompilationResult {
            success: true,
            output_path,
            size_bytes,
            compilation_time_ms,
            warnings,
            errors,
            exported_functions,
            imported_modules,
        })
    }

    pub async fn validate_source(&self, source_path: &Path) -> Result<bool> {
        let cargo_toml = source_path.join("Cargo.toml");
        if !cargo_toml.exists() {
            return Ok(false);
        }

        let content = tokio::fs::read_to_string(&cargo_toml).await?;
        // Cargo doesn't require an explicit [lib]/[[bin]] section — a
        // default binary crate is just `src/main.rs`, a default lib crate
        // is just `src/lib.rs`.
        Ok(content.contains("[lib]")
            || content.contains("[[bin]]")
            || source_path.join("src/main.rs").exists()
            || source_path.join("src/lib.rs").exists())
    }
}

#[derive(Debug, Clone)]
pub struct GoWasmCompiler;

impl GoWasmCompiler {
    pub fn language(&self) -> &str {
        "go"
    }

    pub fn file_extensions(&self) -> &[&str] {
        &["go", "mod", "sum"]
    }

    pub async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        info!("Compiling Go source to Wasm: {}", source_path.display());

        let go_mod = source_path.join("go.mod");
        if !go_mod.exists() {
            return Ok(CompilationResult {
                success: false,
                output_path: PathBuf::new(),
                size_bytes: 0,
                compilation_time_ms: 0,
                warnings: Vec::new(),
                errors: vec!["go.mod not found".to_string()],
                exported_functions: Vec::new(),
                imported_modules: Vec::new(),
            });
        }

        // TinyGo (not the stdlib `js/wasm` target) produces WASI-compatible,
        // realistically sized output; it's not always installed, so degrade
        // to a clear failure rather than a fabricated success when absent.
        let Ok(tinygo) = which::which("tinygo") else {
            return Ok(CompilationResult {
                success: false,
                output_path: PathBuf::new(),
                size_bytes: 0,
                compilation_time_ms: 0,
                warnings: Vec::new(),
                errors: vec![
                    "tinygo not found on PATH — install it (https://tinygo.org/getting-started/install/) to compile Go to wasm".to_string(),
                ],
                exported_functions: Vec::new(),
                imported_modules: Vec::new(),
            });
        };

        let output_path = source_path.join(&config.output_name);
        let mut args = vec![
            "build".to_string(),
            "-o".to_string(),
            output_path.to_string_lossy().to_string(),
            "-target".to_string(),
            "wasi".to_string(),
        ];
        args.extend(config.extra_args.clone());
        args.push(".".to_string());

        let start = std::time::Instant::now();
        let output = tokio::process::Command::new(tinygo)
            .args(&args)
            .current_dir(source_path)
            .output()
            .await
            .context("failed to spawn tinygo build")?;
        let compilation_time_ms = start.elapsed().as_millis() as u64;

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let warnings: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("warning:"))
            .map(String::from)
            .collect();
        let errors: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("error"))
            .map(String::from)
            .collect();

        if !output.status.success() {
            return Ok(CompilationResult {
                success: false,
                output_path: PathBuf::new(),
                size_bytes: 0,
                compilation_time_ms,
                warnings,
                errors,
                exported_functions: Vec::new(),
                imported_modules: Vec::new(),
            });
        }

        anyhow::ensure!(
            output_path.exists(),
            "tinygo build succeeded but no .wasm binary was found at {}",
            output_path.display()
        );
        let size_bytes = tokio::fs::metadata(&output_path).await?.len();
        let (exported_functions, imported_modules) =
            parse_wasm_exports_imports(&output_path).await?;

        Ok(CompilationResult {
            success: true,
            output_path,
            size_bytes,
            compilation_time_ms,
            warnings,
            errors,
            exported_functions,
            imported_modules,
        })
    }

    pub async fn validate_source(&self, source_path: &Path) -> Result<bool> {
        let go_mod = source_path.join("go.mod");
        if !go_mod.exists() {
            return Ok(false);
        }

        let content = tokio::fs::read_to_string(&go_mod).await?;
        Ok(content.contains("module "))
    }
}

#[derive(Debug, Clone)]
pub struct CWasmCompiler;

impl CWasmCompiler {
    pub fn language(&self) -> &str {
        "c"
    }

    pub fn file_extensions(&self) -> &[&str] {
        &["c", "h", "cpp", "cc", "cxx"]
    }

    pub async fn compile(
        &self,
        source_path: &Path,
        _config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        // Not yet wired to a real clang/wasi-sdk toolchain invocation — no
        // clang/wasi-sdk installation is verified present in this
        // environment. Report an honest failure rather than a fabricated
        // success with a zero-byte output, unlike the previous stub.
        info!(
            "C/C++ wasm compilation requested for {} — not yet implemented (needs wasi-sdk/clang)",
            source_path.display()
        );
        Ok(CompilationResult {
            success: false,
            output_path: PathBuf::new(),
            size_bytes: 0,
            compilation_time_ms: 0,
            warnings: Vec::new(),
            errors: vec![
                "C/C++ -> wasm compilation is not yet implemented (requires a wasi-sdk/clang toolchain invocation)".to_string(),
            ],
            exported_functions: Vec::new(),
            imported_modules: Vec::new(),
        })
    }

    pub async fn validate_source(&self, source_path: &Path) -> Result<bool> {
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

#[derive(Debug, Clone)]
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

impl EmscriptenCompiler {
    pub fn language(&self) -> &str {
        "c_cpp_emscripten"
    }

    pub fn file_extensions(&self) -> &[&str] {
        &["c", "cpp", "cc", "cxx", "h", "hpp"]
    }

    pub async fn compile(
        &self,
        source_path: &Path,
        config: &CompilationConfig,
    ) -> Result<CompilationResult> {
        info!("Compiling C/C++ with Emscripten: {}", source_path.display());

        let Ok(emcc) = which::which("emcc") else {
            return Ok(CompilationResult {
                success: false,
                output_path: PathBuf::new(),
                size_bytes: 0,
                compilation_time_ms: 0,
                warnings: Vec::new(),
                errors: vec![
                    "emcc not found on PATH — install the Emscripten SDK (https://emscripten.org/docs/getting_started/downloads.html) to compile C/C++ to wasm".to_string(),
                ],
                exported_functions: Vec::new(),
                imported_modules: Vec::new(),
            });
        };

        let sources: Vec<String> = if source_path.is_file() {
            vec![source_path.to_string_lossy().to_string()]
        } else {
            let mut entries = tokio::fs::read_dir(source_path)
                .await
                .context("failed to read source directory")?;
            let mut found = Vec::new();
            while let Some(entry) = entries.next_entry().await? {
                let ext = entry
                    .path()
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_string();
                if matches!(ext.as_str(), "c" | "cpp" | "cc" | "cxx") {
                    found.push(entry.path().to_string_lossy().to_string());
                }
            }
            found
        };
        anyhow::ensure!(
            !sources.is_empty(),
            "no .c/.cpp source files found under {}",
            source_path.display()
        );

        let output_path = source_path.join(&config.output_name);
        let mut args = sources;
        args.push("-o".to_string());
        args.push(output_path.to_string_lossy().to_string());
        args.push("-s".to_string());
        args.push("WASM=1".to_string());

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

        let start = std::time::Instant::now();
        let output = tokio::process::Command::new(emcc)
            .args(&args)
            .output()
            .await
            .context("failed to spawn emcc")?;
        let compilation_time_ms = start.elapsed().as_millis() as u64;

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let warnings: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("warning:"))
            .map(String::from)
            .collect();
        let errors: Vec<String> = stderr
            .lines()
            .filter(|l| l.contains("error"))
            .map(String::from)
            .collect();

        if !output.status.success() {
            return Ok(CompilationResult {
                success: false,
                output_path: PathBuf::new(),
                size_bytes: 0,
                compilation_time_ms,
                warnings,
                errors,
                exported_functions: Vec::new(),
                imported_modules: Vec::new(),
            });
        }

        anyhow::ensure!(
            output_path.exists(),
            "emcc succeeded but no .wasm binary was found at {}",
            output_path.display()
        );
        let size_bytes = tokio::fs::metadata(&output_path).await?.len();
        let (exported_functions, imported_modules) =
            parse_wasm_exports_imports(&output_path).await?;

        Ok(CompilationResult {
            success: true,
            output_path,
            size_bytes,
            compilation_time_ms,
            warnings,
            errors,
            exported_functions,
            imported_modules,
        })
    }

    pub async fn validate_source(&self, source_path: &Path) -> Result<bool> {
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
    compilers: Vec<WasmCompiler>,
}

impl WasmPipeline {
    pub fn new() -> Self {
        Self {
            compilers: vec![
                WasmCompiler::Rust(RustWasmCompiler),
                WasmCompiler::Go(GoWasmCompiler),
                WasmCompiler::C(CWasmCompiler),
                WasmCompiler::Emscripten(EmscriptenCompiler),
            ],
        }
    }

    pub async fn detect_language(&self, source_path: &Path) -> Option<&WasmCompiler> {
        for compiler in &self.compilers {
            if compiler.validate_source(source_path).await.unwrap_or(false) {
                return Some(compiler);
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
    ) -> Result<WasmModule> {
        let wasm_bytes = std::fs::read(&result.output_path).with_context(|| {
            format!(
                "failed to read compiled wasm module: {}",
                result.output_path.display()
            )
        })?;
        let sha256 = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&wasm_bytes);
            hex::encode(hasher.finalize())
        };

        Ok(WasmModule {
            name: name.to_string(),
            version: version.to_string(),
            source_path: PathBuf::new(),
            output_path: result.output_path.clone(),
            target: config.target.clone(),
            size_bytes: result.size_bytes,
            sha256,
            exports: result.exported_functions.clone(),
            imports: result.imported_modules.clone(),
            memory_pages: None,
            signature: None,
        })
    }

    /// Signs a module's compiled wasm bytes with an ed25519 key, producing a
    /// signature Origin can verify before instantiating the module.
    pub fn sign_module(
        &self,
        module: &WasmModule,
        signing_key: &ed25519_dalek::SigningKey,
    ) -> Result<WasmModuleSignature> {
        use ed25519_dalek::Signer;

        let wasm_bytes = std::fs::read(&module.output_path).with_context(|| {
            format!(
                "failed to read wasm module to sign: {}",
                module.output_path.display()
            )
        })?;

        let signature = signing_key.sign(&wasm_bytes);
        let sha256 = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(&wasm_bytes);
            hex::encode(hasher.finalize())
        };

        Ok(WasmModuleSignature {
            signature: hex::encode(signature.to_bytes()),
            public_key: hex::encode(signing_key.verifying_key().to_bytes()),
            sha256,
        })
    }
}

impl Default for WasmPipeline {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod real_compilation_tests {
    use super::*;

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hello-wasm-crate")
    }

    /// Proves `RustWasmCompiler::compile` actually invokes `cargo build` and
    /// reports real data — a nonzero size, a real elapsed time, and a file
    /// that starts with the wasm binary magic number — rather than the old
    /// hardcoded `size_bytes: 0` stub.
    #[tokio::test]
    async fn compile_produces_a_real_nonzero_wasm_binary() {
        let compiler = RustWasmCompiler;
        let config = CompilationConfig {
            target: CompilationTarget::Wasm32Wasi,
            optimize: false,
            release: false,
            features: Vec::new(),
            extra_args: Vec::new(),
            output_name: "hello-wasm-crate.wasm".to_string(),
        };

        let result = compiler
            .compile(&fixture_dir(), &config)
            .await
            .expect("compile should not error");

        assert!(
            result.success,
            "compile should succeed: {:?}",
            result.errors
        );
        assert!(
            result.size_bytes > 0,
            "a real cargo build must produce a nonzero-size wasm binary"
        );
        assert!(result.output_path.exists());

        let bytes = std::fs::read(&result.output_path).unwrap();
        assert_eq!(
            &bytes[0..4],
            b"\0asm",
            "output file must be a real wasm module, not fabricated data"
        );
    }

    #[tokio::test]
    async fn compile_reports_missing_cargo_toml_honestly() {
        let compiler = RustWasmCompiler;
        let config = CompilationConfig::default();
        let empty_dir =
            std::env::temp_dir().join(format!("chisel-no-cargo-toml-{}", std::process::id()));
        std::fs::create_dir_all(&empty_dir).unwrap();

        let result = compiler.compile(&empty_dir, &config).await.unwrap();
        assert!(!result.success);
        assert_eq!(result.size_bytes, 0);
        assert!(!result.errors.is_empty());

        std::fs::remove_dir_all(&empty_dir).ok();
    }

    #[tokio::test]
    async fn validate_source_accepts_default_binary_crate_without_explicit_bin_section() {
        let compiler = RustWasmCompiler;
        assert!(compiler.validate_source(&fixture_dir()).await.unwrap());
    }
}

#[cfg(test)]
mod signing_tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
    use rand::rngs::OsRng;
    use std::io::Write;

    fn write_fixture(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("create temp file");
        file.write_all(bytes).expect("write fixture bytes");
        file
    }

    #[test]
    fn create_module_from_result_hashes_real_bytes_not_the_name() {
        let fixture = write_fixture(b"not actually wasm, just test bytes");
        let result = CompilationResult {
            success: true,
            output_path: fixture.path().to_path_buf(),
            size_bytes: 35,
            compilation_time_ms: 1,
            warnings: vec![],
            errors: vec![],
            exported_functions: vec![],
            imported_modules: vec![],
        };
        let config = CompilationConfig::default();
        let pipeline = WasmPipeline::new();

        let module = pipeline
            .create_module_from_result("test-module", "1.0.0", &result, &config)
            .expect("hashing a real file should succeed");

        let expected_sha256 = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(std::fs::read(fixture.path()).unwrap());
            hex::encode(hasher.finalize())
        };

        assert_eq!(module.sha256, expected_sha256);
        // Regression check: the old bug hashed the module name instead.
        let name_hash = {
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(b"test-module");
            hex::encode(hasher.finalize())
        };
        assert_ne!(module.sha256, name_hash);
    }

    #[test]
    fn sign_module_round_trips_with_ed25519() {
        let fixture = write_fixture(b"a fake compiled wasm module");
        let result = CompilationResult {
            success: true,
            output_path: fixture.path().to_path_buf(),
            size_bytes: 28,
            compilation_time_ms: 1,
            warnings: vec![],
            errors: vec![],
            exported_functions: vec![],
            imported_modules: vec![],
        };
        let config = CompilationConfig::default();
        let pipeline = WasmPipeline::new();
        let module = pipeline
            .create_module_from_result("test-module", "1.0.0", &result, &config)
            .unwrap();

        let signing_key = SigningKey::generate(&mut OsRng);
        let sig = pipeline.sign_module(&module, &signing_key).unwrap();

        let public_key_bytes: [u8; 32] = hex::decode(&sig.public_key).unwrap().try_into().unwrap();
        let verifying_key = VerifyingKey::from_bytes(&public_key_bytes).unwrap();
        let sig_bytes: [u8; 64] = hex::decode(&sig.signature).unwrap().try_into().unwrap();
        let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);

        let wasm_bytes = std::fs::read(fixture.path()).unwrap();
        assert!(verifying_key.verify_strict(&wasm_bytes, &signature).is_ok());

        // Tampering must invalidate the signature.
        let mut tampered = wasm_bytes.clone();
        tampered[0] ^= 0xFF;
        assert!(verifying_key.verify_strict(&tampered, &signature).is_err());

        // Sanity: signing_key.sign() used inside sign_module matches manual signing.
        let manual_sig = signing_key.sign(&wasm_bytes);
        assert_eq!(hex::encode(manual_sig.to_bytes()), sig.signature);
    }
}
