//! Task state and result query API (reference: Celery AsyncResult).

use serde::{Serialize, Deserialize};
use crate::errors::{Result, XhjobError};
use crate::store::{Task, TaskResult, TaskStore};
use crate::ipc::request as ipc_request;

/// State info returned by `xhjob_state($id)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateInfo {
    pub state: String,
    pub attempts: u32,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub last_error: Option<String>,
    pub execution_count: u32,
    pub max_executions: u32,
    pub paused: bool,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
    pub meta: Option<String>,
    /// IntervalTrigger period in seconds (A7). None if not set.
    pub interval: Option<u64>,
    /// DateTrigger absolute Unix timestamp (A8). None if not set.
    pub run_at: Option<i64>,
    /// Jitter (A9): random offset in seconds added to next_fire. 0 = no jitter.
    pub jitter: u64,
    /// Task-level expires (C6): if a task remains Pending for longer than
    /// `expires` seconds (measured from `created_at`), it transitions to
    /// `Expired` terminal state. 0 = no expiry.
    pub expires: u64,
    /// Retry exponential backoff (C8): when true, retry delays grow
    /// exponentially as `min(retry_delay * 2^(attempts-1), retry_delay * 60)`.
    pub retry_backoff: bool,
    /// ignoreResult (C9): when true, no result row is persisted for this
    /// task — `xhjob_result()` will return null. Default false.
    /// Reference: Celery ignore_result.
    pub ignore_result: bool,
    /// acksLate (C10): when true, Running tasks are auto-reset to Pending on
    /// daemon restart (crash recovery). Default false.
    /// Reference: Celery acks_late.
    pub acks_late: bool,
    /// softTimeout (C11): graceful exit timeout in seconds. None = no soft
    /// timeout (existing hard-kill behavior at `timeout`). When set, the
    /// shell executor sends SIGTERM at `soft_timeout` seconds; SIGKILL is
    /// sent after (timeout - soft_timeout) seconds of grace.
    /// Reference: Celery soft_time_limit.
    pub soft_timeout: Option<u64>,
    /// misfire_grace_time (A13): per-job override of the global default
    /// 60s misfire grace window. 0 = use global default.
    pub misfire_grace_time: u64,
    /// replace_existing (A14): whether dispatch replaces an existing task
    /// with the same id (full overwrite).
    pub replace_existing: bool,
    /// tags (A15): user-supplied labels for grouping / filtering tasks.
    pub tags: Vec<String>,
    /// rate_limit_count (C12): max triggers within rate_limit_window secs.
    /// 0 = no rate limiting.
    pub rate_limit_count: u32,
    /// rate_limit_window (C12): sliding window length in seconds for
    /// rate limiting. 0 = no rate limiting.
    pub rate_limit_window: u64,
    /// acks_on_failure (C13): when true (default), task failures respect
    /// retry_max. When false, failures are retried indefinitely.
    pub acks_on_failure: bool,
    /// timezone (A16): IANA timezone string used when evaluating the cron
    /// expression. None = system local timezone.
    pub timezone: Option<String>,
    /// coalesce (A18): whether to collapse missed triggers into one fire.
    pub coalesce: bool,
    /// Progress percent (0-100). None = not reported yet.
    /// Reference: Celery update_state(state='PROGRESS', meta=...).
    #[serde(default)]
    pub progress: Option<u8>,
    /// Arbitrary JSON metadata accompanying the latest progress report.
    /// Reference: Celery update_state meta.
    #[serde(default)]
    pub progress_meta: Option<String>,
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
            execution_count: task.execution_count,
            max_executions: task.max_executions,
            paused: task.paused,
            start_date: task.start_date,
            end_date: task.end_date,
            meta: task.meta.clone(),
            interval: task.interval,
            run_at: task.run_at,
            jitter: task.jitter,
            expires: task.expires,
            retry_backoff: task.retry_backoff,
            ignore_result: task.ignore_result,
            acks_late: task.acks_late,
            soft_timeout: task.soft_timeout,
            misfire_grace_time: task.misfire_grace_time,
            replace_existing: task.replace_existing,
            tags: task.tags.clone(),
            rate_limit_count: task.rate_limit_count,
            rate_limit_window: task.rate_limit_window,
            acks_on_failure: task.acks_on_failure,
            timezone: task.timezone.clone(),
            coalesce: task.coalesce,
            progress: task.progress,
            progress_meta: task.progress_meta.clone(),
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

/// PHP-side client: send `state` request to daemon for `service_name` with
/// optional `data_dir`.
pub async fn query_state(
    task_id: &str,
    service_name: &str,
    data_dir: Option<&str>,
) -> Result<StateInfo> {
    let payload = serde_json::json!({ "task_id": task_id });
    let resp = ipc_request("state", payload, service_name, data_dir).await?;
    if !resp.ok {
        return Err(XhjobError::ipc(resp.err.unwrap_or_else(|| "unknown".to_string())));
    }
    let info: StateInfo = serde_json::from_value(resp.data)
        .map_err(|e| XhjobError::ipc(format!("deserialize state: {}", e)))?;
    Ok(info)
}

/// PHP-side client: send `result` request to daemon for `service_name` with
/// optional `data_dir`.
pub async fn query_result(
    task_id: &str,
    service_name: &str,
    data_dir: Option<&str>,
) -> Result<TaskResult> {
    let payload = serde_json::json!({ "task_id": task_id });
    let resp = ipc_request("result", payload, service_name, data_dir).await?;
    if !resp.ok {
        return Err(XhjobError::ipc(resp.err.unwrap_or_else(|| "unknown".to_string())));
    }
    let result: TaskResult = serde_json::from_value(resp.data)
        .map_err(|e| XhjobError::ipc(format!("deserialize result: {}", e)))?;
    Ok(result)
}
