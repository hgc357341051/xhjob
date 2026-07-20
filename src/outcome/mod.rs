//! Task state and result query API (reference: Celery AsyncResult).

use serde::{Serialize, Deserialize};
use crate::errors::{Result, XhjobError};
use crate::store::{Task, TaskResult, TaskState, TaskStore};
use crate::ipc::{Request, Response, request as ipc_request};

/// State info returned by `xhjob_state($id)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateInfo {
    pub state: String,
    pub attempts: u32,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub last_error: Option<String>,
}

impl StateInfo {
    pub fn from_task(task: &Task) -> Self {
        Self {
            state: task.state.as_str().to_string(),
            attempts: task.attempts,
            created_at: task.created_at,
            started_at: task.started_at,
            finished_at: task.finished_at,
            last_error: task.last_error.clone(),
        }
    }
}

/// Daemon-side handler: query state from store.
pub async fn handle_state(store: &std::sync::Arc<dyn TaskStore>, task_id: &str) -> Result<StateInfo> {
    let task = store.load_task(task_id).await?
        .ok_or_else(|| XhjobError::TaskNotFound(task_id.to_string()))?;
    Ok(StateInfo::from_task(&task))
}

/// Daemon-side handler: query result from store.
pub async fn handle_result(store: &std::sync::Arc<dyn TaskStore>, task_id: &str) -> Result<TaskResult> {
    let result = store.load_result(task_id).await?
        .ok_or_else(|| XhjobError::TaskNotFound(format!("result for {}", task_id)))?;
    Ok(result)
}

<<<<<<< Updated upstream
/// PHP-side client: send `state` request to daemon.
pub async fn query_state(task_id: &str) -> Result<StateInfo> {
    let payload = serde_json::json!({ "task_id": task_id });
    let resp = ipc_request("state", payload).await?;
=======
/// PHP-side client: send `state` request to daemon for `service_name`.
pub async fn query_state(task_id: &str, service_name: &str) -> Result<StateInfo> {
    let payload = serde_json::json!({ "task_id": task_id });
    let resp = ipc_request("state", payload, service_name).await?;
>>>>>>> Stashed changes
    if !resp.ok {
        return Err(XhjobError::Ipc(resp.err.unwrap_or_else(|| "unknown".to_string())));
    }
    let info: StateInfo = serde_json::from_value(resp.data)
        .map_err(|e| XhjobError::Ipc(format!("deserialize state: {}", e)))?;
    Ok(info)
}

<<<<<<< Updated upstream
/// PHP-side client: send `result` request to daemon.
pub async fn query_result(task_id: &str) -> Result<TaskResult> {
    let payload = serde_json::json!({ "task_id": task_id });
    let resp = ipc_request("result", payload).await?;
=======
/// PHP-side client: send `result` request to daemon for `service_name`.
pub async fn query_result(task_id: &str, service_name: &str) -> Result<TaskResult> {
    let payload = serde_json::json!({ "task_id": task_id });
    let resp = ipc_request("result", payload, service_name).await?;
>>>>>>> Stashed changes
    if !resp.ok {
        return Err(XhjobError::Ipc(resp.err.unwrap_or_else(|| "unknown".to_string())));
    }
    let result: TaskResult = serde_json::from_value(resp.data)
        .map_err(|e| XhjobError::Ipc(format!("deserialize result: {}", e)))?;
    Ok(result)
}
