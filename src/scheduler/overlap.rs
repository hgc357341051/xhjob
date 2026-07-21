//! Task overlap control (reference: APScheduler max_instances / coalesce).
//!
//! - `allowOverlap(false)` (default): skip new trigger if previous run still executing.
//! - `maxInstances(N)`: allow up to N concurrent runs of the same task.
//!   Independent of `allowOverlap` (A10): when N>1 is set, it takes priority
//!   over `allowOverlap` and enforces a hard concurrency cap of N.
//! - `coalesce(true)` (default): merge missed triggers into one.
//! - `coalesce(false)`: use `misfire_grace_time` (default 60s) to decide whether
//!   to fire missed triggers.

use std::sync::Arc;
use tokio::sync::Mutex;
use std::collections::HashMap;
use crate::errors::Result;
use crate::store::{Task, TaskStore};

pub struct OverlapController {
    /// In-memory count of currently running instances per task id.
    /// (used as a fast-path cache; the canonical source is the store.)
    running: Mutex<HashMap<String, u32>>,
}

impl OverlapController {
    pub fn new() -> Self {
        Self { running: Mutex::new(HashMap::new()) }
    }

    /// Check whether a task may be dispatched now.
    /// Returns true if allowed, false if should be skipped (SKIP_OVERLAP).
    ///
    /// Concurrency rules (A10 — `max_instances` is independent of `allow_overlap`):
    /// - `max_instances > 1` (explicitly set): enforce hard cap of N concurrent
    ///   instances. Takes priority over `allow_overlap`. If `allow_overlap=true`
    ///   is also set, emit a warn (max_instances wins).
    /// - `max_instances == 1` (default) + `allow_overlap == true`: unlimited
    ///   concurrency (backward compat with the original "allow overlap" semantics).
    /// - `max_instances == 1` (default) + `allow_overlap == false`: at most 1
    ///   concurrent instance (backward compat with the original "no overlap" semantics).
    pub async fn should_fire(&self, store: &Arc<dyn TaskStore>, task: &Task) -> Result<bool> {
        let max_instances = if task.max_instances > 1 {
            // A10: max_instances(N>1) is independent of allow_overlap and takes priority.
            if task.allow_overlap {
                tracing::warn!(
                    task_id = %task.id,
                    max_instances = task.max_instances,
                    "both allow_overlap=true and max_instances={} are set; using max_instances as the concurrency cap (max_instances takes priority)",
                    task.max_instances
                );
            }
            Some(task.max_instances)
        } else if task.allow_overlap {
            // max_instances == 1 (default) + allow_overlap=true: unlimited (backcompat).
            None
        } else {
            // max_instances == 1 (default) + allow_overlap=false: 1 instance (backcompat).
            Some(1)
        };

        match max_instances {
            None => Ok(true), // unlimited concurrency
            Some(limit) => {
                let running_count = store.count_running_instances(&task.id).await?;
                if running_count >= limit {
                    tracing::debug!(
                        task_id = %task.id,
                        running = running_count,
                        max = limit,
                        "SKIP_OVERLAP"
                    );
                    return Ok(false);
                }
                Ok(true)
            }
        }
    }

    /// Mark a task as starting execution (increment running count).
    pub async fn on_start(&self, task_id: &str) {
        let mut g = self.running.lock().await;
        *g.entry(task_id.to_string()).or_insert(0) += 1;
    }

    /// Mark a task as finished execution (decrement running count).
    pub async fn on_finish(&self, task_id: &str) {
        let mut g = self.running.lock().await;
        if let Some(c) = g.get_mut(task_id) {
            if *c > 0 { *c -= 1; }
            if *c == 0 { g.remove(task_id); }
        }
    }

    /// 判断错过的触发是否应当补执行（保留为未来 cron misfire 策略扩展）。
    /// 当前 misfire 通过 coalesce + grace_time 处理（scan_once 内联判断）；
    /// 未来若需更复杂策略（如 default_replace / max_interval），可启用此函数。
    ///
    /// `missed_count` = number of missed fire times since last execution.
    /// `last_missed_ts` = unix timestamp of the most recent missed fire time.
    /// `now_ts` = current unix timestamp.
    /// `coalesce` = whether to merge missed into one.
    /// `misfire_grace_time` = seconds; if last_missed_ts is older than now - grace, skip.
    #[allow(dead_code)]
    pub fn should_fire_missed(
        coalesce: bool,
        misfire_grace_time: u64,
        last_missed_ts: u64,
        now_ts: u64,
    ) -> bool {
        if now_ts.saturating_sub(last_missed_ts) > misfire_grace_time {
            return false;
        }
        let _ = coalesce; // coalesce=true collapses N misses into 1 fire (already done by caller)
        true
    }
}

impl Default for OverlapController {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemoryStore, TaskState, TaskType};
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use tokio::sync::Mutex as AsyncMutex;

    /// A test-only wrapper around `InMemoryStore` that allows the test to
    /// inject a fixed return value for `count_running_instances`. All other
    /// `TaskStore` methods delegate to the inner store.
    ///
    /// This is required because the in-memory data model only tracks one row
    /// per task ID, so `count_running_instances` can only ever return 0 or 1.
    /// To exercise the `max_instances=N>1` boundary (e.g. "3rd instance skipped
    /// when N=2"), we need to be able to return a count of 2.
    struct CountOverrideStore {
        inner: Arc<InMemoryStore>,
        count_override: Arc<AsyncMutex<Option<u32>>>,
    }

    impl CountOverrideStore {
        fn new() -> Self {
            Self {
                inner: Arc::new(InMemoryStore::new()),
                count_override: Arc::new(AsyncMutex::new(None)),
            }
        }

        async fn set_count(&self, c: Option<u32>) {
            *self.count_override.lock().await = c;
        }
    }

    impl TaskStore for CountOverrideStore {
        fn insert_task(&self, task: Task) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.insert_task(task)
        }
        fn update_state(&self, id: &str, state: TaskState, started_at: Option<u64>, finished_at: Option<u64>) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_state(id, state, started_at, finished_at)
        }
        fn save_result(&self, task_id: &str, result: crate::store::TaskResult) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.save_result(task_id, result)
        }
        fn load_active_tasks(&self) -> Pin<Box<dyn Future<Output = Result<Vec<Task>>> + Send + '_>> {
            self.inner.load_active_tasks()
        }
        fn load_task(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<Option<Task>>> + Send + '_>> {
            self.inner.load_task(id)
        }
        fn load_result(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<Option<crate::store::TaskResult>>> + Send + '_>> {
            self.inner.load_result(id)
        }
        fn count_running_instances(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<u32>> + Send + '_>> {
            let id = id.to_string();
            let inner = Arc::clone(&self.inner);
            let ov = Arc::clone(&self.count_override);
            Box::pin(async move {
                let guard = ov.lock().await;
                if let Some(c) = *guard {
                    return Ok(c);
                }
                drop(guard);
                inner.count_running_instances(&id).await
            })
        }
        fn update_next_fire(&self, id: &str, next_fire: Option<u64>) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_next_fire(id, next_fire)
        }
        fn set_attempts_and_error(&self, id: &str, attempts: u32, last_error: Option<String>) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.set_attempts_and_error(id, attempts, last_error)
        }
        fn delete_task(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.delete_task(id)
        }
        fn increment_execution_count(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<u32>> + Send + '_>> {
            self.inner.increment_execution_count(id)
        }
        fn remove_task(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.remove_task(id)
        }
        fn set_paused(&self, id: &str, paused: bool) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.set_paused(id, paused)
        }
        fn cancel_task(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.cancel_task(id)
        }
        fn cleanup_expired_results(&self) -> Pin<Box<dyn Future<Output = Result<u64>> + Send + '_>> {
            self.inner.cleanup_expired_results()
        }
        fn list_tasks<'a>(&'a self, state_filter: Option<TaskState>, tag_filter: Option<&'a str>) -> Pin<Box<dyn Future<Output = Result<Vec<crate::store::TaskSummary>>> + Send + 'a>> {
            self.inner.list_tasks(state_filter, tag_filter)
        }
        fn requeue_task(&self, id: &str) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + '_>> {
            self.inner.requeue_task(id)
        }
        fn reschedule_task(&self, id: &str, new_cron: &str) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + '_>> {
            self.inner.reschedule_task(id, new_cron)
        }
        fn reset_running_to_pending(&self) -> Pin<Box<dyn Future<Output = Result<u64>> + Send + '_>> {
            self.inner.reset_running_to_pending()
        }
        fn record_event(&self, task_id: &str, event_type: crate::store::EventType, payload: Option<&str>, ts: i64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.record_event(task_id, event_type, payload, ts)
        }
        fn list_events(&self, since_ts: i64, task_id_filter: Option<&str>) -> Pin<Box<dyn Future<Output = Result<Vec<crate::store::TaskEvent>>> + Send + '_>> {
            self.inner.list_events(since_ts, task_id_filter)
        }
        fn cleanup_expired_events(&self, ttl_secs: u64) -> Pin<Box<dyn Future<Output = Result<u64>> + Send + '_>> {
            self.inner.cleanup_expired_events(ttl_secs)
        }
        fn create_chain(&self, chain_id: &str, tasks: &[serde_json::Value], created_at: i64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.create_chain(chain_id, tasks, created_at)
        }
        fn get_chain(&self, chain_id: &str) -> Pin<Box<dyn Future<Output = Result<Option<crate::store::ChainRecord>>> + Send + '_>> {
            self.inner.get_chain(chain_id)
        }
        fn update_chain_step(&self, chain_id: &str, current_step: u32, state: &str, updated_at: i64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_chain_step(chain_id, current_step, state, updated_at)
        }
        fn list_chains_by_state(&self, state: &str) -> Pin<Box<dyn Future<Output = Result<Vec<crate::store::ChainRecord>>> + Send + '_>> {
            self.inner.list_chains_by_state(state)
        }
        fn create_group(&self, group_id: &str, tasks: &[serde_json::Value], created_at: i64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.create_group(group_id, tasks, created_at)
        }
        fn get_group(&self, group_id: &str) -> Pin<Box<dyn Future<Output = Result<Option<crate::store::GroupRecord>>> + Send + '_>> {
            self.inner.get_group(group_id)
        }
        fn update_group_state(&self, group_id: &str, state: &str, updated_at: i64) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.inner.update_group_state(group_id, state, updated_at)
        }
        fn list_groups_by_state(&self, state: &str) -> Pin<Box<dyn Future<Output = Result<Vec<crate::store::GroupRecord>>> + Send + '_>> {
            self.inner.list_groups_by_state(state)
        }
    }

    /// A10: with `max_instances=2`, the controller should allow up to 2 concurrent
    /// instances and skip the 3rd. Uses a `CountOverrideStore` to simulate a
    /// running-count of 2 (which the in-memory data model cannot represent
    /// directly because it tracks one row per task id).
    /// Reference: APScheduler max_instances.
    #[tokio::test]
    async fn test_max_instances_allows_n_concurrent() {
        let store_concrete = Arc::new(CountOverrideStore::new());
        let store: Arc<dyn TaskStore> = store_concrete.clone();
        let overlap = OverlapController::new();

        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo"}));
        task.id = "t-max2".to_string();
        task.max_instances = 2;
        // allow_overlap stays false (default) — max_instances takes priority anyway.
        store_concrete.inner.insert_task(task.clone()).await.unwrap();

        // count=0 (Pending): 0 < 2 → fire (1st instance allowed).
        store_concrete.set_count(Some(0)).await;
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "max_instances=2 with 0 running should fire (1st allowed)"
        );

        // count=1 (one Running): 1 < 2 → fire (2nd instance allowed).
        store_concrete.set_count(Some(1)).await;
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "max_instances=2 with 1 running should fire (2nd allowed)"
        );

        // count=2 (two Running): 2 >= 2 → skip (3rd instance skipped).
        store_concrete.set_count(Some(2)).await;
        assert!(
            !overlap.should_fire(&store, &task).await.unwrap(),
            "max_instances=2 with 2 running should skip (3rd skipped)"
        );
    }

    /// A10 backward compat: default `max_instances=1` + `allow_overlap=false`
    /// should behave as "at most 1 concurrent instance" (the original
    /// no-overlap behavior).
    #[tokio::test]
    async fn test_max_instances_default_1_no_overlap() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = OverlapController::new();

        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo"}));
        task.id = "t-default-no-overlap".to_string();
        // defaults: max_instances=1, allow_overlap=false
        store.insert_task(task.clone()).await.unwrap();

        // Pending (count=0): 0 < 1 → fire.
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "default max_instances=1 + allow_overlap=false with 0 running should fire"
        );

        // Running (count=1): 1 >= 1 → skip.
        store.update_state(&task.id, TaskState::Running, Some(0), None).await.unwrap();
        assert!(
            !overlap.should_fire(&store, &task).await.unwrap(),
            "default max_instances=1 + allow_overlap=false with 1 running should skip"
        );
    }

    /// A10 backward compat: `allow_overlap=true` alone (default max_instances=1)
    /// should still allow unlimited concurrency (the original allow-overlap
    /// semantics).
    #[tokio::test]
    async fn test_allow_overlap_true_unlimited_concurrent_backcompat() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = OverlapController::new();

        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo"}));
        task.id = "t-overlap-true".to_string();
        task.allow_overlap = true;
        // max_instances stays at default 1.
        store.insert_task(task.clone()).await.unwrap();

        // Pending (count=0): should fire.
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "allow_overlap=true with 0 running should fire"
        );

        // Running (count=1): should STILL fire (unlimited concurrency backcompat).
        store.update_state(&task.id, TaskState::Running, Some(0), None).await.unwrap();
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "allow_overlap=true with 1 running should still fire (unlimited backcompat)"
        );
    }
}
