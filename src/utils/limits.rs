//! Worker limits / counters (C5 + C8).
//!
//! Mirrors Celery's `worker_max_tasks_per_child` and
//! `worker_max_memory_per_child` semantics: when either limit is exceeded the
//! daemon shuts itself down so the supervisor (systemd / supervisord / PHP
//! caller of `xhjob_start`) can spawn a fresh process. The supervisor is
//! responsible for restarting; the daemon just initiates an orderly
//! shutdown.
//!
//! `tasks_executed` is the cumulative count of completed task executions
//! (success + failure) since daemon startup. It is exposed via the `stats`
//! IPC op for observability.
//!
//! A process-wide singleton is installed via [`init_worker_limits`] at daemon
//! startup so that the task queue (in `scheduler::queue`) can poll the
//! counters after each task completion without creating a circular module
//! dependency on `daemon_main`.
//!
//! Reference: Celery worker_max_tasks_per_child / worker_max_memory_per_child.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

static WORKER_LIMITS: OnceLock<Arc<WorkerLimits>> = OnceLock::new();

/// Worker limits / counters (C5 + C8).
#[derive(Debug)]
pub struct WorkerLimits {
    /// C5: max number of task executions per daemon lifetime. 0 = unlimited.
    pub max_tasks_per_child: u64,
    /// C8: max RSS (bytes) per daemon lifetime. 0 = unlimited.
    pub max_memory_per_child: u64,
    /// Cumulative count of completed task executions.
    pub tasks_executed: AtomicU64,
}

impl Default for WorkerLimits {
    fn default() -> Self {
        let max_tasks = std::env::var("XHJOB_MAX_TASKS_PER_CHILD")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(0);
        let max_mem = std::env::var("XHJOB_MAX_MEMORY_PER_CHILD")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(0);
        Self {
            max_tasks_per_child: max_tasks,
            max_memory_per_child: max_mem,
            tasks_executed: AtomicU64::new(0),
        }
    }
}

impl WorkerLimits {
    /// Increment the executed counter and return whether the daemon should
    /// shut down because the max_tasks_per_child limit was reached.
    pub fn record_task_execution(&self) -> bool {
        if self.max_tasks_per_child == 0 {
            self.tasks_executed.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let prev = self.tasks_executed.fetch_add(1, Ordering::Relaxed);
        prev + 1 >= self.max_tasks_per_child
    }

    /// Check whether the daemon should shut down because the
    /// max_memory_per_child limit was reached. Reads the current process RSS.
    /// Returns false when the limit is 0 (unlimited) or the RSS reader is
    /// unavailable on the current platform.
    pub fn check_memory_limit(&self) -> bool {
        if self.max_memory_per_child == 0 {
            return false;
        }
        match crate::utils::memory::current_rss_bytes() {
            Some(rss) if rss > 0 => rss > self.max_memory_per_child,
            _ => false,
        }
    }

    /// Snapshot the counters for the `stats` IPC op.
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "tasks_executed": self.tasks_executed.load(Ordering::Relaxed),
            "max_tasks_per_child": self.max_tasks_per_child,
            "max_memory_per_child": self.max_memory_per_child,
            "current_rss_bytes": crate::utils::memory::current_rss_bytes().unwrap_or(0),
        })
    }
}

/// Install the process-wide [`WorkerLimits`] singleton. Idempotent: returns
/// the existing instance if already installed. Called by the daemon at
/// startup.
pub fn init_worker_limits() -> Arc<WorkerLimits> {
    WORKER_LIMITS
        .get_or_init(|| Arc::new(WorkerLimits::default()))
        .clone()
}

/// Access the installed [`WorkerLimits`] singleton. Returns `None` if
/// [`init_worker_limits`] has not been called yet (e.g. in unit tests).
pub fn worker_limits() -> Option<&'static Arc<WorkerLimits>> {
    WORKER_LIMITS.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_task_execution_unlimited() {
        let l = WorkerLimits {
            max_tasks_per_child: 0,
            max_memory_per_child: 0,
            tasks_executed: AtomicU64::new(0),
        };
        assert!(!l.record_task_execution());
        assert!(!l.record_task_execution());
        assert_eq!(l.tasks_executed.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_record_task_execution_limit_reached() {
        let l = WorkerLimits {
            max_tasks_per_child: 2,
            max_memory_per_child: 0,
            tasks_executed: AtomicU64::new(0),
        };
        assert!(!l.record_task_execution()); // 1
        assert!(l.record_task_execution());  // 2 -> reached
    }
}
