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
        // Migrate legacy databases that predate the proxy/encoding/timezone
        // columns. CREATE TABLE IF NOT EXISTS is a no-op for existing DBs,
        // so we must ALTER TABLE explicitly to add the missing columns.
        migrate_schema(&conn)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }
}

/// Add any missing `proxy` / `encoding` / `timezone` columns to the `tasks`
/// table. SQLite's `ALTER TABLE ADD COLUMN` does not support `IF NOT EXISTS`,
/// so we introspect via `PRAGMA table_info` and only add what's missing.
/// Idempotent: a fully-migrated schema is a no-op.
fn migrate_schema(conn: &rusqlite::Connection) -> Result<()> {
    let cols: std::collections::HashSet<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(tasks)")
            .map_err(|e| XhjobError::Store(format!("pragma table_info: {}", e)))?;
        let rows = stmt
            .query_map([], |row| Ok(row.get::<_, String>(1)?))
            .map_err(|e| XhjobError::Store(format!("pragma query_map: {}", e)))?;
        let mut set = std::collections::HashSet::new();
        for r in rows {
            set.insert(r.map_err(|e| XhjobError::Store(format!("pragma row: {}", e)))?);
        }
        set
    };
    for col in &["proxy", "encoding", "timezone"] {
        if !cols.contains(*col) {
            conn.execute(&format!("ALTER TABLE tasks ADD COLUMN {} TEXT", col), [])
                .map_err(|e| XhjobError::Store(format!("alter table {}: {}", col, e)))?;
            tracing::info!("schema migrated: added column {}", col);
        }
    }

    // Backfill NULL values in NOT-NULL-with-default columns. Legacy databases
    // (created before the NOT NULL constraints were enforced, or via manual
    // INSERTs that omitted columns) may contain NULLs that would break
    // `task_from_row` when read back as non-Option integer types. Each UPDATE
    // is a no-op on rows that already have a value, so this is safe to run
    // on every open.
    let backfills: &[(&str, &str)] = &[
        ("retry_max", "0"),
        ("retry_delay", "1"),
        ("timeout", "30"),
        ("priority", "0"),
        ("allow_overlap", "0"),
        ("max_instances", "1"),
        ("coalesce", "1"),
        ("persist", "0"),
        ("attempts", "0"),
    ];
    for (col, default_val) in backfills {
        // Skip columns that don't exist (e.g. a legacy schema missing a
        // column entirely — they would have been added above with NULL
        // default, but defensive check anyway).
        if !cols.contains(*col) {
            continue;
        }
        let sql = format!("UPDATE tasks SET {} = {} WHERE {} IS NULL", col, default_val, col);
        let affected = conn.execute(&sql, [])
            .map_err(|e| XhjobError::Store(format!("backfill {}: {}", col, e)))?;
        if affected > 0 {
            tracing::info!(col, affected, "backfilled NULL values to default");
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the legacy `tasks` schema (pre-Task-25, no proxy/encoding/timezone
    /// columns) on the given connection.
    fn create_legacy_schema(conn: &rusqlite::Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE tasks (
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
                last_error    TEXT
            );
            "#,
        )
        .expect("create legacy schema");
    }

    fn column_names(conn: &rusqlite::Connection) -> std::collections::HashSet<String> {
        let mut stmt = conn.prepare("PRAGMA table_info(tasks)").unwrap();
        let rows = stmt.query_map([], |row| Ok(row.get::<_, String>(1)?)).unwrap();
        let mut set = std::collections::HashSet::new();
        for r in rows {
            set.insert(r.unwrap());
        }
        set
    }

    #[test]
    fn migrate_schema_adds_missing_columns() {
        // Use an in-memory SQLite so the test is hermetic and fast.
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory");
        // 1. Start from a legacy schema missing proxy/encoding/timezone.
        create_legacy_schema(&conn);
        let before = column_names(&conn);
        assert!(!before.contains("proxy"), "legacy schema must not have proxy");
        assert!(!before.contains("encoding"), "legacy schema must not have encoding");
        assert!(!before.contains("timezone"), "legacy schema must not have timezone");

        // 2. Run the migration.
        migrate_schema(&conn).expect("migrate_schema should succeed on legacy schema");

        // 3. Verify all three columns now exist.
        let after = column_names(&conn);
        assert!(after.contains("proxy"), "proxy column should be added");
        assert!(after.contains("encoding"), "encoding column should be added");
        assert!(after.contains("timezone"), "timezone column should be added");

        // 4. Sanity-check that the original columns are still present.
        for legacy in [
            "id", "type", "payload", "cron", "retry_max", "retry_delay", "timeout",
            "priority", "allow_overlap", "max_instances", "coalesce", "persist",
            "state", "attempts", "next_fire", "created_at", "started_at",
            "finished_at", "last_error",
        ] {
            assert!(after.contains(legacy), "legacy column {} should remain", legacy);
        }
    }

    #[test]
    fn migrate_schema_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().expect("open in-memory");
        // Start from a fully-migrated schema (all columns present).
        conn.execute_batch(
            r#"
            CREATE TABLE tasks (
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
            "#,
        )
        .expect("create full schema");

        // Running migrate_schema on an already-migrated DB must be a no-op.
        migrate_schema(&conn).expect("migrate_schema should be idempotent");
        migrate_schema(&conn).expect("migrate_schema should be idempotent on second call");

        let cols = column_names(&conn);
        assert!(cols.contains("proxy"));
        assert!(cols.contains("encoding"));
        assert!(cols.contains("timezone"));
    }
}
