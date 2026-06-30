use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;

use super::error::LlmError;

/// Extract the retry-after duration from HTTP response headers.
/// Supports both integer (seconds) and HTTP-date formats.
pub fn extract_retry_after(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
}

pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
}

impl RetryPolicy {
    pub fn new(max_retries: u32, base_delay: Duration) -> Self {
        Self {
            max_retries,
            base_delay,
        }
    }

    pub async fn execute<F, Fut>(&self, f: F) -> Result<String, LlmError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<String, LlmError>>,
    {
        let mut last_err = None;

        for attempt in 0..=self.max_retries {
            match f().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    match &e {
                        LlmError::RateLimit { retry_after } => {
                            let delay = retry_after
                                .map(Duration::from_secs)
                                .unwrap_or_else(|| self.backoff(attempt));
                            tokio::time::sleep(delay).await;
                            last_err = Some(e);
                        }
                        LlmError::Unavailable(_) if attempt < self.max_retries => {
                            tokio::time::sleep(self.backoff(attempt)).await;
                            last_err = Some(e);
                        }
                        _ => return Err(e),
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            LlmError::Unavailable("all retries exhausted".into())
        }))
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let base_ms = self.base_delay.as_millis() as u64;
        let exp = 1u64 << attempt.min(10);
        let ceiling = base_ms.saturating_mul(exp).min(60_000);
        let jitter = pseudo_random() * (ceiling as f64 * 0.25);
        Duration::from_millis(ceiling + jitter as u64)
    }
}

fn pseudo_random() -> f64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 1_000_000) as f64 / 1_000_000.0
}
