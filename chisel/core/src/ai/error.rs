use thiserror::Error;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("rate limit exceeded{retry_after:?}", retry_after = retry_after.map(|s| format!(" — retry after {s}s")))]
    RateLimit {
        retry_after: Option<u64>,
    },

    #[error("authentication failed: {0}")]
    AuthError(String),

    #[error("context length exceeded: input {context_length} tokens, max {max}")]
    ContextLengthExceeded {
        context_length: usize,
        max: usize,
    },

    #[error("provider unavailable: {0}")]
    Unavailable(String),

    #[error("failed to parse response: {0}")]
    ParseError(String),

    #[error("network error: {0}")]
    Network(String),
}
