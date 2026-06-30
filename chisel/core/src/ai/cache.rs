use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

pub struct PromptCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

struct CacheEntry {
    value: String,
    inserted_at: Instant,
}

impl PromptCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    pub fn get(&self, prompt: &str) -> Option<String> {
        let key = Self::hash(prompt);
        let mut cache = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.get(&key) {
            if entry.inserted_at.elapsed() < self.ttl {
                return Some(entry.value.clone());
            }
            cache.remove(&key);
        }
        None
    }

    pub fn insert(&self, prompt: &str, response: &str) {
        let key = Self::hash(prompt);
        let entry = CacheEntry {
            value: response.to_string(),
            inserted_at: Instant::now(),
        };
        self.entries.lock().unwrap_or_else(|e| e.into_inner()).insert(key, entry);
    }

    pub fn clear(&self) {
        self.entries.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn hash(prompt: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(prompt.as_bytes());
        hex::encode(hasher.finalize())
    }
}
