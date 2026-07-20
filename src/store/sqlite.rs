//! Optional SQLite-backed task store. Enabled via `persist` cargo feature.

use std::sync::Arc;
use tokio::sync::Mutex;
use rusqlite::{Connection, params};
use crate::errors::{Result, XhjobError};
use super::{Task, TaskType, TaskState, TaskResult, TaskStore};

pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Open or create the SQLite database at `path`.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| XhjobError::Store(format!("open {}: {}", path, e)))?;
        // Enable WAL mode
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| XhjobError::Store(format!("set WAL: {}", e)))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| XhjobError::Store(format!("set synchronous: {}", e)))?;
        // Schema
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS tasks (
                id            TEXT PRIMARY KEY,
                type          TEXT NOT NULL,
                payload       TEXT NOT NULL,
                cron          TEXT,
                retry_max     INTEGER NOT NULL DEFAULT 0,
                retry_delay   INTEGER NOT NULL DEFAULT 1,
                timeout       INTEGER NOT NULL DEFAULT 30,
                priority      INTEGER NOT NULL DEFAULT 0,
                allow_overlap INTEGER NOT NULL DEFAULT 0,
                max_instances INTEGER NOT NULL DEFAULT 1,
                coalesce      INTEGER NOT NULL DEFAULT 1,
                persist       INTEGER NOT NULL DEFAULT 0,
                state         TEXT NOT NULL,
                attempts      INTEGER NOT NULL DEFAULT 0,
                next_fire     INTEGER,
                created_at    INTEGER NOT NULL,
                started_at    INTEGER,
                finished_at   INTEGER,
                last_error    TEXT,
                proxy         TEXT,
                encoding      TEXT,
                timezone      TEXT
            );
            CREATE TABLE IF NOT EXISTS results (
                task_id      TEXT PRIMARY KEY,
                body         TEXT,
                status_code  INTEGER,
                stdout       TEXT,
                stderr       TEXT,
                exit_code    INTEGER,
                FOREIGN KEY (task_id) REFERENCES tasks(id)
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);
            CREATE INDEX IF NOT EXISTS idx_tasks_next_fire ON tasks(next_fire);
            "#,
        ).map_err(|e| XhjobError::Store(format!("create schema: {}", e)))?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let type_str: String = row.get("type")?;
    let task_type = match type_str.as_str() {
        "http" => TaskType::Http,
        "shell" => TaskType::Shell,
        _ => TaskType::Shell,
    };
    let payload_str: String = row.get("payload")?;
    let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
    let state_str: String = row.get("state")?;
    let state = match state_str.as_str() {
        "PENDING" => TaskState::Pending,
        "RUNNING" => TaskState::Running,
        "SUCCESS" => TaskState::Success,
        "FAILED" => TaskState::Failed,
        "INTERRUPTED" => TaskState::Interrupted,
        _ => TaskState::Pending,
    };
    Ok(Task {
        id: row.get("id")?,
        task_type,
        payload,
        cron: row.get("cron")?,
        retry_max: row.get("retry_max")?,
        retry_delay: row.get("retry_delay")?,
        timeout: row.get("timeout")?,
        priority: row.get("priority")?,
        allow_overlap: row.get::<_, i64>("allow_overlap")? != 0,
        max_instances: row.get("max_instances")?,
        coalesce: row.get::<_, i64>("coalesce")? != 0,
        persist: row.get::<_, i64>("persist")? != 0,
        // These columns may be NULL for legacy rows that predate Task 25;
        // rusqlite transparently maps SQL NULL to Option::<String>::None.
        proxy: row.get("proxy")?,
        encoding: row.get("encoding")?,
        timezone: row.get("timezone")?,
        state,
        attempts: row.get("attempts")?,
        next_fire: row.get("next_fire")?,
        created_at: row.get("created_at")?,
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        last_error: row.get("last_error")?,
    })
}

impl TaskStore for SqliteStore {
    fn insert_task(&self, task: Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let payload_str = serde_json::to_string(&task.payload).unwrap_or_default();
            let type_str = task.task_type.as_str();
            let state_str = task.state.as_str();
            conn.execute(
                "INSERT OR REPLACE INTO tasks
                 (id, type, payload, cron, retry_max, retry_delay, timeout, priority,
                  allow_overlap, max_instances, coalesce, persist, state, attempts,
                  next_fire, created_at, started_at, finished_at, last_error,
                  proxy, encoding, timezone)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
                params![
                    task.id, type_str, payload_str, task.cron,
                    task.retry_max, task.retry_delay, task.timeout, task.priority,
                    task.allow_overlap as i64, task.max_instances, task.coalesce as i64, task.persist as i64,
                    state_str, task.attempts, task.next_fire,
                    task.created_at, task.started_at, task.finished_at, task.last_error,
                    task.proxy, task.encoding, task.timezone,
                ],
            ).map_err(|e| XhjobError::Store(format!("insert: {}", e)))?;
            Ok(())
        })
    }

    fn update_state(&self, id: &str, state: TaskState, started_at: Option<u64>, finished_at: Option<u64>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute(
                "UPDATE tasks SET state = ?1, started_at = COALESCE(?2, started_at), finished_at = COALESCE(?3, finished_at) WHERE id = ?4",
                params![state.as_str(), started_at, finished_at, id],
            ).map_err(|e| XhjobError::Store(format!("update_state: {}", e)))?;
            Ok(())
        })
    }

    fn save_result(&self, task_id: &str, result: TaskResult) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let task_id = task_id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute(
                "INSERT OR REPLACE INTO results (task_id, body, status_code, stdout, stderr, exit_code)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![task_id, result.body, result.status_code, result.stdout, result.stderr, result.exit_code],
            ).map_err(|e| XhjobError::Store(format!("save_result: {}", e)))?;
            Ok(())
        })
    }

    fn load_active_tasks(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Task>>> + Send + '_>> {
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT * FROM tasks WHERE state IN ('PENDING', 'RUNNING', 'INTERRUPTED')"
            ).map_err(|e| XhjobError::Store(format!("prepare: {}", e)))?;
            let rows = stmt.query_map([], task_from_row)
                .map_err(|e| XhjobError::Store(format!("query: {}", e)))?;
            let mut tasks = Vec::new();
            for row in rows {
                if let Ok(t) = row { tasks.push(t); }
            }
            Ok(tasks)
        })
    }

    fn load_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Task>>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare("SELECT * FROM tasks WHERE id = ?1")
                .map_err(|e| XhjobError::Store(format!("prepare: {}", e)))?;
            let mut rows = stmt.query_map(params![id], task_from_row)
                .map_err(|e| XhjobError::Store(format!("query: {}", e)))?;
            if let Some(row) = rows.next() {
                if let Ok(t) = row { return Ok(Some(t)); }
            }
            Ok(None)
        })
    }

    fn load_result(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<TaskResult>>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare("SELECT body, status_code, stdout, stderr, exit_code FROM results WHERE task_id = ?1")
                .map_err(|e| XhjobError::Store(format!("prepare: {}", e)))?;
            let mut rows = stmt.query_map(params![id], |row| {
                Ok(TaskResult {
                    body: row.get(0)?,
                    status_code: row.get(1)?,
                    stdout: row.get(2)?,
                    stderr: row.get(3)?,
                    exit_code: row.get(4)?,
                })
            }).map_err(|e| XhjobError::Store(format!("query: {}", e)))?;
            if let Some(row) = rows.next() {
                if let Ok(r) = row { return Ok(Some(r)); }
            }
            Ok(None)
        })
    }

    fn count_running_instances(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE id = ?1 AND state = 'RUNNING'",
                params![id],
                |row| row.get(0),
            ).map_err(|e| XhjobError::Store(format!("count: {}", e)))?;
            Ok(count as u32)
        })
    }

    fn update_next_fire(&self, id: &str, next_fire: Option<u64>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute(
                "UPDATE tasks SET next_fire = ?1 WHERE id = ?2",
                params![next_fire, id],
            ).map_err(|e| XhjobError::Store(format!("update_next_fire: {}", e)))?;
            Ok(())
        })
    }

    fn set_attempts_and_error(&self, id: &str, attempts: u32, last_error: Option<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute(
                "UPDATE tasks SET attempts = ?1, last_error = ?2 WHERE id = ?3",
                params![attempts, last_error, id],
            ).map_err(|e| XhjobError::Store(format!("set_attempts: {}", e)))?;
            Ok(())
        })
    }

    fn delete_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute("DELETE FROM results WHERE task_id = ?1", params![id])
                .map_err(|e| XhjobError::Store(format!("delete result: {}", e)))?;
            conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])
                .map_err(|e| XhjobError::Store(format!("delete task: {}", e)))?;
            Ok(())
        })
    }
}
