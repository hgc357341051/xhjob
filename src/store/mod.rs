//! Task store abstraction: in-memory (default) and SQLite (optional `persist` feature).

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};
use crate::errors::{Result, XhjobError};

pub mod in_memory;
#[cfg(feature = "persist")]
pub mod sqlite;
#[cfg(feature = "persist")]
pub mod crypto;

pub use in_memory::InMemoryStore;
#[cfg(feature = "persist")]
pub use sqlite::SqliteStore;

/// Task type: HTTP or Shell.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    Http,
    Shell,
}

impl TaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskType::Http => "http",
            TaskType::Shell => "shell",
        }
    }
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "http" => Ok(TaskType::Http),
            "shell" => Ok(TaskType::Shell),
            other => Err(XhjobError::InvalidTask(format!("unknown task type: {}", other))),
        }
    }
}

/// Task state machine.
///
/// 所有对外 API（`xhjob_state` / `xhjob_list` / `xhjob_get` 等）统一返回
/// serde 风格的小写形式（`"pending"` / `"running"` / `"success"` ...）。
/// `as_str()` 与 serde 序列化保持一致，均输出小写。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Pending,
    Running,
    Success,
    Failed,
    Interrupted,
    /// Task was cancelled (terminal state). Reference: Celery revoke.
    Cancelled,
    /// Task expired before being executed (terminal state).
    /// Triggered when `task.expires > 0 && now > task.created_at + expires`
    /// while the task is still Pending. Reference: APScheduler expires.
    Expired,
}

impl TaskState {
    /// 返回小写的状态字符串，与 serde 序列化结果一致。
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Pending => "pending",
            TaskState::Running => "running",
            TaskState::Success => "success",
            TaskState::Failed => "failed",
            TaskState::Interrupted => "interrupted",
            TaskState::Cancelled => "cancelled",
            TaskState::Expired => "expired",
        }
    }
    /// 解析状态字符串。同时接受新的小写形式与历史的大写形式，
    /// 以便兼容旧数据库 / 旧调用方。
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "pending" | "PENDING" => Ok(TaskState::Pending),
            "running" | "RUNNING" => Ok(TaskState::Running),
            "success" | "SUCCESS" => Ok(TaskState::Success),
            "failed" | "FAILED" => Ok(TaskState::Failed),
            "interrupted" | "INTERRUPTED" => Ok(TaskState::Interrupted),
            "cancelled" | "CANCELLED" => Ok(TaskState::Cancelled),
            "expired" | "EXPIRED" => Ok(TaskState::Expired),
            other => Err(XhjobError::Store(format!("unknown state: {}", other))),
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskState::Success | TaskState::Failed | TaskState::Cancelled | TaskState::Expired)
    }
    pub fn is_running(&self) -> bool {
        matches!(self, TaskState::Running)
    }
}

/// HTTP task payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpPayload {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

/// Shell task payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellPayload {
    pub cmd: String,
}

/// Task definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub task_type: TaskType,
    pub payload: serde_json::Value,
    pub cron: Option<String>,
    pub retry_max: u32,
    pub retry_delay: u64, // seconds
    pub timeout: u64, // seconds
    pub priority: i32,
    pub allow_overlap: bool,
    pub max_instances: u32,
    pub coalesce: bool,
    pub persist: bool,
    /// Optional proxy URL for HTTP tasks. None = direct connection.
    #[serde(default)]
    pub proxy: Option<String>,
    /// Optional output encoding for Shell tasks (e.g. `GBK`, `Big5`, `auto`).
    /// None = treat stdout/stderr as UTF-8 (lossy).
    #[serde(default)]
    pub encoding: Option<String>,
    /// Optional IANA timezone (e.g. `Asia/Shanghai`, `America/New_York`) used
    /// when evaluating the cron expression. None = system local timezone.
    #[serde(default)]
    pub timezone: Option<String>,
    pub state: TaskState,
    pub attempts: u32,
    pub next_fire: Option<u64>, // Unix timestamp
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub finished_at: Option<u64>,
    pub last_error: Option<String>,
    /// Maximum executions for cron tasks (0 = unlimited, default).
    #[serde(default)]
    pub max_executions: u32,
    /// Number of times this task has been executed (incremented per cron trigger).
    #[serde(default)]
    pub execution_count: u32,
    /// Whether this cron task is paused (cron tick should skip it).
    /// Reference: APScheduler pause_job. Persisted to store.
    #[serde(default)]
    pub paused: bool,
    /// Whether cancel has been requested for this task.
    /// For Pending tasks, state becomes Cancelled immediately.
    /// For Running tasks, the current execution finishes but no retry/cron re-trigger occurs.
    /// Reference: Celery revoke.
    #[serde(default)]
    pub cancel_requested: bool,
    /// Optional start date (Unix timestamp). Cron triggers before this time are skipped.
    /// Reference: APScheduler start_date.
    #[serde(default)]
    pub start_date: Option<i64>,
    /// Optional end date (Unix timestamp). After this time, task state becomes Success terminal.
    /// Reference: APScheduler end_date.
    #[serde(default)]
    pub end_date: Option<i64>,
    /// Result time-to-live in seconds (0 = keep forever, default).
    /// Reference: Celery result_expires.
    #[serde(default)]
    pub result_ttl: u64,
    /// Optional user metadata (JSON string). Persisted to store.
    /// Reference: Celery update_state meta.
    #[serde(default)]
    pub meta: Option<String>,
    /// IntervalTrigger period in seconds (A7). When set, the task fires every
    /// `interval` seconds. Mutually exclusive with `cron` and `run_at` (cron /
    /// run_at take priority; interval is ignored if either is set).
    /// Reference: APScheduler IntervalTrigger.
    #[serde(default)]
    pub interval: Option<u64>,
    /// DateTrigger absolute Unix timestamp (A8). When set, the task fires once
    /// at the given timestamp and immediately transitions to Success terminal
    /// state. Highest scheduling priority (overrides cron + interval).
    /// Reference: APScheduler DateTrigger.
    #[serde(default)]
    pub run_at: Option<i64>,
    /// Jitter (A9): random offset in seconds added to next_fire for cron /
    /// interval tasks. Helps avoid thundering-herd effects when many tasks
    /// share the same trigger time. Default 0 = no jitter. Ignored for runAt
    /// tasks (which are one-shot at a precise timestamp).
    /// Reference: APScheduler jitter.
    #[serde(default)]
    pub jitter: u64,
    /// Task-level expires (C6): if a task remains Pending for longer than
    /// `expires` seconds (measured from `created_at`), it transitions to
    /// `Expired` terminal state. Default 0 = no expiry. Only affects Pending
    /// tasks; Running tasks are not interrupted.
    /// Reference: APScheduler expires.
    #[serde(default)]
    pub expires: u64,
    /// Retry exponential backoff (C8): when true, retry delays grow
    /// exponentially as `min(retry_delay * 2^(attempts-1), retry_delay * 60)`.
    /// When false (default), retry delays are fixed at `retry_delay`.
    /// Reference: Celery retry_backoff.
    #[serde(default)]
    pub retry_backoff: bool,
    /// ignoreResult (C9): when true, the daemon skips `save_result` for this
    /// task — fire-and-forget semantics. The task state machine still runs
    /// (Pending → Running → Success/Failed), but no result row is persisted
    /// so `xhjob_result()` will return null. Useful for high-throughput
    /// tasks whose result is not needed by the caller. Default false.
    /// Reference: Celery ignore_result.
    #[serde(default)]
    pub ignore_result: bool,
    /// acksLate (C10): when true, the task is "acked late" — on daemon
    /// restart, Running tasks with `acks_late=true` are automatically reset
    /// to Pending so they will be re-triggered (crash recovery semantics).
    /// When false (default), Running tasks on daemon restart stay Running
    /// (or are left to manual intervention) — this matches the previous
    /// behavior. Reference: Celery acks_late.
    #[serde(default)]
    pub acks_late: bool,
    /// softTimeout (C11): graceful exit timeout in seconds. When set and
    /// less than `timeout`, the shell executor sends SIGTERM at
    /// `soft_timeout` seconds; if the child does not exit within
    /// (timeout - soft_timeout) seconds after SIGTERM, SIGKILL is sent.
    /// None (default) = no soft timeout (existing hard-kill behavior at
    /// `timeout`). HTTP tasks ignore this field (HTTP clients cannot be
    /// gracefully interrupted). A value of 0 is treated as None.
    /// Reference: Celery soft_time_limit.
    #[serde(default)]
    pub soft_timeout: Option<u64>,
    /// misfire_grace_time (A13): per-job override of the global default
    /// 60s misfire grace window. 0 = use global default (60s). When
    /// `now - next_fire > grace_time`, the trigger is considered misfired;
    /// `coalesce=true` collapses missed triggers into one fire (still
    /// executes once), `coalesce=false` skips the trigger entirely. Only
    /// effective for cron tasks; interval / runAt tasks ignore this field
    /// (warn + ignore). Reference: APScheduler misfire_grace_time.
    #[serde(default)]
    pub misfire_grace_time: u64,
    /// replace_existing (A14): when true and `id` is set, dispatch will
    /// replace an existing task with the same id (full overwrite, state /
    /// attempts / execution_count reset). When false (default), dispatch
    /// returns an error on id conflict. Reference: APScheduler
    /// replace_existing.
    #[serde(default)]
    pub replace_existing: bool,
    /// tags (A15): user-supplied labels for grouping / filtering tasks.
    /// Used by `list_tasks(tag_filter)` to filter the task list. Empty by
    /// default. Reference: APScheduler tags / Celery queue routing.
    #[serde(default)]
    pub tags: Vec<String>,
    /// rate_limit_count (C12): max number of triggers allowed within
    /// `rate_limit_window` seconds. 0 = no rate limiting (default). Pairs
    /// with `rate_limit_window`. Reference: Celery rate_limit.
    #[serde(default)]
    pub rate_limit_count: u32,
    /// rate_limit_window (C12): sliding window length in seconds for
    /// rate limiting. 0 = no rate limiting (default). Reference: Celery
    /// rate_limit.
    #[serde(default)]
    pub rate_limit_window: u64,
    /// acks_on_failure (C13): when true (default), task failures respect
    /// `retry_max` (transition to Failed terminal after exhausting retries).
    /// When false, failures are retried indefinitely (ignoring retry_max)
    /// until the task succeeds or is cancelled/removed. Complementary to
    /// `acks_late`. Reference: Celery acks_on_failure.
    #[serde(default = "default_acks_on_failure_true")]
    pub acks_on_failure: bool,
    /// idempotent: when true, the task is declared safe to retry even if it
    /// uses a non-idempotent HTTP method (POST/PUT/DELETE/PATCH). When false
    /// (default), HTTP tasks using non-idempotent methods are NOT retried on
    /// 5xx responses to prevent duplicate side effects (e.g. double-charging
    /// a credit card). GET/HEAD/OPTIONS are always retryable regardless of
    /// this flag because they have no side effects per HTTP spec.
    /// Network errors (no status_code at all) are always retryable because
    /// the request likely never reached the server.
    /// Reference: HTTP method safety/idempotency (RFC 7231 §4.2.1-2).
    #[serde(default)]
    pub idempotent: bool,
    /// Progress percent (0-100). None = not reported yet.
    /// Reference: Celery update_state(state='PROGRESS', meta=...).
    #[serde(default)]
    pub progress: Option<u8>,
    /// Arbitrary JSON metadata accompanying the latest progress report.
    /// Reference: Celery update_state meta.
    #[serde(default)]
    pub progress_meta: Option<String>,
    /// Optional chord callback correlation id. When set, this task is part
    /// of a chord and the callback should fire after the chord completes.
    /// Reference: Celery chord.
    #[serde(default)]
    pub chord_id: Option<String>,
    /// Owner of this task (for multi-tenant isolation). Set from XHJOB_OWNER
    /// at dispatch time. Empty string = no ownership (backward compatible,
    /// all workers can access). When set, only the same owner can query/modify.
    #[serde(default)]
    pub owner: String,
}

fn default_acks_on_failure_true() -> bool { true }

impl Task {
    pub fn new(task_type: TaskType, payload: serde_json::Value) -> Self {
        let now = now_ts();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            task_type,
            payload,
            cron: None,
            retry_max: 0,
            retry_delay: 1,
            timeout: 30,
            priority: 0,
            allow_overlap: false,
            max_instances: 1,
            coalesce: true,
            persist: false,
            proxy: None,
            encoding: None,
            timezone: None,
            state: TaskState::Pending,
            attempts: 0,
            next_fire: None,
            created_at: now,
            started_at: None,
            finished_at: None,
            last_error: None,
            max_executions: 0,
            execution_count: 0,
            paused: false,
            cancel_requested: false,
            start_date: None,
            end_date: None,
            result_ttl: 0,
            meta: None,
            interval: None,
            run_at: None,
            jitter: 0,
            expires: 0,
            retry_backoff: false,
            ignore_result: false,
            acks_late: false,
            soft_timeout: None,
            misfire_grace_time: 0,
            replace_existing: false,
            tags: Vec::new(),
            rate_limit_count: 0,
            rate_limit_window: 0,
            acks_on_failure: true,
            idempotent: false,
            progress: None,
            progress_meta: None,
            chord_id: None,
            owner: String::new(),
        }
    }
}

/// Task execution result.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskResult {
    pub body: Option<String>,
    pub status_code: Option<i32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub exit_code: Option<i32>,
}

/// Summary of a task for listing queries (lighter than full Task).
/// Reference: APScheduler get_jobs.
#[derive(Debug, Clone, Serialize)]
pub struct TaskSummary {
    pub id: String,
    pub task_type: TaskType,
    pub state: TaskState,
    pub cron: Option<String>,
    pub attempts: u32,
    pub priority: i32,
    pub next_fire: Option<u64>,
    pub paused: bool,
    pub max_executions: u32,
    pub execution_count: u32,
    pub start_date: Option<i64>,
    pub end_date: Option<i64>,
    pub meta: Option<String>,
    pub created_at: u64,
    pub finished_at: Option<u64>,
    /// Tags (A15): user-supplied labels for grouping / filtering.
    pub tags: Vec<String>,
    /// Progress percent (0-100). None = not reported yet.
    pub progress: Option<u8>,
    /// Optional chord callback correlation id.
    pub chord_id: Option<String>,
    /// P0-17: owner for multi-tenant filtering on list/inspect queries.
    /// Empty = legacy/unowned (visible to all callers, backward compat).
    pub owner: String,
}

impl From<&Task> for TaskSummary {
    fn from(t: &Task) -> Self {
        Self {
            id: t.id.clone(),
            task_type: t.task_type.clone(),
            state: t.state,
            cron: t.cron.clone(),
            attempts: t.attempts,
            priority: t.priority,
            next_fire: t.next_fire,
            paused: t.paused,
            max_executions: t.max_executions,
            execution_count: t.execution_count,
            start_date: t.start_date,
            end_date: t.end_date,
            meta: t.meta.clone(),
            created_at: t.created_at,
            finished_at: t.finished_at,
            tags: t.tags.clone(),
            progress: t.progress,
            chord_id: t.chord_id.clone(),
            owner: t.owner.clone(),
        }
    }
}

/// Aggregate worker / queue statistics. Returned by `worker_stats()`.
/// Reference: Celery inspect stats.
#[derive(Debug, Clone, Serialize)]
pub struct WorkerStats {
    /// Total number of tasks tracked by the store.
    pub total: u32,
    /// Tasks in Pending state.
    pub pending: u32,
    /// Tasks in Running state.
    pub running: u32,
    /// Tasks in Success terminal state.
    pub success: u32,
    /// Tasks in Failed terminal state.
    pub failed: u32,
    /// Number of tasks with a future `next_fire` (queue depth).
    pub queue_depth: u32,
}

/// Event types emitted during task lifecycle (A17).
/// Reference: APScheduler EVENT_JOB_* constants.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EventType {
    Started,
    Succeeded,
    Failed,
    Missed,
    Cancelled,
    Paused,
    Resumed,
    Expired,
    MaxInstancesReached,
    RateLimited,
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::Started => "started",
            EventType::Succeeded => "succeeded",
            EventType::Failed => "failed",
            EventType::Missed => "missed",
            EventType::Cancelled => "cancelled",
            EventType::Paused => "paused",
            EventType::Resumed => "resumed",
            EventType::Expired => "expired",
            EventType::MaxInstancesReached => "max_instances_reached",
            EventType::RateLimited => "rate_limited",
        }
    }
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "started" => Ok(EventType::Started),
            "succeeded" => Ok(EventType::Succeeded),
            "failed" => Ok(EventType::Failed),
            "missed" => Ok(EventType::Missed),
            "cancelled" => Ok(EventType::Cancelled),
            "paused" => Ok(EventType::Paused),
            "resumed" => Ok(EventType::Resumed),
            "expired" => Ok(EventType::Expired),
            "max_instances_reached" => Ok(EventType::MaxInstancesReached),
            "rate_limited" => Ok(EventType::RateLimited),
            other => Err(XhjobError::Store(format!("unknown event type: {}", other))),
        }
    }
}

/// A single task execution event record (A17).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    pub task_id: String,
    pub event_type: EventType,
    pub payload: Option<String>,
    pub ts: i64,
}

/// Chain record (C15). Persists a sequence of task configs to execute in order.
/// Reference: Celery chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainRecord {
    pub chain_id: String,
    /// Ordered list of task builder JSON configs.
    pub tasks: Vec<serde_json::Value>,
    pub current_step: u32,
    /// "pending" / "running" / "success" / "failed"（与 TaskState::as_str() 一致）。
    pub state: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Group record (C16). Persists a parallel batch of task configs.
/// Reference: Celery group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupRecord {
    pub group_id: String,
    /// Parallel task configs.
    pub tasks: Vec<serde_json::Value>,
    /// "pending" / "running" / "success" / "partial_failed" / "failed"
    /// （"success"/"failed" 与 TaskState::as_str() 一致）。
    pub state: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Chord record (C16+). A chord = header (parallel tasks) + body (callback).
/// When all header tasks succeed, the body is dispatched with meta carrying
/// all header results. If any header fails, chord -> partial_failed.
/// Reference: Celery chord.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChordRecord {
    pub id: String,
    /// Header task ids (the parallel tasks).
    pub header_task_ids: Vec<String>,
    /// The callback TaskBuilder JSON (serialized). Dispatched when all headers succeed.
    pub callback_json: String,
    /// Dispatched callback task id (None until body is dispatched).
    pub callback_task_id: Option<String>,
    pub state: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Unified task store abstraction.
///
/// Uses manual `Pin<Box<dyn Future + Send>>` return types instead of `async_trait`
/// to avoid adding the `async-trait` crate dependency. Each method returns a
/// boxed future that borrows `&self` for its lifetime (`'_`).
pub trait TaskStore: Send + Sync {
    fn insert_task(&self, task: Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    fn update_state(&self, id: &str, state: TaskState, started_at: Option<u64>, finished_at: Option<u64>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    fn save_result(&self, task_id: &str, result: TaskResult) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    fn load_active_tasks(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Task>>> + Send + '_>>;
    fn load_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Task>>> + Send + '_>>;
    fn load_result(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<TaskResult>>> + Send + '_>>;
    fn count_running_instances(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>>;
    fn update_next_fire(&self, id: &str, next_fire: Option<u64>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    fn set_attempts_and_error(&self, id: &str, attempts: u32, last_error: Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    fn delete_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    /// Increment execution_count for a task. Returns the new value.
    fn increment_execution_count(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>>;
    /// Remove a task definition from the store (does not affect running instances).
    /// Reference: APScheduler remove_job.
    fn remove_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    /// Set paused flag for a task (true=pause, false=resume).
    /// Reference: APScheduler pause_job / resume_job.
    fn set_paused(&self, id: &str, paused: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    /// Cancel a task.
    /// - If state=Pending: transition to Cancelled (terminal).
    /// - If state=Running: set cancel_requested=true (running instance finishes, no retry/cron re-trigger).
    /// Reference: Celery revoke.
    fn cancel_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;
    /// Delete result rows where (now - finished_at) > result_ttl, but only for tasks
    /// whose result_ttl > 0. Returns the number of deleted rows.
    /// Reference: Celery result_expires auto-cleanup.
    fn cleanup_expired_results(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>>;
    /// List all tasks in this store, optionally filtered by state and/or tag.
    /// When `tag_filter` is `Some(tag)`, only tasks whose `tags` array
    /// contains `tag` are returned. Reference: APScheduler get_jobs +
    /// tag-based filtering.
    fn list_tasks<'a>(&'a self, state_filter: Option<TaskState>, tag_filter: Option<&'a str>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + 'a>>;
    /// 将终态任务（Cancelled / Failed / Expired / Success）重新入队为 Pending，
    /// 以便再次触发执行。重置 `attempts=0` 并设置 `next_fire=now`，
    /// 使下一次扫描立即拾取该任务。返回 `true` 表示已重新入队，
    /// `false` 表示任务不处于可重新入队的终态（或不存在）。
    /// Reference: Celery requeue.
    fn requeue_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>>;

    /// Reschedule a cron task's cron expression online (A11). Updates the
    /// cron field and re-computes `next_fire` from now using the task's
    /// configured timezone. Preserves task state, execution_count, attempts,
    /// and meta — only `cron` and `next_fire` change.
    ///
    /// Returns:
    /// - `Ok(true)` if rescheduled successfully.
    /// - `Ok(false)` if the task does not exist, is not a cron task (cron
    ///   field is None — e.g. interval / runAt), or is in a terminal state
    ///   (Success / Failed / Cancelled / Expired).
    /// - `Err(XhjobError::CronParse(...))` if `new_cron` is not a valid cron
    ///   expression (the error message contains "invalid cron").
    ///
    /// Reference: APScheduler reschedule_job.
    fn reschedule_task(&self, id: &str, new_cron: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>>;

    /// Reset Running tasks with `acks_late=true` to Pending and set
    /// `next_fire=now` so they will be re-triggered immediately. Used for
    /// crash recovery on daemon startup (C10): if the daemon was killed
    /// mid-execution, in-flight tasks with `acks_late=true` are re-queued
    /// instead of being left in the Running state forever.
    ///
    /// Returns the number of tasks reset.
    ///
    /// Reference: Celery acks_late.
    fn reset_running_to_pending(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>>;

    // ----- Event log (A17) -----

    /// Record a task lifecycle event (started / succeeded / failed / etc.).
    /// Reference: APScheduler add_listener + EVENT_JOB_*.
    fn record_event(&self, task_id: &str, event_type: EventType, payload: Option<&str>, ts: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    /// List events since `since_ts` (Unix seconds), optionally filtered by
    /// `task_id_filter`. Ordered by ts ASC.
    fn list_events(&self, since_ts: i64, task_id_filter: Option<&str>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskEvent>>> + Send + '_>>;

    /// Delete events older than `ttl_secs` seconds. Returns the count deleted.
    fn cleanup_expired_events(&self, ttl_secs: u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>>;

    // ----- Task chain (C15) -----

    /// Create a new chain record with the given task configs.
    fn create_chain(&self, chain_id: &str, tasks: &[serde_json::Value], created_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    /// Load a chain record by id.
    fn get_chain(&self, chain_id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<ChainRecord>>> + Send + '_>>;

    /// Update chain step + state.
    fn update_chain_step(&self, chain_id: &str, current_step: u32, state: &str, updated_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    /// List chains by state (for daemon restart recovery).
    fn list_chains_by_state(&self, state: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<ChainRecord>>> + Send + '_>>;

    // ----- Task group (C16) -----

    /// Create a new group record.
    fn create_group(&self, group_id: &str, tasks: &[serde_json::Value], created_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    /// Load a group record by id.
    fn get_group(&self, group_id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<GroupRecord>>> + Send + '_>>;

    /// Update group state.
    fn update_group_state(&self, group_id: &str, state: &str, updated_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    /// List groups by state (for daemon restart recovery).
    fn list_groups_by_state(&self, state: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<GroupRecord>>> + Send + '_>>;

    // ----- Task chord (C16+) -----

    /// Create a new chord record. The header task ids and the serialized
    /// callback TaskBuilder JSON are persisted. The chord starts in the
    /// "pending" state and transitions to "running" / "success" /
    /// "partial_failed" as header tasks complete.
    /// Reference: Celery chord.
    fn create_chord(&self, id: &str, header_task_ids: &[String], callback_json: &str, created_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    /// Load a chord record by id.
    fn get_chord(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<ChordRecord>>> + Send + '_>>;

    /// Update chord state and (optionally) record the dispatched callback
    /// task id. Pass `callback_task_id = None` to leave it unchanged; pass
    /// `Some(id)` when the body has just been dispatched.
    fn update_chord_state(&self, id: &str, state: &str, callback_task_id: Option<String>, updated_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    // ----- Progress / inspect (Task 1-5) -----

    /// Update progress (0-100) and optional meta JSON for a task.
    /// Reference: Celery update_state(state='PROGRESS', meta=...).
    fn update_progress(&self, id: &str, percent: u8, meta: Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>>;

    /// List summaries of all currently running tasks.
    /// Reference: Celery inspect active.
    fn list_active_summary(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>>;

    /// List summaries of all registered (cron / interval) tasks.
    /// Reference: Celery inspect registered.
    fn list_registered_summary(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>>;

    /// List summaries of all tasks scheduled to fire after `now`
    /// (next_fire.is_some() && next_fire > now).
    /// Reference: Celery inspect scheduled.
    fn list_scheduled_summary(&self, now: u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>>;

    /// Aggregate worker / queue statistics.
    /// Reference: Celery inspect stats.
    fn worker_stats(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<WorkerStats>> + Send + '_>>;
}

/// Choose store backend based on persist flag.
///
/// 保留为未来 store 工厂模式扩展。当前 daemon_main 直接构造 in_memory/sqlite，
/// 未来若需根据配置动态选择 store 类型可启用此工厂函数。
#[allow(dead_code)]
pub fn make_store(use_persist: bool, db_path: Option<&str>) -> Result<Arc<dyn TaskStore>> {
    #[cfg(feature = "persist")]
    if use_persist {
        let path = db_path.map(|s| s.to_string()).unwrap_or_else(default_db_path);
        let store = SqliteStore::open(&path)?;
        return Ok(Arc::new(store));
    }
    let _ = (use_persist, db_path);
    Ok(Arc::new(InMemoryStore::new()))
}

/// Compute the SQLite DB path for `service_name` with optional `data_dir`.
///
/// Path resolution priority (highest first):
///   1. `data_dir` argument (if `Some`)
///   2. `XHJOB_DB_DIR` env var (fine-grained override)
///   3. `XHJOB_DATA_DIR` env var (unified data directory)
///   4. Platform default (`/tmp` on Unix, `%TEMP%` on Windows)
///
/// Unix: `<dir>/xhjob.{name}.db`
/// Windows: `<dir>\xhjob.{name}.db`
pub fn db_path_for(service_name: &str, data_dir: Option<&str>) -> String {
    let dir = if let Some(d) = data_dir {
        if !d.is_empty() { d.to_string() } else { fallback_db_dir() }
    } else if let Ok(d) = std::env::var("XHJOB_DB_DIR") {
        if !d.is_empty() { d } else { fallback_db_dir() }
    } else if let Ok(d) = std::env::var("XHJOB_DATA_DIR") {
        if !d.is_empty() { d } else { fallback_db_dir() }
    } else {
        fallback_db_dir()
    };
    std::path::PathBuf::from(dir)
        .join(format!("xhjob.{}.db", service_name))
        .to_string_lossy()
        .to_string()
}

/// Fallback DB directory when no explicit dir is provided.
fn fallback_db_dir() -> String {
    #[cfg(unix)]
    { "/tmp".to_string() }
    #[cfg(windows)]
    {
        std::env::temp_dir().to_string_lossy().to_string()
    }
}

/// 保留为未来 store 工厂模式扩展。当前 daemon_main 直接构造 in_memory/sqlite，
/// 未来若需根据配置动态选择 store 类型可启用此辅助函数。
#[allow(dead_code)]
fn default_db_path() -> String {
    db_path_for(&crate::service::current(), crate::service::current_data_dir().as_deref())
}

pub fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TaskBuilder;

    #[test]
    fn test_max_executions_field_default() {
        let task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        assert_eq!(task.max_executions, 0);
        assert_eq!(task.execution_count, 0);
    }

    #[test]
    fn test_builder_max_executions() {
        let task = TaskBuilder::new()
            .via_shell("echo hi")
            .max_executions(3)
            .build()
            .unwrap();
        assert_eq!(task.max_executions, 3);
        assert_eq!(task.execution_count, 0);
    }

    #[test]
    fn test_builder_max_executions_default_zero() {
        let task = TaskBuilder::new()
            .via_shell("echo hi")
            .build()
            .unwrap();
        assert_eq!(task.max_executions, 0);
    }

    #[test]
    fn test_task_serde_roundtrip_preserves_max_executions() {
        let task = TaskBuilder::new()
            .via_shell("echo hi")
            .max_executions(5)
            .build()
            .unwrap();
        let json = serde_json::to_string(&task).unwrap();
        // Simulate an old JSON without the new fields by removing them.
        let stripped = {
            let mut v: serde_json::Value = serde_json::from_str(&json).unwrap();
            if let Some(obj) = v.as_object_mut() {
                obj.remove("max_executions");
                obj.remove("execution_count");
            }
            serde_json::to_string(&v).unwrap()
        };
        let restored: Task = serde_json::from_str(&stripped).unwrap();
        // #[serde(default)] should fill these in with 0.
        assert_eq!(restored.max_executions, 0);
        assert_eq!(restored.execution_count, 0);
    }

    #[test]
    fn test_task_state_cancelled_is_terminal() {
        assert!(TaskState::Cancelled.is_terminal());
        // Other terminal states remain terminal.
        assert!(TaskState::Success.is_terminal());
        assert!(TaskState::Failed.is_terminal());
        // Non-terminal states remain non-terminal.
        assert!(!TaskState::Pending.is_terminal());
        assert!(!TaskState::Running.is_terminal());
        assert!(!TaskState::Interrupted.is_terminal());
    }

    #[test]
    fn test_task_state_cancelled_roundtrip() {
        let s = TaskState::Cancelled.as_str();
        assert_eq!(s, "cancelled");
        assert_eq!(TaskState::from_str(s).unwrap(), TaskState::Cancelled);
    }

    #[test]
    fn test_task_paused_cancel_requested_default_false() {
        let task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        assert!(!task.paused);
        assert!(!task.cancel_requested);
    }

    #[tokio::test]
    async fn test_in_memory_cancel_pending_to_cancelled() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-cancel-pending".to_string();
        // Default state is Pending.
        store.insert_task(task.clone()).await.unwrap();
        store.cancel_task("t-cancel-pending").await.unwrap();
        let loaded = store.load_task("t-cancel-pending").await.unwrap().unwrap();
        assert_eq!(loaded.state, TaskState::Cancelled);
        assert!(loaded.finished_at.is_some());
    }

    #[tokio::test]
    async fn test_in_memory_cancel_running_sets_cancel_requested() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-cancel-running".to_string();
        task.state = TaskState::Running;
        store.insert_task(task.clone()).await.unwrap();
        store.cancel_task("t-cancel-running").await.unwrap();
        let loaded = store.load_task("t-cancel-running").await.unwrap().unwrap();
        // Running tasks: cancel_requested=true, state stays Running.
        assert_eq!(loaded.state, TaskState::Running);
        assert!(loaded.cancel_requested);
    }

    #[tokio::test]
    async fn test_in_memory_cancel_terminal_returns_error() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-cancel-terminal".to_string();
        task.state = TaskState::Success;
        store.insert_task(task.clone()).await.unwrap();
        let res = store.cancel_task("t-cancel-terminal").await;
        assert!(res.is_err(), "cancelling a terminal task should error");
    }

    #[tokio::test]
    async fn test_in_memory_set_paused_roundtrip() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-pause".to_string();
        store.insert_task(task.clone()).await.unwrap();
        // Pause
        store.set_paused("t-pause", true).await.unwrap();
        let loaded = store.load_task("t-pause").await.unwrap().unwrap();
        assert!(loaded.paused);
        // Resume
        store.set_paused("t-pause", false).await.unwrap();
        let loaded = store.load_task("t-pause").await.unwrap().unwrap();
        assert!(!loaded.paused);
    }

    #[tokio::test]
    async fn test_in_memory_remove_task_deletes_task_and_result() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-remove".to_string();
        store.insert_task(task.clone()).await.unwrap();
        store.save_result("t-remove", TaskResult {
            stdout: Some("ok".to_string()),
            ..Default::default()
        }).await.unwrap();
        // Remove
        store.remove_task("t-remove").await.unwrap();
        // Both task and result should be gone.
        assert!(store.load_task("t-remove").await.unwrap().is_none());
        assert!(store.load_result("t-remove").await.unwrap().is_none());
        // Removing a non-existent task should error.
        let res = store.remove_task("t-remove").await;
        assert!(res.is_err());
    }

    #[test]
    fn test_task_start_end_date_default_none() {
        let task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        assert_eq!(task.start_date, None);
        assert_eq!(task.end_date, None);
        assert_eq!(task.result_ttl, 0);
        assert_eq!(task.meta, None);
    }

    #[test]
    fn test_builder_start_end_date() {
        let task = TaskBuilder::new()
            .via_shell("echo hi")
            .start_at(1000)
            .end_at(2000)
            .build()
            .unwrap();
        assert_eq!(task.start_date, Some(1000));
        assert_eq!(task.end_date, Some(2000));
    }

    #[test]
    fn test_builder_result_ttl_and_meta() {
        let task = TaskBuilder::new()
            .via_shell("echo hi")
            .result_ttl(3600)
            .meta(r#"{"k":"v"}"#)
            .build()
            .unwrap();
        assert_eq!(task.result_ttl, 3600);
        assert_eq!(task.meta, Some(r#"{"k":"v"}"#.to_string()));
    }

    #[tokio::test]
    async fn test_in_memory_cleanup_expired_results_removes_expired() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-ttl".to_string();
        task.result_ttl = 10;
        // finished 100s ago: should be expired (now - finished_at > 10).
        task.finished_at = Some(now_ts().saturating_sub(100));
        task.state = TaskState::Success;
        store.insert_task(task).await.unwrap();
        store.save_result("t-ttl", TaskResult {
            stdout: Some("ok".to_string()),
            ..Default::default()
        }).await.unwrap();
        let deleted = store.cleanup_expired_results().await.unwrap();
        assert_eq!(deleted, 1);
        assert!(store.load_result("t-ttl").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_in_memory_cleanup_expired_results_keeps_unexpired() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-ttl-fresh".to_string();
        task.result_ttl = 3600;
        // finished just now: should NOT be expired.
        task.finished_at = Some(now_ts());
        task.state = TaskState::Success;
        store.insert_task(task).await.unwrap();
        store.save_result("t-ttl-fresh", TaskResult {
            stdout: Some("ok".to_string()),
            ..Default::default()
        }).await.unwrap();
        let deleted = store.cleanup_expired_results().await.unwrap();
        assert_eq!(deleted, 0);
        assert!(store.load_result("t-ttl-fresh").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_in_memory_cleanup_expired_results_skips_zero_ttl() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-ttl-zero".to_string();
        // result_ttl = 0 means keep forever.
        task.result_ttl = 0;
        task.finished_at = Some(now_ts().saturating_sub(10_000));
        task.state = TaskState::Success;
        store.insert_task(task).await.unwrap();
        store.save_result("t-ttl-zero", TaskResult {
            stdout: Some("ok".to_string()),
            ..Default::default()
        }).await.unwrap();
        let deleted = store.cleanup_expired_results().await.unwrap();
        assert_eq!(deleted, 0);
        assert!(store.load_result("t-ttl-zero").await.unwrap().is_some());
    }

    /// `list_tasks` with no filter returns all tasks, ordered by created_at ASC.
    /// Reference: APScheduler get_jobs.
    #[tokio::test]
    async fn test_in_memory_list_tasks_returns_all_ordered_by_created_at() {
        let store = InMemoryStore::new();
        let now = now_ts();

        let mut t1 = Task::new(TaskType::Shell, serde_json::json!({"cmd": "a"}));
        t1.id = "t1".to_string();
        t1.created_at = now;
        t1.priority = 5;
        store.insert_task(t1).await.unwrap();

        let mut t2 = Task::new(TaskType::Shell, serde_json::json!({"cmd": "b"}));
        t2.id = "t2".to_string();
        t2.created_at = now + 10;
        t2.priority = 1;
        t2.state = TaskState::Success;
        store.insert_task(t2).await.unwrap();

        let summaries = store.list_tasks(None, None).await.unwrap();
        assert_eq!(summaries.len(), 2);
        // Ordering: created_at ASC -> t1 before t2.
        assert_eq!(summaries[0].id, "t1");
        assert_eq!(summaries[1].id, "t2");
        // TaskSummary carries priority + state.
        assert_eq!(summaries[0].priority, 5);
        assert_eq!(summaries[1].state, TaskState::Success);
    }

    /// `list_tasks` with a state filter returns only matching tasks.
    #[tokio::test]
    async fn test_in_memory_list_tasks_with_state_filter() {
        let store = InMemoryStore::new();

        let mut t1 = Task::new(TaskType::Shell, serde_json::json!({"cmd": "a"}));
        t1.id = "t1".to_string();
        t1.state = TaskState::Pending;
        store.insert_task(t1).await.unwrap();

        let mut t2 = Task::new(TaskType::Shell, serde_json::json!({"cmd": "b"}));
        t2.id = "t2".to_string();
        t2.state = TaskState::Success;
        store.insert_task(t2).await.unwrap();

        let mut t3 = Task::new(TaskType::Shell, serde_json::json!({"cmd": "c"}));
        t3.id = "t3".to_string();
        t3.state = TaskState::Pending;
        store.insert_task(t3).await.unwrap();

        let pending = store.list_tasks(Some(TaskState::Pending), None).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|s| s.state == TaskState::Pending));

        let success = store.list_tasks(Some(TaskState::Success), None).await.unwrap();
        assert_eq!(success.len(), 1);
        assert_eq!(success[0].id, "t2");
    }

    /// `TaskSummary::from(&Task)` carries the expected fields.
    #[test]
    fn test_task_summary_from_task() {
        let mut task = Task::new(TaskType::Http, serde_json::json!({"method":"GET","url":"http://x"}));
        task.id = "abc".to_string();
        task.cron = Some("*/5 * * * *".to_string());
        task.attempts = 3;
        task.priority = 7;
        task.next_fire = Some(12345);
        task.paused = true;
        task.max_executions = 10;
        task.execution_count = 2;
        task.start_date = Some(100);
        task.end_date = Some(200);
        task.meta = Some(r#"{"k":"v"}"#.to_string());
        task.created_at = 99;
        task.finished_at = Some(150);
        task.state = TaskState::Running;

        let s = TaskSummary::from(&task);
        assert_eq!(s.id, "abc");
        assert_eq!(s.task_type, TaskType::Http);
        assert_eq!(s.state, TaskState::Running);
        assert_eq!(s.cron.as_deref(), Some("*/5 * * * *"));
        assert_eq!(s.attempts, 3);
        assert_eq!(s.priority, 7);
        assert_eq!(s.next_fire, Some(12345));
        assert!(s.paused);
        assert_eq!(s.max_executions, 10);
        assert_eq!(s.execution_count, 2);
        assert_eq!(s.start_date, Some(100));
        assert_eq!(s.end_date, Some(200));
        assert_eq!(s.meta.as_deref(), Some(r#"{"k":"v"}"#));
        assert_eq!(s.created_at, 99);
        assert_eq!(s.finished_at, Some(150));
    }

    /// Requeue (C7): Cancelled / Failed / Expired terminal tasks should be
    /// requeueable to Pending with attempts=0 and next_fire=now.
    /// Reference: Celery requeue.
    #[tokio::test]
    async fn test_requeue_resets_terminal_to_pending() {
        for state in &[TaskState::Cancelled, TaskState::Failed, TaskState::Expired] {
            let store = InMemoryStore::new();
            let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo r"}));
            task.id = format!("t-requeue-{:?}", state);
            task.state = *state;
            task.attempts = 3;
            task.last_error = Some("boom".to_string());
            task.finished_at = Some(now_ts().saturating_sub(60));
            task.cancel_requested = true;
            store.insert_task(task).await.unwrap();

            let ok = store.requeue_task(&format!("t-requeue-{:?}", state)).await.unwrap();
            assert!(ok, "requeue of {:?} should return true", state);

            let loaded = store.load_task(&format!("t-requeue-{:?}", state)).await.unwrap().unwrap();
            assert_eq!(loaded.state, TaskState::Pending,
                "after requeue, state should be Pending for {:?}", state);
            assert_eq!(loaded.attempts, 0,
                "after requeue, attempts should be reset to 0 for {:?}", state);
            assert_eq!(loaded.last_error, None,
                "after requeue, last_error should be cleared for {:?}", state);
            assert_eq!(loaded.cancel_requested, false,
                "after requeue, cancel_requested should be cleared for {:?}", state);
            assert!(loaded.next_fire.is_some(),
                "after requeue, next_fire should be set so scan picks it up for {:?}", state);
        }
    }

    /// Requeue (C7): Running and Pending tasks should NOT be requeueable.
    /// Only terminal Cancelled / Failed / Expired tasks are eligible.
    /// Reference: Celery requeue.
    #[tokio::test]
    async fn test_requeue_rejects_running_task() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo r"}));
        task.id = "t-requeue-running".to_string();
        task.state = TaskState::Running;
        task.attempts = 1;
        store.insert_task(task).await.unwrap();

        let ok = store.requeue_task("t-requeue-running").await.unwrap();
        assert!(!ok, "requeue of Running task should return false");

        let loaded = store.load_task("t-requeue-running").await.unwrap().unwrap();
        assert_eq!(loaded.state, TaskState::Running, "Running state should be preserved");
        assert_eq!(loaded.attempts, 1, "attempts should be unchanged");

        // Same for Pending.
        let mut p = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo p"}));
        p.id = "t-requeue-pending".to_string();
        p.state = TaskState::Pending;
        store.insert_task(p).await.unwrap();
        let ok = store.requeue_task("t-requeue-pending").await.unwrap();
        assert!(!ok, "requeue of Pending task should return false");

        // Non-existent task should also return false (not an error).
        let ok = store.requeue_task("does-not-exist").await.unwrap();
        assert!(!ok, "requeue of non-existent task should return false");
    }

    /// Reschedule (A11): reschedule_task updates the cron field and
    /// re-computes next_fire while preserving state / execution_count /
    /// attempts. Reference: APScheduler reschedule_job.
    #[tokio::test]
    async fn test_reschedule_updates_cron_keeps_state() {
        let store = InMemoryStore::new();
        // Simulate a cron task that has executed 3 times: cron='*/5 * * * *',
        // execution_count=3, attempts=1, state=Pending (between triggers).
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo r"}));
        task.id = "t-resched".to_string();
        task.cron = Some("*/5 * * * *".to_string());
        task.execution_count = 3;
        task.attempts = 1;
        task.state = TaskState::Pending;
        task.next_fire = Some(now_ts() + 60);
        // next_fire was previously computed for */5 * * * * (>= 60s away).
        store.insert_task(task).await.unwrap();

        // Reschedule to every minute.
        let ok = store.reschedule_task("t-resched", "*/1 * * * *").await.unwrap();
        assert!(ok, "reschedule of an active cron task should return true");

        let loaded = store.load_task("t-resched").await.unwrap().unwrap();
        // Cron field updated.
        assert_eq!(loaded.cron.as_deref(), Some("*/1 * * * *"),
            "cron should be updated to */1 * * * *");
        // State, execution_count, attempts preserved.
        assert_eq!(loaded.execution_count, 3,
            "execution_count should be preserved (was 3)");
        assert_eq!(loaded.attempts, 1,
            "attempts should be preserved (was 1)");
        assert_eq!(loaded.state, TaskState::Pending,
            "state should be preserved as Pending");
        // next_fire re-computed for */1 * * * * (within the next minute or so).
        let now = now_ts();
        let new_next = loaded.next_fire.expect("next_fire should be set");
        assert!(new_next > now,
            "next_fire should be in the future, got {} (now={})", new_next, now);
        assert!(new_next <= now + 65,
            "next_fire for */1 * * * * should be within 65s, got {} (now+65={})",
            new_next, now + 65);
    }

    /// Reschedule (A11): reschedule_task of a non-cron task (interval /
    /// runAt) returns false (no error). Reference: APScheduler reschedule_job.
    #[tokio::test]
    async fn test_reschedule_rejects_non_cron_task() {
        let store = InMemoryStore::new();
        // Interval task (no cron).
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo i"}));
        task.id = "t-interval-resched".to_string();
        task.interval = Some(10);
        task.state = TaskState::Pending;
        store.insert_task(task).await.unwrap();

        let ok = store.reschedule_task("t-interval-resched", "*/1 * * * *").await.unwrap();
        assert!(!ok, "reschedule of a non-cron interval task should return false");

        // Same for runAt.
        let mut task2 = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo r"}));
        task2.id = "t-runat-resched".to_string();
        task2.run_at = Some(now_ts() as i64 + 60);
        task2.state = TaskState::Pending;
        store.insert_task(task2).await.unwrap();

        let ok = store.reschedule_task("t-runat-resched", "*/1 * * * *").await.unwrap();
        assert!(!ok, "reschedule of a non-cron runAt task should return false");

        // Non-existent task also returns false (no error).
        let ok = store.reschedule_task("does-not-exist", "*/1 * * * *").await.unwrap();
        assert!(!ok, "reschedule of non-existent task should return false");
    }

    /// Reschedule (A11): reschedule_task of a terminal task returns false.
    /// Reference: APScheduler reschedule_job.
    #[tokio::test]
    async fn test_reschedule_rejects_terminal_task() {
        for state in &[TaskState::Success, TaskState::Failed, TaskState::Cancelled, TaskState::Expired] {
            let store = InMemoryStore::new();
            let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo t"}));
            task.id = format!("t-resched-{:?}", state);
            task.cron = Some("*/5 * * * *".to_string());
            task.state = *state;
            store.insert_task(task).await.unwrap();

            let ok = store.reschedule_task(&format!("t-resched-{:?}", state), "*/1 * * * *").await.unwrap();
            assert!(!ok, "reschedule of {:?} terminal task should return false", state);
        }
    }

    /// Reschedule (A11): reschedule_task with an invalid cron expression
    /// returns Err, and the error message contains "invalid cron". The cron
    /// field is left unchanged (the UPDATE is not applied).
    /// Reference: APScheduler reschedule_job.
    #[tokio::test]
    async fn test_reschedule_invalid_cron_returns_false() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo r"}));
        task.id = "t-resched-invalid".to_string();
        task.cron = Some("*/5 * * * *".to_string());
        task.state = TaskState::Pending;
        task.next_fire = Some(now_ts() + 60);
        store.insert_task(task).await.unwrap();

        // "not a valid cron ###" is not parseable: cron::Schedule::from_str rejects it.
        let result = store.reschedule_task("t-resched-invalid", "not a valid cron ###").await;
        assert!(result.is_err(), "invalid cron should return Err");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("invalid cron") || msg.contains("cron parse"),
            "error message should mention invalid cron, got: {}", msg);

        // The task's cron field should be UNCHANGED (not updated to the invalid value).
        let loaded = store.load_task("t-resched-invalid").await.unwrap().unwrap();
        assert_eq!(loaded.cron.as_deref(), Some("*/5 * * * *"),
            "cron field should be unchanged after invalid reschedule");
    }

    /// get_job (A12): the daemon handler serializes a Task to JSON; verify
    /// that the serialized JSON contains the expected configuration fields
    /// (cron / retry_max / timeout / priority / allow_overlap / max_instances
    /// / coalesce / interval / run_at / etc.) so that `xhjob_get` callers
    /// receive the full task definition.
    /// Reference: APScheduler get_job.
    #[tokio::test]
    async fn test_xhjob_get_returns_full_task_json() {
        let store = InMemoryStore::new();
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hi"}));
        task.id = "t-get-full".to_string();
        task.cron = Some("*/5 * * * *".to_string());
        task.retry_max = 7;
        task.retry_delay = 13;
        task.timeout = 45;
        task.priority = 9;
        task.allow_overlap = true;
        task.max_instances = 3;
        task.coalesce = false;
        task.max_executions = 5;
        task.result_ttl = 120;
        task.meta = Some(r#"{"k":"v"}"#.to_string());
        task.interval = Some(20);
        task.run_at = Some(1234567);
        task.jitter = 4;
        task.expires = 60;
        task.retry_backoff = true;
        task.start_date = Some(1000);
        task.end_date = Some(2000);
        task.timezone = Some("Asia/Shanghai".to_string());
        task.proxy = Some("http://proxy.example".to_string());
        task.encoding = Some("GBK".to_string());
        store.insert_task(task).await.unwrap();

        // Mimic handle_get_op: load_task + serialize to JSON.
        let loaded = store.load_task("t-get-full").await.unwrap().unwrap();
        let json = serde_json::to_string(&loaded).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = v.as_object().expect("task json should be an object");

        // Configuration fields that distinguish `get` from `state`.
        assert_eq!(obj.get("cron").and_then(|v| v.as_str()), Some("*/5 * * * *"));
        assert_eq!(obj.get("retry_max").and_then(|v| v.as_u64()), Some(7));
        assert_eq!(obj.get("retry_delay").and_then(|v| v.as_u64()), Some(13));
        assert_eq!(obj.get("timeout").and_then(|v| v.as_u64()), Some(45));
        assert_eq!(obj.get("priority").and_then(|v| v.as_i64()), Some(9));
        assert_eq!(obj.get("allow_overlap").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(obj.get("max_instances").and_then(|v| v.as_u64()), Some(3));
        assert_eq!(obj.get("coalesce").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(obj.get("max_executions").and_then(|v| v.as_u64()), Some(5));
        assert_eq!(obj.get("result_ttl").and_then(|v| v.as_u64()), Some(120));
        assert_eq!(obj.get("meta").and_then(|v| v.as_str()), Some(r#"{"k":"v"}"#));
        assert_eq!(obj.get("interval").and_then(|v| v.as_u64()), Some(20));
        assert_eq!(obj.get("run_at").and_then(|v| v.as_i64()), Some(1234567));
        assert_eq!(obj.get("jitter").and_then(|v| v.as_u64()), Some(4));
        assert_eq!(obj.get("expires").and_then(|v| v.as_u64()), Some(60));
        assert_eq!(obj.get("retry_backoff").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(obj.get("start_date").and_then(|v| v.as_i64()), Some(1000));
        assert_eq!(obj.get("end_date").and_then(|v| v.as_i64()), Some(2000));
        assert_eq!(obj.get("timezone").and_then(|v| v.as_str()), Some("Asia/Shanghai"));
        assert_eq!(obj.get("proxy").and_then(|v| v.as_str()), Some("http://proxy.example"));
        assert_eq!(obj.get("encoding").and_then(|v| v.as_str()), Some("GBK"));
        // Identity + execution metadata also present.
        // TaskState 通过 `#[serde(rename_all = "lowercase")]` 序列化为小写形式，
        // 与 `as_str()` 输出一致；xhjob_get / xhjob_state / xhjob_list 均返回该小写形式。
        assert_eq!(obj.get("id").and_then(|v| v.as_str()), Some("t-get-full"));
        assert_eq!(obj.get("task_type").and_then(|v| v.as_str()), Some("shell"));
        assert_eq!(obj.get("state").and_then(|v| v.as_str()), Some("pending"));
        assert_eq!(obj.get("attempts").and_then(|v| v.as_u64()), Some(0));
        assert_eq!(obj.get("execution_count").and_then(|v| v.as_u64()), Some(0));
        assert!(obj.contains_key("created_at"));
        assert!(obj.contains_key("next_fire"));
        assert!(obj.contains_key("started_at"));
        assert!(obj.contains_key("finished_at"));
        assert!(obj.contains_key("last_error"));
        assert!(obj.contains_key("paused"));
        assert!(obj.contains_key("cancel_requested"));
        assert!(obj.contains_key("persist"));
        assert!(obj.contains_key("payload"));
    }

    /// get_job (A12): `load_task` of a non-existent id returns None, which
    /// the daemon handler translates to `{"ok": false, "error": "not found"}`
    /// and the PHP `xhjob_get` maps to `null`. Verify the store contract.
    /// Reference: APScheduler get_job.
    #[tokio::test]
    async fn test_xhjob_get_nonexistent_returns_none() {
        let store = InMemoryStore::new();
        let loaded = store.load_task("does-not-exist").await.unwrap();
        assert!(loaded.is_none(),
            "load_task of a non-existent id should return None (translated to PHP null)");
    }

    /// acksLate (C10): `reset_running_to_pending` only resets Running tasks
    /// with `acks_late=true` to Pending + next_fire=now. Running tasks with
    /// `acks_late=false` are left untouched (Celery "ack early" semantics).
    /// Reference: Celery acks_late.
    #[tokio::test]
    async fn test_reset_running_to_pending_only_acks_late() {
        let store = InMemoryStore::new();

        // acks_late=true Running task — should be reset.
        let mut t_late = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo late"}));
        t_late.id = "t-running-late".to_string();
        t_late.state = TaskState::Running;
        t_late.acks_late = true;
        t_late.started_at = Some(now_ts().saturating_sub(10));
        store.insert_task(t_late).await.unwrap();

        // acks_late=false Running task — should NOT be reset.
        let mut t_early = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo early"}));
        t_early.id = "t-running-early".to_string();
        t_early.state = TaskState::Running;
        t_early.acks_late = false;
        t_early.started_at = Some(now_ts().saturating_sub(5));
        store.insert_task(t_early).await.unwrap();

        let reset = store.reset_running_to_pending().await.unwrap();
        assert_eq!(reset, 1, "only the acks_late=true Running task should be reset");

        let loaded_late = store.load_task("t-running-late").await.unwrap().unwrap();
        assert_eq!(loaded_late.state, TaskState::Pending,
            "acks_late=true Running task should be reset to Pending");
        assert!(loaded_late.next_fire.is_some(),
            "acks_late=true Running task should have next_fire set so scan picks it up");
        assert!(loaded_late.started_at.is_none(),
            "acks_late=true Running task should have started_at cleared");
        assert!(loaded_late.finished_at.is_none(),
            "acks_late=true Running task should have finished_at cleared");

        let loaded_early = store.load_task("t-running-early").await.unwrap().unwrap();
        assert_eq!(loaded_early.state, TaskState::Running,
            "acks_late=false Running task should stay Running (ack early semantics)");
        assert!(loaded_early.started_at.is_some(),
            "acks_late=false Running task should keep its started_at");
    }

    /// acksLate (C10): `reset_running_to_pending` does not affect Pending or
    /// terminal-state tasks — only Running tasks with `acks_late=true` are
    /// eligible for reset.
    /// Reference: Celery acks_late.
    #[tokio::test]
    async fn test_reset_running_to_pending_skips_non_running() {
        let store = InMemoryStore::new();
        let now = now_ts();

        // Pending task with acks_late=true — should NOT be reset (already Pending).
        let mut t_pending = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo p"}));
        t_pending.id = "t-pending".to_string();
        t_pending.state = TaskState::Pending;
        t_pending.acks_late = true;
        t_pending.next_fire = Some(now + 60);
        store.insert_task(t_pending).await.unwrap();

        // Success task with acks_late=true — should NOT be reset (terminal).
        let mut t_success = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo s"}));
        t_success.id = "t-success".to_string();
        t_success.state = TaskState::Success;
        t_success.acks_late = true;
        t_success.finished_at = Some(now.saturating_sub(30));
        store.insert_task(t_success).await.unwrap();

        // Failed task with acks_late=true — should NOT be reset (terminal).
        let mut t_failed = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo f"}));
        t_failed.id = "t-failed".to_string();
        t_failed.state = TaskState::Failed;
        t_failed.acks_late = true;
        t_failed.finished_at = Some(now.saturating_sub(30));
        store.insert_task(t_failed).await.unwrap();

        let reset = store.reset_running_to_pending().await.unwrap();
        assert_eq!(reset, 0, "no Running tasks -> reset count should be 0");

        // Verify states are unchanged.
        let p = store.load_task("t-pending").await.unwrap().unwrap();
        assert_eq!(p.state, TaskState::Pending);
        assert_eq!(p.next_fire, Some(now + 60),
            "Pending task next_fire should be unchanged");

        let s = store.load_task("t-success").await.unwrap().unwrap();
        assert_eq!(s.state, TaskState::Success);

        let f = store.load_task("t-failed").await.unwrap().unwrap();
        assert_eq!(f.state, TaskState::Failed);
    }
}
