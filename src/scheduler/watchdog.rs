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
                    "watchdog: hung task signal_cancel found no in-flight executor (will recheck state before marking Interrupted)"
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
            // Reload the task from the store before transitioning. The
            // `active` snapshot was taken once at the top of `scan_once`; in
            // the window between that snapshot and here the executor future
            // may have completed and transitioned the task to a terminal
            // state (Success / Failed / Cancelled / Expired). Marking it
            // Interrupted now would clobber that terminal state
            // (stale-snapshot clobber, High-severity). Only proceed when the
            // task is STILL Running. A task with no cancel flag registered
            // (signaled=false) that is still Running past the threshold is a
            // genuine hang — mark it Interrupted without relying on
            // signal_cancel having signaled. Key invariant: never overwrite
            // a terminal state with Interrupted.
            let reloaded = match self.store.load_task(&task_id).await? {
                Some(t) => t,
                None => {
                    tracing::warn!(
                        task_id = %task_id,
                        "watchdog: task vanished after signal_cancel, skipping Interrupted transition"
                    );
                    continue;
                }
            };
            if reloaded.state != TaskState::Running {
                tracing::info!(
                    task_id = %task_id,
                    current_state = ?reloaded.state,
                    "watchdog: task already left Running state, skipping Interrupted transition (no terminal-state clobber)"
                );
                continue;
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
    use crate::store::{
        ChainRecord, ChordRecord, GroupRecord, InMemoryStore, Task, TaskEvent, TaskResult,
        TaskSummary, TaskType, WorkerStats,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

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

    /// Test-only store wrapper that simulates a STALE `load_active_tasks`
    /// snapshot, reproducing the High-severity watchdog clobber race.
    ///
    /// `scan_once` snapshots `load_active_tasks()` once at the top of the
    /// loop, then iterates. In production a task can transition to a terminal
    /// state (Success / Failed / Cancelled / Expired) in the window between
    /// that snapshot and the watchdog's `update_state(Interrupted)` call —
    /// clobbering the terminal state back to Interrupted. This wrapper
    /// reproduces that race deterministically: `load_active_tasks` injects a
    /// cached "stale" Running snapshot for a registered task id (overriding
    /// the inner store's real, possibly-terminal, view), while `load_task`
    /// returns the real current state from the inner store. Used by
    /// `repro_watchdog_does_not_clobber_terminal_state`.
    struct StaleSnapshotStore {
        inner: Arc<InMemoryStore>,
        /// task_id -> stale Running snapshot injected into load_active_tasks.
        stale: Mutex<HashMap<String, Task>>,
    }

    impl StaleSnapshotStore {
        fn new() -> Self {
            Self {
                inner: Arc::new(InMemoryStore::new()),
                stale: Mutex::new(HashMap::new()),
            }
        }

        /// Register a stale Running snapshot for `id`. Subsequent
        /// `load_active_tasks` calls include this snapshot (as Running) even
        /// after the real task transitions to a terminal state.
        fn register_stale(&self, task: Task) {
            self.stale.lock().unwrap().insert(task.id.clone(), task);
        }

        /// Remove the stale snapshot for `id` so subsequent scans see only
        /// the real state from the inner store.
        fn clear_stale(&self, id: &str) {
            self.stale.lock().unwrap().remove(id);
        }
    }

    impl TaskStore for StaleSnapshotStore {
        fn load_active_tasks(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Task>>> + Send + '_>>
        {
            Box::pin(async move {
                let mut out = self.inner.load_active_tasks().await?;
                // Inject / override with stale Running snapshots. This
                // simulates the snapshot being captured BEFORE the task
                // transitioned to a terminal state.
                let stale = self.stale.lock().unwrap();
                for (id, task) in stale.iter() {
                    if let Some(pos) = out.iter().position(|t| &t.id == id) {
                        out[pos] = task.clone();
                    } else {
                        out.push(task.clone());
                    }
                }
                Ok(out)
            })
        }

        fn insert_task(
            &self,
            task: Task,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.insert_task(task)
        }

        fn update_state(
            &self,
            id: &str,
            state: TaskState,
            started_at: Option<u64>,
            finished_at: Option<u64>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_state(id, state, started_at, finished_at)
        }

        fn update_worker_pid(
            &self,
            id: &str,
            worker_pid: Option<u32>,
            starttime: Option<u64>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_worker_pid(id, worker_pid, starttime)
        }

        fn save_result(
            &self,
            task_id: &str,
            result: TaskResult,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.save_result(task_id, result)
        }

        fn load_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Task>>> + Send + '_>>
        {
            self.inner.load_task(id)
        }

        fn load_result(
            &self,
            id: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<TaskResult>>> + Send + '_>,
        > {
            self.inner.load_result(id)
        }

        fn count_running_instances(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
            self.inner.count_running_instances(id)
        }

        fn update_next_fire(
            &self,
            id: &str,
            next_fire: Option<u64>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_next_fire(id, next_fire)
        }

        fn set_attempts_and_error(
            &self,
            id: &str,
            attempts: u32,
            last_error: Option<String>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.set_attempts_and_error(id, attempts, last_error)
        }

        fn delete_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.delete_task(id)
        }

        fn increment_execution_count(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
            self.inner.increment_execution_count(id)
        }

        fn remove_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.remove_task(id)
        }

        fn set_paused(
            &self,
            id: &str,
            paused: bool,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.set_paused(id, paused)
        }

        fn cancel_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.cancel_task(id)
        }

        fn cleanup_expired_results(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
            self.inner.cleanup_expired_results()
        }

        fn list_tasks<'a>(
            &'a self,
            state_filter: Option<TaskState>,
            tag_filter: Option<&'a str>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + 'a>,
        > {
            self.inner.list_tasks(state_filter, tag_filter)
        }

        fn requeue_task(
            &self,
            id: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>>
        {
            self.inner.requeue_task(id)
        }

        fn reschedule_task(
            &self,
            id: &str,
            new_cron: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>>
        {
            self.inner.reschedule_task(id, new_cron)
        }

        fn reset_running_to_pending(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
            self.inner.reset_running_to_pending()
        }

        fn mark_running_as_interrupted(
            &self,
            reason: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
            self.inner.mark_running_as_interrupted(reason)
        }

        fn record_event(
            &self,
            task_id: &str,
            event_type: EventType,
            payload: Option<&str>,
            ts: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.record_event(task_id, event_type, payload, ts)
        }

        fn list_events(
            &self,
            since_ts: i64,
            task_id_filter: Option<&str>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskEvent>>> + Send + '_>>
        {
            self.inner.list_events(since_ts, task_id_filter)
        }

        fn cleanup_expired_events(
            &self,
            ttl_secs: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
            self.inner.cleanup_expired_events(ttl_secs)
        }

        fn create_chain(
            &self,
            chain_id: &str,
            tasks: &[serde_json::Value],
            created_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.create_chain(chain_id, tasks, created_at)
        }

        fn get_chain(
            &self,
            chain_id: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<ChainRecord>>> + Send + '_>,
        > {
            self.inner.get_chain(chain_id)
        }

        fn update_chain_step(
            &self,
            chain_id: &str,
            current_step: u32,
            state: &str,
            updated_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner
                .update_chain_step(chain_id, current_step, state, updated_at)
        }

        fn list_chains_by_state(
            &self,
            state: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<ChainRecord>>> + Send + '_>,
        > {
            self.inner.list_chains_by_state(state)
        }

        fn create_group(
            &self,
            group_id: &str,
            tasks: &[serde_json::Value],
            created_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.create_group(group_id, tasks, created_at)
        }

        fn get_group(
            &self,
            group_id: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<GroupRecord>>> + Send + '_>,
        > {
            self.inner.get_group(group_id)
        }

        fn update_group_state(
            &self,
            group_id: &str,
            state: &str,
            updated_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_group_state(group_id, state, updated_at)
        }

        fn list_groups_by_state(
            &self,
            state: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<GroupRecord>>> + Send + '_>,
        > {
            self.inner.list_groups_by_state(state)
        }

        fn create_chord(
            &self,
            id: &str,
            header_task_ids: &[String],
            callback_json: &str,
            created_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner
                .create_chord(id, header_task_ids, callback_json, created_at)
        }

        fn get_chord(
            &self,
            id: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Option<ChordRecord>>> + Send + '_>,
        > {
            self.inner.get_chord(id)
        }

        fn update_chord_state(
            &self,
            id: &str,
            state: &str,
            callback_task_id: Option<String>,
            updated_at: i64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner
                .update_chord_state(id, state, callback_task_id, updated_at)
        }

        fn update_progress(
            &self,
            id: &str,
            percent: u8,
            meta: Option<String>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_progress(id, percent, meta)
        }

        fn list_active_summary(
            &self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>,
        > {
            self.inner.list_active_summary()
        }

        fn list_registered_summary(
            &self,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>,
        > {
            self.inner.list_registered_summary()
        }

        fn list_scheduled_summary(
            &self,
            now: u64,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>,
        > {
            self.inner.list_scheduled_summary(now)
        }

        fn worker_stats(
            &self,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<WorkerStats>> + Send + '_>>
        {
            self.inner.worker_stats()
        }

        fn modify_job(
            &self,
            id: &str,
            patch: &serde_json::Value,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>>
        {
            self.inner.modify_job(id, patch)
        }
    }

    /// Repro for the High-severity stale-snapshot clobber in `scan_once`.
    ///
    /// Bug: `scan_once` snapshotted `load_active_tasks()` once, then for each
    /// Running task past the threshold it unconditionally called
    /// `update_state(Interrupted, ...)`. If the task had already completed to
    /// a terminal state (Success / Failed / Cancelled) in the window between
    /// the snapshot and the update — e.g. the executor future finished and
    /// transitioned the task — the watchdog clobbered that terminal state
    /// back to Interrupted. The fix reloads the task from the store after
    /// `signal_cancel` and skips the Interrupted transition unless the task
    /// is STILL Running (key invariant: never overwrite a terminal state
    /// with Interrupted).
    ///
    /// Because `load_active_tasks` excludes terminal states, a plain
    /// `InMemoryStore` cannot reproduce the race (a Success task never
    /// appears in the snapshot). `StaleSnapshotStore` decouples the snapshot
    /// (`load_active_tasks` returns a cached Running view) from the live
    /// state (`load_task` returns the real, terminal, state), deterministically
    /// reproducing the snapshot/transition window.
    ///
    /// The test seeds a Running task past the threshold, registers a stale
    /// Running snapshot, transitions the real task to Success (simulating the
    /// race window), runs `scan_once`, and asserts the task remains Success.
    /// It also verifies the positive case: a genuinely hung Running task past
    /// the threshold still gets Interrupted.
    #[tokio::test]
    async fn repro_watchdog_does_not_clobber_terminal_state() {
        let store_concrete = Arc::new(StaleSnapshotStore::new());
        let store: Arc<dyn TaskStore> = store_concrete.clone();
        let overlap = Arc::new(super::super::OverlapController::new());
        let queue = Arc::new(TaskQueue::new(Arc::clone(&store), overlap));
        let watchdog = Watchdog::new(
            Arc::clone(&store),
            Arc::clone(&queue),
            Duration::from_secs(5),
            2,
        );

        // --- Clobber case ---------------------------------------------------
        // timeout=2, factor=2 → threshold=4s. started 100s ago → hung by elapsed.
        let id = seed_running_task(&store, 2, 100).await;

        // Cache a stale Running snapshot. scan_once's load_active_tasks will
        // keep seeing this Running snapshot even after the real task moves to
        // a terminal state — reproducing the snapshot/transition race window.
        let stale_running = store.load_task(&id).await.unwrap().unwrap();
        store_concrete.register_stale(stale_running);

        // Simulate the race window: the executor completed and transitioned
        // the task to Success AFTER the watchdog snapshotted it as Running.
        store
            .update_state(&id, TaskState::Success, None, Some(now_ts()))
            .await
            .unwrap();

        // Run the watchdog scan. With the bug, the stale snapshot still has
        // the task as Running and scan_once clobbers Success → Interrupted.
        // With the fix, scan_once reloads the task, sees Success, and skips.
        watchdog.scan_once().await.unwrap();

        let task = store.load_task(&id).await.unwrap().unwrap();
        assert_eq!(
            task.state,
            TaskState::Success,
            "task that already reached Success must NOT be clobbered to \
             Interrupted by the watchdog (stale-snapshot clobber, High-severity)"
        );

        // --- Positive case --------------------------------------------------
        // A genuinely hung Running task past the threshold still gets
        // Interrupted (the reload sees Running and proceeds). Drop the stale
        // snapshot for `id` so this scan only sees the fresh Running task.
        store_concrete.clear_stale(&id);
        let id2 = seed_running_task(&store, 2, 100).await;

        watchdog.scan_once().await.unwrap();

        let task2 = store.load_task(&id2).await.unwrap().unwrap();
        assert_eq!(
            task2.state,
            TaskState::Interrupted,
            "genuinely hung Running task must still be Interrupted by the watchdog"
        );

        // A HungDetected event must be present for the genuinely hung task.
        let events = store.list_events(0, Some(&id2)).await.unwrap();
        let has_hung = events
            .iter()
            .any(|e| e.event_type == EventType::HungDetected);
        assert!(
            has_hung,
            "HungDetected event must be recorded for the genuinely hung task"
        );
    }
}
