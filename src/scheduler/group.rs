//! Task group (C16). Parallel batch of tasks executed concurrently.
//!
//! Reference: Celery `group(t1, t2, t3)`.
//!
//! The group record is persisted as a list of `TaskBuilder` JSON configs.
//! On group creation, the daemon dispatches all tasks concurrently. As
//! each task completes, the daemon updates a per-task completion record
//! (stored in the group's `payload` JSON: a map of `task_id -> result`).
//!
//! Group state transitions: pending -> running -> success (all ok) /
//! partial_failed (some failed) / failed (all failed)
//! （"success"/"failed" 与 TaskState::as_str() 一致，统一为小写）。
//!
//! The current implementation does NOT persist per-task completion to keep
//! the data model simple — `group_state` queries the live task states in
//! the store on demand. For long-lived groups this is acceptable; for very
//! large groups (>10k tasks) a denormalized completion counter may be added
//! in the future.

use crate::errors::Result;
use crate::store::{GroupRecord, TaskState, TaskStore};
use crate::store::now_ts;

/// Inspect the group record by id.
pub async fn inspect(
    store: &std::sync::Arc<dyn TaskStore>,
    group_id: &str,
) -> Result<Option<GroupRecord>> {
    store.get_group(group_id).await
}

/// Compute the group completion summary by inspecting the live task states
/// of the group's task ids (stored in the group's `tasks` JSON array as
/// objects with an `id` field, OR as bare strings).
///
/// Returns `(total, succeeded, failed, pending_or_running)`.
pub async fn summarize(
    store: &std::sync::Arc<dyn TaskStore>,
    group_id: &str,
) -> Result<(u32, u32, u32, u32)> {
    let record = match store.get_group(group_id).await? {
        Some(r) => r,
        None => return Ok((0, 0, 0, 0)),
    };
    let mut total = 0u32;
    let mut succeeded = 0u32;
    let mut failed = 0u32;
    let mut pending = 0u32;
    for task_json in &record.tasks {
        let id = task_json.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
        if let Some(id) = id {
            total += 1;
            match store.load_task(&id).await {
                Ok(Some(t)) => match t.state {
                    TaskState::Success => succeeded += 1,
                    TaskState::Failed | TaskState::Cancelled | TaskState::Expired => failed += 1,
                    _ => pending += 1,
                },
                _ => pending += 1,
            }
        }
    }
    Ok((total, succeeded, failed, pending))
}

/// Update the group state based on the per-task summary. Called by the
/// daemon after each task in the group completes.
pub async fn refresh_state(
    store: &std::sync::Arc<dyn TaskStore>,
    group_id: &str,
) -> Result<&'static str> {
    let (total, succeeded, failed, pending) = summarize(store, group_id).await?;
    let new_state = if total == 0 {
        "pending"
    } else if pending > 0 {
        "running"
    } else if failed == 0 {
        "success"
    } else if succeeded == 0 {
        "failed"
    } else {
        "partial_failed"
    };
    store.update_group_state(group_id, new_state, now_ts() as i64).await?;
    Ok(new_state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{InMemoryStore, Task, TaskType};
    use serde_json::json;

    #[tokio::test]
    async fn test_summarize_empty_group() {
        let store: std::sync::Arc<dyn TaskStore> = std::sync::Arc::new(InMemoryStore::new());
        store.create_group("g-empty", &[], now_ts() as i64).await.unwrap();
        let (total, ok, fail, pend) = summarize(&store, "g-empty").await.unwrap();
        assert_eq!((total, ok, fail, pend), (0, 0, 0, 0));
    }

    #[tokio::test]
    async fn test_summarize_all_success() {
        let store: std::sync::Arc<dyn TaskStore> = std::sync::Arc::new(InMemoryStore::new());
        let mut t1 = Task::new(TaskType::Shell, json!({"cmd":"a"}));
        t1.id = "t1".to_string();
        t1.state = TaskState::Success;
        store.insert_task(t1).await.unwrap();
        let mut t2 = Task::new(TaskType::Shell, json!({"cmd":"b"}));
        t2.id = "t2".to_string();
        t2.state = TaskState::Success;
        store.insert_task(t2).await.unwrap();
        let tasks = vec![json!({"id":"t1"}), json!({"id":"t2"})];
        store.create_group("g-ok", &tasks, now_ts() as i64).await.unwrap();
        let (total, ok, fail, pend) = summarize(&store, "g-ok").await.unwrap();
        assert_eq!((total, ok, fail, pend), (2, 2, 0, 0));
        let state = refresh_state(&store, "g-ok").await.unwrap();
        assert_eq!(state, "success");
    }

    #[tokio::test]
    async fn test_summarize_partial_failure() {
        let store: std::sync::Arc<dyn TaskStore> = std::sync::Arc::new(InMemoryStore::new());
        let mut t1 = Task::new(TaskType::Shell, json!({"cmd":"a"}));
        t1.id = "t1".to_string();
        t1.state = TaskState::Success;
        store.insert_task(t1).await.unwrap();
        let mut t2 = Task::new(TaskType::Shell, json!({"cmd":"b"}));
        t2.id = "t2".to_string();
        t2.state = TaskState::Failed;
        store.insert_task(t2).await.unwrap();
        let tasks = vec![json!({"id":"t1"}), json!({"id":"t2"})];
        store.create_group("g-pf", &tasks, now_ts() as i64).await.unwrap();
        let state = refresh_state(&store, "g-pf").await.unwrap();
        assert_eq!(state, "partial_failed");
    }

    #[tokio::test]
    async fn test_summarize_all_failed() {
        let store: std::sync::Arc<dyn TaskStore> = std::sync::Arc::new(InMemoryStore::new());
        let mut t1 = Task::new(TaskType::Shell, json!({"cmd":"a"}));
        t1.id = "t1".to_string();
        t1.state = TaskState::Failed;
        store.insert_task(t1).await.unwrap();
        let tasks = vec![json!({"id":"t1"})];
        store.create_group("g-af", &tasks, now_ts() as i64).await.unwrap();
        let state = refresh_state(&store, "g-af").await.unwrap();
        assert_eq!(state, "failed");
    }
}
