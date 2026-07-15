use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EnrichmentError {
    #[error("failed to read cgroup: {0}")]
    CgroupReadError(String),
    #[error("container not found for pid {0}")]
    ContainerNotFound(u32),
    #[error("failed to read container metadata: {0}")]
    MetadataError(String),
}

#[derive(Debug, Clone, Default)]
pub struct EnrichmentData {
    pub container_id: String,
    pub image_name: String,
    pub image_tag: String,
    pub pod_name: String,
    pub namespace: String,
    pub node_name: String,
    pub container_name: String,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
    pub pid: u32,
}

pub struct EnrichmentProvider {
    cache: HashMap<u32, EnrichmentData>,
    _cgroup_path: PathBuf,
    proc_path: PathBuf,
}

impl Default for EnrichmentProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl EnrichmentProvider {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            _cgroup_path: PathBuf::from("/sys/fs/cgroup"),
            proc_path: PathBuf::from("/proc"),
        }
    }

    pub fn with_paths(cgroup_path: PathBuf, proc_path: PathBuf) -> Self {
        Self {
            cache: HashMap::new(),
            _cgroup_path: cgroup_path,
            proc_path,
        }
    }

    pub fn enrich(&mut self, pid: u32) -> Result<EnrichmentData, EnrichmentError> {
        if let Some(data) = self.cache.get(&pid) {
            return Ok(data.clone());
        }

        let container_id = self.extract_container_id(pid)?;
        let data = self.build_enrichment(pid, &container_id)?;
        self.cache.insert(pid, data.clone());
        Ok(data)
    }

    pub fn invalidate(&mut self, pid: u32) {
        self.cache.remove(&pid);
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    fn extract_container_id(&self, pid: u32) -> Result<String, EnrichmentError> {
        let cgroup_file = self.proc_path.join(pid.to_string()).join("cgroup");
        let content = fs::read_to_string(&cgroup_file)
            .map_err(|e| EnrichmentError::CgroupReadError(e.to_string()))?;

        for line in content.lines() {
            if let Some(id) = self.parse_container_id_from_cgroup(line) {
                return Ok(id);
            }
        }

        Err(EnrichmentError::ContainerNotFound(pid))
    }

    fn parse_container_id_from_cgroup(&self, line: &str) -> Option<String> {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 3 {
            return None;
        }
        let path = parts[2];
        let segments: Vec<&str> = path.split('/').collect();

        for segment in &segments {
            if segment.len() >= 12 && segment.chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(segment.to_string());
            }
        }

        None
    }

    fn build_enrichment(
        &self,
        pid: u32,
        container_id: &str,
    ) -> Result<EnrichmentData, EnrichmentError> {
        let mut data = EnrichmentData {
            container_id: container_id.to_string(),
            pid,
            ..Default::default()
        };

        if let Ok(environ) = self.read_proc_environ(pid) {
            self.extract_k8s_metadata(&environ, &mut data);
        }

        Ok(data)
    }

    fn read_proc_environ(&self, pid: u32) -> Result<Vec<String>, EnrichmentError> {
        let environ_path = self.proc_path.join(pid.to_string()).join("environ");
        let content = fs::read_to_string(&environ_path)
            .map_err(|e| EnrichmentError::MetadataError(e.to_string()))?;

        Ok(content.split('\0').map(String::from).collect())
    }

    fn extract_k8s_metadata(&self, environ: &[String], data: &mut EnrichmentData) {
        for env in environ {
            if let Some(value) = env.strip_prefix("K8S_POD_NAME=") {
                data.pod_name = value.to_string();
            } else if let Some(value) = env.strip_prefix("K8S_NAMESPACE=") {
                data.namespace = value.to_string();
            } else if let Some(value) = env.strip_prefix("K8S_NODE_NAME=") {
                data.node_name = value.to_string();
            } else if let Some(value) = env.strip_prefix("K8S_CONTAINER_NAME=") {
                data.container_name = value.to_string();
            } else if let Some(value) = env.strip_prefix("CONTAINER_IMAGE=") {
                if let Some((name, tag)) = value.rsplit_once(':') {
                    data.image_name = name.to_string();
                    data.image_tag = tag.to_string();
                } else {
                    data.image_name = value.to_string();
                }
            }
        }
    }

    pub fn enrich_with_fallback(&mut self, pid: u32) -> EnrichmentData {
        self.enrich(pid).unwrap_or_else(|_| EnrichmentData {
            container_id: format!("pid-{}", pid),
            pid,
            ..Default::default()
        })
    }
}

pub struct ContainerMetadata {
    pub container_id: String,
    pub image: String,
    pub labels: HashMap<String, String>,
    pub annotations: HashMap<String, String>,
}

pub fn parse_container_id_from_path(path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').collect();
    for segment in &segments {
        if segment.len() >= 12 && segment.chars().all(|c| c.is_ascii_hexdigit()) {
            return Some(segment.to_string());
        }
    }
    None
}

pub fn image_name_from_id(container_id: &str) -> String {
    format!("container-{}", &container_id[..12.min(container_id.len())])
}
