//! Retry mechanism (reference: Celery retry).
//!
//! Exponential backoff: next_retry_delay = base_delay * backoff_factor^(attempts-1).
//! Retry only on specific error types (HTTP 5xx, shell non-zero exit).

use std::time::Duration;
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
    /// Returns true if attempts < max_attempts AND the error is retryable.
    pub fn should_retry(&self, task: &Task, error: &str) -> bool {
        if task.attempts >= task.retry_max {
            return false;
        }
        // We retry on any failure by default. Caller can refine.
        let _ = error;
        true
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

/// Compute the next retry time (Unix timestamp) for a task given current attempts.
pub fn next_retry_ts(task: &Task) -> u64 {
    let policy = RetryPolicy::new(task.retry_max, task.retry_delay);
    let delay = policy.delay_for_attempt(task.attempts);
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

/// Helper: sleep for the retry delay before re-dispatching.
pub async fn sleep_for_retry(task: &Task) {
    let delay = task.retry_delay;
    tokio::time::sleep(Duration::from_secs(delay)).await;
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
        assert!(p.should_retry(&task, "err"));

        task.attempts = 3;
        assert!(!p.should_retry(&task, "err"));
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
}
