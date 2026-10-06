//! Process-local, in-memory rate limiting for the unauthenticated endpoints.
//!
//! The deployment is a single process in front of a single SQLite file, so a
//! `HashMap` behind a mutex is the whole story — no Redis, no extra dependency.
//! A restart clears the counters, which is acceptable for slowing down password
//! guessing and sign-up spam.
//!
//! Scope note: the app never sees the client IP (the proxy terminates the
//! connection and nothing reads `X-Forwarded-For`), so keys are the submitted
//! username. Per-IP limiting therefore belongs in Caddy/nginx in front.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Failed logins one account may accumulate inside [`LOGIN_WINDOW`] before
/// further attempts are refused with 429.
pub const LOGIN_MAX_FAILURES: u32 = 8;
pub const LOGIN_WINDOW: Duration = Duration::from_secs(300);

/// Registrations one process accepts inside [`REGISTER_WINDOW`]. Deliberately
/// generous — it is a backstop against automated sign-up, not a growth cap.
pub const REGISTER_MAX_ATTEMPTS: u32 = 60;
pub const REGISTER_WINDOW: Duration = Duration::from_secs(3600);

/// Above this many tracked keys, expired buckets are swept on write so an
/// attacker cycling unique usernames cannot grow the map without bound.
const SWEEP_THRESHOLD: usize = 4096;

struct Bucket {
    count: u32,
    reset_at: Instant,
}

/// Fixed-window counter keyed by an arbitrary string.
pub struct RateLimiter {
    max: u32,
    window: Duration,
    buckets: Mutex<HashMap<String, Bucket>>,
}

impl RateLimiter {
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Bucket>> {
        // The critical sections below cannot panic, so poisoning is not expected;
        // recover instead of taking the whole server down if it ever happens.
        self.buckets.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How long the caller must wait before `key` may try again, or `None` when
    /// it still has budget. Never consumes budget itself.
    pub fn retry_after(&self, key: &str) -> Option<u64> {
        let now = Instant::now();
        let mut buckets = self.lock();
        let bucket = buckets.get_mut(key)?;
        if bucket.reset_at <= now {
            buckets.remove(key);
            return None;
        }
        if bucket.count < self.max {
            return None;
        }
        Some(bucket.reset_at.saturating_duration_since(now).as_secs().max(1))
    }

    /// Count one attempt against `key`, starting a fresh window when the previous
    /// one has elapsed.
    pub fn record(&self, key: &str) {
        let now = Instant::now();
        let mut buckets = self.lock();

        if buckets.len() > SWEEP_THRESHOLD {
            buckets.retain(|_, b| b.reset_at > now);
        }

        match buckets.get_mut(key) {
            Some(bucket) if bucket.reset_at > now => bucket.count += 1,
            _ => {
                buckets.insert(
                    key.to_string(),
                    Bucket {
                        count: 1,
                        reset_at: now + self.window,
                    },
                );
            }
        }
    }

    /// Forget everything recorded for `key`, e.g. after a successful login.
    pub fn clear(&self, key: &str) {
        self.lock().remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_is_not_consumed_by_inspecting_it() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        assert_eq!(limiter.retry_after("alice"), None);
        assert_eq!(limiter.retry_after("alice"), None);
        assert_eq!(limiter.retry_after("alice"), None);
    }

    #[test]
    fn refuses_once_the_budget_is_spent_and_reports_a_wait() {
        let limiter = RateLimiter::new(2, Duration::from_secs(60));
        limiter.record("alice");
        assert_eq!(limiter.retry_after("alice"), None);
        limiter.record("alice");
        let wait = limiter.retry_after("alice").expect("over budget");
        assert!((1..=60).contains(&wait), "unexpected wait {wait}");
    }

    #[test]
    fn keys_are_independent() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        limiter.record("alice");
        assert!(limiter.retry_after("alice").is_some());
        assert_eq!(limiter.retry_after("bob"), None);
    }

    #[test]
    fn clear_restores_the_budget() {
        let limiter = RateLimiter::new(1, Duration::from_secs(60));
        limiter.record("alice");
        assert!(limiter.retry_after("alice").is_some());
        limiter.clear("alice");
        assert_eq!(limiter.retry_after("alice"), None);
    }

    #[test]
    fn an_elapsed_window_starts_over() {
        let limiter = RateLimiter::new(1, Duration::from_millis(0));
        limiter.record("alice");
        limiter.record("alice");
        assert_eq!(limiter.retry_after("alice"), None);
    }
}
