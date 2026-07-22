//! Task chord (C16+). Header (parallel) + body (callback).
//!
//! Reference: Celery `chord(header, body)`.
//!
//! The chord record persists:
//! - `header_task_ids`: the ids of the parallel header tasks
//! - `callback_json`: the serialized `TaskBuilder` JSON for the body
//!
//! State transitions:
//! - `pending` (initial) -> `running` (some header tasks still in flight)
//! - `running` -> `success` (all header tasks succeeded -> body dispatched)
//! - `running` -> `partial_failed` (any header task failed / cancelled /
//!   expired; the body is NOT dispatched)
//!
//! The callback task's `meta` is set to a JSON array of `{ id, result }`
//! objects carrying each header task's result (stdout / stderr / exit_code /
//! body / status_code). The callback task is inserted into the store by
//! `refresh_state` and the new task id is returned via
//! `ChordRefreshResult::callback_task_id`; the caller (the daemon's queue
//! completion hook) is responsible for enqueuing it on the in-memory queue.

use crate::errors::Result;
use crate::store::{ChordRecord, TaskState, TaskStore, now_ts};
use crate::task::TaskBuilder;
use std::sync::Arc;

/// Result of a `refresh_state` call. The caller inspects `new_state` and,
/// when `callback_task_id` is `Some`, loads that task from the store and
/// enqueues it for execution.
#[derive(Debug, Clone)]
pub struct ChordRefreshResult {
    /// The chord's new state: "pending" / "running" / "success" /
    /// "partial_failed".
    pub new_state: &'static str,
    /// When the chord just succeeded and the body was dispatched, this holds
    /// the callback task's id. The caller must enqueue it. `None` otherwise.
    pub callback_task_id: Option<String>,
}

/// Inspect the chord record by id.
pub async fn inspect(
    store: &Arc<dyn TaskStore>,
    chord_id: &str,
) -> Result<Option<ChordRecord>> {
    store.get_chord(chord_id).await
}

/// Compute the chord state based on the live header task states. Called by
/// the daemon after each header task completes (linked via the
/// `task.chord_id` field set by `handle_chord_op`).
///
/// Returns a `ChordRefreshResult`:
/// - If all header tasks succeed: dispatches the callback (inserts it into
///   the store with `meta` carrying all header results), records the
///   callback task id on the chord record, and returns
///   `{ new_state: "success", callback_task_id: Some(...) }`.
/// - If any header task failed / cancelled / expired: transitions the chord
///   to `partial_failed` and returns
///   `{ new_state: "partial_failed", callback_task_id: None }`.
/// - Otherwise (some header tasks still pending / running): transitions to
///   `running` and returns
///   `{ new_state: "running", callback_task_id: None }`.
///
/// Terminal chord records (already `success` or `partial_failed`) are not
/// recomputed — the function returns the current state without side effects.
pub async fn refresh_state(
    store: &Arc<dyn TaskStore>,
    chord_id: &str,
) -> Result<ChordRefreshResult> {
    let record = match store.get_chord(chord_id).await? {
        Some(r) => r,
        None => {
            return Ok(ChordRefreshResult {
                new_state: "pending",
                callback_task_id: None,
            });
        }
    };
    // Already terminal — return the current state without recomputing.
    match record.state.as_str() {
        "success" => {
            return Ok(ChordRefreshResult {
                new_state: "success",
                callback_task_id: record.callback_task_id.clone(),
            });
        }
        "partial_failed" => {
            return Ok(ChordRefreshResult {
                new_state: "partial_failed",
                callback_task_id: None,
            });
        }
        _ => {}
    }

    let mut succeeded = 0u32;
    let mut failed = 0u32;
    let mut results: Vec<serde_json::Value> = Vec::new();
    for tid in &record.header_task_ids {
        match store.load_task(tid).await? {
            Some(t) => match t.state {
                TaskState::Success => {
                    succeeded += 1;
                    // Collect the header task's result so the callback can
                    // consume it via its `meta` field.
                    if let Ok(Some(r)) = store.load_result(tid).await {
                        results.push(serde_json::json!({
                            "id": tid,
                            "result": {
                                "stdout": r.stdout,
                                "stderr": r.stderr,
                                "exit_code": r.exit_code,
                                "body": r.body,
                                "status_code": r.status_code,
                            }
                        }));
                    } else {
                        results.push(serde_json::json!({"id": tid, "result": null}));
                    }
                }
                TaskState::Failed | TaskState::Expired | TaskState::Cancelled => failed += 1,
                _ => {}
            },
            None => {} // task not found — treat as pending
        }
    }

    let total = record.header_task_ids.len() as u32;
    let now = now_ts() as i64;

    if failed > 0 {
        store.update_chord_state(chord_id, "partial_failed", None, now).await?;
        return Ok(ChordRefreshResult {
            new_state: "partial_failed",
            callback_task_id: None,
        });
    }

    if succeeded == total && total > 0 {
        // All header tasks succeeded — dispatch the callback.
        let mut callback: TaskBuilder = serde_json::from_str(&record.callback_json)
            .map_err(|e| crate::errors::XhjobError::Store(format!("chord callback parse: {}", e)))?;
        let meta_json = serde_json::to_string(&results)
            .map_err(|e| crate::errors::XhjobError::Store(format!("chord meta encode: {}", e)))?;
        callback = callback.meta(meta_json);
        let task = callback.build()?;
        let callback_task_id = task.id.clone();
        store.insert_task(task).await?;
        store
            .update_chord_state(chord_id, "success", Some(callback_task_id.clone()), now)
            .await?;
        return Ok(ChordRefreshResult {
            new_state: "success",
            callback_task_id: Some(callback_task_id),
        });
    }

    // Still in flight.
    store.update_chord_state(chord_id, "running", None, now).await?;
    Ok(ChordRefreshResult {
        new_state: "running",
        callback_task_id: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemoryStore, Task, TaskType};
    use serde_json::json;

    /// Empty chord (no header tasks) — refresh_state returns "pending" and
    /// never dispatches the callback (nothing to wait for, nothing to
    /// aggregate). This matches Celery's `chord([])` no-op behavior.
    #[tokio::test]
    async fn test_refresh_empty_chord_returns_pending() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        store.create_chord("c-empty", &[], "{}", now_ts() as i64).await.unwrap();
        let r = refresh_state(&store, "c-empty").await.unwrap();
        assert_eq!(r.new_state, "running");
        assert!(r.callback_task_id.is_none());
    }

    /// All header tasks succeeded -> chord transitions to "success" and the
    /// callback task id is returned for enqueuing.
    #[tokio::test]
    async fn test_refresh_all_success_dispatches_callback() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let mut t1 = Task::new(TaskType::Shell, json!({"cmd":"a"}));
        t1.id = "h1".to_string();
        t1.state = TaskState::Success;
        store.insert_task(t1).await.unwrap();
        store.save_result("h1", crate::store::TaskResult {
            stdout: Some("ok1".to_string()),
            exit_code: Some(0),
            ..Default::default()
        }).await.unwrap();

        let mut t2 = Task::new(TaskType::Shell, json!({"cmd":"b"}));
        t2.id = "h2".to_string();
        t2.state = TaskState::Success;
        store.insert_task(t2).await.unwrap();
        store.save_result("h2", crate::store::TaskResult {
            stdout: Some("ok2".to_string()),
            exit_code: Some(0),
            ..Default::default()
        }).await.unwrap();

        let callback = TaskBuilder::new().via_shell("echo done").to_json();
        store
            .create_chord("c-ok", &["h1".to_string(), "h2".to_string()], &callback, now_ts() as i64)
            .await
            .unwrap();

        let r = refresh_state(&store, "c-ok").await.unwrap();
        assert_eq!(r.new_state, "success");
        let cb_id = r.callback_task_id.expect("callback should be dispatched");
        // The callback task was inserted into the store.
        let cb_task = store.load_task(&cb_id).await.unwrap().unwrap();
        // The callback's meta carries both header results as a JSON array.
        let meta = cb_task.meta.expect("callback meta should be set");
        let arr: serde_json::Value = serde_json::from_str(&meta).unwrap();
        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // The chord record reflects the dispatched callback id.
        let rec = store.get_chord("c-ok").await.unwrap().unwrap();
        assert_eq!(rec.state, "success");
        assert_eq!(rec.callback_task_id, Some(cb_id));
    }

    /// One header task failed -> chord transitions to "partial_failed" and
    /// the callback is NOT dispatched.
    #[tokio::test]
    async fn test_refresh_partial_failure_no_callback() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let mut t1 = Task::new(TaskType::Shell, json!({"cmd":"a"}));
        t1.id = "h1".to_string();
        t1.state = TaskState::Success;
        store.insert_task(t1).await.unwrap();

        let mut t2 = Task::new(TaskType::Shell, json!({"cmd":"b"}));
        t2.id = "h2".to_string();
        t2.state = TaskState::Failed;
        store.insert_task(t2).await.unwrap();

        let callback = TaskBuilder::new().via_shell("echo done").to_json();
        store
            .create_chord("c-pf", &["h1".to_string(), "h2".to_string()], &callback, now_ts() as i64)
            .await
            .unwrap();

        let r = refresh_state(&store, "c-pf").await.unwrap();
        assert_eq!(r.new_state, "partial_failed");
        assert!(r.callback_task_id.is_none());
        let rec = store.get_chord("c-pf").await.unwrap().unwrap();
        assert_eq!(rec.state, "partial_failed");
        assert!(rec.callback_task_id.is_none());
    }

    /// Some header tasks still pending -> chord stays "running", no callback.
    #[tokio::test]
    async fn test_refresh_in_flight_returns_running() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let mut t1 = Task::new(TaskType::Shell, json!({"cmd":"a"}));
        t1.id = "h1".to_string();
        t1.state = TaskState::Success;
        store.insert_task(t1).await.unwrap();

        let mut t2 = Task::new(TaskType::Shell, json!({"cmd":"b"}));
        t2.id = "h2".to_string();
        t2.state = TaskState::Pending;
        store.insert_task(t2).await.unwrap();

        let callback = TaskBuilder::new().via_shell("echo done").to_json();
        store
            .create_chord("c-run", &["h1".to_string(), "h2".to_string()], &callback, now_ts() as i64)
            .await
            .unwrap();

        let r = refresh_state(&store, "c-run").await.unwrap();
        assert_eq!(r.new_state, "running");
        assert!(r.callback_task_id.is_none());
    }

    /// A terminal chord record is not recomputed — refresh_state returns the
    /// stored state without side effects.
    #[tokio::test]
    async fn test_refresh_terminal_chord_is_idempotent() {
        let store: Arc<dyn TaskStore> = Arc::new(InMemoryStore::new());
        let callback = TaskBuilder::new().via_shell("echo done").to_json();
        store
            .create_chord("c-term", &["h1".to_string()], &callback, now_ts() as i64)
            .await
            .unwrap();
        store.update_chord_state("c-term", "partial_failed", None, now_ts() as i64).await.unwrap();
        let r = refresh_state(&store, "c-term").await.unwrap();
        assert_eq!(r.new_state, "partial_failed");
        assert!(r.callback_task_id.is_none());
    }
}
