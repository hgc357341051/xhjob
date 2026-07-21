//! Per-task rate limiting (C12). Sliding-window counter implementation.
//!
//! Reference: Celery `rate_limit(count, window)`.
//!
//! Algorithm: keep a list of recent trigger timestamps for each task id.
//! A new trigger is allowed if the number of triggers within the last
//! `window` seconds is strictly less than `count`. When denied, the trigger
//! is recorded as a `RateLimited` event and the daemon advances
//! `next_fire` by `window` seconds (so the task is re-evaluated later).
//!
//! State is kept in-memory (process-local). On daemon restart the rate
//! window resets to "empty" — this matches Celery semantics where
//! rate_limit is a worker-local concept, not a persistent broker state.
//!
//! The limiter is intentionally simple and lock-light: a single
//! `tokio::sync::Mutex<HashMap<String, Vec<u64>>>` per `RateLimiter`
//! instance. Cron triggers happen at most a few times per second per task,
//! so contention is negligible.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use crate::store::Task;

/// Per-task rate limiter keyed by task id.
#[derive(Debug, Default)]
pub struct RateLimiter {
    /// Map of task_id -> sorted list of recent fire timestamps (Unix secs).
    inner: Arc<Mutex<HashMap<String, Vec<u64>>>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self { inner: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Check whether `task` is allowed to fire at `now_ts`. Returns
    /// `Ok(true)` if allowed (and records the trigger), or `Ok(false)` if
    /// rate-limited (no state mutation).
    ///
    /// Tasks with `rate_limit_count == 0` (default) always pass through.
    pub async fn check_and_record(&self, task: &Task, now_ts: u64) -> bool {
        if task.rate_limit_count == 0 || task.rate_limit_window == 0 {
            return true;
        }
        let mut guard = self.inner.lock().await;
        let bucket = guard.entry(task.id.clone()).or_default();
        let cutoff = now_ts.saturating_sub(task.rate_limit_window);
        // Drop timestamps older than the window.
        bucket.retain(|t| *t > cutoff);
        if (bucket.len() as u32) >= task.rate_limit_count {
            return false;
        }
        bucket.push(now_ts);
        true
    }

    /// Forget the rate-limit state for a task (e.g. when the task is removed
    /// or reaches a terminal state). Idempotent.
    pub async fn forget(&self, task_id: &str) {
        let mut guard = self.inner.lock().await;
        guard.remove(task_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Task, TaskType};

    fn make_task(id: &str, count: u32, window: u64) -> Task {
        let mut t = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo r"}));
        t.id = id.to_string();
        t.rate_limit_count = count;
        t.rate_limit_window = window;
        t
    }

    #[tokio::test]
    async fn test_zero_count_always_allows() {
        let limiter = RateLimiter::new();
        let task = make_task("t-zero", 0, 10);
        assert!(limiter.check_and_record(&task, 100).await);
        assert!(limiter.check_and_record(&task, 101).await);
        assert!(limiter.check_and_record(&task, 102).await);
    }

    #[tokio::test]
    async fn test_allows_up_to_count_then_denies() {
        let limiter = RateLimiter::new();
        let task = make_task("t-limit", 2, 60);
        // Two triggers within the window: both allowed.
        assert!(limiter.check_and_record(&task, 100).await);
        assert!(limiter.check_and_record(&task, 105).await);
        // Third trigger within the window: denied.
        assert!(!limiter.check_and_record(&task, 110).await);
    }

    #[tokio::test]
    async fn test_sliding_window_evicts_old_entries() {
        let limiter = RateLimiter::new();
        let task = make_task("t-slide", 2, 10);
        // Two triggers at t=100 and t=105 (both within window=10).
        assert!(limiter.check_and_record(&task, 100).await);
        assert!(limiter.check_and_record(&task, 105).await);
        // Third trigger at t=108: cutoff=98, both 100 and 105 are kept
        // (both > 98), so bucket.len()==2 >= count=2 -> deny.
        assert!(!limiter.check_and_record(&task, 108).await);
        // At t=120: cutoff=110, both 100 and 105 are dropped (neither > 110).
        // bucket is empty -> allow. Verifies sliding-window eviction.
        assert!(limiter.check_and_record(&task, 120).await);
    }

    #[tokio::test]
    async fn test_forget_clears_state() {
        let limiter = RateLimiter::new();
        let task = make_task("t-forget", 1, 60);
        assert!(limiter.check_and_record(&task, 100).await);
        // Second trigger denied (only 1 allowed).
        assert!(!limiter.check_and_record(&task, 105).await);
        // After forget, the next trigger is allowed.
        limiter.forget("t-forget").await;
        assert!(limiter.check_and_record(&task, 110).await);
    }
}
