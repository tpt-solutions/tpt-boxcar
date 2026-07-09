use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tracing::debug;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub strategy: RateLimitStrategy,
    pub max_requests: u64,
    pub window_duration_secs: u64,
    pub burst_size: Option<u64>,
    pub refill_rate: Option<f64>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            strategy: RateLimitStrategy::SlidingWindow,
            max_requests: 100,
            window_duration_secs: 60,
            burst_size: None,
            refill_rate: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RateLimitStrategy {
    SlidingWindow,
    TokenBucket,
}

#[derive(Debug, Clone)]
pub struct RateLimitResult {
    pub allowed: bool,
    pub remaining: u64,
    pub retry_after: Option<Duration>,
}

struct SlidingWindowCounter {
    requests: Vec<(Instant, u64)>,
    total: u64,
}

impl SlidingWindowCounter {
    fn new() -> Self {
        Self {
            requests: Vec::new(),
            total: 0,
        }
    }

    fn record(&mut self, now: Instant, window: Duration, max: u64) -> RateLimitResult {
        let cutoff = now.checked_sub(window).unwrap_or(now);

        self.requests.retain(|(ts, _)| *ts >= cutoff);

        self.total = self.requests.iter().map(|(_, count)| count).sum();

        if self.total >= max {
            let oldest = self.requests.first().map(|(ts, _)| *ts).unwrap_or(now);
            let retry_after = window
                .checked_sub(now.duration_since(oldest))
                .unwrap_or(Duration::ZERO);
            return RateLimitResult {
                allowed: false,
                remaining: 0,
                retry_after: Some(retry_after),
            };
        }

        self.total += 1;
        self.requests.push((now, 1));

        RateLimitResult {
            allowed: true,
            remaining: max - self.total,
            retry_after: None,
        }
    }
}

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(max_tokens: u64, refill_rate: f64) -> Self {
        Self {
            tokens: max_tokens as f64,
            max_tokens: max_tokens as f64,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self, now: Instant) -> RateLimitResult {
        self.refill(now);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            RateLimitResult {
                allowed: true,
                remaining: self.tokens as u64,
                retry_after: None,
            }
        } else {
            let deficit = 1.0 - self.tokens;
            let retry_after = Duration::from_secs_f64(deficit / self.refill_rate);
            RateLimitResult {
                allowed: false,
                remaining: 0,
                retry_after: Some(retry_after),
            }
        }
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.max_tokens);
        self.last_refill = now;
    }
}

pub struct RateLimiter {
    config: RateLimitConfig,
    windows: HashMap<String, Arc<Mutex<SlidingWindowCounter>>>,
    buckets: HashMap<String, Arc<Mutex<TokenBucket>>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            windows: HashMap::new(),
            buckets: HashMap::new(),
        }
    }

    pub fn check_rate_limit(&self, key: &str) -> RateLimitResult {
        let now = Instant::now();

        match self.config.strategy {
            RateLimitStrategy::SlidingWindow => {
                let window = self.windows.get(key);
                match window {
                    Some(counter) => {
                        let mut counter = counter.lock();
                        counter.record(
                            now,
                            Duration::from_secs(self.config.window_duration_secs),
                            self.config.max_requests,
                        )
                    }
                    None => RateLimitResult {
                        allowed: true,
                        remaining: self.config.max_requests,
                        retry_after: None,
                    },
                }
            }
            RateLimitStrategy::TokenBucket => {
                let bucket = self.buckets.get(key);
                match bucket {
                    Some(bucket) => {
                        let mut bucket = bucket.lock();
                        bucket.try_consume(now)
                    }
                    None => RateLimitResult {
                        allowed: true,
                        remaining: self.config.max_requests,
                        retry_after: None,
                    },
                }
            }
        }
    }

    pub fn register_key(&mut self, key: &str) {
        match self.config.strategy {
            RateLimitStrategy::SlidingWindow => {
                let counter = Arc::new(Mutex::new(SlidingWindowCounter::new()));
                self.windows.insert(key.to_string(), counter);
            }
            RateLimitStrategy::TokenBucket => {
                let refill_rate = self.config.refill_rate.unwrap_or(
                    self.config.max_requests as f64 / self.config.window_duration_secs as f64,
                );
                let max_tokens = self.config.burst_size.unwrap_or(self.config.max_requests);
                let bucket = Arc::new(Mutex::new(TokenBucket::new(max_tokens, refill_rate)));
                self.buckets.insert(key.to_string(), bucket);
            }
        }
        debug!("registered rate limit key: {}", key);
    }

    pub fn remove_key(&mut self, key: &str) {
        match self.config.strategy {
            RateLimitStrategy::SlidingWindow => {
                self.windows.remove(key);
            }
            RateLimitStrategy::TokenBucket => {
                self.buckets.remove(key);
            }
        }
        debug!("removed rate limit key: {}", key);
    }

    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }

    pub fn active_key_count(&self) -> usize {
        match self.config.strategy {
            RateLimitStrategy::SlidingWindow => self.windows.len(),
            RateLimitStrategy::TokenBucket => self.buckets.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_requests, 100);
        assert_eq!(config.window_duration_secs, 60);
        assert!(matches!(config.strategy, RateLimitStrategy::SlidingWindow));
    }

    #[test]
    fn test_rate_limiter_new() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        assert_eq!(limiter.active_key_count(), 0);
    }

    #[test]
    fn test_sliding_window_allows_requests() {
        let config = RateLimitConfig {
            strategy: RateLimitStrategy::SlidingWindow,
            max_requests: 5,
            window_duration_secs: 60,
            burst_size: None,
            refill_rate: None,
        };
        let mut limiter = RateLimiter::new(config);
        limiter.register_key("test");

        for _ in 0..5 {
            let result = limiter.check_rate_limit("test");
            assert!(result.allowed);
        }

        let result = limiter.check_rate_limit("test");
        assert!(!result.allowed);
        assert_eq!(result.remaining, 0);
    }

    #[test]
    fn test_token_bucket_allows_requests() {
        let config = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            max_requests: 5,
            window_duration_secs: 60,
            burst_size: Some(5),
            refill_rate: Some(1.0),
        };
        let mut limiter = RateLimiter::new(config);
        limiter.register_key("test");

        for _ in 0..5 {
            let result = limiter.check_rate_limit("test");
            assert!(result.allowed);
        }

        let result = limiter.check_rate_limit("test");
        assert!(!result.allowed);
    }
}
