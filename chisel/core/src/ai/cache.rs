use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lru::LruCache;

pub struct PromptCache {
    entries: Mutex<LruCache<String, CacheEntry>>,
    ttl: Duration,
}

struct CacheEntry {
    value: String,
    inserted_at: Instant,
}

impl PromptCache {
    pub fn new(ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(10_000).expect("capacity is non-zero");
        Self {
            entries: Mutex::new(LruCache::new(cap)),
            ttl,
        }
    }

    pub fn get(&self, prompt: &str) -> Option<String> {
        let key = prompt.to_string();
        let mut cache = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = cache.get(&key) {
            if entry.inserted_at.elapsed() < self.ttl {
                return Some(entry.value.clone());
            }
            cache.pop(&key);
        }
        None
    }

    pub fn insert(&self, prompt: &str, response: &str) {
        let key = prompt.to_string();
        let entry = CacheEntry {
            value: response.to_string(),
            inserted_at: Instant::now(),
        };
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .put(key, entry);
    }

    pub fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
}
