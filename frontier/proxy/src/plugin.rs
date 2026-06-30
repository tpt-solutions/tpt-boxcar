use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{info, error};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    pub path: PathBuf,
    pub enabled: bool,
    pub config: HashMap<String, String>,
}

pub struct PluginState {
    pub plugin_name: String,
    pub config: HashMap<String, String>,
    limiter: PluginResourceLimiter,
}

struct PluginResourceLimiter;

impl wasmtime::ResourceLimiter for PluginResourceLimiter {
    fn memory_growing(
        &mut self,
        _old: usize,
        _new: usize,
        _maximum: Option<usize>,
    ) -> Result<bool> {
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _old: u32,
        _new: u32,
        _maximum: Option<u32>,
    ) -> Result<bool> {
        Ok(true)
    }
}

pub struct PluginLoader {
    engine: wasmtime::Engine,
    modules: RwLock<HashMap<String, wasmtime::Module>>,
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

        let engine = wasmtime::Engine::new(&engine_config)
            .context("failed to create wasmtime engine")?;

        info!("plugin loader initialized, dir={}", plugin_dir.as_ref().display());

        Ok(Self {
            engine,
            modules: RwLock::new(HashMap::new()),
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

        let module = wasmtime::Module::new(&self.engine, &wasm_bytes)
            .context(format!("failed to compile plugin: {}", config.name))?;

        info!("loaded plugin '{}' from {}", config.name, config.path.display());

        let mut modules = self.modules.write();
        modules.insert(config.name, module);

        Ok(())
    }

    pub fn unload_plugin(&self, name: &str) -> Result<()> {
        let mut modules = self.modules.write();
        modules.remove(name)
            .context(format!("plugin '{}' not found", name))?;
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

        let entries = std::fs::read_dir(&self.plugin_dir)
            .context("failed to read plugin directory")?;

        for entry in entries {
            let entry = entry.context("failed to read dir entry")?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
                continue;
            }

            let modified = entry.metadata()
                .and_then(|m| m.modified())
                .ok();

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
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                let config = PluginConfig {
                    name: name.clone(),
                    path: path.clone(),
                    enabled: true,
                    config: HashMap::new(),
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

    pub fn instantiate_plugin(&self, plugin_name: &str) -> Result<(wasmtime::Store<PluginState>, wasmtime::Instance)> {
        let modules = self.modules.read();
        let module = modules.get(plugin_name)
            .context(format!("plugin '{}' not found", plugin_name))?;

        let mut store = wasmtime::Store::new(&self.engine, PluginState {
            plugin_name: plugin_name.to_string(),
            config: HashMap::new(),
            limiter: PluginResourceLimiter,
        });

        store.limiter(|state| &mut state.limiter);

        let linker = wasmtime::Linker::<PluginState>::new(&self.engine);
        let instance = linker.instantiate(&mut store, module)
            .context(format!("failed to instantiate plugin '{}'", plugin_name))?;

        Ok((store, instance))
    }

    pub fn engine(&self) -> &wasmtime::Engine {
        &self.engine
    }
}
