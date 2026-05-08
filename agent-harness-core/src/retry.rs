use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static RETRY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// splitmix64-inspired deterministic jitter.
/// Ensures jitter spans the full backoff range even with coarse system clocks.
/// Ported from Claw Code `providers/anthropic.rs` lines 596-617.
fn jitter_millis(max_ms: u64) -> u64 {
    let mut x = RETRY_COUNTER.fetch_add(1, Ordering::Relaxed);
    x = x.wrapping_add(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64,
    );
    x = x.wrapping_mul(0x9E3779B97F4A7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^= x >> 27;
    x % (max_ms + 1)
}

/// Exponential backoff with jitter.
/// Base: 1s, max: 128s, max retries implied by caller.
pub fn backoff_duration(retry_count: u32) -> Duration {
    let base_ms = 1000u64;
    let max_ms = 128_000u64;
    let raw = base_ms.saturating_mul(1u64 << retry_count.min(7));
    let clamped = raw.min(max_ms);
    let jittered = clamped + jitter_millis(clamped / 4);
    Duration::from_millis(jittered)
}

/// Classification of API errors for retry decisions.
/// Ported from Hermes `error_classifier.py`.
#[derive(Debug, Clone, PartialEq)]
pub enum ErrorClass {
    /// 429 — rate limit, retry after backoff.
    RateLimited,
    /// 5xx — may succeed on retry.
    ServerError,
    /// 400 with context_length_exceeded — needs compaction, not retry.
    ContextOverflow,
    /// 400 — do not retry (bad request).
    BadRequest,
    /// 401/403 — do not retry (auth).
    AuthError,
    /// Network error — retry.
    NetworkError,
}

impl ErrorClass {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ErrorClass::RateLimited | ErrorClass::ServerError | ErrorClass::NetworkError
        )
    }

    /// Classify from HTTP status code + response body.
    /// Peers into the body for context_length_exceeded detection.
    pub fn from_http_status_and_body(status: u16, body: &str) -> Self {
        match status {
            429 => ErrorClass::RateLimited,
            500..=599 => ErrorClass::ServerError,
            400 => {
                if body.contains("context_length_exceeded")
                    || body.contains("maximum context")
                    || body.contains("too long")
                {
                    ErrorClass::ContextOverflow
                } else {
                    ErrorClass::BadRequest
                }
            }
            401 | 403 => ErrorClass::AuthError,
            _ => ErrorClass::BadRequest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_increases() {
        let d1 = backoff_duration(0);
        let d2 = backoff_duration(3);
        assert!(d2 > d1);
    }

    #[test]
    fn test_backoff_clamped() {
        let d = backoff_duration(20);
        assert!(d <= Duration::from_millis(160_000));
    }

    #[test]
    fn test_rate_limited_is_retryable() {
        assert!(ErrorClass::RateLimited.is_retryable());
    }

    #[test]
    fn test_bad_request_is_not_retryable() {
        assert!(!ErrorClass::BadRequest.is_retryable());
    }

    #[test]
    fn test_context_overflow_detection() {
        let class =
            ErrorClass::from_http_status_and_body(400, "context_length_exceeded: max 128000");
        assert_eq!(class, ErrorClass::ContextOverflow);
    }
}
