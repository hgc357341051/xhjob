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
//! "succeeded".
//!
//! Chain state transitions: pending -> running -> succeeded / failed.

use crate::errors::{Result, XhjobError};
use crate::store::{ChainRecord, TaskStore};
use crate::store::now_ts;

/// Advance to the next step in the chain after a successful task execution.
///
/// Returns `Ok(Some(next_task_json))` when there is a next step to dispatch,
/// `Ok(None)` when the chain has completed (all steps done), or `Err` on
/// store error / invalid chain state.
pub async fn advance(
    store: &std::sync::Arc<dyn TaskStore>,
    chain_id: &str,
) -> Result<Option<serde_json::Value>> {
    let now = now_ts() as i64;
    let mut record = store.get_chain(chain_id).await?
        .ok_or_else(|| XhjobError::Store(format!("chain not found: {}", chain_id)))?;
    if record.state == "succeeded" || record.state == "failed" {
        return Ok(None);
    }
    let next_step = record.current_step;
    if (next_step as usize) >= record.tasks.len() {
        // All steps done: mark as succeeded.
        store.update_chain_step(chain_id, next_step, "succeeded", now).await?;
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
        // Fourth advance returns None (chain complete) and marks succeeded.
        let s3 = advance(&store, "c1").await.unwrap();
        assert!(s3.is_none(), "after last step advance returns None");
        let record = store.get_chain("c1").await.unwrap().unwrap();
        assert_eq!(record.state, "succeeded");
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
