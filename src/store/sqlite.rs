//! Optional SQLite-backed task store. Enabled via `persist` cargo feature.

use std::sync::Arc;
use tokio::sync::Mutex;
use rusqlite::{Connection, params, OptionalExtension};
use crate::errors::{Result, XhjobError};
use super::{Task, TaskType, TaskState, TaskResult, TaskStore, TaskSummary, TaskEvent, ChainRecord, GroupRecord};

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
                timezone      TEXT,
                max_executions INTEGER NOT NULL DEFAULT 0,
                execution_count INTEGER NOT NULL DEFAULT 0,
                paused INTEGER NOT NULL DEFAULT 0,
                cancel_requested INTEGER NOT NULL DEFAULT 0,
                start_date INTEGER,
                end_date INTEGER,
                result_ttl INTEGER NOT NULL DEFAULT 0,
                meta TEXT,
                interval INTEGER,
                run_at INTEGER,
                jitter INTEGER NOT NULL DEFAULT 0,
                expires INTEGER NOT NULL DEFAULT 0,
                retry_backoff INTEGER NOT NULL DEFAULT 0,
                ignore_result INTEGER NOT NULL DEFAULT 0,
                acks_late INTEGER NOT NULL DEFAULT 0,
                soft_timeout INTEGER,
                misfire_grace_time INTEGER NOT NULL DEFAULT 0,
                replace_existing INTEGER NOT NULL DEFAULT 0,
                tags TEXT NOT NULL DEFAULT '[]',
                rate_limit_count INTEGER NOT NULL DEFAULT 0,
                rate_limit_window INTEGER NOT NULL DEFAULT 0,
                acks_on_failure INTEGER NOT NULL DEFAULT 1
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
            CREATE TABLE IF NOT EXISTS events (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id     TEXT NOT NULL,
                event_type  TEXT NOT NULL,
                payload    TEXT,
                ts         INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_ts ON events(ts);
            CREATE INDEX IF NOT EXISTS idx_events_task_id ON events(task_id);
            CREATE TABLE IF NOT EXISTS chains (
                chain_id     TEXT PRIMARY KEY,
                tasks        TEXT NOT NULL,
                current_step INTEGER NOT NULL DEFAULT 0,
                state        TEXT NOT NULL DEFAULT 'pending',
                created_at   INTEGER NOT NULL,
                updated_at   INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS groups (
                group_id    TEXT PRIMARY KEY,
                tasks       TEXT NOT NULL,
                state       TEXT NOT NULL DEFAULT 'pending',
                created_at  INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);
            CREATE INDEX IF NOT EXISTS idx_tasks_next_fire ON tasks(next_fire);
            "#,
        ).map_err(|e| XhjobError::Store(format!("create schema: {}", e)))?;
        // Legacy DB migration: add new columns if missing.
        ensure_column(&conn, "max_executions", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "execution_count", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "paused", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "cancel_requested", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "start_date", "INTEGER")?;
        ensure_column(&conn, "end_date", "INTEGER")?;
        ensure_column(&conn, "result_ttl", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "meta", "TEXT")?;
        ensure_column(&conn, "interval", "INTEGER")?;
        ensure_column(&conn, "run_at", "INTEGER")?;
        ensure_column(&conn, "jitter", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "expires", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "retry_backoff", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "ignore_result", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "acks_late", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "soft_timeout", "INTEGER")?;
        ensure_column(&conn, "misfire_grace_time", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "replace_existing", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "tags", "TEXT NOT NULL DEFAULT '[]'")?;
        ensure_column(&conn, "rate_limit_count", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "rate_limit_window", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(&conn, "acks_on_failure", "INTEGER NOT NULL DEFAULT 1")?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }
}

/// Add a column to the `tasks` table if it does not already exist. Used to
/// migrate older databases that predate a schema change.
fn ensure_column(conn: &Connection, name: &str, sql_type: &str) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(tasks)")
        .map_err(|e| XhjobError::Store(format!("pragma table_info: {}", e)))?;
    let cols: Vec<String> = stmt.query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| XhjobError::Store(format!("pragma query_map: {}", e)))?
        .filter_map(|r| r.ok())
        .collect();
    if !cols.iter().any(|c| c == name) {
        conn.execute(
            &format!("ALTER TABLE tasks ADD COLUMN {} {}", name, sql_type),
            [],
        ).map_err(|e| XhjobError::Store(format!("alter table add {}: {}", name, e)))?;
    }
    Ok(())
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
        "CANCELLED" => TaskState::Cancelled,
        "EXPIRED" => TaskState::Expired,
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
        max_executions: row.get::<_, i64>("max_executions").unwrap_or(0) as u32,
        execution_count: row.get::<_, i64>("execution_count").unwrap_or(0) as u32,
        paused: row.get::<_, i64>("paused").unwrap_or(0) != 0,
        cancel_requested: row.get::<_, i64>("cancel_requested").unwrap_or(0) != 0,
        start_date: row.get::<_, Option<i64>>("start_date").ok().flatten(),
        end_date: row.get::<_, Option<i64>>("end_date").ok().flatten(),
        result_ttl: row.get::<_, i64>("result_ttl").unwrap_or(0) as u64,
        meta: row.get::<_, Option<String>>("meta").ok().flatten(),
        interval: row.get::<_, Option<i64>>("interval").ok().flatten().map(|v| v as u64),
        run_at: row.get::<_, Option<i64>>("run_at").ok().flatten(),
        jitter: row.get::<_, i64>("jitter").unwrap_or(0) as u64,
        expires: row.get::<_, i64>("expires").unwrap_or(0) as u64,
        retry_backoff: row.get::<_, i64>("retry_backoff").unwrap_or(0) != 0,
        ignore_result: row.get::<_, i64>("ignore_result").unwrap_or(0) != 0,
        acks_late: row.get::<_, i64>("acks_late").unwrap_or(0) != 0,
        soft_timeout: row.get::<_, Option<i64>>("soft_timeout").ok().flatten().map(|v| v as u64),
        // Round 4 fields. Use unwrap_or for legacy-row safety.
        misfire_grace_time: row.get::<_, i64>("misfire_grace_time").unwrap_or(0) as u64,
        replace_existing: row.get::<_, i64>("replace_existing").unwrap_or(0) != 0,
        tags: {
            let s: String = row.get::<_, String>("tags").unwrap_or_else(|_| "[]".to_string());
            serde_json::from_str(&s).unwrap_or_default()
        },
        rate_limit_count: row.get::<_, i64>("rate_limit_count").unwrap_or(0) as u32,
        rate_limit_window: row.get::<_, i64>("rate_limit_window").unwrap_or(0) as u64,
        acks_on_failure: row.get::<_, i64>("acks_on_failure").unwrap_or(1) != 0,
    })
}

impl TaskStore for SqliteStore {
    fn insert_task(&self, task: Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let payload_str = serde_json::to_string(&task.payload).unwrap_or_default();
            let type_str = task.task_type.as_str();
            let state_str = task.state.as_str();
            let tags_str = serde_json::to_string(&task.tags).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "INSERT OR REPLACE INTO tasks
                 (id, type, payload, cron, retry_max, retry_delay, timeout, priority,
                  allow_overlap, max_instances, coalesce, persist, state, attempts,
                  next_fire, created_at, started_at, finished_at, last_error,
                  proxy, encoding, timezone, max_executions, execution_count,
                  paused, cancel_requested, start_date, end_date, result_ttl, meta,
                  interval, run_at, jitter, expires, retry_backoff, ignore_result,
                  acks_late, soft_timeout, misfire_grace_time, replace_existing,
                  tags, rate_limit_count, rate_limit_window, acks_on_failure)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43)",
                params![
                    task.id, type_str, payload_str, task.cron,
                    task.retry_max, task.retry_delay, task.timeout, task.priority,
                    task.allow_overlap as i64, task.max_instances, task.coalesce as i64, task.persist as i64,
                    state_str, task.attempts, task.next_fire,
                    task.created_at, task.started_at, task.finished_at, task.last_error,
                    task.proxy, task.encoding, task.timezone,
                    task.max_executions as i64, task.execution_count as i64,
                    task.paused as i64, task.cancel_requested as i64,
                    task.start_date, task.end_date, task.result_ttl as i64, task.meta,
                    task.interval.map(|v| v as i64), task.run_at,
                    task.jitter as i64, task.expires as i64,
                    task.retry_backoff as i64, task.ignore_result as i64,
                    task.acks_late as i64,
                    task.soft_timeout.map(|v| v as i64),
                    task.misfire_grace_time as i64,
                    task.replace_existing as i64,
                    tags_str,
                    task.rate_limit_count as i64,
                    task.rate_limit_window as i64,
                    task.acks_on_failure as i64,
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

    fn increment_execution_count(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute(
                "UPDATE tasks SET execution_count = execution_count + 1 WHERE id = ?1",
                params![id],
            ).map_err(|e| XhjobError::Store(format!("increment_execution_count update: {}", e)))?;
            let new_count: i64 = conn.query_row(
                "SELECT execution_count FROM tasks WHERE id = ?1",
                params![id],
                |row| row.get(0),
            ).map_err(|e| XhjobError::Store(format!("increment_execution_count select: {}", e)))?;
            Ok(new_count as u32)
        })
    }

    fn remove_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let affected = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])
                .map_err(|e| XhjobError::Store(format!("remove_task: {}", e)))?;
            if affected == 0 {
                return Err(XhjobError::TaskNotFound(id));
            }
            conn.execute("DELETE FROM results WHERE task_id = ?1", params![id])
                .map_err(|e| XhjobError::Store(format!("remove_task results: {}", e)))?;
            Ok(())
        })
    }

    fn set_paused(&self, id: &str, paused: bool) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let affected = conn.execute(
                "UPDATE tasks SET paused = ?1 WHERE id = ?2",
                params![paused as i64, id],
            ).map_err(|e| XhjobError::Store(format!("set_paused: {}", e)))?;
            if affected == 0 {
                return Err(XhjobError::TaskNotFound(id));
            }
            Ok(())
        })
    }

    fn cancel_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let state_str: String = conn.query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                params![id],
                |row| row.get(0),
            ).map_err(|e| XhjobError::Store(format!("cancel_task query: {}", e)))?;
            if state_str == "PENDING" {
                conn.execute(
                    "UPDATE tasks SET state = 'CANCELLED', cancel_requested = 1, finished_at = ?1 WHERE id = ?2",
                    params![crate::store::now_ts() as i64, id],
                ).map_err(|e| XhjobError::Store(format!("cancel_task update: {}", e)))?;
            } else if state_str == "RUNNING" {
                conn.execute(
                    "UPDATE tasks SET cancel_requested = 1 WHERE id = ?1",
                    params![id],
                ).map_err(|e| XhjobError::Store(format!("cancel_task update: {}", e)))?;
            } else {
                return Err(XhjobError::InvalidTask(format!(
                    "task {} already in terminal state: {}",
                    id, state_str
                )));
            }
            Ok(())
        })
    }

    fn cleanup_expired_results(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let now = crate::store::now_ts() as i64;
            let deleted = conn.execute(
                "DELETE FROM results WHERE task_id IN (
                    SELECT id FROM tasks
                    WHERE result_ttl > 0
                      AND finished_at IS NOT NULL
                      AND (?1 - finished_at) > result_ttl
                )",
                params![now],
            ).map_err(|e| XhjobError::Store(format!("cleanup_expired_results: {}", e)))?;
            Ok(deleted as u64)
        })
    }

    fn list_tasks<'a>(&'a self, state_filter: Option<TaskState>, tag_filter: Option<&'a str>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + 'a>> {
        let tag_filter = tag_filter.map(|s| s.to_string());
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut result = Vec::new();
            match state_filter {
                Some(state) => {
                    let mut stmt = conn.prepare(
                        "SELECT * FROM tasks WHERE state = ?1 ORDER BY created_at ASC"
                    ).map_err(|e| XhjobError::Store(format!("list_tasks prepare: {}", e)))?;
                    let rows = stmt.query_map(params![state.as_str()], task_from_row)
                        .map_err(|e| XhjobError::Store(format!("list_tasks query: {}", e)))?;
                    for row in rows {
                        let task = row.map_err(|e| XhjobError::Store(format!("list_tasks row: {}", e)))?;
                        if let Some(tag) = &tag_filter {
                            if !task.tags.iter().any(|t| t == tag) { continue; }
                        }
                        result.push(TaskSummary::from(&task));
                    }
                }
                None => {
                    let mut stmt = conn.prepare(
                        "SELECT * FROM tasks ORDER BY created_at ASC"
                    ).map_err(|e| XhjobError::Store(format!("list_tasks prepare: {}", e)))?;
                    let rows = stmt.query_map([], task_from_row)
                        .map_err(|e| XhjobError::Store(format!("list_tasks query: {}", e)))?;
                    for row in rows {
                        let task = row.map_err(|e| XhjobError::Store(format!("list_tasks row: {}", e)))?;
                        if let Some(tag) = &tag_filter {
                            if !task.tags.iter().any(|t| t == tag) { continue; }
                        }
                        result.push(TaskSummary::from(&task));
                    }
                }
            }
            Ok(result)
        })
    }

    fn requeue_task(&self, id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let state_str: Option<String> = conn.query_row(
                "SELECT state FROM tasks WHERE id = ?1",
                params![id],
                |row| row.get(0),
            ).optional()
                .map_err(|e| XhjobError::Store(format!("requeue_task query: {}", e)))?;
            let state_str = match state_str {
                Some(s) => s,
                None => return Ok(false), // task does not exist
            };
            // Only requeue terminal Cancelled / Failed / Expired tasks.
            let requeueable = matches!(state_str.as_str(), "CANCELLED" | "FAILED" | "EXPIRED");
            if !requeueable {
                return Ok(false);
            }
            let now = crate::store::now_ts() as i64;
            conn.execute(
                "UPDATE tasks SET state = 'PENDING', attempts = 0, last_error = NULL, \
                 started_at = NULL, finished_at = NULL, cancel_requested = 0, \
                 next_fire = ?1 WHERE id = ?2",
                params![now, id],
            ).map_err(|e| XhjobError::Store(format!("requeue_task update: {}", e)))?;
            Ok(true)
        })
    }

    fn reschedule_task(&self, id: &str, new_cron: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        let id = id.to_string();
        let new_cron = new_cron.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            // Load the existing task to check eligibility.
            let row_opt: Option<(Option<String>, String, Option<String>)> = conn.query_row(
                "SELECT cron, state, timezone FROM tasks WHERE id = ?1",
                params![id],
                |row| {
                    let cron: Option<String> = row.get(0)?;
                    let state: String = row.get(1)?;
                    let tz: Option<String> = row.get(2)?;
                    Ok((cron, state, tz))
                },
            ).optional()
                .map_err(|e| XhjobError::Store(format!("reschedule_task query: {}", e)))?;
            let (cron_opt, state_str, tz_opt) = match row_opt {
                Some(r) => r,
                None => return Ok(false), // task not found
            };
            // Only reschedule cron tasks (interval / runAt tasks return false).
            if cron_opt.is_none() {
                return Ok(false);
            }
            // Don't reschedule terminal tasks.
            let is_terminal = matches!(state_str.as_str(),
                "SUCCESS" | "FAILED" | "CANCELLED" | "EXPIRED");
            if is_terminal {
                return Ok(false);
            }
            // Validate the new cron by computing next_fire.
            let now = crate::store::now_ts();
            let new_next = crate::scheduler::cron::next_fire(&new_cron, now, tz_opt.as_deref())
                .map_err(|e| XhjobError::CronParse(format!("invalid cron '{}': {}", new_cron, e)))?;
            conn.execute(
                "UPDATE tasks SET cron = ?1, next_fire = ?2 WHERE id = ?3",
                params![new_cron, new_next, id],
            ).map_err(|e| XhjobError::Store(format!("reschedule_task update: {}", e)))?;
            // state / execution_count / attempts / meta preserved (untouched by UPDATE).
            Ok(true)
        })
    }

    fn reset_running_to_pending(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let now = crate::store::now_ts();
            // Reset Running tasks with acks_late=1 to Pending + next_fire=now.
            // Clear started_at / finished_at so the next execution records
            // fresh timestamps.
            let changed = conn.execute(
                "UPDATE tasks
                 SET state = 'PENDING',
                     next_fire = ?1,
                     started_at = NULL,
                     finished_at = NULL
                 WHERE state = 'RUNNING' AND acks_late = 1",
                params![now],
            ).map_err(|e| XhjobError::Store(format!("reset_running_to_pending update: {}", e)))?;
            Ok(changed as u64)
        })
    }

    // ----- Event log (A17) -----

    fn record_event(&self, task_id: &str, event_type: super::EventType, payload: Option<&str>, ts: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let task_id = task_id.to_string();
        let payload = payload.map(|s| s.to_string());
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute(
                "INSERT INTO events (task_id, event_type, payload, ts) VALUES (?1, ?2, ?3, ?4)",
                params![task_id, event_type.as_str(), payload, ts],
            ).map_err(|e| XhjobError::Store(format!("record_event: {}", e)))?;
            Ok(())
        })
    }

    fn list_events(&self, since_ts: i64, task_id_filter: Option<&str>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskEvent>>> + Send + '_>> {
        let task_id_filter = task_id_filter.map(|s| s.to_string());
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut out = Vec::new();
            match task_id_filter {
                Some(tid) => {
                    let mut stmt = conn.prepare(
                        "SELECT task_id, event_type, payload, ts FROM events
                         WHERE ts >= ?1 AND task_id = ?2 ORDER BY ts ASC"
                    ).map_err(|e| XhjobError::Store(format!("list_events prepare: {}", e)))?;
                    let rows = stmt.query_map(params![since_ts, tid], |row| {
                        let event_type_str: String = row.get(1)?;
                        let event_type = super::EventType::from_str(&event_type_str)
                            .unwrap_or(super::EventType::Started);
                        Ok(TaskEvent {
                            task_id: row.get(0)?,
                            event_type,
                            payload: row.get(2)?,
                            ts: row.get(3)?,
                        })
                    }).map_err(|e| XhjobError::Store(format!("list_events query: {}", e)))?;
                    for r in rows {
                        if let Ok(e) = r { out.push(e); }
                    }
                }
                None => {
                    let mut stmt = conn.prepare(
                        "SELECT task_id, event_type, payload, ts FROM events
                         WHERE ts >= ?1 ORDER BY ts ASC"
                    ).map_err(|e| XhjobError::Store(format!("list_events prepare: {}", e)))?;
                    let rows = stmt.query_map(params![since_ts], |row| {
                        let event_type_str: String = row.get(1)?;
                        let event_type = super::EventType::from_str(&event_type_str)
                            .unwrap_or(super::EventType::Started);
                        Ok(TaskEvent {
                            task_id: row.get(0)?,
                            event_type,
                            payload: row.get(2)?,
                            ts: row.get(3)?,
                        })
                    }).map_err(|e| XhjobError::Store(format!("list_events query: {}", e)))?;
                    for r in rows {
                        if let Ok(e) = r { out.push(e); }
                    }
                }
            }
            Ok(out)
        })
    }

    fn cleanup_expired_events(&self, ttl_secs: u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let now = crate::store::now_ts() as i64;
            let cutoff = now.saturating_sub(ttl_secs as i64);
            let deleted = conn.execute(
                "DELETE FROM events WHERE ts < ?1",
                params![cutoff],
            ).map_err(|e| XhjobError::Store(format!("cleanup_expired_events: {}", e)))?;
            Ok(deleted as u64)
        })
    }

    // ----- Task chain (C15) -----

    fn create_chain(&self, chain_id: &str, tasks: &[serde_json::Value], created_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let chain_id = chain_id.to_string();
        let tasks = tasks.to_vec();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let tasks_str = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "INSERT OR REPLACE INTO chains (chain_id, tasks, current_step, state, created_at, updated_at)
                 VALUES (?1, ?2, 0, 'pending', ?3, ?3)",
                params![chain_id, tasks_str, created_at],
            ).map_err(|e| XhjobError::Store(format!("create_chain: {}", e)))?;
            Ok(())
        })
    }

    fn get_chain(&self, chain_id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<ChainRecord>>> + Send + '_>> {
        let chain_id = chain_id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT chain_id, tasks, current_step, state, created_at, updated_at FROM chains WHERE chain_id = ?1"
            ).map_err(|e| XhjobError::Store(format!("get_chain prepare: {}", e)))?;
            let mut rows = stmt.query_map(params![chain_id], |row| {
                let tasks_str: String = row.get(1)?;
                let tasks: Vec<serde_json::Value> = serde_json::from_str(&tasks_str).unwrap_or_default();
                Ok(ChainRecord {
                    chain_id: row.get(0)?,
                    tasks,
                    current_step: row.get::<_, i64>(2)? as u32,
                    state: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            }).map_err(|e| XhjobError::Store(format!("get_chain query: {}", e)))?;
            if let Some(row) = rows.next() {
                if let Ok(c) = row { return Ok(Some(c)); }
            }
            Ok(None)
        })
    }

    fn update_chain_step(&self, chain_id: &str, current_step: u32, state: &str, updated_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let chain_id = chain_id.to_string();
        let state = state.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute(
                "UPDATE chains SET current_step = ?1, state = ?2, updated_at = ?3 WHERE chain_id = ?4",
                params![current_step as i64, state, updated_at, chain_id],
            ).map_err(|e| XhjobError::Store(format!("update_chain_step: {}", e)))?;
            Ok(())
        })
    }

    fn list_chains_by_state(&self, state: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<ChainRecord>>> + Send + '_>> {
        let state = state.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT chain_id, tasks, current_step, state, created_at, updated_at FROM chains WHERE state = ?1 ORDER BY created_at ASC"
            ).map_err(|e| XhjobError::Store(format!("list_chains_by_state prepare: {}", e)))?;
            let rows = stmt.query_map(params![state], |row| {
                let tasks_str: String = row.get(1)?;
                let tasks: Vec<serde_json::Value> = serde_json::from_str(&tasks_str).unwrap_or_default();
                Ok(ChainRecord {
                    chain_id: row.get(0)?,
                    tasks,
                    current_step: row.get::<_, i64>(2)? as u32,
                    state: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            }).map_err(|e| XhjobError::Store(format!("list_chains_by_state query: {}", e)))?;
            let mut out = Vec::new();
            for r in rows {
                if let Ok(c) = r { out.push(c); }
            }
            Ok(out)
        })
    }

    // ----- Task group (C16) -----

    fn create_group(&self, group_id: &str, tasks: &[serde_json::Value], created_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let group_id = group_id.to_string();
        let tasks = tasks.to_vec();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let tasks_str = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());
            conn.execute(
                "INSERT OR REPLACE INTO groups (group_id, tasks, state, created_at, updated_at)
                 VALUES (?1, ?2, 'pending', ?3, ?3)",
                params![group_id, tasks_str, created_at],
            ).map_err(|e| XhjobError::Store(format!("create_group: {}", e)))?;
            Ok(())
        })
    }

    fn get_group(&self, group_id: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<GroupRecord>>> + Send + '_>> {
        let group_id = group_id.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT group_id, tasks, state, created_at, updated_at FROM groups WHERE group_id = ?1"
            ).map_err(|e| XhjobError::Store(format!("get_group prepare: {}", e)))?;
            let mut rows = stmt.query_map(params![group_id], |row| {
                let tasks_str: String = row.get(1)?;
                let tasks: Vec<serde_json::Value> = serde_json::from_str(&tasks_str).unwrap_or_default();
                Ok(GroupRecord {
                    group_id: row.get(0)?,
                    tasks,
                    state: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            }).map_err(|e| XhjobError::Store(format!("get_group query: {}", e)))?;
            if let Some(row) = rows.next() {
                if let Ok(g) = row { return Ok(Some(g)); }
            }
            Ok(None)
        })
    }

    fn update_group_state(&self, group_id: &str, state: &str, updated_at: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let group_id = group_id.to_string();
        let state = state.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            conn.execute(
                "UPDATE groups SET state = ?1, updated_at = ?2 WHERE group_id = ?3",
                params![state, updated_at, group_id],
            ).map_err(|e| XhjobError::Store(format!("update_group_state: {}", e)))?;
            Ok(())
        })
    }

    fn list_groups_by_state(&self, state: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<GroupRecord>>> + Send + '_>> {
        let state = state.to_string();
        Box::pin(async move {
            let conn = self.conn.lock().await;
            let mut stmt = conn.prepare(
                "SELECT group_id, tasks, state, created_at, updated_at FROM groups WHERE state = ?1 ORDER BY created_at ASC"
            ).map_err(|e| XhjobError::Store(format!("list_groups_by_state prepare: {}", e)))?;
            let rows = stmt.query_map(params![state], |row| {
                let tasks_str: String = row.get(1)?;
                let tasks: Vec<serde_json::Value> = serde_json::from_str(&tasks_str).unwrap_or_default();
                Ok(GroupRecord {
                    group_id: row.get(0)?,
                    tasks,
                    state: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            }).map_err(|e| XhjobError::Store(format!("list_groups_by_state query: {}", e)))?;
            let mut out = Vec::new();
            for r in rows {
                if let Ok(g) = r { out.push(g); }
            }
            Ok(out)
        })
    }
}
