//! Task overlap control (reference: APScheduler max_instances / coalesce).
//!
//! - `allowOverlap(false)` (default): skip new trigger if previous run still executing.
//! - `maxInstances(N)`: allow up to N concurrent runs of the same task.
//!   Independent of `allowOverlap` (A10): when N>1 is set, it takes priority
//!   over `allowOverlap` and enforces a hard concurrency cap of N.
//! - `coalesce(true)` (default): merge missed triggers into one.
//! - `coalesce(false)`: use `misfire_grace_time` (default 60s) to decide whether
//!   to fire missed triggers.

use crate::errors::Result;
use crate::store::{Task, TaskStore};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct OverlapController {
    /// In-memory count of currently running instances per task id.
    /// This is the canonical source for concurrency limiting: it correctly
    /// tracks N>1 concurrent instances (unlike store.count_running_instances
    /// which is based on the single-row tasks table and can only return 0/1).
    /// Maintained by on_start / on_finish; reset to 0 on daemon restart (which
    /// is consistent with reset_running_to_pending clearing all Running state).
    running: Mutex<HashMap<String, u32>>,
}

impl OverlapController {
    pub fn new() -> Self {
        Self {
            running: Mutex::new(HashMap::new()),
        }
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
                // Use the in-memory counter (maintained by on_start/on_finish)
                // as the primary source — it correctly tracks N>1 concurrent
                // instances. store.count_running_instances is based on the
                // single-row tasks table and can only return 0/1, so it cannot
                // express max_instances>1 concurrency. We take the max of both
                // as a defensive measure (covers daemon-restart edge cases
                // where the store still has Running rows before
                // reset_running_to_pending runs).
                let mem_count = {
                    let g = self.running.lock().await;
                    g.get(&task.id).copied().unwrap_or(0)
                };
                let store_count = store.count_running_instances(&task.id).await?;
                let running_count = mem_count.max(store_count);
                if running_count >= limit {
                    tracing::debug!(
                        task_id = %task.id,
                        running = running_count,
                        mem_running = mem_count,
                        store_running = store_count,
                        max = limit,
                        "SKIP_OVERLAP"
                    );
                    // Record a MaxInstancesReached event so listeners can
                    // observe the concurrency cap being hit (previously this
                    // EventType variant was defined but never emitted).
                    let now = crate::store::now_ts() as i64;
                    if let Err(e) = store
                        .record_event(
                            &task.id,
                            crate::store::EventType::MaxInstancesReached,
                            None,
                            now,
                        )
                        .await
                    {
                        tracing::warn!(task_id = %task.id, error = %e, "record_event MaxInstancesReached failed");
                    }
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
            if *c > 0 {
                *c -= 1;
            }
            if *c == 0 {
                g.remove(task_id);
            }
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
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemoryStore, TaskState, TaskType};
    use std::sync::Arc;

    /// A10: with `max_instances=2`, the controller should allow up to 2 concurrent
    /// instances and skip the 3rd. Uses on_start to simulate concurrent instances
    /// (the in-memory HashMap correctly tracks N>1, unlike store.count_running_instances
    /// which is based on the single-row tasks table).
    /// Reference: APScheduler max_instances.
    #[tokio::test]
    async fn test_max_instances_allows_n_concurrent() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let overlap = OverlapController::new();

        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo"}));
        task.id = "t-max2".to_string();
        task.max_instances = 2;
        // allow_overlap stays false (default) — max_instances takes priority anyway.
        store.insert_task(task.clone()).await.unwrap();

        // 0 running: 0 < 2 → fire (1st instance allowed).
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "max_instances=2 with 0 running should fire (1st allowed)"
        );

        // Simulate 1st instance running.
        overlap.on_start(&task.id).await;
        // 1 running: 1 < 2 → fire (2nd instance allowed).
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "max_instances=2 with 1 running should fire (2nd allowed)"
        );

        // Simulate 2nd instance running.
        overlap.on_start(&task.id).await;
        // 2 running: 2 >= 2 → skip (3rd instance skipped).
        assert!(
            !overlap.should_fire(&store, &task).await.unwrap(),
            "max_instances=2 with 2 running should skip (3rd skipped)"
        );

        // Simulate 1 instance finishing: 1 running → fire again.
        overlap.on_finish(&task.id).await;
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "max_instances=2 with 1 running (after finish) should fire"
        );

        // Cleanup.
        overlap.on_finish(&task.id).await;
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
        store
            .update_state(&task.id, TaskState::Running, Some(0), None)
            .await
            .unwrap();
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
        store
            .update_state(&task.id, TaskState::Running, Some(0), None)
            .await
            .unwrap();
        assert!(
            overlap.should_fire(&store, &task).await.unwrap(),
            "allow_overlap=true with 1 running should still fire (unlimited backcompat)"
        );
    }
}
