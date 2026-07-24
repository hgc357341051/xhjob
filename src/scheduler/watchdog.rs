//! Running-task watchdog: detects fake-death (hung) tasks.
//!
//! Periodically scans all `Running` tasks. When a task's elapsed runtime
//! exceeds `timeout * factor`, the watchdog:
//!   1. Signals cancel via the queue (terminates the orphan child process),
//!   2. Marks the task `Interrupted` (terminal-ish; reset on next startup),
//!   3. Records an `EventType::HungDetected` event.
//!
//! This closes the blind spot where a child process hangs on IO (stuck read
//! on a fifo / NFS / DNS) without tripping the tokio `timeout` future: the
//! task stays `Running` forever and only a daemon restart can recover it.
//!
//! Configuration (env vars, read in `daemon_main`):
//!   - `XHJOB_WATCHDOG_INTERVAL` (seconds, default 5): scan period. `0`
//!     disables the watchdog entirely (no background task spawned).
//!   - `XHJOB_WATCHDOG_FACTOR` (default 2): tolerance multiplier. A task is
//!     only interrupted after `timeout * factor` seconds, so a task that
//!     legitimately runs slightly past its `timeout` (e.g. mid-SIGTERM grace)
//!     is not mis-killed.
//!
//! Tasks with `timeout == 0` (no timeout configured) are skipped — without a
//! timeout there is no baseline to judge "hung" against.

use super::queue::TaskQueue;
use crate::errors::Result;
use crate::store::{now_ts, EventType, TaskState, TaskStore};
use std::sync::Arc;
use std::time::Duration;

/// Watchdog that periodically scans `Running` tasks for fake-death.
pub struct Watchdog {
    store: Arc<dyn TaskStore>,
    queue: Arc<TaskQueue>,
    interval: Duration,
    factor: u64,
    /// Shutdown signal. `stop()` notifies all waiters; the scan loop
    /// `select!`s against this so shutdown is prompt (no full interval wait).
    shutdown: Arc<tokio::sync::Notify>,
}

impl Watchdog {
    /// Create a new watchdog. `interval` is the scan period; `factor` is the
    /// tolerance multiplier applied to each task's `timeout`.
    pub fn new(
        store: Arc<dyn TaskStore>,
        queue: Arc<TaskQueue>,
        interval: Duration,
        factor: u64,
    ) -> Self {
        Self {
            store,
            queue,
            interval,
            factor,
            shutdown: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Start the background scan loop. Returns the `JoinHandle` so the caller
    /// can await its termination during shutdown. Consumes the `Arc<Watchdog>`
    /// so the handle owns the watchdog for the loop's lifetime.
    ///
    /// If `interval` is zero the watchdog is disabled: no task is spawned and
    /// a no-op handle is returned (matches the spec's "interval=0 disables").
    pub fn start(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        // interval=0 → watchdog disabled. Return an already-completed handle
        // so `daemon_main`'s `watchdog_handle.await` is a no-op.
        if self.interval.is_zero() {
            tracing::info!("watchdog disabled (interval=0)");
            return tokio::spawn(async {});
        }
        tracing::info!(
            interval_secs = self.interval.as_secs(),
            factor = self.factor,
            "watchdog started"
        );
        let this = Arc::clone(&self);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(this.interval) => {
                        if let Err(e) = this.scan_once().await {
                            tracing::warn!(error = %e, "watchdog scan_once failed");
                        }
                    }
                    _ = this.shutdown.notified() => {
                        tracing::info!("watchdog shutting down");
                        break;
                    }
                }
            }
        })
    }

    /// Signal the scan loop to stop. The `start` handle will complete shortly
    /// after (once the `select!` wakes on the notify).
    pub fn stop(&self) {
        self.shutdown.notify_waiters();
    }

    /// Scan all `Running` tasks once. For each task whose elapsed runtime
    /// exceeds `timeout * factor`, signal cancel + mark `Interrupted` +
    /// record `HungDetected`. Tasks with `timeout == 0` or no `started_at`
    /// are skipped.
    async fn scan_once(&self) -> Result<()> {
        let active = self.store.load_active_tasks().await?;
        let now = now_ts();
        for task in &active {
            // Only inspect Running tasks. Pending / Interrupted tasks are
            // not executing and have no worker to interrupt.
            if task.state != TaskState::Running {
                continue;
            }
            // timeout=0 means "no timeout" — there is no baseline to judge
            // hung-ness against, so skip (otherwise factor*0=0 would
            // immediately interrupt every such task).
            if task.timeout == 0 {
                continue;
            }
            let started = match task.started_at {
                Some(t) => t,
                None => continue, // no started_at → not actually running yet
            };
            let elapsed = now.saturating_sub(started);
            let threshold = task.timeout.saturating_mul(self.factor);
            if elapsed <= threshold {
                continue;
            }
            // Hung: elapsed runtime exceeds timeout * factor. Best-effort
            // signal the executor to terminate the child process (returns
            // false if no cancel flag is registered, e.g. the task future
            // already completed but state wasn't updated — we still mark
            // Interrupted + record the event below).
            let task_id = task.id.clone();
            let signaled = self.queue.signal_cancel(&task_id).await;
            if !signaled {
                tracing::warn!(
                    task_id = %task_id,
                    elapsed_secs = elapsed,
                    timeout_secs = task.timeout,
                    factor = self.factor,
                    "watchdog: hung task signal_cancel found no in-flight executor (marking Interrupted anyway)"
                );
            } else {
                tracing::warn!(
                    task_id = %task_id,
                    elapsed_secs = elapsed,
                    timeout_secs = task.timeout,
                    factor = self.factor,
                    "watchdog: hung task detected, signaled cancel"
                );
            }
            // Transition to Interrupted (finished_at = now). started_at is
            // preserved so operators can see how long it ran.
            if let Err(e) = self
                .store
                .update_state(&task_id, TaskState::Interrupted, None, Some(now))
                .await
            {
                tracing::warn!(task_id = %task_id, error = %e, "watchdog: update_state to Interrupted failed");
            }
            // Record a HungDetected event so the audit log shows *why* the
            // task was interrupted (distinguishes watchdog kill from
            // graceful-shutdown Interrupted).
            if let Err(e) = self
                .store
                .record_event(&task_id, EventType::HungDetected, None, now as i64)
                .await
            {
                tracing::warn!(task_id = %task_id, error = %e, "watchdog: record_event HungDetected failed");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemoryStore, Task, TaskType};

    /// Build a `Running` task with the given `timeout` (seconds) and
    /// `started_at` offset (seconds ago from now), insert it into the store,
    /// and return its id.
    async fn seed_running_task(
        store: &Arc<dyn TaskStore>,
        timeout: u64,
        started_secs_ago: u64,
    ) -> String {
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "sleep 999"}));
        task.timeout = timeout;
        task.state = TaskState::Running;
        task.started_at = Some(now_ts().saturating_sub(started_secs_ago));
        let id = task.id.clone();
        store.insert_task(task).await.unwrap();
        id
    }

    /// A hung task (elapsed > timeout * factor) is interrupted and a
    /// `HungDetected` event is recorded.
    #[tokio::test]
    async fn test_watchdog_interrupts_hung_task() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(super::super::OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), overlap));
        let watchdog = Watchdog::new(
            Arc::clone(&store),
            Arc::clone(&queue),
            Duration::from_secs(5),
            2,
        );

        // timeout=2, factor=2 → threshold=4s. started 100s ago → hung.
        let id = seed_running_task(&store, 2, 100).await;

        watchdog.scan_once().await.unwrap();

        let task = store.load_task(&id).await.unwrap().unwrap();
        assert_eq!(
            task.state,
            TaskState::Interrupted,
            "hung task must be Interrupted after scan"
        );

        // A HungDetected event must be present in the event log.
        let events = store.list_events(0, Some(&id)).await.unwrap();
        let has_hung = events
            .iter()
            .any(|e| e.event_type == EventType::HungDetected);
        assert!(
            has_hung,
            "HungDetected event must be recorded for the hung task"
        );
    }

    /// A normally-running task (elapsed <= timeout * factor) is left alone.
    #[tokio::test]
    async fn test_watchdog_skips_normal_task() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(super::super::OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), overlap));
        let watchdog = Watchdog::new(
            Arc::clone(&store),
            Arc::clone(&queue),
            Duration::from_secs(5),
            2,
        );

        // timeout=100, factor=2 → threshold=200s. started 1s ago → not hung.
        let id = seed_running_task(&store, 100, 1).await;

        watchdog.scan_once().await.unwrap();

        let task = store.load_task(&id).await.unwrap().unwrap();
        assert_eq!(
            task.state,
            TaskState::Running,
            "normal task must remain Running"
        );

        let events = store.list_events(0, Some(&id)).await.unwrap();
        assert!(
            events.is_empty(),
            "no events should be recorded for a normal task"
        );
    }

    /// A task with `timeout == 0` (no timeout) is never interrupted, even if
    /// it has been running for a long time. Without a timeout there is no
    /// baseline to judge "hung" against.
    #[tokio::test]
    async fn test_watchdog_skips_zero_timeout_task() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = Arc::new(super::super::OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), overlap));
        let watchdog = Watchdog::new(
            Arc::clone(&store),
            Arc::clone(&queue),
            Duration::from_secs(5),
            2,
        );

        // timeout=0 → skipped regardless of elapsed time.
        let id = seed_running_task(&store, 0, 1000).await;

        watchdog.scan_once().await.unwrap();

        let task = store.load_task(&id).await.unwrap().unwrap();
        assert_eq!(
            task.state,
            TaskState::Running,
            "timeout=0 task must not be interrupted"
        );

        let events = store.list_events(0, Some(&id)).await.unwrap();
        assert!(events.is_empty(), "no events for timeout=0 task");
    }
}
