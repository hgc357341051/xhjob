//! Retry mechanism (reference: Celery retry).
//!
//! Exponential backoff: next_retry_delay = base_delay * backoff_factor^(attempts-1).
//! Retry only on specific error types (HTTP 5xx, shell non-zero exit).

use crate::errors::Result;
use crate::store::{Task, TaskState, TaskResult, TaskType, TaskStore};
use std::sync::Arc;

/// Retry policy configuration.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: u64, // seconds
    pub backoff_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 0,
            base_delay: 1,
            backoff_factor: 2.0,
        }
    }
}

impl RetryPolicy {
    pub fn new(max_attempts: u32, base_delay: u64) -> Self {
        Self {
            max_attempts,
            base_delay,
            backoff_factor: 2.0,
        }
    }

    /// Compute the delay (seconds) before the next retry.
    /// delay = base_delay * backoff_factor^(attempts_so_far)
    /// attempts_so_far = number of failed attempts so far (0-indexed)
    pub fn delay_for_attempt(&self, attempts_so_far: u32) -> u64 {
        if attempts_so_far == 0 {
            return self.base_delay;
        }
        let exp = attempts_so_far as f64;
        let delay = self.base_delay as f64 * self.backoff_factor.powf(exp);
        // cap at 1 hour
        delay.min(3600.0) as u64
    }

    /// Whether the task should be retried.
    /// Returns true if attempts < max_attempts AND the result indicates a
    /// retryable failure (HTTP 5xx, shell non-zero exit, or network error).
    pub fn should_retry(&self, task: &Task, result: &TaskResult) -> bool {
        if task.attempts >= task.retry_max {
            return false;
        }
        match task.task_type {
            TaskType::Http => match result.status_code {
                Some(code) => Self::is_retryable_http_status(code),
                None => true, // network error: retryable
            },
            TaskType::Shell => match result.exit_code {
                Some(0) => false, // success: not retryable
                Some(code) => Self::is_retryable_shell_exit(code), // any non-zero
                None => true, // dispatch error: retryable
            },
        }
    }

    /// Whether an HTTP status code is retryable (5xx by default).
    pub fn is_retryable_http_status(status: i32) -> bool {
        (500..600).contains(&status)
    }

    /// Whether a shell exit code is retryable (any non-zero by default).
    pub fn is_retryable_shell_exit(exit_code: i32) -> bool {
        exit_code != 0
    }
}

/// Determine if a task result indicates failure that warrants retry.
pub fn is_failure(task: &Task, result: &TaskResult) -> bool {
    match task.task_type {
        TaskType::Http => {
            // 2xx = success
            result.status_code.map(|c| !(200..300).contains(&c)).unwrap_or(true)
        }
        TaskType::Shell => {
            result.exit_code.map(|c| c != 0).unwrap_or(true)
        }
    }
}

/// Compute the retry delay (seconds) for a task based on its current
/// `attempts` count and whether exponential backoff is enabled.
/// - `retry_backoff=false`: fixed delay = `task.retry_delay`.
/// - `retry_backoff=true`: exponential delay =
///   `min(task.retry_delay * 2^task.attempts, task.retry_delay * 60)`.
/// `task.attempts` is the count BEFORE the upcoming retry (i.e. the number
/// of prior failed attempts). For attempts=0 the delay equals `retry_delay`;
/// for attempts=1 it equals `2 * retry_delay`; for attempts=2 it equals
/// `4 * retry_delay`; and so on, capped at `60 * retry_delay`.
/// Reference: Celery retry_backoff.
pub fn backoff_delay(task: &Task) -> u64 {
    if !task.retry_backoff {
        return task.retry_delay;
    }
    let cap = task.retry_delay.saturating_mul(60);
    // Compute retry_delay * 2^attempts, capped at `cap`.
    // Use a 63-bit shift ceiling to avoid overflow on huge attempt counts.
    let factor = if task.attempts >= 63 {
        u64::MAX
    } else {
        1u64 << task.attempts
    };
    let raw = task.retry_delay.saturating_mul(factor);
    raw.min(cap)
}

/// Compute the next retry time (Unix timestamp) for a task given current attempts.
pub fn next_retry_ts(task: &Task) -> u64 {
    let delay = backoff_delay(task);
    crate::store::now_ts() + delay
}

/// Schedule a retry for the task: increment attempts, set last_error,
/// reset state to PENDING, and update next_fire to the retry time.
///
/// Returns true if retry was scheduled, false if max attempts reached.
pub async fn schedule_retry(
    store: &Arc<dyn TaskStore>,
    task: &Task,
    error: String,
) -> Result<bool> {
    if task.attempts >= task.retry_max {
        // Mark as FAILED permanently
        store.update_state(
            &task.id,
            TaskState::Failed,
            None,
            Some(crate::store::now_ts()),
        ).await?;
        store.set_attempts_and_error(&task.id, task.attempts + 1, Some(error)).await?;
        return Ok(false);
    }

    let next = next_retry_ts(task);
    store.set_attempts_and_error(&task.id, task.attempts + 1, Some(error)).await?;
    store.update_state(&task.id, TaskState::Pending, None, None).await?;
    store.update_next_fire(&task.id, Some(next)).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delay_for_attempt() {
        let p = RetryPolicy::new(3, 1);
        assert_eq!(p.delay_for_attempt(0), 1);
        assert_eq!(p.delay_for_attempt(1), 2); // 1 * 2^1
        assert_eq!(p.delay_for_attempt(2), 4); // 1 * 2^2
        assert_eq!(p.delay_for_attempt(3), 8); // 1 * 2^3
    }

    #[test]
    fn test_should_retry() {
        let mut task = Task::new(TaskType::Http, serde_json::json!({}));
        task.retry_max = 3;
        task.attempts = 0;
        let p = RetryPolicy::default();

        // HTTP 200 -> success, not retryable
        let mut ok = TaskResult::default();
        ok.status_code = Some(200);
        assert!(!p.should_retry(&task, &ok));

        // HTTP 500 -> retryable
        let mut server_err = TaskResult::default();
        server_err.status_code = Some(500);
        assert!(p.should_retry(&task, &server_err));

        // HTTP 404 -> not retryable (4xx)
        let mut not_found = TaskResult::default();
        not_found.status_code = Some(404);
        assert!(!p.should_retry(&task, &not_found));

        // attempts >= retry_max -> never retryable
        task.attempts = 3;
        assert!(!p.should_retry(&task, &server_err));
    }

    #[test]
    fn test_should_retry_http_404_not_retryable() {
        let mut task = Task::new(TaskType::Http, serde_json::json!({}));
        task.retry_max = 3;
        task.attempts = 0;
        let p = RetryPolicy::default();
        let mut result = TaskResult::default();
        result.status_code = Some(404);
        assert!(!p.should_retry(&task, &result),
            "HTTP 404 must not be retryable (only 5xx)");
    }

    #[test]
    fn test_should_retry_network_error_retryable() {
        let mut task = Task::new(TaskType::Http, serde_json::json!({}));
        task.retry_max = 3;
        task.attempts = 0;
        let p = RetryPolicy::default();
        // Network error: no status_code at all -> retryable.
        let result = TaskResult::default();
        assert!(p.should_retry(&task, &result),
            "network error (status_code=None) should be retryable");
    }

    #[test]
    fn test_is_failure() {
        let mut task = Task::new(TaskType::Http, serde_json::json!({}));
        let mut result = TaskResult::default();
        result.status_code = Some(200);
        assert!(!is_failure(&task, &result));

        result.status_code = Some(500);
        assert!(is_failure(&task, &result));

        task.task_type = TaskType::Shell;
        result.status_code = None;
        result.exit_code = Some(0);
        assert!(!is_failure(&task, &result));

        result.exit_code = Some(1);
        assert!(is_failure(&task, &result));
    }

    /// Exponential backoff (C8): with `retry_backoff=true` and `retry_delay=1`,
    /// the delay sequence as `attempts` advances (0,1,2,3,4,...) should be
    /// 1, 2, 4, 8, 16, ... capped at `retry_delay * 60 = 60` seconds.
    /// Reference: Celery retry_backoff.
    #[test]
    fn test_exponential_backoff_delay_sequence() {
        let mut task = Task::new(TaskType::Http, serde_json::json!({}));
        task.retry_delay = 1;
        task.retry_backoff = true;

        // attempts=0 → 1 * 2^0 = 1 (the upcoming retry is the 1st retry)
        task.attempts = 0;
        assert_eq!(backoff_delay(&task), 1, "attempts=0 should give 1s delay");

        // attempts=1 → 1 * 2^1 = 2 (2nd retry)
        task.attempts = 1;
        assert_eq!(backoff_delay(&task), 2, "attempts=1 should give 2s delay");

        // attempts=2 → 1 * 2^2 = 4 (3rd retry)
        task.attempts = 2;
        assert_eq!(backoff_delay(&task), 4, "attempts=2 should give 4s delay");

        // attempts=3 → 1 * 2^3 = 8 (4th retry)
        task.attempts = 3;
        assert_eq!(backoff_delay(&task), 8, "attempts=3 should give 8s delay");

        // attempts=4 → 1 * 2^4 = 16 (5th retry)
        task.attempts = 4;
        assert_eq!(backoff_delay(&task), 16, "attempts=4 should give 16s delay");

        // Cap kicks in at retry_delay * 60 = 60s.
        // attempts=5 → 1 * 2^5 = 32 (under cap)
        task.attempts = 5;
        assert_eq!(backoff_delay(&task), 32, "attempts=5 should give 32s delay");
        // attempts=6 → 1 * 2^6 = 64, capped at 60
        task.attempts = 6;
        assert_eq!(backoff_delay(&task), 60, "attempts=6 should hit the 60s cap");
        // attempts=10 → 1 * 2^10 = 1024, capped at 60
        task.attempts = 10;
        assert_eq!(backoff_delay(&task), 60, "attempts=10 should remain at 60s cap");
    }

    /// Exponential backoff (C8): when `retry_backoff=false` (default), the
    /// delay is always `retry_delay` regardless of attempts (backward compat).
    #[test]
    fn test_backoff_disabled_returns_fixed_delay() {
        let mut task = Task::new(TaskType::Http, serde_json::json!({}));
        task.retry_delay = 5;
        task.retry_backoff = false;
        for a in 0..5 {
            task.attempts = a;
            assert_eq!(backoff_delay(&task), 5,
                "attempts={} with backoff off should give fixed 5s delay", a);
        }
    }

    /// Exponential backoff (C8): with `retry_delay=2` the cap is `2 * 60 = 120s`.
    #[test]
    fn test_backoff_cap_scales_with_retry_delay() {
        let mut task = Task::new(TaskType::Http, serde_json::json!({}));
        task.retry_delay = 2;
        task.retry_backoff = true;
        // attempts=5 → 2 * 2^5 = 64 (under cap of 120)
        task.attempts = 5;
        assert_eq!(backoff_delay(&task), 64);
        // attempts=6 → 2 * 2^6 = 128, capped at 120
        task.attempts = 6;
        assert_eq!(backoff_delay(&task), 120, "attempts=6 with retry_delay=2 should cap at 120");
    }
}
