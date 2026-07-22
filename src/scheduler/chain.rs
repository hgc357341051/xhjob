//! Task chain (C15). Sequential pipeline of tasks where each task's stdout
//! is fed into the next task's input.
//!
//! Reference: Celery `chain(t1, t2, t3)`.
//!
//! The chain record is persisted as a list of `TaskBuilder` JSON configs.
//! After each task completes successfully, the daemon advances
//! `current_step` and dispatches the next task in the chain. On any step
//! failure, the chain state becomes "failed" and remaining steps are
//! skipped. After all steps complete successfully, the chain state becomes
//! "success".
//!
//! Chain state transitions: pending -> running -> success / failed
//! （与 TaskState::as_str() 一致，统一为小写）。

use crate::errors::{Result, XhjobError};
use crate::store::{ChainRecord, TaskStore};
use crate::store::now_ts;
use std::collections::HashMap;
use tokio::sync::Mutex;

/// P0 fix: per-chain mutex map. Prevents the check-then-act race in
/// advance() where two concurrent task completions could both read
/// current_step=N, both return tasks[N], and both increment current_step
/// to N+2 — skipping a step and duplicating a step. The mutex serializes
/// advance per chain_id so only one caller reads + increments current_step.
static CHAIN_LOCKS: std::sync::OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = std::sync::OnceLock::new();

fn chain_locks() -> &'static Mutex<HashMap<String, std::sync::Arc<Mutex<()>>>> {
    CHAIN_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn get_chain_lock(chain_id: &str) -> std::sync::Arc<Mutex<()>> {
    let mut map = chain_locks().lock().await;
    map.entry(chain_id.to_string())
        .or_insert_with(|| std::sync::Arc::new(Mutex::new(())))
        .clone()
}

use std::sync::Arc;

/// Advance to the next step in the chain after a successful task execution.
///
/// Returns `Ok(Some(next_task_json))` when there is a next step to dispatch,
/// `Ok(None)` when the chain has completed (all steps done), or `Err` on
/// store error / invalid chain state.
pub async fn advance(
    store: &std::sync::Arc<dyn TaskStore>,
    chain_id: &str,
) -> Result<Option<serde_json::Value>> {
    // P0 fix: acquire per-chain mutex before the check-then-act sequence.
    let lock = get_chain_lock(chain_id).await;
    let _guard = lock.lock().await;

    let now = now_ts() as i64;
    let mut record = store.get_chain(chain_id).await?
        .ok_or_else(|| XhjobError::store(format!("chain not found: {}", chain_id)))?;
    if record.state == "success" || record.state == "failed" {
        return Ok(None);
    }
    let next_step = record.current_step;
    if (next_step as usize) >= record.tasks.len() {
        // All steps done: mark as success.
        store.update_chain_step(chain_id, next_step, "success", now).await?;
        return Ok(None);
    }
    // Mark as running (first time) and pick the next task config.
    if record.state == "pending" {
        store.update_chain_step(chain_id, next_step, "running", now).await?;
        record.state = "running".to_string();
    }
    let next_task = record.tasks[next_step as usize].clone();
    // Advance current_step for the next call.
    store.update_chain_step(chain_id, next_step + 1, &record.state, now).await?;
    Ok(Some(next_task))
}

/// Mark the chain as failed (called when a step's task fails).
pub async fn mark_failed(
    store: &std::sync::Arc<dyn TaskStore>,
    chain_id: &str,
) -> Result<()> {
    let now = now_ts() as i64;
    store.update_chain_step(chain_id, 0, "failed", now).await?;
    Ok(())
}

/// Inspect the chain record by id.
pub async fn inspect(
    store: &std::sync::Arc<dyn TaskStore>,
    chain_id: &str,
) -> Result<Option<ChainRecord>> {
    store.get_chain(chain_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::InMemoryStore;
    use serde_json::json;

    #[tokio::test]
    async fn test_advance_returns_steps_in_order() {
        let store: std::sync::Arc<dyn TaskStore> = std::sync::Arc::new(InMemoryStore::new());
        let tasks = vec![json!({"cmd":"step1"}), json!({"cmd":"step2"}), json!({"cmd":"step3"})];
        store.create_chain("c1", &tasks, now_ts() as i64).await.unwrap();
        // First advance returns step 0.
        let s0 = advance(&store, "c1").await.unwrap().expect("step0");
        assert_eq!(s0, json!({"cmd":"step1"}));
        // Second advance returns step 1.
        let s1 = advance(&store, "c1").await.unwrap().expect("step1");
        assert_eq!(s1, json!({"cmd":"step2"}));
        // Third advance returns step 2.
        let s2 = advance(&store, "c1").await.unwrap().expect("step2");
        assert_eq!(s2, json!({"cmd":"step3"}));
        // Fourth advance returns None (chain complete) and marks success.
        let s3 = advance(&store, "c1").await.unwrap();
        assert!(s3.is_none(), "after last step advance returns None");
        let record = store.get_chain("c1").await.unwrap().unwrap();
        assert_eq!(record.state, "success");
        assert_eq!(record.current_step, 3);
    }

    #[tokio::test]
    async fn test_mark_failed_sets_failed_state() {
        let store: std::sync::Arc<dyn TaskStore> = std::sync::Arc::new(InMemoryStore::new());
        let tasks = vec![json!({"cmd":"s1"}), json!({"cmd":"s2"})];
        store.create_chain("c-fail", &tasks, now_ts() as i64).await.unwrap();
        mark_failed(&store, "c-fail").await.unwrap();
        let record = store.get_chain("c-fail").await.unwrap().unwrap();
        assert_eq!(record.state, "failed");
        // advance after failure returns None.
        let next = advance(&store, "c-fail").await.unwrap();
        assert!(next.is_none());
    }
}
