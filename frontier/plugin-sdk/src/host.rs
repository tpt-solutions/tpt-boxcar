use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::RwLock;
use tracing::{debug, info, warn};

use crate::abi::{
    HttpRequest, HttpResponse, LogLevel, PluginHost,
    PluginManifest, FilterResult,
};

pub struct HostState {
    shared_data: RwLock<HashMap<String, Vec<u8>>>,
    config: RwLock<HashMap<String, String>>,
    logs: RwLock<Vec<LogEntry>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub timestamp: String,
}

impl HostState {
    pub fn new() -> Self {
        Self {
            shared_data: RwLock::new(HashMap::new()),
            config: RwLock::new(HashMap::new()),
            logs: RwLock::new(Vec::new()),
        }
    }

    pub fn set_config(&self, config: HashMap<String, String>) {
        let mut cfg = self.config.write();
        *cfg = config;
    }

    pub fn get_logs(&self) -> Vec<LogEntry> {
        self.logs.read().clone()
    }
}

pub struct HostImpl {
    state: Arc<HostState>,
}

impl HostImpl {
    pub fn new(state: Arc<HostState>) -> Self {
        Self { state }
    }
}

impl PluginHost for HostImpl {
    fn log(&self, level: LogLevel, message: &str) {
        let entry = LogEntry {
            level,
            message: message.to_string(),
            timestamp: chrono_free_timestamp(),
        };
        self.state.logs.write().push(entry);

        match level {
            LogLevel::Debug => debug!("[plugin] {}", message),
            LogLevel::Info => info!("[plugin] {}", message),
            LogLevel::Warn => warn!("[plugin] {}", message),
            LogLevel::Error => warn!("[plugin:ERROR] {}", message),
        }
    }

    fn get_shared_data(&self, key: &str) -> Option<Vec<u8>> {
        let data = self.state.shared_data.read();
        data.get(key).cloned()
    }

    fn set_shared_data(&self, key: &str, value: &[u8]) {
        let mut data = self.state.shared_data.write();
        data.insert(key.to_string(), value.to_vec());
        debug!("shared data set: key={}", key);
    }

    fn get_config(&self, key: &str) -> Option<String> {
        let config = self.state.config.read();
        config.get(key).cloned()
    }

    fn http_request(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        warn!("plugin http_request not yet implemented for: {}", request.path);
        Ok(HttpResponse {
            status: 501,
            headers: HashMap::new(),
            body: b"not implemented".to_vec(),
        })
    }
}

pub struct PluginManager {
    host_state: Arc<HostState>,
    plugins: RwLock<Vec<LoadedPlugin>>,
}

struct LoadedPlugin {
    name: String,
    manifest: PluginManifest,
    enabled: bool,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            host_state: Arc::new(HostState::new()),
            plugins: RwLock::new(Vec::new()),
        }
    }

    pub fn register_plugin(&self, manifest: PluginManifest) -> Result<()> {
        let name = manifest.metadata.name.clone();
        info!("registering plugin: {} v{}", name, manifest.metadata.version);

        let mut plugins = self.plugins.write();
        plugins.push(LoadedPlugin {
            name: name.clone(),
            manifest,
            enabled: true,
        });

        Ok(())
    }

    pub fn unregister_plugin(&self, name: &str) -> Result<()> {
        let mut plugins = self.plugins.write();
        let initial_len = plugins.len();
        plugins.retain(|p| p.name != name);

        if plugins.len() == initial_len {
            return Err(anyhow::anyhow!("plugin '{}' not found", name));
        }

        info!("unregistered plugin: {}", name);
        Ok(())
    }

    pub fn enable_plugin(&self, name: &str) -> Result<()> {
        let mut plugins = self.plugins.write();
        let plugin = plugins.iter_mut()
            .find(|p| p.name == name)
            .context(format!("plugin '{}' not found", name))?;
        plugin.enabled = true;
        info!("enabled plugin: {}", name);
        Ok(())
    }

    pub fn disable_plugin(&self, name: &str) -> Result<()> {
        let mut plugins = self.plugins.write();
        let plugin = plugins.iter_mut()
            .find(|p| p.name == name)
            .context(format!("plugin '{}' not found", name))?;
        plugin.enabled = false;
        info!("disabled plugin: {}", name);
        Ok(())
    }

    pub fn list_plugins(&self) -> Vec<PluginManifest> {
        let plugins = self.plugins.read();
        plugins.iter().map(|p| p.manifest.clone()).collect()
    }

    pub fn host_state(&self) -> Arc<HostState> {
        self.host_state.clone()
    }

    pub fn process_request(
        &self,
        _request: &HttpRequest,
    ) -> Result<FilterResult> {
        let plugins = self.plugins.read();
        for plugin in plugins.iter() {
            if !plugin.enabled {
                continue;
            }
            debug!("processing request through plugin: {}", plugin.name);
        }
        Ok(FilterResult::Continue)
    }

    pub fn process_response(
        &self,
        _request: &HttpRequest,
        _response: &HttpResponse,
    ) -> Result<FilterResult> {
        let plugins = self.plugins.read();
        for plugin in plugins.iter() {
            if !plugin.enabled {
                continue;
            }
            debug!("processing response through plugin: {}", plugin.name);
        }
        Ok(FilterResult::Continue)
    }
}

fn chrono_free_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string()
}
