use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};

use ahash::AHasher;
use parking_lot::{Mutex, RwLock};
use rand::Rng;
use serde::{Deserialize, Serialize};

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
    unhealthy: RwLock<HashSet<String>>,
}

impl RoundRobinBalancer {
    pub fn new(endpoints: Vec<BackendEndpoint>) -> Self {
        Self {
            endpoints,
            index: AtomicUsize::new(0),
            unhealthy: RwLock::new(HashSet::new()),
        }
    }
}

impl LoadBalancer for RoundRobinBalancer {
    fn next_endpoint(&self) -> Option<BackendEndpoint> {
        if self.endpoints.is_empty() {
            return None;
        }

        let unhealthy = self.unhealthy.read();
        let healthy: Vec<&BackendEndpoint> = self.endpoints.iter()
            .filter(|ep| !unhealthy.contains(&ep.addr.to_string()))
            .collect();

        // Fall back to all endpoints if every upstream is unhealthy (best-effort)
        if healthy.is_empty() {
            let idx = self.index.fetch_add(1, Ordering::Relaxed) % self.endpoints.len();
            return Some(self.endpoints[idx].clone());
        }

        let idx = self.index.fetch_add(1, Ordering::Relaxed) % healthy.len();
        Some((*healthy[idx]).clone())
    }

    fn mark_healthy(&self, endpoint: &BackendEndpoint) {
        self.unhealthy.write().remove(&endpoint.addr.to_string());
    }

    fn mark_unhealthy(&self, endpoint: &BackendEndpoint) {
        self.unhealthy.write().insert(endpoint.addr.to_string());
    }

    fn healthy_endpoints(&self) -> Vec<BackendEndpoint> {
        let unhealthy = self.unhealthy.read();
        self.endpoints.iter()
            .filter(|ep| !unhealthy.contains(&ep.addr.to_string()))
            .cloned()
            .collect()
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
    unhealthy: RwLock<HashSet<String>>,
}

impl ConsistentHashBalancer {
    pub fn new(endpoints: Vec<BackendEndpoint>, virtual_nodes: usize) -> Self {
        let balancer = Self {
            endpoints,
            ring: Mutex::new(Vec::new()),
            virtual_nodes,
            unhealthy: RwLock::new(HashSet::new()),
        };
        balancer.rebuild_ring();
        balancer
    }

    fn rebuild_ring(&self) {
        let unhealthy = self.unhealthy.read();
        let mut ring = self.ring.lock();
        ring.clear();

        for (idx, ep) in self.endpoints.iter().enumerate() {
            if unhealthy.contains(&ep.addr.to_string()) {
                continue;
            }
            for i in 0..self.virtual_nodes {
                let key = format!("{}:{}", ep.addr, i);
                let hash = Self::hash(&key);
                ring.push((hash, idx));
            }
        }

        // If all are unhealthy, fall back to including everyone (best-effort)
        if ring.is_empty() {
            for (idx, ep) in self.endpoints.iter().enumerate() {
                for i in 0..self.virtual_nodes {
                    let key = format!("{}:{}", ep.addr, i);
                    let hash = Self::hash(&key);
                    ring.push((hash, idx));
                }
            }
        }

        ring.sort_by_key(|(hash, _)| *hash);
    }

    fn hash(key: &str) -> u64 {
        let mut hasher = AHasher::default();
        key.as_bytes().hash(&mut hasher);
        hasher.finish()
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
        let key = format!("req-{}", rand::thread_rng().gen::<u64>());
        self.get_node(&key)
    }

    fn mark_healthy(&self, endpoint: &BackendEndpoint) {
        {
            self.unhealthy.write().remove(&endpoint.addr.to_string());
        }
        self.rebuild_ring();
    }

    fn mark_unhealthy(&self, endpoint: &BackendEndpoint) {
        {
            self.unhealthy.write().insert(endpoint.addr.to_string());
        }
        self.rebuild_ring();
    }

    fn healthy_endpoints(&self) -> Vec<BackendEndpoint> {
        let unhealthy = self.unhealthy.read();
        self.endpoints.iter()
            .filter(|ep| !unhealthy.contains(&ep.addr.to_string()))
            .cloned()
            .collect()
    }
}
