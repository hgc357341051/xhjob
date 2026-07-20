//! Task store abstraction: in-memory (default) and SQLite (optional `persist` feature).

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};
use tokio::sync::RwLock;
use crate::errors::{Result, XhjobError};

pub mod in_memory;
#[cfg(feature = "persist")]
pub mod sqlite;

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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Running,
    Success,
    Failed,
    Interrupted,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Pending => "PENDING",
            TaskState::Running => "RUNNING",
            TaskState::Success => "SUCCESS",
            TaskState::Failed => "FAILED",
            TaskState::Interrupted => "INTERRUPTED",
        }
    }
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "PENDING" => Ok(TaskState::Pending),
            "RUNNING" => Ok(TaskState::Running),
            "SUCCESS" => Ok(TaskState::Success),
            "FAILED" => Ok(TaskState::Failed),
            "INTERRUPTED" => Ok(TaskState::Interrupted),
            other => Err(XhjobError::Store(format!("unknown state: {}", other))),
        }
    }
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskState::Success | TaskState::Failed)
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
}

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
}

/// Choose store backend based on persist flag.
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
