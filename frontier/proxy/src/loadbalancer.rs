use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendEndpoint {
    pub addr: SocketAddr,
    pub weight: u32,
    pub tags: HashMap<String, String>,
}

pub trait LoadBalancer: Send + Sync {
    fn next_endpoint(&self) -> Option<BackendEndpoint>;
    fn mark_healthy(&self, endpoint: &BackendEndpoint);
    fn mark_unhealthy(&self, endpoint: &BackendEndpoint);
    fn healthy_endpoints(&self) -> Vec<BackendEndpoint>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadBalancerStrategy {
    RoundRobin,
    LeastConnections,
    ConsistentHash,
}

pub struct RoundRobinBalancer {
    endpoints: Vec<BackendEndpoint>,
    index: AtomicUsize,
}

impl RoundRobinBalancer {
    pub fn new(endpoints: Vec<BackendEndpoint>) -> Self {
        Self {
            endpoints,
            index: AtomicUsize::new(0),
        }
    }
}

impl LoadBalancer for RoundRobinBalancer {
    fn next_endpoint(&self) -> Option<BackendEndpoint> {
        if self.endpoints.is_empty() {
            return None;
        }
        let index = self.index.fetch_add(1, Ordering::Relaxed) % self.endpoints.len();
        Some(self.endpoints[index].clone())
    }

    fn mark_healthy(&self, _endpoint: &BackendEndpoint) {}

    fn mark_unhealthy(&self, _endpoint: &BackendEndpoint) {}

    fn healthy_endpoints(&self) -> Vec<BackendEndpoint> {
        self.endpoints.clone()
    }
}

pub struct LeastConnectionsBalancer {
    endpoints: Vec<BackendEndpoint>,
    connections: Mutex<HashMap<SocketAddr, usize>>,
}

impl LeastConnectionsBalancer {
    pub fn new(endpoints: Vec<BackendEndpoint>) -> Self {
        let connections = endpoints.iter()
            .map(|ep| (ep.addr, 0))
            .collect();
        Self {
            endpoints,
            connections: Mutex::new(connections),
        }
    }
}

impl LoadBalancer for LeastConnectionsBalancer {
    fn next_endpoint(&self) -> Option<BackendEndpoint> {
        let connections = self.connections.lock();
        self.endpoints.iter()
            .filter(|ep| connections.get(&ep.addr).copied().unwrap_or(usize::MAX) < 1000)
            .min_by_key(|ep| connections.get(&ep.addr).copied().unwrap_or(0))
            .cloned()
    }

    fn mark_healthy(&self, endpoint: &BackendEndpoint) {
        let mut connections = self.connections.lock();
        if let Some(count) = connections.get_mut(&endpoint.addr) {
            *count = count.saturating_sub(1);
        }
    }

    fn mark_unhealthy(&self, endpoint: &BackendEndpoint) {
        let mut connections = self.connections.lock();
        connections.insert(endpoint.addr, usize::MAX);
    }

    fn healthy_endpoints(&self) -> Vec<BackendEndpoint> {
        let connections = self.connections.lock();
        self.endpoints.iter()
            .filter(|ep| connections.get(&ep.addr).copied().unwrap_or(usize::MAX) < 1000)
            .cloned()
            .collect()
    }
}

pub struct ConsistentHashBalancer {
    endpoints: Vec<BackendEndpoint>,
    ring: Mutex<Vec<(u64, usize)>>,
    virtual_nodes: usize,
}

impl ConsistentHashBalancer {
    pub fn new(endpoints: Vec<BackendEndpoint>, virtual_nodes: usize) -> Self {
        let balancer = Self {
            endpoints,
            ring: Mutex::new(Vec::new()),
            virtual_nodes,
        };
        balancer.rebuild_ring();
        balancer
    }

    fn rebuild_ring(&self) {
        let mut ring = self.ring.lock();
        ring.clear();

        for (idx, ep) in self.endpoints.iter().enumerate() {
            for i in 0..self.virtual_nodes {
                let key = format!("{}:{}", ep.addr, i);
                let hash = Self::hash(&key);
                ring.push((hash, idx));
            }
        }

        ring.sort_by_key(|(hash, _)| *hash);
    }

    fn hash(key: &str) -> u64 {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let result = hasher.finalize();
        u64::from_be_bytes(result[..8].try_into().unwrap())
    }

    fn get_node(&self, key: &str) -> Option<BackendEndpoint> {
        let ring = self.ring.lock();
        if ring.is_empty() {
            return None;
        }

        let hash = Self::hash(key);

        let idx = match ring.binary_search_by_key(&hash, |(h, _)| *h) {
            Ok(idx) => idx,
            Err(idx) => idx % ring.len(),
        };

        Some(self.endpoints[ring[idx].1].clone())
    }
}

impl LoadBalancer for ConsistentHashBalancer {
    fn next_endpoint(&self) -> Option<BackendEndpoint> {
        let key = format!("req-{}", rand_simple());
        self.get_node(&key)
    }

    fn mark_healthy(&self, _endpoint: &BackendEndpoint) {}

    fn mark_unhealthy(&self, _endpoint: &BackendEndpoint) {
        self.rebuild_ring();
    }

    fn healthy_endpoints(&self) -> Vec<BackendEndpoint> {
        self.endpoints.clone()
    }
}

fn rand_simple() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap();
    now.as_nanos() as u64
}
