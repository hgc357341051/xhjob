//! Task overlap control (reference: APScheduler max_instances / coalesce).
//!
//! - `allowOverlap(false)` (default): skip new trigger if previous run still executing.
//! - `maxInstances(N)`: allow up to N concurrent runs of the same task.
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
    pub async fn should_fire(&self, store: &Arc<dyn TaskStore>, task: &Task) -> Result<bool> {
        let max_instances = if task.allow_overlap {
            task.max_instances.max(1)
        } else {
            1
        };

        let running_count = store.count_running_instances(&task.id).await?;
        if running_count >= max_instances {
            tracing::debug!(
                task_id = %task.id,
                running = running_count,
                max = max_instances,
                "SKIP_OVERLAP"
            );
            return Ok(false);
        }
        Ok(true)
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

    /// Decide whether to fire a missed trigger based on coalesce + misfire_grace_time.
    /// `missed_count` = number of missed fire times since last execution.
    /// `last_missed_ts` = unix timestamp of the most recent missed fire time.
    /// `now_ts` = current unix timestamp.
    /// `coalesce` = whether to merge missed into one.
    /// `misfire_grace_time` = seconds; if last_missed_ts is older than now - grace, skip.
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
