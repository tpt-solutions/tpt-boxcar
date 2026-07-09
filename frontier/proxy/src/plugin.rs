use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    #[serde(default)]
    pub path: PathBuf,
    pub enabled: bool,
    pub config: HashMap<String, String>,
    /// Where to load the plugin's wasm bytes from. Defaults to `File`
    /// (backward compatible with existing configs that only set `path`).
    #[serde(default)]
    pub source: PluginSource,
    /// Overrides `ResourceLimits::default().max_memory_bytes` for this plugin.
    #[serde(default)]
    pub max_memory_bytes: Option<usize>,
    /// Overrides `ResourceLimits::default().max_table_elements` for this plugin.
    #[serde(default)]
    pub max_table_elements: Option<u32>,
}

/// Caps on a single plugin instance's linear memory and table growth,
/// enforced by `PluginResourceLimiter` so a malicious or buggy plugin can't
/// exhaust host memory.
#[derive(Debug, Clone, Copy)]
pub struct ResourceLimits {
    pub max_memory_bytes: usize,
    pub max_table_elements: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 64 * 1024 * 1024, // 64 MiB
            max_table_elements: 10_000,
        }
    }
}

/// Source of a plugin's compiled wasm bytes.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PluginSource {
    /// Read from `PluginConfig.path` on local disk (existing behavior).
    #[default]
    File,
    /// Pull as a single-layer OCI artifact from any registry that speaks
    /// the OCI Distribution Spec (e.g. ghcr.io), using the convention
    /// established by `wasm-pkg-tools`/`wkg`: media type
    /// `application/vnd.module.wasm.content.layer.v1+wasm`.
    Oci { reference: String },
}

pub struct PluginState {
    pub plugin_name: String,
    pub config: HashMap<String, String>,
    limiter: PluginResourceLimiter,
}

struct PluginResourceLimiter {
    limits: ResourceLimits,
    plugin_name: String,
}

impl PluginResourceLimiter {
    fn new(plugin_name: impl Into<String>, limits: ResourceLimits) -> Self {
        Self {
            limits,
            plugin_name: plugin_name.into(),
        }
    }
}

impl wasmtime::ResourceLimiter for PluginResourceLimiter {
    fn memory_growing(&mut self, _old: usize, new: usize, maximum: Option<usize>) -> Result<bool> {
        // Honor whichever cap is stricter: the module's own declared max or
        // our configured limit.
        let cap = maximum.map_or(self.limits.max_memory_bytes, |m| {
            m.min(self.limits.max_memory_bytes)
        });
        if new > cap {
            error!(
                "plugin '{}' denied memory growth to {} bytes (cap {} bytes)",
                self.plugin_name, new, cap
            );
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(&mut self, _old: u32, new: u32, maximum: Option<u32>) -> Result<bool> {
        let cap = maximum.map_or(self.limits.max_table_elements, |m| {
            m.min(self.limits.max_table_elements)
        });
        if new > cap {
            error!(
                "plugin '{}' denied table growth to {} elements (cap {} elements)",
                self.plugin_name, new, cap
            );
            return Ok(false);
        }
        Ok(true)
    }
}

pub struct PluginLoader {
    engine: wasmtime::Engine,
    modules: RwLock<HashMap<String, wasmtime::Module>>,
    /// Retained per-plugin so `instantiate_plugin` can build the right
    /// `ResourceLimits` (and other per-plugin settings) at instantiation
    /// time; `modules` alone only carries the compiled bytes.
    configs: RwLock<HashMap<String, PluginConfig>>,
    plugin_dir: PathBuf,
    watcher_state: RwLock<HashMap<PathBuf, std::time::SystemTime>>,
}

impl PluginLoader {
    pub fn new(plugin_dir: impl AsRef<Path>) -> Result<Self> {
        let mut engine_config = wasmtime::Config::new();
        engine_config
            .async_support(false)
            .consume_fuel(true)
            .epoch_interruption(true);

        let engine =
            wasmtime::Engine::new(&engine_config).context("failed to create wasmtime engine")?;

        info!(
            "plugin loader initialized, dir={}",
            plugin_dir.as_ref().display()
        );

        Ok(Self {
            engine,
            modules: RwLock::new(HashMap::new()),
            configs: RwLock::new(HashMap::new()),
            plugin_dir: plugin_dir.as_ref().to_path_buf(),
            watcher_state: RwLock::new(HashMap::new()),
        })
    }

    pub fn load_plugin(&self, config: PluginConfig) -> Result<()> {
        if !config.enabled {
            info!("plugin '{}' is disabled, skipping", config.name);
            return Ok(());
        }

        let wasm_bytes = std::fs::read(&config.path)
            .context(format!("failed to read plugin: {}", config.path.display()))?;

        self.register_module(&config.name, &wasm_bytes)?;
        self.configs
            .write()
            .insert(config.name.clone(), config.clone());
        info!(
            "loaded plugin '{}' from {}",
            config.name,
            config.path.display()
        );
        Ok(())
    }

    /// Pulls a plugin's wasm bytes from an OCI registry as a single-layer
    /// artifact and registers it, using the same compile path as the
    /// file-based `load_plugin`. `hot_reload` does not cover OCI-sourced
    /// plugins — they're refreshed via an explicit re-pull (i.e. calling
    /// this again), not filesystem mtime polling, since polling a registry
    /// on every hot-reload tick would add network load with no
    /// digest-based change detection in this slice.
    pub async fn load_plugin_from_oci(&self, name: &str, reference: &str) -> Result<()> {
        use oci_client::client::{Client, ClientConfig};
        use oci_client::secrets::RegistryAuth;
        use oci_client::Reference;

        const WASM_LAYER_MEDIA_TYPE: &str = "application/vnd.module.wasm.content.layer.v1+wasm";

        let reference: Reference = reference
            .parse()
            .with_context(|| format!("invalid OCI reference for plugin '{name}': {reference}"))?;

        let client = Client::new(ClientConfig::default());
        let image_data = client
            .pull(
                &reference,
                &RegistryAuth::Anonymous,
                vec![WASM_LAYER_MEDIA_TYPE],
            )
            .await
            .with_context(|| format!("failed to pull plugin '{name}' from {reference}"))?;

        let layer = image_data
            .layers
            .into_iter()
            .next()
            .with_context(|| format!("OCI artifact for plugin '{name}' has no layers"))?;

        self.register_module(name, &layer.data)?;
        self.configs.write().insert(
            name.to_string(),
            PluginConfig {
                name: name.to_string(),
                path: PathBuf::new(),
                enabled: true,
                config: HashMap::new(),
                source: PluginSource::Oci {
                    reference: reference.to_string(),
                },
                max_memory_bytes: None,
                max_table_elements: None,
            },
        );
        info!("loaded plugin '{}' from OCI reference {}", name, reference);
        Ok(())
    }

    /// Compiles wasm bytes and registers the module under `name`, shared by
    /// both the file-based and OCI-based load paths.
    fn register_module(&self, name: &str, wasm_bytes: &[u8]) -> Result<()> {
        let module = wasmtime::Module::new(&self.engine, wasm_bytes)
            .context(format!("failed to compile plugin: {}", name))?;

        let mut modules = self.modules.write();
        modules.insert(name.to_string(), module);
        Ok(())
    }

    pub fn unload_plugin(&self, name: &str) -> Result<()> {
        let mut modules = self.modules.write();
        modules
            .remove(name)
            .context(format!("plugin '{}' not found", name))?;
        self.configs.write().remove(name);
        info!("unloaded plugin '{}'", name);
        Ok(())
    }

    pub fn has_plugin(&self, name: &str) -> bool {
        let modules = self.modules.read();
        modules.contains_key(name)
    }

    pub fn list_plugins(&self) -> Vec<String> {
        let modules = self.modules.read();
        modules.keys().cloned().collect()
    }

    pub fn hot_reload(&self) -> Result<Vec<String>> {
        let mut reloaded = Vec::new();

        let entries =
            std::fs::read_dir(&self.plugin_dir).context("failed to read plugin directory")?;

        for entry in entries {
            let entry = entry.context("failed to read dir entry")?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
                continue;
            }

            let modified = entry.metadata().and_then(|m| m.modified()).ok();

            let mut watcher = self.watcher_state.write();
            let last_modified = watcher.get(&path).copied();

            let should_reload = match (modified, last_modified) {
                (Some(current), Some(previous)) => current > previous,
                (Some(_current), None) => true,
                _ => false,
            };

            if let Some(mtime) = modified {
                watcher.insert(path.clone(), mtime);
            }

            drop(watcher);

            if should_reload {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                // Preserve any previously configured resource limits across
                // a reload instead of resetting them to defaults.
                let previous = self.configs.read().get(&name).cloned();
                let config = PluginConfig {
                    name: name.clone(),
                    path: path.clone(),
                    enabled: true,
                    config: previous
                        .as_ref()
                        .map(|c| c.config.clone())
                        .unwrap_or_default(),
                    source: PluginSource::File,
                    max_memory_bytes: previous.as_ref().and_then(|c| c.max_memory_bytes),
                    max_table_elements: previous.as_ref().and_then(|c| c.max_table_elements),
                };

                match self.load_plugin(config) {
                    Ok(()) => {
                        info!("hot-reloaded plugin '{}' from {}", name, path.display());
                        reloaded.push(name);
                    }
                    Err(e) => {
                        error!("failed to hot-reload plugin '{}': {}", name, e);
                    }
                }
            }
        }

        Ok(reloaded)
    }

    pub fn instantiate_plugin(
        &self,
        plugin_name: &str,
    ) -> Result<(wasmtime::Store<PluginState>, wasmtime::Instance)> {
        let modules = self.modules.read();
        let module = modules
            .get(plugin_name)
            .context(format!("plugin '{}' not found", plugin_name))?;

        let configs = self.configs.read();
        let plugin_config = configs.get(plugin_name);
        let limits = ResourceLimits {
            max_memory_bytes: plugin_config
                .and_then(|c| c.max_memory_bytes)
                .unwrap_or_else(|| ResourceLimits::default().max_memory_bytes),
            max_table_elements: plugin_config
                .and_then(|c| c.max_table_elements)
                .unwrap_or_else(|| ResourceLimits::default().max_table_elements),
        };
        let config_map = plugin_config.map(|c| c.config.clone()).unwrap_or_default();
        drop(configs);

        let mut store = wasmtime::Store::new(
            &self.engine,
            PluginState {
                plugin_name: plugin_name.to_string(),
                config: config_map,
                limiter: PluginResourceLimiter::new(plugin_name, limits),
            },
        );

        store.limiter(|state| &mut state.limiter);

        let linker = wasmtime::Linker::<PluginState>::new(&self.engine);
        let instance = linker
            .instantiate(&mut store, module)
            .context(format!("failed to instantiate plugin '{}'", plugin_name))?;

        Ok((store, instance))
    }

    pub fn engine(&self) -> &wasmtime::Engine {
        &self.engine
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_source_file_round_trips_through_json() {
        let source = PluginSource::File;
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, r#"{"type":"file"}"#);
        let back: PluginSource = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, PluginSource::File));
    }

    #[test]
    fn plugin_source_oci_round_trips_through_json() {
        let source = PluginSource::Oci {
            reference: "ghcr.io/example/plugin:v1".to_string(),
        };
        let json = serde_json::to_string(&source).unwrap();
        let back: PluginSource = serde_json::from_str(&json).unwrap();
        match back {
            PluginSource::Oci { reference } => {
                assert_eq!(reference, "ghcr.io/example/plugin:v1");
            }
            other => panic!("expected Oci variant, got {other:?}"),
        }
    }

    #[test]
    fn plugin_config_defaults_to_file_source_when_omitted() {
        let json = r#"{"name":"legacy","path":"plugins/legacy.wasm","enabled":true,"config":{}}"#;
        let config: PluginConfig = serde_json::from_str(json).unwrap();
        assert!(matches!(config.source, PluginSource::File));
    }

    #[test]
    fn memory_growth_beyond_limit_is_denied() {
        use wasmtime::ResourceLimiter;
        let mut limiter = PluginResourceLimiter::new(
            "test-plugin",
            ResourceLimits {
                max_memory_bytes: 65536,
                max_table_elements: 100,
            }, // 1 page cap
        );
        // growing from 1 page (65536 bytes) to 2 pages (131072 bytes) exceeds the cap
        assert_eq!(limiter.memory_growing(65536, 131072, None).unwrap(), false);
    }

    #[test]
    fn memory_growth_within_limit_is_allowed() {
        use wasmtime::ResourceLimiter;
        let mut limiter = PluginResourceLimiter::new("test-plugin", ResourceLimits::default());
        assert_eq!(limiter.memory_growing(0, 65536, None).unwrap(), true);
    }

    #[test]
    fn table_growth_beyond_limit_is_denied() {
        use wasmtime::ResourceLimiter;
        let mut limiter = PluginResourceLimiter::new(
            "test-plugin",
            ResourceLimits {
                max_memory_bytes: ResourceLimits::default().max_memory_bytes,
                max_table_elements: 10,
            },
        );
        assert_eq!(limiter.table_growing(0, 11, None).unwrap(), false);
    }

    /// End-to-end proof that the limit is enforced by a real running wasm
    /// guest, not just checked in isolation: a module whose `_start` tries
    /// to grow linear memory past a deliberately tiny configured cap must
    /// see `memory.grow` actually fail (return -1) when it executes.
    #[test]
    fn running_plugin_cannot_grow_memory_past_configured_limit() {
        let dir = std::env::temp_dir().join(format!("frontier-plugin-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let loader = PluginLoader::new(&dir).unwrap();

        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "_start") (result i32)
                    (memory.grow (i32.const 10))))
        "#;
        let wasm_bytes = wat::parse_str(wat).unwrap();
        let wasm_path = dir.join("tiny-memory.wasm");
        std::fs::write(&wasm_path, &wasm_bytes).unwrap();

        loader
            .load_plugin(PluginConfig {
                name: "tiny-memory".to_string(),
                path: wasm_path,
                enabled: true,
                config: HashMap::new(),
                source: PluginSource::File,
                max_memory_bytes: Some(65536), // cap at the module's starting 1 page
                max_table_elements: None,
            })
            .unwrap();

        let (mut store, instance) = loader.instantiate_plugin("tiny-memory").unwrap();
        // The engine has `consume_fuel(true)` and `epoch_interruption(true)`
        // set; a store must be given an explicit fuel budget and epoch
        // deadline before any call, or wasmtime traps immediately on entry.
        store.set_fuel(1_000_000).unwrap();
        store.set_epoch_deadline(1_000_000);
        let start = instance
            .get_typed_func::<(), i32>(&mut store, "_start")
            .unwrap();
        let result = start.call(&mut store, ()).unwrap();

        // memory.grow returns -1 (not a trap) when growth is denied by the host limiter.
        assert_eq!(
            result, -1,
            "memory.grow should have been denied by the configured 1-page limit"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// Requires network access to a real OCI registry hosting a test wasm
    /// artifact; run manually with `cargo test -p tpt-frontier-proxy --
    /// --ignored plugin::tests::pulls_plugin_from_oci_registry`.
    #[tokio::test]
    #[ignore]
    async fn pulls_plugin_from_oci_registry() {
        let dir = std::env::temp_dir();
        let loader = PluginLoader::new(&dir).unwrap();
        loader
            .load_plugin_from_oci("test-plugin", "ghcr.io/example/test-plugin:v1")
            .await
            .expect("pull should succeed against a real registry");
        assert!(loader.has_plugin("test-plugin"));
        loader.instantiate_plugin("test-plugin").unwrap();
    }
}
