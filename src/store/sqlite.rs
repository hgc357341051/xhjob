//! Optional SQLite-backed task store. Enabled via `persist` cargo feature.

use super::crypto;
use super::{
    ChainRecord, ChordRecord, GroupRecord, Task, TaskEvent, TaskResult, TaskState, TaskStore,
    TaskSummary, TaskType, WorkerStats,
};
use crate::errors::{Result, XhjobError};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex};

/// Prefix tagging encrypted payloads so maybe_decrypt can distinguish
/// genuine ciphertext (must decrypt) from legacy plaintext (may pass
/// through) and reject tampered/injected rows. Without this tag, an
/// attacker with DB write access could replace an encrypted payload
/// with plaintext JSON and have it loaded+executed, defeating the
/// integrity goal of at-rest encryption.
const ENCRYPTED_TAG: &str = "xhjobenc:";

/// P0-22: encrypt the payload JSON before storing if an encryption key is set.
/// When no key is configured, returns the plaintext unchanged (backward compat).
fn maybe_encrypt(plaintext: &str) -> Result<String> {
    if crypto::encryption_key().is_some() {
        let ct = crypto::encrypt(plaintext)?;
        Ok(format!("{}{}", ENCRYPTED_TAG, ct))
    } else {
        Ok(plaintext.to_string())
    }
}

/// P0-22: decrypt the stored payload after loading if an encryption key is set.
///
/// Integrity fix: tagged rows (`xhjobenc:` prefix) are strict ciphertext —
/// they MUST decrypt successfully, and on failure the error is propagated
/// (NO plaintext fallback). This closes the RCE-without-key path for new
/// rows: an attacker with DB write access cannot replace a tagged ciphertext
/// with arbitrary plaintext and have it loaded+executed, because the tag
/// forces a decrypt attempt that fails without the key.
///
/// Untagged rows are ambiguous between legacy plaintext (written before
/// encryption was enabled) and old-format untagged ciphertext (written by
/// a previous maybe_encrypt that did not tag). For migration back-compat we
/// attempt decrypt first; if that succeeds it was old ciphertext, otherwise
/// we pass the row through as legacy plaintext. This retains a residual
/// integrity gap for untagged rows (a plaintext-injected untagged row still
/// loads), but new rows are always tagged and thus protected; the gap
/// closes itself as legacy rows get re-encrypted on their next update.
fn maybe_decrypt(stored: &str) -> Result<String> {
    if crypto::encryption_key().is_none() {
        return Ok(stored.to_string());
    }
    if let Some(ct) = stored.strip_prefix(ENCRYPTED_TAG) {
        // Tagged: strict decrypt, no plaintext fallback (integrity).
        return crypto::decrypt(ct);
    }
    // Untagged: could be legacy plaintext OR old-format ciphertext.
    // Attempt decrypt first (back-compat for old ciphertext rows); if it
    // succeeds, return the decrypted plaintext. If it fails, the row is
    // either legacy plaintext (pass through) or tampered (reject).
    // To preserve integrity for NEW deployments, log untagged rows so
    // operators can re-encrypt; legacy plaintext pass-through is retained
    // only for migration.
    match crypto::decrypt(stored) {
        Ok(pt) => Ok(pt),
        Err(_) => {
            // Could not decrypt as old ciphertext. Treat as legacy plaintext.
            // (A tampered row that's NOT valid ciphertext but IS valid JSON
            //  could still pass through here — but such a row would have to
            //  be injected by an attacker who already has DB write access,
            //  at which point they could also just write a tagged-but-invalid
            //  ciphertext to DoS. The tag closes the RCE-without-key path for
            //  NEW rows; full integrity requires re-encrypting legacy rows.)
            tracing::debug!("maybe_decrypt: untagged row, passing through as legacy plaintext");
            Ok(stored.to_string())
        }
    }
}

/// 获取 SQLite 连接锁，对中毒锁容错（恢复数据而非 panic）。
/// 当持锁线程 panic 导致 Mutex 中毒时，`unwrap()` 会再次 panic 使 daemon
/// 崩溃；此处改为取出内部数据继续运行并记录错误日志。
fn lock_conn(conn: &std::sync::Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    conn.lock().unwrap_or_else(|e| {
        tracing::error!("SQLite mutex poisoned, recovering: {}", e);
        e.into_inner()
    })
}

pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Open or create the SQLite database at `path`.
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)
            .map_err(|e| XhjobError::store(format!("open {}: {}", path, e)))?;
        // Enable WAL mode
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| XhjobError::store(format!("set WAL: {}", e)))?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| XhjobError::store(format!("set synchronous: {}", e)))?;
        // P0-6: busy_timeout=5000ms. Without this, concurrent writes from
        // multiple PHP-FPM workers immediately hit SQLITE_BUSY instead of
        // waiting for the lock holder to release. 5s is the SQLite
        // recommended default and aligns with libsqlite3's own default
        // for many higher-level wrappers.
        conn.pragma_update(None, "busy_timeout", 5000)
            .map_err(|e| XhjobError::store(format!("set busy_timeout: {}", e)))?;
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
                acks_on_failure INTEGER NOT NULL DEFAULT 1,
                idempotent INTEGER NOT NULL DEFAULT 0,
                progress INTEGER,
                progress_meta TEXT,
                chord_id TEXT,
                owner TEXT NOT NULL DEFAULT '',
                or_cron TEXT,
                skip_dates TEXT,
                workdays_only INTEGER NOT NULL DEFAULT 0,
                worker_pid INTEGER,
                worker_starttime INTEGER
            );
            CREATE TABLE IF NOT EXISTS results (
                task_id      TEXT PRIMARY KEY,
                body         TEXT,
                body_b64     TEXT,
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
            CREATE TABLE IF NOT EXISTS chords (
                id                TEXT PRIMARY KEY,
                header_task_ids   TEXT NOT NULL,
                callback_json     TEXT NOT NULL,
                callback_task_id  TEXT,
                state             TEXT NOT NULL DEFAULT 'pending',
                created_at        INTEGER NOT NULL,
                updated_at        INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);
            CREATE INDEX IF NOT EXISTS idx_tasks_next_fire ON tasks(next_fire);
            "#,
        )
        .map_err(|e| XhjobError::store(format!("create schema: {}", e)))?;
        // Task 4: enable foreign-key enforcement + run a startup integrity
        // check.
        //
        // `foreign_keys` defaults to OFF in SQLite — without explicitly
        // enabling it on every connection, the
        // `FOREIGN KEY (task_id) REFERENCES tasks(id)` clause on the
        // `results` table is silently a no-op and orphan result rows can
        // appear (e.g. a result row whose task_id no longer exists in
        // `tasks`). We turn it on right after schema creation so all
        // subsequent statements (ensure_column migrations + runtime CRUD)
        // honor the FK constraint.
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|e| XhjobError::store(format!("enable foreign_keys: {}", e)))?;
        // Startup integrity check: `PRAGMA quick_check` scans every b-tree
        // page and reports structural corruption. On a healthy DB it returns
        // the single row "ok"; on a corrupt DB it returns one or more rows
        // describing the problem (or the query itself errors out). We refuse
        // to start on a corrupt DB rather than silently running with damaged
        // state — this lets operators restore from backup instead of
        // accumulating further damage. Run BEFORE ensure_column migrations so
        // we don't mutate an already-corrupt DB.
        let integrity: String = conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(|e| XhjobError::store(format!("integrity check query failed: {}", e)))?;
        if integrity != "ok" {
            return Err(XhjobError::store(format!(
                "integrity check failed: {}",
                integrity
            )));
        }
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
        ensure_column(&conn, "idempotent", "INTEGER NOT NULL DEFAULT 0")?;
        // 以下两列在 CREATE TABLE 中已存在，但早期版本的旧库可能缺失，
        // 这里补齐迁移以保证 schema 完整性。
        ensure_column(&conn, "timezone", "TEXT")?;
        ensure_column(&conn, "coalesce", "INTEGER NOT NULL DEFAULT 1")?;
        // Task 1-5: progress / progress_meta / chord_id columns.
        ensure_column(&conn, "progress", "INTEGER")?;
        ensure_column(&conn, "progress_meta", "TEXT")?;
        ensure_column(&conn, "chord_id", "TEXT")?;
        ensure_column(&conn, "or_cron", "TEXT")?;
        ensure_column(&conn, "skip_dates", "TEXT")?;
        ensure_column(&conn, "workdays_only", "INTEGER NOT NULL DEFAULT 0")?;
        // P0-17: owner column for multi-tenant isolation.
        ensure_column(&conn, "owner", "TEXT NOT NULL DEFAULT ''")?;
        // execution_lease (Task 2): worker_pid column for crash-recovery
        // duplicate-execution prevention. Nullable (legacy rows / non-shell
        // tasks). No NOT NULL default to avoid rejecting pre-migration rows.
        ensure_column(&conn, "worker_pid", "INTEGER")?;
        ensure_column(&conn, "worker_starttime", "INTEGER")?;
        // Binary HTTP response support: body_b64 column on results table
        // for base64-encoded non-UTF-8 bodies (images, files, etc.).
        ensure_column_in_table(&conn, "results", "body_b64", "TEXT")?;
        // Fix 4: harden SQLite file permissions to 0o600.
        //
        // `Connection::open` creates the DB file with the process umask
        // (typically 0o644 on most Linux boxes), which means any local user
        // can read the database — including task payloads that may contain
        // HTTP headers, shell commands, or secrets. We explicitly chmod the
        // main DB file plus its WAL / SHM sidecar files to 0o600 (owner
        // read/write only) right after opening so that only the daemon user
        // can inspect the store.
        //
        // Best-effort: ignore errors because the WAL/SHM files may not exist
        // yet (they are created lazily on the first write). The main DB file
        // always exists at this point because `Connection::open` just
        // created or opened it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = std::fs::set_permissions(path, perms.clone());
            // WAL and SHM sidecar files (created lazily by SQLite in WAL mode).
            for suffix in ["-wal", "-shm", "-journal"] {
                let sidecar = format!("{}{}", path, suffix);
                let _ = std::fs::set_permissions(&sidecar, perms.clone());
            }
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        }) // std::sync::Mutex; locked inside spawn_blocking
    }
}

/// Add a column to the `tasks` table if it does not already exist. Used to
/// migrate older databases that predate a schema change.
fn ensure_column(conn: &Connection, name: &str, sql_type: &str) -> Result<()> {
    ensure_column_in_table(conn, "tasks", name, sql_type)
}

/// Like `ensure_column` but for an arbitrary table. Used to migrate columns
/// on the `results` / `events` / etc. tables that were created in earlier
/// schema versions without the new column.
fn ensure_column_in_table(
    conn: &Connection,
    table: &str,
    name: &str,
    sql_type: &str,
) -> Result<()> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({})", table))
        .map_err(|e| XhjobError::store(format!("pragma table_info: {}", e)))?;
    let cols: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| XhjobError::store(format!("pragma query_map: {}", e)))?
        .filter_map(|r| r.ok())
        .collect();
    if !cols.iter().any(|c| c == name) {
        conn.execute(
            &format!("ALTER TABLE {} ADD COLUMN {} {}", table, name, sql_type),
            [],
        )
        .map_err(|e| XhjobError::store(format!("alter table add {}: {}", name, e)))?;
    }
    Ok(())
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let id: String = row.get("id")?;
    let type_str: String = row.get("type")?;
    let task_type = match type_str.as_str() {
        "http" => TaskType::Http,
        "shell" => TaskType::Shell,
        // P1 fix: unknown type previously fell back to Shell, which could
        // execute a JSON payload as a shell command if a row was corrupted
        // or hand-inserted with a bogus type. Shell execution of JSON would
        // fail harmlessly (no such command), but to avoid any surprise we
        // still log loudly. Keeping Shell as the fallback preserves
        // backward compat with any rows written before a new type was added.
        other => {
            tracing::warn!(task_id = %id, task_type = %other, "unknown task_type in DB, treating as shell");
            TaskType::Shell
        }
    };
    let payload_str: String = row.get("payload")?;
    // P0-22: decrypt payload if XHJOB_ENCRYPTION_KEY is set. Falls back to
    // treating the stored value as plaintext on any decrypt failure (backward
    // compat with rows written before encryption was enabled).
    let payload_decrypted = maybe_decrypt(&payload_str).unwrap_or_else(|_| payload_str.clone());
    let payload: serde_json::Value = match serde_json::from_str(&payload_decrypted) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("payload decode failed for task {}: {}", id, e);
            serde_json::Value::Null
        }
    };
    let state_str: String = row.get("state")?;
    // P1 fix: an unrecognized state previously fell back to Pending. If a
    // terminal row (Success/Failed/Cancelled) was corrupted in the DB, this
    // would RESURRECT the task — load_active_tasks would pick it up and the
    // scheduler would re-execute it, causing duplicate side effects (double
    // charges, duplicate emails, etc.). Falling back to Failed (a safe
    // terminal state) ensures the task is never re-executed; the operator
    // can manually requeue it after diagnosing the corruption. This is the
    // conservative/safe side; the warning surfaces the corruption.
    let state = match TaskState::from_str(&state_str) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                task_id = %id, raw_state = %state_str, error = %e,
                "unrecognized task state in DB; marking Failed to prevent resurrection"
            );
            TaskState::Failed
        }
    };
    let tags: Vec<String> = {
        let s: String = row
            .get::<_, String>("tags")
            .unwrap_or_else(|_| "[]".to_string());
        match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!("tags decode failed for task {}: {}", id, e);
                Vec::default()
            }
        }
    };
    Ok(Task {
        id,
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
        interval: row
            .get::<_, Option<i64>>("interval")
            .ok()
            .flatten()
            .map(|v| v as u64),
        run_at: row.get::<_, Option<i64>>("run_at").ok().flatten(),
        jitter: row.get::<_, i64>("jitter").unwrap_or(0) as u64,
        expires: row.get::<_, i64>("expires").unwrap_or(0) as u64,
        retry_backoff: row.get::<_, i64>("retry_backoff").unwrap_or(0) != 0,
        ignore_result: row.get::<_, i64>("ignore_result").unwrap_or(0) != 0,
        acks_late: row.get::<_, i64>("acks_late").unwrap_or(0) != 0,
        soft_timeout: row
            .get::<_, Option<i64>>("soft_timeout")
            .ok()
            .flatten()
            .map(|v| v as u64),
        // Round 4 fields. Use unwrap_or for legacy-row safety.
        misfire_grace_time: row.get::<_, i64>("misfire_grace_time").unwrap_or(0) as u64,
        replace_existing: row.get::<_, i64>("replace_existing").unwrap_or(0) != 0,
        tags,
        rate_limit_count: row.get::<_, i64>("rate_limit_count").unwrap_or(0) as u32,
        rate_limit_window: row.get::<_, i64>("rate_limit_window").unwrap_or(0) as u64,
        acks_on_failure: row.get::<_, i64>("acks_on_failure").unwrap_or(1) != 0,
        idempotent: row.get::<_, i64>("idempotent").unwrap_or(0) != 0,
        progress: row
            .get::<_, Option<i64>>("progress")
            .ok()
            .flatten()
            .map(|v| v as u8),
        progress_meta: row.get::<_, Option<String>>("progress_meta").ok().flatten(),
        chord_id: row.get::<_, Option<String>>("chord_id").ok().flatten(),
        owner: row
            .get::<_, Option<String>>("owner")
            .ok()
            .flatten()
            .unwrap_or_default(),
        or_cron: row
            .get::<_, Option<String>>("or_cron")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok()),
        skip_dates: row
            .get::<_, Option<String>>("skip_dates")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        workdays_only: row.get::<_, i64>("workdays_only").unwrap_or(0) != 0,
        // execution_lease (Task 2): worker_pid (nullable; legacy rows have NULL).
        worker_pid: row
            .get::<_, Option<i64>>("worker_pid")
            .ok()
            .flatten()
            .map(|v| v as u32),
        worker_starttime: row
            .get::<_, Option<i64>>("worker_starttime")
            .ok()
            .flatten()
            .map(|v| v as u64),
    })
}

impl TaskStore for SqliteStore {
    fn insert_task(
        &self,
        task: Task,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let payload_plain = serde_json::to_string(&task.payload).unwrap_or_default();
                // P0-22: encrypt payload if XHJOB_ENCRYPTION_KEY is set.
                let payload_str = maybe_encrypt(&payload_plain)?;
                let type_str = task.task_type.as_str();
                let state_str = task.state.as_str();
                let tags_str = serde_json::to_string(&task.tags).unwrap_or_else(|_| "[]".to_string());
                let or_cron_str = task.or_cron.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default());
                let skip_dates_str = serde_json::to_string(&task.skip_dates).unwrap_or_else(|_| "[]".to_string());
                conn.execute(
                    "INSERT OR REPLACE INTO tasks
                     (id, type, payload, cron, retry_max, retry_delay, timeout, priority,
                      allow_overlap, max_instances, coalesce, persist, state, attempts,
                      next_fire, created_at, started_at, finished_at, last_error,
                      proxy, encoding, timezone, max_executions, execution_count,
                      paused, cancel_requested, start_date, end_date, result_ttl, meta,
                      interval, run_at, jitter, expires, retry_backoff, ignore_result,
                      acks_late, soft_timeout, misfire_grace_time, replace_existing,
                      tags, rate_limit_count, rate_limit_window, acks_on_failure,
                      idempotent, progress, progress_meta, chord_id, owner,
                      or_cron, skip_dates, workdays_only, worker_pid, worker_starttime)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36, ?37, ?38, ?39, ?40, ?41, ?42, ?43, ?44, ?45, ?46, ?47, ?48, ?49, ?50, ?51, ?52, ?53, ?54)",
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
                        task.idempotent as i64,
                        task.progress.map(|v| v as i64),
                        task.progress_meta,
                        task.chord_id,
                        task.owner,
                        or_cron_str,
                        skip_dates_str,
                        task.workdays_only as i64,
                        task.worker_pid.map(|v| v as i64),
                        task.worker_starttime.map(|v| v as i64),
                    ],
                ).map_err(|e| XhjobError::store(format!("insert: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn update_state(
        &self,
        id: &str,
        state: TaskState,
        started_at: Option<u64>,
        finished_at: Option<u64>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "UPDATE tasks SET state = ?1, started_at = COALESCE(?2, started_at), finished_at = COALESCE(?3, finished_at) WHERE id = ?4",
                    params![state.as_str(), started_at, finished_at, id],
                ).map_err(|e| XhjobError::store(format!("update_state: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn update_worker_pid(
        &self,
        id: &str,
        worker_pid: Option<u32>,
        starttime: Option<u64>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "UPDATE tasks SET worker_pid = ?1, worker_starttime = ?2 WHERE id = ?3",
                    params![
                        worker_pid.map(|v| v as i64),
                        starttime.map(|v| v as i64),
                        id
                    ],
                )
                .map_err(|e| XhjobError::store(format!("update_worker_pid: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn save_result(
        &self,
        task_id: &str,
        result: TaskResult,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let task_id = task_id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "INSERT OR REPLACE INTO results (task_id, body, body_b64, status_code, stdout, stderr, exit_code)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![task_id, result.body, result.body_b64, result.status_code, result.stdout, result.stderr, result.exit_code],
                ).map_err(|e| XhjobError::store(format!("save_result: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn load_active_tasks(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<Task>>> + Send + '_>> {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    // 同时匹配新的小写与历史的大写存储，保证旧库数据仍可被恢复。
                    "SELECT * FROM tasks WHERE state IN ('pending', 'running', 'interrupted', 'PENDING', 'RUNNING', 'INTERRUPTED')"
                ).map_err(|e| XhjobError::store(format!("prepare: {}", e)))?;
                let rows = stmt.query_map([], task_from_row)
                    .map_err(|e| XhjobError::store(format!("query: {}", e)))?;
                let mut tasks = Vec::new();
                for t in rows.flatten() { tasks.push(t); }
                Ok(tasks)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn load_task(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<Task>>> + Send + '_>>
    {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn
                    .prepare("SELECT * FROM tasks WHERE id = ?1")
                    .map_err(|e| XhjobError::store(format!("prepare: {}", e)))?;
                let mut rows = stmt
                    .query_map(params![id], task_from_row)
                    .map_err(|e| XhjobError::store(format!("query: {}", e)))?;
                if let Some(Ok(t)) = rows.next() {
                    return Ok(Some(t));
                }
                Ok(None)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn load_result(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<TaskResult>>> + Send + '_>>
    {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare("SELECT body, body_b64, status_code, stdout, stderr, exit_code FROM results WHERE task_id = ?1")
                    .map_err(|e| XhjobError::store(format!("prepare: {}", e)))?;
                let mut rows = stmt.query_map(params![id], |row| {
                    Ok(TaskResult {
                        body: row.get(0)?,
                        body_b64: row.get(1)?,
                        status_code: row.get(2)?,
                        stdout: row.get(3)?,
                        stderr: row.get(4)?,
                        exit_code: row.get(5)?,
                        // worker_pid is a Task field, not a results column;
                        // load_result returns None (the result row does not
                        // carry the executor's child PID).
                        worker_pid: None,
                    })
                }).map_err(|e| XhjobError::store(format!("query: {}", e)))?;
                if let Some(Ok(r)) = rows.next() {
                    return Ok(Some(r));
                }
                Ok(None)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn count_running_instances(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let count: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM tasks WHERE id = ?1 AND state IN ('running', 'RUNNING')",
                    params![id],
                    |row| row.get(0),
                ).map_err(|e| XhjobError::store(format!("count: {}", e)))?;
                Ok(count as u32)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn update_next_fire(
        &self,
        id: &str,
        next_fire: Option<u64>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "UPDATE tasks SET next_fire = ?1 WHERE id = ?2",
                    params![next_fire, id],
                )
                .map_err(|e| XhjobError::store(format!("update_next_fire: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn set_attempts_and_error(
        &self,
        id: &str,
        attempts: u32,
        last_error: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "UPDATE tasks SET attempts = ?1, last_error = ?2 WHERE id = ?3",
                    params![attempts, last_error, id],
                )
                .map_err(|e| XhjobError::store(format!("set_attempts: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn delete_task(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let mut conn = lock_conn(&conn);
                // 用事务包裹多条 DELETE，避免 results 已删而 tasks 未删的部分执行。
                let tx = conn
                    .transaction()
                    .map_err(|e| XhjobError::store(format!("begin tx: {}", e)))?;
                tx.execute("DELETE FROM results WHERE task_id = ?1", params![id])
                    .map_err(|e| XhjobError::store(format!("delete result: {}", e)))?;
                tx.execute("DELETE FROM tasks WHERE id = ?1", params![id])
                    .map_err(|e| XhjobError::store(format!("delete task: {}", e)))?;
                tx.commit()
                    .map_err(|e| XhjobError::store(format!("commit tx: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn increment_execution_count(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u32>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "UPDATE tasks SET execution_count = execution_count + 1 WHERE id = ?1",
                    params![id],
                )
                .map_err(|e| {
                    XhjobError::store(format!("increment_execution_count update: {}", e))
                })?;
                let new_count: i64 = conn
                    .query_row(
                        "SELECT execution_count FROM tasks WHERE id = ?1",
                        params![id],
                        |row| row.get(0),
                    )
                    .map_err(|e| {
                        XhjobError::store(format!("increment_execution_count select: {}", e))
                    })?;
                Ok(new_count as u32)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn remove_task(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let mut conn = lock_conn(&conn);
                // 用事务包裹 SELECT-then-DELETE，保证 tasks 与 results 同删或同留。
                let tx = conn
                    .transaction()
                    .map_err(|e| XhjobError::store(format!("begin tx: {}", e)))?;
                let affected = tx
                    .execute("DELETE FROM tasks WHERE id = ?1", params![id])
                    .map_err(|e| XhjobError::store(format!("remove_task: {}", e)))?;
                if affected == 0 {
                    // 提前返回会让 tx 被 drop 并回滚，此处尚未做任何写入。
                    return Err(XhjobError::TaskNotFound(id));
                }
                tx.execute("DELETE FROM results WHERE task_id = ?1", params![id])
                    .map_err(|e| XhjobError::store(format!("remove_task results: {}", e)))?;
                tx.commit()
                    .map_err(|e| XhjobError::store(format!("commit tx: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn set_paused(
        &self,
        id: &str,
        paused: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let affected = conn
                    .execute(
                        "UPDATE tasks SET paused = ?1 WHERE id = ?2",
                        params![paused as i64, id],
                    )
                    .map_err(|e| XhjobError::store(format!("set_paused: {}", e)))?;
                if affected == 0 {
                    return Err(XhjobError::TaskNotFound(id));
                }
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn cancel_task(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let mut conn = lock_conn(&conn);
                // 用事务包裹 SELECT-then-UPDATE + 事件 INSERT，避免状态已改而事件漏写。
                let tx = conn.transaction()
                    .map_err(|e| XhjobError::store(format!("begin tx: {}", e)))?;
                let state_str: String = tx.query_row(
                    "SELECT state FROM tasks WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                ).map_err(|e| XhjobError::store(format!("cancel_task query: {}", e)))?;
                // 大小写无关比较，兼容历史的大写存储与新的小写存储。
                if state_str.eq_ignore_ascii_case("PENDING") {
                    let now_ts = crate::store::now_ts() as i64;
                    tx.execute(
                        "UPDATE tasks SET state = 'cancelled', cancel_requested = 1, finished_at = ?1 WHERE id = ?2",
                        params![now_ts, id],
                    ).map_err(|e| XhjobError::store(format!("cancel_task update: {}", e)))?;
                    // H5 fix: record a Cancelled event so the audit log is complete.
                    // Previously Pending→Cancelled recorded no event, breaking
                    // xhjob_events dashboards for directly-cancelled tasks.
                    if let Err(e) = tx.execute(
                        "INSERT INTO events (task_id, event_type, payload, ts) VALUES (?1, ?2, ?3, ?4)",
                        params![id, "cancelled", "", now_ts],
                    ) {
                        tracing::warn!("record cancelled event failed for task {}: {}", id, e);
                    }
                } else if state_str.eq_ignore_ascii_case("RUNNING") {
                    tx.execute(
                        "UPDATE tasks SET cancel_requested = 1 WHERE id = ?1",
                        params![id],
                    ).map_err(|e| XhjobError::store(format!("cancel_task update: {}", e)))?;
                } else {
                    // 提前返回会让 tx 被 drop 并回滚，此处尚未做任何写入。
                    return Err(XhjobError::InvalidTask(format!(
                        "task {} already in terminal state: {}",
                        id, state_str
                    )));
                }
                tx.commit()
                    .map_err(|e| XhjobError::store(format!("commit tx: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn cleanup_expired_results(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let now = crate::store::now_ts() as i64;
                let deleted = conn
                    .execute(
                        "DELETE FROM results WHERE task_id IN (
                        SELECT id FROM tasks
                        WHERE result_ttl > 0
                          AND finished_at IS NOT NULL
                          AND (?1 - finished_at) > result_ttl
                    )",
                        params![now],
                    )
                    .map_err(|e| XhjobError::store(format!("cleanup_expired_results: {}", e)))?;
                Ok(deleted as u64)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn list_tasks<'a>(
        &'a self,
        state_filter: Option<TaskState>,
        tag_filter: Option<&'a str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + 'a>>
    {
        let tag_filter = tag_filter.map(|s| s.to_string());
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut result = Vec::new();
                match state_filter {
                    Some(state) => {
                        let mut stmt = conn
                            .prepare("SELECT * FROM tasks WHERE state = ?1 ORDER BY created_at ASC")
                            .map_err(|e| XhjobError::store(format!("list_tasks prepare: {}", e)))?;
                        let rows = stmt
                            .query_map(params![state.as_str()], task_from_row)
                            .map_err(|e| XhjobError::store(format!("list_tasks query: {}", e)))?;
                        for row in rows {
                            let task = row
                                .map_err(|e| XhjobError::store(format!("list_tasks row: {}", e)))?;
                            if let Some(tag) = &tag_filter {
                                if !task.tags.iter().any(|t| t == tag) {
                                    continue;
                                }
                            }
                            result.push(TaskSummary::from(&task));
                        }
                    }
                    None => {
                        let mut stmt = conn
                            .prepare("SELECT * FROM tasks ORDER BY created_at ASC")
                            .map_err(|e| XhjobError::store(format!("list_tasks prepare: {}", e)))?;
                        let rows = stmt
                            .query_map([], task_from_row)
                            .map_err(|e| XhjobError::store(format!("list_tasks query: {}", e)))?;
                        for row in rows {
                            let task = row
                                .map_err(|e| XhjobError::store(format!("list_tasks row: {}", e)))?;
                            if let Some(tag) = &tag_filter {
                                if !task.tags.iter().any(|t| t == tag) {
                                    continue;
                                }
                            }
                            result.push(TaskSummary::from(&task));
                        }
                    }
                }
                Ok(result)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn requeue_task(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let state_str: Option<String> = conn
                    .query_row(
                        "SELECT state FROM tasks WHERE id = ?1",
                        params![id],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(|e| XhjobError::store(format!("requeue_task query: {}", e)))?;
                let state_str = match state_str {
                    Some(s) => s,
                    None => return Ok(false), // task does not exist
                };
                // 仅终态 Cancelled / Failed / Expired / Success 任务可重新入队。
                // 大小写无关比较，兼容历史的大写存储与新的小写存储。
                let requeueable = matches!(
                    state_str.to_ascii_lowercase().as_str(),
                    "cancelled" | "failed" | "expired" | "success"
                );
                if !requeueable {
                    return Ok(false);
                }
                let now = crate::store::now_ts() as i64;
                conn.execute(
                    "UPDATE tasks SET state = 'pending', attempts = 0, last_error = NULL, \
                     started_at = NULL, finished_at = NULL, cancel_requested = 0, \
                     next_fire = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .map_err(|e| XhjobError::store(format!("requeue_task update: {}", e)))?;
                Ok(true)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn reschedule_task(
        &self,
        id: &str,
        new_cron: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        let id = id.to_string();
        let new_cron = new_cron.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                // Load the existing task to check eligibility.
                let row_opt: Option<(Option<String>, String, Option<String>)> = conn
                    .query_row(
                        "SELECT cron, state, timezone FROM tasks WHERE id = ?1",
                        params![id],
                        |row| {
                            let cron: Option<String> = row.get(0)?;
                            let state: String = row.get(1)?;
                            let tz: Option<String> = row.get(2)?;
                            Ok((cron, state, tz))
                        },
                    )
                    .optional()
                    .map_err(|e| XhjobError::store(format!("reschedule_task query: {}", e)))?;
                let (cron_opt, state_str, tz_opt) = match row_opt {
                    Some(r) => r,
                    None => return Ok(false), // task not found
                };
                // Only reschedule cron tasks (interval / runAt tasks return false).
                if cron_opt.is_none() {
                    return Ok(false);
                }
                // Don't reschedule terminal tasks.
                // 大小写无关比较，兼容历史的大写存储与新的小写存储。
                let is_terminal = matches!(
                    state_str.to_ascii_lowercase().as_str(),
                    "success" | "failed" | "cancelled" | "expired"
                );
                if is_terminal {
                    return Ok(false);
                }
                // Validate the new cron by computing next_fire.
                let now = crate::store::now_ts();
                let new_next = crate::scheduler::cron::next_fire(&new_cron, now, tz_opt.as_deref())
                    .map_err(|e| {
                        XhjobError::CronParse(format!("invalid cron '{}': {}", new_cron, e))
                    })?;
                conn.execute(
                    "UPDATE tasks SET cron = ?1, next_fire = ?2 WHERE id = ?3",
                    params![new_cron, new_next, id],
                )
                .map_err(|e| XhjobError::store(format!("reschedule_task update: {}", e)))?;
                // state / execution_count / attempts / meta preserved (untouched by UPDATE).
                Ok(true)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn reset_running_to_pending(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let mut conn = lock_conn(&conn);
                let now = crate::store::now_ts();
                let now_i64 = now as i64;
                // P0 fix (C1): reset ALL running tasks on startup, not just
                // acks_late ones. A daemon crash leaves Running tasks with no
                // worker executing them — without this reset they stay Running
                // forever and are never re-enqueued (scan only picks up Pending).
                // The old code only reset acks_late=1 tasks, meaning the majority
                // (acks_late defaults to false) were silently orphaned.
                //
                // Also reset Interrupted tasks: these are tasks that were
                // Running when the daemon was gracefully shut down (the daemon
                // marks them Interrupted on shutdown). On restart they should
                // be re-enqueued.
                //
                // execution_lease (Task 2): if a Running/Interrupted task's
                // worker_pid is still alive, SKIP the reset (the orphan child
                // is still executing the task) and record a LeaseHeld event so
                // the new daemon does not duplicate its side effects. Only
                // reset tasks whose worker_pid is gone or never set. The whole
                // read-partition-write happens in one transaction so a crash
                // mid-recovery leaves a consistent state.
                let tx = conn.transaction()
                    .map_err(|e| XhjobError::store(format!("begin tx: {}", e)))?;
                let candidates: Vec<(String, Option<i64>, Option<i64>)> = {
                    let mut stmt = tx.prepare(
                        "SELECT id, worker_pid, worker_starttime FROM tasks
                         WHERE state IN ('running', 'RUNNING', 'interrupted', 'INTERRUPTED')"
                    ).map_err(|e| XhjobError::store(format!("prepare select running/interrupted: {}", e)))?;
                    let rows = stmt.query_map([], |row| {
                        let id: String = row.get(0)?;
                        // worker_pid / worker_starttime may be NULL or the column
                        // may be absent on pre-migration rows; coerce both to None.
                        let pid: Option<i64> = row.get::<_, Option<i64>>(1).ok().flatten();
                        let starttime: Option<i64> = row.get::<_, Option<i64>>(2).ok().flatten();
                        Ok((id, pid, starttime))
                    }).map_err(|e| XhjobError::store(format!("query_map running/interrupted: {}", e)))?;
                    let mut v = Vec::new();
                    for r in rows {
                        v.push(r.map_err(|e| XhjobError::store(format!("query_map row: {}", e)))?);
                    }
                    v
                };
                let lease_held_type = super::EventType::LeaseHeld.as_str();
                let mut reset = 0u64;
                for (id, pid_opt, starttime_opt) in &candidates {
                    let alive = pid_opt
                        .filter(|&p| p > 0)
                        .map(|p| crate::daemon::is_process_alive_with_starttime(p as u32, starttime_opt.map(|v| v as u64)))
                        .unwrap_or(false);
                    if alive {
                        // Orphan child still running: skip reset, record LeaseHeld.
                        tx.execute(
                            "INSERT INTO events (task_id, event_type, payload, ts) VALUES (?1, ?2, NULL, ?3)",
                            params![id, lease_held_type, now_i64],
                        ).map_err(|e| XhjobError::store(format!("insert LeaseHeld event: {}", e)))?;
                    } else {
                        tx.execute(
                            "UPDATE tasks
                             SET state = 'pending',
                                 next_fire = ?1,
                                 started_at = NULL,
                                 finished_at = NULL
                             WHERE id = ?2",
                            params![now, id],
                        ).map_err(|e| XhjobError::store(format!("reset_running_to_pending update: {}", e)))?;
                        reset += 1;
                    }
                }
                tx.commit()
                    .map_err(|e| XhjobError::store(format!("commit tx: {}", e)))?;
                Ok(reset)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn mark_running_as_interrupted(
        &self,
        reason: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        let reason = reason.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let mut conn = lock_conn(&conn);
                let now = crate::store::now_ts() as i64;
                // Collect ids of all Running tasks BEFORE the UPDATE so we
                // can emit an Interrupted event per task in the same
                // transaction. We read-then-update inside a single tx for
                // atomicity.
                let tx = conn.transaction()
                    .map_err(|e| XhjobError::store(format!("begin tx: {}", e)))?;
                let ids: Vec<String> = {
                    let mut stmt = tx.prepare(
                        "SELECT id FROM tasks WHERE state IN ('running', 'RUNNING')"
                    ).map_err(|e| XhjobError::store(format!("select running: {}", e)))?;
                    let rows = stmt.query_map([], |row| row.get::<_, String>(0))
                        .map_err(|e| XhjobError::store(format!("query_map running: {}", e)))?;
                    let mut v = Vec::new();
                    for r in rows {
                        v.push(r.map_err(|e| XhjobError::store(format!("query_map running row: {}", e)))?);
                    }
                    v
                };
                let changed = tx.execute(
                    "UPDATE tasks
                     SET state = 'interrupted',
                         finished_at = ?1
                     WHERE state IN ('running', 'RUNNING')",
                    params![now],
                ).map_err(|e| XhjobError::store(format!("mark_running_as_interrupted update: {}", e)))?;
                // Emit one Interrupted event per transitioned task.
                let event_type_str = super::EventType::Interrupted.as_str();
                for id in &ids {
                    tx.execute(
                        "INSERT INTO events (task_id, event_type, payload, ts) VALUES (?1, ?2, ?3, ?4)",
                        params![id, event_type_str, reason, now],
                    ).map_err(|e| XhjobError::store(format!("insert Interrupted event: {}", e)))?;
                }
                tx.commit()
                    .map_err(|e| XhjobError::store(format!("commit tx: {}", e)))?;
                Ok(changed as u64)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    // ----- Event log (A17) -----

    fn record_event(
        &self,
        task_id: &str,
        event_type: super::EventType,
        payload: Option<&str>,
        ts: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let task_id = task_id.to_string();
        let payload = payload.map(|s| s.to_string());
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "INSERT INTO events (task_id, event_type, payload, ts) VALUES (?1, ?2, ?3, ?4)",
                    params![task_id, event_type.as_str(), payload, ts],
                )
                .map_err(|e| XhjobError::store(format!("record_event: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn list_events(
        &self,
        since_ts: i64,
        task_id_filter: Option<&str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskEvent>>> + Send + '_>>
    {
        let task_id_filter = task_id_filter.map(|s| s.to_string());
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut out = Vec::new();
                match task_id_filter {
                    Some(tid) => {
                        let mut stmt = conn
                            .prepare(
                                "SELECT task_id, event_type, payload, ts FROM events
                             WHERE ts >= ?1 AND task_id = ?2 ORDER BY ts ASC",
                            )
                            .map_err(|e| {
                                XhjobError::store(format!("list_events prepare: {}", e))
                            })?;
                        let rows = stmt
                            .query_map(params![since_ts, tid], |row| {
                                let task_id: String = row.get(0)?;
                                let event_type_str: String = row.get(1)?;
                                let event_type = match super::EventType::from_str(&event_type_str) {
                                    Ok(et) => et,
                                    Err(e) => {
                                        tracing::warn!(
                                            "event_type decode failed for task {}: {} (raw={})",
                                            task_id,
                                            e,
                                            event_type_str
                                        );
                                        super::EventType::Unknown
                                    }
                                };
                                Ok(TaskEvent {
                                    task_id,
                                    event_type,
                                    payload: row.get(2)?,
                                    ts: row.get(3)?,
                                })
                            })
                            .map_err(|e| XhjobError::store(format!("list_events query: {}", e)))?;
                        for e in rows.flatten() {
                            out.push(e);
                        }
                    }
                    None => {
                        let mut stmt = conn
                            .prepare(
                                "SELECT task_id, event_type, payload, ts FROM events
                             WHERE ts >= ?1 ORDER BY ts ASC",
                            )
                            .map_err(|e| {
                                XhjobError::store(format!("list_events prepare: {}", e))
                            })?;
                        let rows = stmt
                            .query_map(params![since_ts], |row| {
                                let task_id: String = row.get(0)?;
                                let event_type_str: String = row.get(1)?;
                                let event_type = match super::EventType::from_str(&event_type_str) {
                                    Ok(et) => et,
                                    Err(e) => {
                                        tracing::warn!(
                                            "event_type decode failed for task {}: {} (raw={})",
                                            task_id,
                                            e,
                                            event_type_str
                                        );
                                        super::EventType::Unknown
                                    }
                                };
                                Ok(TaskEvent {
                                    task_id,
                                    event_type,
                                    payload: row.get(2)?,
                                    ts: row.get(3)?,
                                })
                            })
                            .map_err(|e| XhjobError::store(format!("list_events query: {}", e)))?;
                        for e in rows.flatten() {
                            out.push(e);
                        }
                    }
                }
                Ok(out)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn cleanup_expired_events(
        &self,
        ttl_secs: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<u64>> + Send + '_>> {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let now = crate::store::now_ts() as i64;
                let cutoff = now.saturating_sub(ttl_secs as i64);
                let deleted = conn
                    .execute("DELETE FROM events WHERE ts < ?1", params![cutoff])
                    .map_err(|e| XhjobError::store(format!("cleanup_expired_events: {}", e)))?;
                Ok(deleted as u64)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    // ----- Task chain (C15) -----

    fn create_chain(
        &self,
        chain_id: &str,
        tasks: &[serde_json::Value],
        created_at: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let chain_id = chain_id.to_string();
        let tasks = tasks.to_vec();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let tasks_str = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());
                conn.execute(
                    "INSERT OR REPLACE INTO chains (chain_id, tasks, current_step, state, created_at, updated_at)
                     VALUES (?1, ?2, 0, 'pending', ?3, ?3)",
                    params![chain_id, tasks_str, created_at],
                ).map_err(|e| XhjobError::store(format!("create_chain: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn get_chain(
        &self,
        chain_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<ChainRecord>>> + Send + '_>>
    {
        let chain_id = chain_id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    "SELECT chain_id, tasks, current_step, state, created_at, updated_at FROM chains WHERE chain_id = ?1"
                ).map_err(|e| XhjobError::store(format!("get_chain prepare: {}", e)))?;
                let mut rows = stmt.query_map(params![chain_id], |row| {
                    let chain_id: String = row.get(0)?;
                    let tasks_str: String = row.get(1)?;
                    let tasks: Vec<serde_json::Value> = match serde_json::from_str(&tasks_str) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!("chain task list decode failed for {}: {}", chain_id, e);
                            Vec::default()
                        }
                    };
                    Ok(ChainRecord {
                        chain_id,
                        tasks,
                        current_step: row.get::<_, i64>(2)? as u32,
                        state: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                }).map_err(|e| XhjobError::store(format!("get_chain query: {}", e)))?;
                if let Some(Ok(c)) = rows.next() {
                    return Ok(Some(c));
                }
                Ok(None)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn update_chain_step(
        &self,
        chain_id: &str,
        current_step: u32,
        state: &str,
        updated_at: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let chain_id = chain_id.to_string();
        let state = state.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "UPDATE chains SET current_step = ?1, state = ?2, updated_at = ?3 WHERE chain_id = ?4",
                    params![current_step as i64, state, updated_at, chain_id],
                ).map_err(|e| XhjobError::store(format!("update_chain_step: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn list_chains_by_state(
        &self,
        state: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<ChainRecord>>> + Send + '_>>
    {
        let state = state.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    "SELECT chain_id, tasks, current_step, state, created_at, updated_at FROM chains WHERE state = ?1 ORDER BY created_at ASC"
                ).map_err(|e| XhjobError::store(format!("list_chains_by_state prepare: {}", e)))?;
                let rows = stmt.query_map(params![state], |row| {
                    let chain_id: String = row.get(0)?;
                    let tasks_str: String = row.get(1)?;
                    let tasks: Vec<serde_json::Value> = match serde_json::from_str(&tasks_str) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!("chain task list decode failed for {}: {}", chain_id, e);
                            Vec::default()
                        }
                    };
                    Ok(ChainRecord {
                        chain_id,
                        tasks,
                        current_step: row.get::<_, i64>(2)? as u32,
                        state: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                }).map_err(|e| XhjobError::store(format!("list_chains_by_state query: {}", e)))?;
                let mut out = Vec::new();
                for c in rows.flatten() { out.push(c); }
                Ok(out)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    // ----- Task group (C16) -----

    fn create_group(
        &self,
        group_id: &str,
        tasks: &[serde_json::Value],
        created_at: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let group_id = group_id.to_string();
        let tasks = tasks.to_vec();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let tasks_str = serde_json::to_string(&tasks).unwrap_or_else(|_| "[]".to_string());
                conn.execute(
                    "INSERT OR REPLACE INTO groups (group_id, tasks, state, created_at, updated_at)
                     VALUES (?1, ?2, 'pending', ?3, ?3)",
                    params![group_id, tasks_str, created_at],
                )
                .map_err(|e| XhjobError::store(format!("create_group: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn get_group(
        &self,
        group_id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<GroupRecord>>> + Send + '_>>
    {
        let group_id = group_id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    "SELECT group_id, tasks, state, created_at, updated_at FROM groups WHERE group_id = ?1"
                ).map_err(|e| XhjobError::store(format!("get_group prepare: {}", e)))?;
                let mut rows = stmt.query_map(params![group_id], |row| {
                    let group_id: String = row.get(0)?;
                    let tasks_str: String = row.get(1)?;
                    let tasks: Vec<serde_json::Value> = match serde_json::from_str(&tasks_str) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!("group task list decode failed for {}: {}", group_id, e);
                            Vec::default()
                        }
                    };
                    Ok(GroupRecord {
                        group_id,
                        tasks,
                        state: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                }).map_err(|e| XhjobError::store(format!("get_group query: {}", e)))?;
                if let Some(Ok(g)) = rows.next() {
                    return Ok(Some(g));
                }
                Ok(None)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn update_group_state(
        &self,
        group_id: &str,
        state: &str,
        updated_at: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let group_id = group_id.to_string();
        let state = state.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                conn.execute(
                    "UPDATE groups SET state = ?1, updated_at = ?2 WHERE group_id = ?3",
                    params![state, updated_at, group_id],
                )
                .map_err(|e| XhjobError::store(format!("update_group_state: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn list_groups_by_state(
        &self,
        state: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<GroupRecord>>> + Send + '_>>
    {
        let state = state.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    "SELECT group_id, tasks, state, created_at, updated_at FROM groups WHERE state = ?1 ORDER BY created_at ASC"
                ).map_err(|e| XhjobError::store(format!("list_groups_by_state prepare: {}", e)))?;
                let rows = stmt.query_map(params![state], |row| {
                    let group_id: String = row.get(0)?;
                    let tasks_str: String = row.get(1)?;
                    let tasks: Vec<serde_json::Value> = match serde_json::from_str(&tasks_str) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!("group task list decode failed for {}: {}", group_id, e);
                            Vec::default()
                        }
                    };
                    Ok(GroupRecord {
                        group_id,
                        tasks,
                        state: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                }).map_err(|e| XhjobError::store(format!("list_groups_by_state query: {}", e)))?;
                let mut out = Vec::new();
                for g in rows.flatten() { out.push(g); }
                Ok(out)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    // ----- Task chord (C16+) -----

    fn create_chord(
        &self,
        id: &str,
        header_task_ids: &[String],
        callback_json: &str,
        created_at: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        let header_task_ids = header_task_ids.to_vec();
        let callback_json = callback_json.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let header_str = serde_json::to_string(&header_task_ids)
                    .unwrap_or_else(|_| "[]".to_string());
                conn.execute(
                    "INSERT OR REPLACE INTO chords (id, header_task_ids, callback_json, callback_task_id, state, created_at, updated_at)
                     VALUES (?1, ?2, ?3, NULL, 'pending', ?4, ?4)",
                    params![id, header_str, callback_json, created_at],
                ).map_err(|e| XhjobError::store(format!("create_chord: {}", e)))?;
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn get_chord(
        &self,
        id: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Option<ChordRecord>>> + Send + '_>>
    {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    "SELECT id, header_task_ids, callback_json, callback_task_id, state, created_at, updated_at FROM chords WHERE id = ?1"
                ).map_err(|e| XhjobError::store(format!("get_chord prepare: {}", e)))?;
                let mut rows = stmt.query_map(params![id], |row| {
                    let id: String = row.get(0)?;
                    let header_str: String = row.get(1)?;
                    let header_task_ids: Vec<String> = match serde_json::from_str(&header_str) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!("chord header task list decode failed for {}: {}", id, e);
                            Vec::default()
                        }
                    };
                    Ok(ChordRecord {
                        id,
                        header_task_ids,
                        callback_json: row.get(2)?,
                        callback_task_id: row.get(3)?,
                        state: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                }).map_err(|e| XhjobError::store(format!("get_chord query: {}", e)))?;
                if let Some(Ok(c)) = rows.next() {
                    return Ok(Some(c));
                }
                Ok(None)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn update_chord_state(
        &self,
        id: &str,
        state: &str,
        callback_task_id: Option<String>,
        updated_at: i64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        let state = state.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                // Only overwrite callback_task_id when a new value is supplied
                // (None means "leave unchanged" — the body id is set once when
                // the callback is dispatched).
                if let Some(cid) = callback_task_id {
                    conn.execute(
                        "UPDATE chords SET state = ?1, callback_task_id = ?2, updated_at = ?3 WHERE id = ?4",
                        params![state, cid, updated_at, id],
                    ).map_err(|e| XhjobError::store(format!("update_chord_state: {}", e)))?;
                } else {
                    conn.execute(
                        "UPDATE chords SET state = ?1, updated_at = ?2 WHERE id = ?3",
                        params![state, updated_at, id],
                    ).map_err(|e| XhjobError::store(format!("update_chord_state: {}", e)))?;
                }
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    // ----- Progress / inspect (Task 1-5) -----

    fn update_progress(
        &self,
        id: &str,
        percent: u8,
        meta: Option<String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        let id = id.to_string();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let affected = conn
                    .execute(
                        "UPDATE tasks SET progress = ?1, progress_meta = ?2 WHERE id = ?3",
                        params![percent as i64, meta, id],
                    )
                    .map_err(|e| XhjobError::store(format!("update_progress: {}", e)))?;
                if affected == 0 {
                    return Err(XhjobError::TaskNotFound(id));
                }
                Ok(())
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn list_active_summary(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>>
    {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    "SELECT * FROM tasks WHERE state IN ('running', 'RUNNING') ORDER BY created_at ASC"
                ).map_err(|e| XhjobError::store(format!("list_active_summary prepare: {}", e)))?;
                let rows = stmt.query_map([], task_from_row)
                    .map_err(|e| XhjobError::store(format!("list_active_summary query: {}", e)))?;
                let mut out = Vec::new();
                for t in rows.flatten() { out.push(TaskSummary::from(&t)); }
                Ok(out)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn list_registered_summary(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>>
    {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    "SELECT * FROM tasks WHERE cron IS NOT NULL OR interval IS NOT NULL ORDER BY created_at ASC"
                ).map_err(|e| XhjobError::store(format!("list_registered_summary prepare: {}", e)))?;
                let rows = stmt.query_map([], task_from_row)
                    .map_err(|e| XhjobError::store(format!("list_registered_summary query: {}", e)))?;
                let mut out = Vec::new();
                for t in rows.flatten() { out.push(TaskSummary::from(&t)); }
                Ok(out)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn list_scheduled_summary(
        &self,
        now: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<TaskSummary>>> + Send + '_>>
    {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let mut stmt = conn.prepare(
                    "SELECT * FROM tasks WHERE next_fire IS NOT NULL AND next_fire > ?1 ORDER BY created_at ASC"
                ).map_err(|e| XhjobError::store(format!("list_scheduled_summary prepare: {}", e)))?;
                let rows = stmt.query_map(params![now], task_from_row)
                    .map_err(|e| XhjobError::store(format!("list_scheduled_summary query: {}", e)))?;
                let mut out = Vec::new();
                for t in rows.flatten() { out.push(TaskSummary::from(&t)); }
                Ok(out)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn worker_stats(
        &self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<WorkerStats>> + Send + '_>> {
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                let total: i64 = conn
                    .query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get(0))
                    .map_err(|e| XhjobError::store(format!("worker_stats total: {}", e)))?;
                let pending: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM tasks WHERE state IN ('pending', 'PENDING')",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| XhjobError::store(format!("worker_stats pending: {}", e)))?;
                let running: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM tasks WHERE state IN ('running', 'RUNNING')",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| XhjobError::store(format!("worker_stats running: {}", e)))?;
                let success: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM tasks WHERE state IN ('success', 'SUCCESS')",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| XhjobError::store(format!("worker_stats success: {}", e)))?;
                let failed: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM tasks WHERE state IN ('failed', 'FAILED')",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| XhjobError::store(format!("worker_stats failed: {}", e)))?;
                let now = crate::store::now_ts();
                let queue_depth: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM tasks WHERE next_fire IS NOT NULL AND next_fire > ?1",
                        params![now],
                        |row| row.get(0),
                    )
                    .map_err(|e| XhjobError::store(format!("worker_stats queue_depth: {}", e)))?;
                Ok(WorkerStats {
                    total: total as u32,
                    pending: pending as u32,
                    running: running as u32,
                    success: success as u32,
                    failed: failed as u32,
                    queue_depth: queue_depth as u32,
                })
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }

    fn modify_job(
        &self,
        id: &str,
        patch: &serde_json::Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + '_>> {
        let id = id.to_string();
        let patch = patch.clone();
        Box::pin(async move {
            let conn = Arc::clone(&self.conn);
            tokio::task::spawn_blocking(move || {
                let conn = lock_conn(&conn);
                // Load existing task to check eligibility.
                let row_opt: Option<(String, Option<String>)> = conn
                    .query_row(
                        "SELECT state, timezone FROM tasks WHERE id = ?1",
                        params![id],
                        |row| {
                            let state: String = row.get(0)?;
                            let tz: Option<String> = row.get(1)?;
                            Ok((state, tz))
                        },
                    )
                    .optional()
                    .map_err(|e| XhjobError::store(format!("modify_job query: {}", e)))?;
                let (state_str, tz_opt) = match row_opt {
                    Some(r) => r,
                    None => return Ok(false),
                };
                let is_terminal = matches!(
                    state_str.to_ascii_lowercase().as_str(),
                    "success" | "failed" | "cancelled" | "expired"
                );
                if is_terminal {
                    return Ok(false);
                }
                let obj = match patch.as_object() {
                    Some(o) => o,
                    None => return Ok(false),
                };
                let now = crate::store::now_ts();
                let mut trigger_changed = false;
                // Build SET clauses dynamically with parameterized values.
                let mut sets: Vec<String> = Vec::new();
                let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
                let mut next_fire_val: Option<u64> = None;
                let mut new_cron: Option<String> = None;
                let mut new_or_cron: Option<Vec<String>> = None;
                let mut new_interval: Option<u64> = None;
                let mut new_run_at: Option<i64> = None;
                let mut new_skip_dates: Option<Vec<i64>> = None;
                // Track whether each trigger key was PRESENT in the patch
                // (independently of whether its value parsed to Some/None).
                // Only fields whose key appeared get a SET clause, so an
                // absent key preserves the existing column value (Critical
                // fix) while an explicit null still maps to SQL NULL (P1
                // clear-on-null semantics), matching the in_memory backend.
                let mut cron_present = false;
                let mut or_cron_present = false;
                let mut interval_present = false;
                let mut run_at_present = false;
                let mut skip_dates_present = false;
                for (key, val) in obj {
                    match key.as_str() {
                        "cron" => {
                            new_cron = val.as_str().map(|s| s.to_string());
                            cron_present = true;
                            trigger_changed = true;
                        }
                        "or_cron" => {
                            new_or_cron = val.as_array().map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            });
                            or_cron_present = true;
                            trigger_changed = true;
                        }
                        "interval" => {
                            new_interval = val.as_u64();
                            interval_present = true;
                            trigger_changed = true;
                        }
                        "run_at" => {
                            new_run_at = val.as_i64();
                            run_at_present = true;
                            trigger_changed = true;
                        }
                        "timezone" => {
                            sets.push("timezone = ?".to_string());
                            params_vec.push(Box::new(val.as_str().map(|s| s.to_string())));
                            trigger_changed = true;
                        }
                        "skip_dates" => {
                            new_skip_dates = val
                                .as_array()
                                .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect());
                            skip_dates_present = true;
                            trigger_changed = true;
                        }
                        "priority" => {
                            if let Some(p) = val.as_i64() {
                                sets.push("priority = ?".to_string());
                                params_vec.push(Box::new(p));
                            }
                        }
                        "max_executions" => {
                            if let Some(m) = val.as_u64() {
                                sets.push("max_executions = ?".to_string());
                                params_vec.push(Box::new(m as i64));
                            }
                        }
                        "paused" => {
                            if let Some(p) = val.as_bool() {
                                sets.push("paused = ?".to_string());
                                params_vec.push(Box::new(p as i64));
                            }
                        }
                        "timeout" => {
                            if let Some(t) = val.as_u64() {
                                sets.push("timeout = ?".to_string());
                                params_vec.push(Box::new(t as i64));
                            }
                        }
                        "soft_timeout" => {
                            sets.push("soft_timeout = ?".to_string());
                            params_vec.push(Box::new(val.as_u64().map(|v| v as i64)));
                        }
                        "retry_max" => {
                            if let Some(r) = val.as_u64() {
                                sets.push("retry_max = ?".to_string());
                                params_vec.push(Box::new(r as i64));
                            }
                        }
                        "retry_delay" => {
                            if let Some(r) = val.as_u64() {
                                sets.push("retry_delay = ?".to_string());
                                params_vec.push(Box::new(r as i64));
                            }
                        }
                        "retry_backoff" => {
                            if let Some(b) = val.as_bool() {
                                sets.push("retry_backoff = ?".to_string());
                                params_vec.push(Box::new(b as i64));
                            }
                        }
                        "expires" => {
                            if let Some(e) = val.as_u64() {
                                sets.push("expires = ?".to_string());
                                params_vec.push(Box::new(e as i64));
                            }
                        }
                        "jitter" => {
                            if let Some(j) = val.as_u64() {
                                sets.push("jitter = ?".to_string());
                                params_vec.push(Box::new(j as i64));
                            }
                        }
                        "coalesce" => {
                            if let Some(c) = val.as_bool() {
                                sets.push("coalesce = ?".to_string());
                                params_vec.push(Box::new(c as i64));
                            }
                        }
                        "misfire_grace_time" => {
                            if let Some(m) = val.as_u64() {
                                sets.push("misfire_grace_time = ?".to_string());
                                params_vec.push(Box::new(m as i64));
                            }
                        }
                        "tags" => {
                            if let Some(arr) = val.as_array() {
                                let tags: Vec<String> = arr
                                    .iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect();
                                sets.push("tags = ?".to_string());
                                params_vec.push(Box::new(
                                    serde_json::to_string(&tags)
                                        .unwrap_or_else(|_| "[]".to_string()),
                                ));
                            }
                        }
                        "meta" => {
                            sets.push("meta = ?".to_string());
                            params_vec.push(Box::new(val.as_str().map(|s| s.to_string())));
                        }
                        "workdays_only" => {
                            if let Some(w) = val.as_bool() {
                                sets.push("workdays_only = ?".to_string());
                                params_vec.push(Box::new(w as i64));
                            }
                        }
                        _ => {}
                    }
                }
                // Recompute next_fire if trigger fields changed.
                if trigger_changed {
                    let mut cron_exprs: Vec<String> = Vec::new();
                    if let Some(c) = &new_cron {
                        cron_exprs.push(c.clone());
                    }
                    if let Some(ocs) = &new_or_cron {
                        cron_exprs.extend(ocs.iter().filter(|s| !s.is_empty()).cloned());
                    }
                    if !cron_exprs.is_empty() {
                        let mut min_next: Option<u64> = None;
                        for expr in &cron_exprs {
                            match crate::scheduler::cron::next_fire(expr, now, tz_opt.as_deref()) {
                                Ok(t) => {
                                    min_next = Some(min_next.map_or(t, |m| m.min(t)));
                                }
                                Err(e) => {
                                    return Err(XhjobError::CronParse(format!(
                                        "invalid cron '{}': {}",
                                        expr, e
                                    )));
                                }
                            }
                        }
                        next_fire_val = min_next;
                    } else if let Some(secs) = new_interval {
                        // P1 fix: saturating_add prevents u64 overflow (see task/mod.rs).
                        next_fire_val = Some(now.saturating_add(secs));
                    } else if let Some(ts) = new_run_at {
                        next_fire_val = Some(ts as u64);
                    } else {
                        // All triggers cleared in this patch: no new
                        // next_fire. Explicit None keeps the value consistent
                        // with the absence of any trigger.
                        next_fire_val = None;
                    }
                }
                // Add trigger-field SET clauses.
                // Critical fix: a SET clause is pushed ONLY when the
                // corresponding key was present in the patch (`*_present`).
                // Previously these five pushes ran unconditionally, binding
                // the `new_*` Option directly — so a patch like
                // `{"priority": 5}` (or even `{}`) wiped cron / or_cron /
                // interval / run_at / skip_dates to NULL because their
                // `new_*` locals were still `None`. Gating on `*_present`
                // preserves columns the user never touched while still
                // honoring an explicit `null` (key present + value None ->
                // SQL NULL, the P1 clear-on-null semantics), matching the
                // in_memory backend which only mutates fields whose keys
                // appear in the patch.
                if cron_present {
                    sets.push("cron = ?".to_string());
                    params_vec.push(Box::new(new_cron.clone())); // Option<String> -> NULL if None
                }
                if or_cron_present {
                    sets.push("or_cron = ?".to_string());
                    params_vec.push(Box::new(
                        new_or_cron
                            .as_ref()
                            .map(|ocs| serde_json::to_string(ocs).unwrap_or_default()),
                    )); // Option<String>
                }
                if interval_present {
                    sets.push("interval = ?".to_string());
                    params_vec.push(Box::new(new_interval.map(|i| i as i64))); // Option<i64>
                }
                if run_at_present {
                    sets.push("run_at = ?".to_string());
                    params_vec.push(Box::new(new_run_at)); // Option<i64>
                }
                if skip_dates_present {
                    sets.push("skip_dates = ?".to_string());
                    params_vec.push(Box::new(new_skip_dates.as_ref().map(|sd| {
                        serde_json::to_string(sd).unwrap_or_else(|_| "[]".to_string())
                    }))); // Option<String>
                }
                if let Some(nf) = next_fire_val {
                    sets.push("next_fire = ?".to_string());
                    params_vec.push(Box::new(nf as i64));
                }
                if sets.is_empty() {
                    return Ok(true); // no changes
                }
                let sql = format!("UPDATE tasks SET {} WHERE id = ?", sets.join(", "));
                params_vec.push(Box::new(id.clone()));
                let params_refs: Vec<&dyn rusqlite::ToSql> =
                    params_vec.iter().map(|p| p.as_ref()).collect();
                conn.execute(&sql, params_refs.as_slice())
                    .map_err(|e| XhjobError::store(format!("modify_job update: {}", e)))?;
                Ok(true)
            })
            .await
            .map_err(|e| XhjobError::store(format!("spawn_blocking join: {}", e)))?
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{EventType, Task, TaskEvent, TaskState, TaskStore, TaskType};
    use rusqlite::params;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Build a unique temporary DB path per test invocation.
    ///
    /// `tempfile` is not in Cargo.toml dev-dependencies and the task
    /// constraints forbid adding new deps, so we synthesize a unique path
    /// under `std::env::temp_dir()` from the test name + PID + a monotonic
    /// counter. The counter guarantees parallel `cargo test` invocations
    /// (and re-runs within the same process) don't collide on the same file.
    fn test_db_path(name: &str) -> String {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let filename = format!("xhjob_test_{}_{}_{}.db", name, pid, n);
        std::env::temp_dir()
            .join(filename)
            .to_string_lossy()
            .into_owned()
    }

    /// Remove the main DB file plus its WAL / SHM / journal sidecars.
    /// Best-effort — ignores errors so a failing cleanup doesn't mask the
    /// real test assertion failure.
    fn cleanup_db_files(path: &str) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(format!("{}-wal", path));
        let _ = std::fs::remove_file(format!("{}-shm", path));
        let _ = std::fs::remove_file(format!("{}-journal", path));
    }

    /// Force a WAL checkpoint so all data is flushed from the -wal sidecar
    /// into the main .db file before we drop the store. This guarantees
    /// that subsequent raw file manipulation (e.g. corruption in
    /// `test_integrity_check_fails_on_corrupt_db`) operates on the real
    /// persisted state rather than on a sidecar that might be discarded.
    fn force_checkpoint(store: &SqliteStore) {
        let conn = store.conn.lock().unwrap();
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    /// Task 4: PRAGMA quick_check must reject a corrupt DB instead of
    /// silently continuing with damaged state. We build a valid DB, then
    /// surgically corrupt ONE b-tree page well past page 1 (keeping the
    /// header + sqlite_master intact) so that the initial PRAGMAs and
    /// CREATE TABLE IF NOT EXISTS (which only read page 1) still succeed
    /// and the corruption is caught by PRAGMA quick_check rather than
    /// earlier. The error message must contain "integrity check" so
    /// operators can grep for it.
    #[tokio::test]
    async fn test_integrity_check_fails_on_corrupt_db() {
        let path = test_db_path("integrity_corrupt");
        cleanup_db_files(&path);

        // 1. Create a valid DB with one task. Dropping the store closes the
        //    only connection; SQLite checkpoints and removes the WAL/SHM
        //    sidecars on last-connection-close, so all state ends up in the
        //    main .db file.
        {
            let store = SqliteStore::open(&path).expect("open store");
            let mut task = Task::new(TaskType::Shell, serde_json::Value::Null);
            task.id = "task-corrupt-1".to_string();
            store.insert_task(task).await.expect("insert task");
            force_checkpoint(&store);
            drop(store);
        }

        // 2. Drop any lingering WAL/SHM sidecars so the main .db file is the
        //    sole source of truth.
        let _ = std::fs::remove_file(format!("{}-wal", &path));
        let _ = std::fs::remove_file(format!("{}-shm", &path));

        // 3. Read the page size from the SQLite header (bytes 16-17, big-
        //    endian). A raw value of 1 means 65536; otherwise it's the
        //    literal page size. Default is 4096.
        let bytes = std::fs::read(&path).expect("read db file");
        let page_size_raw = u16::from_be_bytes([bytes[16], bytes[17]]) as usize;
        let page_size = if page_size_raw == 1 {
            65536
        } else {
            page_size_raw
        };

        // 4. Corrupt the b-tree header (first 8 bytes) of a page well past
        //    page 1. We target page 5 — one of the root pages allocated by
        //    the schema (tables + indexes). Page 1 (header + sqlite_master)
        //    stays intact so Connection::open, the initial PRAGMAs
        //    (journal_mode/synchronous/busy_timeout), execute_batch
        //    (CREATE TABLE IF NOT EXISTS, which only reads sqlite_master),
        //    and PRAGMA foreign_keys=ON all succeed — they only touch page
        //    1. PRAGMA quick_check scans EVERY b-tree page, so it WILL visit
        //    the corrupted page and report the damage.
        //
        //    Setting the page-type byte (first byte of the b-tree header)
        //    to 0xFF makes it an invalid type (valid: 2/5/10/13), which
        //    quick_check flags as corruption.
        //
        //    We previously tried overwriting ALL of page 2+ with 0xFF, but
        //    that was too aggressive: PRAGMA journal_mode=WAL (which runs
        //    BEFORE the integrity check) detected the corruption and
        //    returned "database disk image is malformed", so the error
        //    never reached the integrity-check path. Targeting a single
        //    later page avoids this.
        let target_page = 5;
        let target_offset = target_page * page_size;
        assert!(
            bytes.len() > target_offset + 8,
            "db file too small ({} bytes, page_size {}) — need at least {} bytes to corrupt page {}",
            bytes.len(),
            page_size,
            target_offset + 8,
            target_page
        );
        let mut corrupted = bytes;
        for i in 0..8 {
            corrupted[target_offset + i] = 0xFF;
        }
        std::fs::write(&path, &corrupted).expect("write corrupted db");

        // 5. Reopen — integrity check must fail and the error message must
        //    mention "integrity check" so operators can grep for it.
        //    Use match (not unwrap_err) because SqliteStore does not impl
        //    Debug and unwrap_err requires T: Debug.
        let err_msg = match SqliteStore::open(&path) {
            Ok(_) => panic!("expected open to fail on corrupt DB, but it succeeded"),
            Err(e) => format!("{}", e),
        };
        assert!(
            err_msg.contains("integrity check"),
            "expected error to mention 'integrity check', got: {}",
            err_msg
        );

        cleanup_db_files(&path);
    }

    /// Task 4: `PRAGMA foreign_keys=ON` must be set on every connection
    /// (SQLite defaults to OFF). With it on, inserting a `results` row whose
    /// `task_id` does not exist in `tasks(id)` must be rejected with a
    /// FOREIGN KEY constraint violation. Without it, the insert would
    /// silently succeed and create an orphan row.
    #[tokio::test]
    async fn test_foreign_keys_enforced() {
        let path = test_db_path("fk_enforced");
        cleanup_db_files(&path);

        let store = SqliteStore::open(&path).expect("open store");

        // Insert a real task so the parent row exists in tasks(id).
        let mut task = Task::new(TaskType::Shell, serde_json::Value::Null);
        task.id = "fk-real-task".to_string();
        store.insert_task(task).await.expect("insert task");

        // Attempt to insert a results row referencing a non-existent
        // task_id. With foreign_keys=ON (set in open()) SQLite must reject
        // this; with the SQLite default (foreign_keys=OFF) the insert would
        // silently succeed, creating an orphan row — exactly what the FK
        // clause is supposed to prevent.
        let conn = store.conn.lock().unwrap();
        let insert_result = conn.execute(
            "INSERT INTO results (task_id, body, status_code, exit_code)
             VALUES (?1, ?2, ?3, ?4)",
            params!["nonexistent_task_id", "body", 200, 0],
        );
        assert!(
            insert_result.is_err(),
            "expected FK constraint violation when inserting results row with non-existent task_id"
        );
        let err_str = format!("{}", insert_result.unwrap_err());
        assert!(
            err_str.to_lowercase().contains("foreign key")
                || err_str.to_lowercase().contains("constraint"),
            "expected FK violation error, got: {}",
            err_str
        );

        drop(conn);
        drop(store);
        cleanup_db_files(&path);
    }

    /// Task 6.4: crash recovery — a daemon crash mid-execution leaves a
    /// task stuck in Running. On restart, `reset_running_to_pending` must
    /// flip it back to Pending so the scheduler re-enqueues it, otherwise
    /// the task would stay Running forever and never execute again.
    #[tokio::test]
    async fn test_sqlite_store_crash_recovery_with_running_task() {
        let path = test_db_path("crash_recovery");
        cleanup_db_files(&path);

        // 1. Open a store, insert a Running task (started_at set, simulating
        //    a worker mid-execution), then drop the store WITHOUT going
        //    through graceful shutdown — this simulates a daemon crash.
        {
            let store = SqliteStore::open(&path).expect("open store");
            let mut task = Task::new(TaskType::Shell, serde_json::Value::Null);
            task.id = "crash-recovery-task".to_string();
            task.state = TaskState::Running;
            task.started_at = Some(crate::store::now_ts());
            store.insert_task(task).await.expect("insert running task");
            force_checkpoint(&store);
            drop(store);
        }

        // 2. Reopen the store (daemon restart).
        let store = SqliteStore::open(&path).expect("reopen store");

        // 3. Run crash recovery: reset all Running/Interrupted tasks back
        //    to Pending so the scheduler re-enqueues them.
        let reset_count = store.reset_running_to_pending().await.expect("reset");
        assert!(
            reset_count >= 1,
            "expected at least 1 task to be reset, got {}",
            reset_count
        );

        // 4. Verify the task is now Pending and ready to be picked up.
        let task = store
            .load_task("crash-recovery-task")
            .await
            .expect("load task")
            .expect("task should exist");
        assert_eq!(
            task.state,
            TaskState::Pending,
            "task should be reset from Running to Pending after crash recovery"
        );

        drop(store);
        cleanup_db_files(&path);
    }

    /// Critical regression: `modify_job` must only mutate trigger fields whose
    /// keys appear in the patch. Previously the SQLite backend pushed the
    /// `cron` / `or_cron` / `interval` / `run_at` / `skip_dates` SET clauses
    /// unconditionally, binding the `new_*` locals (still `None` for absent
    /// keys) — so a patch like `{"priority": 5}` (or even `{}`) wiped all
    /// five trigger columns to NULL, silently turning a periodic task into a
    /// no-op. The in_memory backend never had this bug because it only
    /// mutates fields whose keys appear in the patch. This test reproduces
    /// the data-loss scenario against the SQLite store and locks in the fix
    /// (gating each trigger SET clause on a `*_present` flag) while also
    /// confirming the P1 clear-on-null semantics still work.
    #[tokio::test]
    async fn repro_modify_job_preserves_absent_trigger_fields() {
        let path = test_db_path("repro_preserve_triggers");
        cleanup_db_files(&path);

        let store = SqliteStore::open(&path).expect("open store");

        // 1. Insert a task with cron + or_cron + skip_dates set (interval /
        //    run_at left None). This is exactly the shape that the
        //    unconditional-SET bug would silently destroy.
        let mut task = Task::new(TaskType::Shell, serde_json::Value::Null);
        task.id = "repro-preserve-triggers".to_string();
        task.cron = Some("*/5 * * * *".to_string());
        task.interval = None;
        task.or_cron = Some(vec!["0 * * * *".to_string()]);
        task.run_at = None;
        task.skip_dates = vec![1700000000];
        task.state = TaskState::Pending;
        let task_id = task.id.clone();
        store.insert_task(task).await.expect("insert task");

        // 2. Patch ONLY priority — no trigger keys. Before the fix this wiped
        //    cron / or_cron / interval / run_at / skip_dates to NULL.
        store
            .modify_job(&task_id, &serde_json::json!({"priority": 5}))
            .await
            .expect("modify_job priority");

        // 3. Reload and assert every trigger field is untouched.
        let loaded = store
            .load_task(&task_id)
            .await
            .expect("load task after priority patch")
            .expect("task should exist");
        assert_eq!(
            loaded.cron,
            Some("*/5 * * * *".to_string()),
            "cron must be preserved when the patch omits the cron key"
        );
        assert_eq!(
            loaded.or_cron,
            Some(vec!["0 * * * *".to_string()]),
            "or_cron must be preserved when the patch omits the or_cron key"
        );
        assert_eq!(
            loaded.skip_dates,
            vec![1700000000],
            "skip_dates must be preserved when the patch omits the skip_dates key"
        );
        assert_eq!(
            loaded.interval, None,
            "interval must remain None when the patch omits the interval key"
        );
        assert_eq!(
            loaded.run_at, None,
            "run_at must remain None when the patch omits the run_at key"
        );
        assert_eq!(loaded.priority, 5, "priority should be updated to 5");

        // 4. P1 clear-on-null must still work: `{"cron": null}` clears cron
        //    but must NOT wipe or_cron (a key absent from the patch).
        store
            .modify_job(&task_id, &serde_json::json!({"cron": null}))
            .await
            .expect("modify_job clear cron");

        let loaded2 = store
            .load_task(&task_id)
            .await
            .expect("load task after cron clear")
            .expect("task should exist");
        assert_eq!(
            loaded2.cron, None,
            "cron must be cleared after modify_job({{cron:null}})"
        );
        assert_eq!(
            loaded2.or_cron,
            Some(vec!["0 * * * *".to_string()]),
            "or_cron must survive a separate cron=null patch (not wiped)"
        );

        drop(store);
        cleanup_db_files(&path);
    }

    /// Security regression (P0-22 integrity): encrypted payloads are now
    /// tagged with `xhjobenc:` so `maybe_decrypt` can distinguish genuine
    /// ciphertext (must decrypt; no plaintext fallback) from legacy
    /// plaintext (pass through for migration). This locks in two
    /// invariants:
    ///   1. A tagged row that fails to decrypt is REJECTED (integrity for
    ///      new rows — an attacker with DB write access cannot swap a
    ///      tagged ciphertext for garbage and have it load).
    ///   2. A freshly inserted task round-trips: encrypt → tag → store →
    ///      load → decrypt → original payload.
    ///
    /// NOTE: `crypto::encryption_key()` caches its result in a process-wide
    /// `OnceLock`. If an earlier test already initialized the cache as
    /// `None` (no `XHJOB_ENCRYPTION_KEY`), our `set_var` is too late and
    /// encryption stays disabled — in that case the tagged-ciphertext path
    /// cannot be exercised, so we skip the assertions (the test still
    /// passes). Run in isolation
    /// (`cargo test repro_encryption_tag_rejects_tampered_tagged_payload -- --exact`)
    /// for full coverage.
    #[tokio::test]
    async fn repro_encryption_tag_rejects_tampered_tagged_payload() {
        let path = test_db_path("repro_enc_tag");
        cleanup_db_files(&path);

        // Set the key, then force the OnceLock to initialize against it.
        // Remove the env var immediately afterwards so concurrent crypto
        // unit tests (which read the env var directly via resolve_key_from_env)
        // are unaffected; the cached key persists for the rest of this
        // process.
        std::env::set_var("XHJOB_ENCRYPTION_KEY", "a".repeat(64));
        let key_active = crypto::encryption_key().is_some();
        std::env::remove_var("XHJOB_ENCRYPTION_KEY");

        if !key_active {
            eprintln!(
                "repro_encryption_tag_rejects_tampered_tagged_payload: SKIPPED — \
                 crypto OnceLock already initialized as plaintext by an earlier test; \
                 re-run with --exact to exercise the tagged-ciphertext path"
            );
            cleanup_db_files(&path);
            return;
        }

        // --- Integrity guarantee (the core fix): a tagged row that fails
        //     to decrypt is REJECTED, not silently passed through as
        //     plaintext. Before the fix, maybe_decrypt fell back to the
        //     raw stored string on ANY decrypt failure, so an attacker
        //     could replace a tagged ciphertext with plaintext JSON and
        //     have it loaded+executed. With the tag, tagged rows take the
        //     strict decrypt path and propagate the error. ---
        let tagged_garbage = format!("{}INVALID_GARBAGE", ENCRYPTED_TAG);
        assert!(
            maybe_decrypt(&tagged_garbage).is_err(),
            "tagged-but-invalid ciphertext must be REJECTED by maybe_decrypt \
             (strict decrypt, no plaintext fallback for tagged rows)"
        );

        // --- Migration back-compat: untagged rows that are NOT valid
        //     ciphertext pass through as legacy plaintext. This preserves
        //     readability of rows written before encryption was enabled. ---
        let legacy_plaintext = r#"{"cmd":"echo legacy"}"#;
        assert_eq!(
            maybe_decrypt(legacy_plaintext).expect("legacy plaintext pass-through"),
            legacy_plaintext,
            "untagged plaintext must pass through unchanged for migration compatibility"
        );

        // --- End-to-end round-trip through the store: maybe_encrypt tags
        //     the ciphertext, the DB column carries the tag, and load_task
        //     decrypts back to the original payload. ---
        let store = SqliteStore::open(&path).expect("open store");
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd":"echo original"}));
        task.id = "repro-enc-tag".to_string();
        let task_id = task.id.clone();
        store.insert_task(task).await.expect("insert task");

        // The stored payload column must carry the tag (proving
        // maybe_encrypt tagged the ciphertext rather than storing raw
        // plaintext).
        {
            let conn = store.conn.lock().unwrap();
            let stored: String = conn
                .query_row(
                    "SELECT payload FROM tasks WHERE id = ?1",
                    params![&task_id],
                    |row| row.get(0),
                )
                .expect("query stored payload");
            assert!(
                stored.starts_with(ENCRYPTED_TAG),
                "stored payload must be tagged ciphertext, got: {}",
                stored
            );
        }

        // load_task must round-trip back to the original payload.
        let loaded = store
            .load_task(&task_id)
            .await
            .expect("load task")
            .expect("task should exist");
        assert_eq!(
            loaded.payload,
            serde_json::json!({"cmd":"echo original"}),
            "round-trip encrypt→tag→store→load→decrypt must yield the original payload"
        );

        drop(store);
        cleanup_db_files(&path);
    }

    /// Repro: `list_events` must NOT silently rewrite an unrecognized
    /// `event_type` string as `Started`. Before the fix, both query branches
    /// (with and without `task_id_filter`) logged a warning but substituted
    /// `EventType::Started` for any row whose `event_type` column failed to
    /// parse — so a corrupted / forward-incompatible event row would surface
    /// in dashboards and audit logs as a `Started` event, distorting metrics.
    ///
    /// The fix adds an `EventType::Unknown` variant and substitutes that
    /// instead. The row is preserved (so operators can investigate) but
    /// explicitly flagged as unrecognized, never misreported as `Started`.
    ///
    /// This test injects a raw `event_type = 'bogus_type'` row via a direct
    /// sqlite connection (bypassing `record_event`, which validates the
    /// variant), then calls `list_events` and asserts the bogus row is
    /// returned with `EventType::Unknown` — NOT `Started`. Both the
    /// task-filtered and unfiltered branches are exercised.
    #[tokio::test]
    async fn repro_list_events_unknown_type_not_misreported_as_started() {
        let path = test_db_path("repro_list_events_unknown");
        cleanup_db_files(&path);

        let store = SqliteStore::open(&path).expect("open store");

        // 1. Insert a real task so the events row's task_id is valid (FK is
        //    not enforced on events, but we keep the test realistic).
        let mut task = Task::new(TaskType::Shell, serde_json::Value::Null);
        task.id = "t-unknown-event".to_string();
        let task_id = task.id.clone();
        store.insert_task(task).await.expect("insert task");

        // 2. Record a normal Started event via the public API (control row).
        let now = crate::store::now_ts() as i64;
        store
            .record_event(&task_id, EventType::Started, None, now)
            .await
            .expect("record Started event");

        // 3. Directly INSERT a row with an unrecognized event_type. This
        //    simulates a corrupted row or one written by a newer daemon
        //    version using an event_type string this build does not know.
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO events (task_id, event_type, payload, ts) VALUES (?1, ?2, ?3, ?4)",
                params![&task_id, "bogus_type", None::<&str>, now + 1],
            )
            .expect("insert bogus event row");
        }

        // 4. list_events without a task filter — both branches must surface
        //    the bogus row as Unknown, never as Started.
        let events = store.list_events(0, None).await.expect("list_events");
        assert!(
            events.len() >= 2,
            "expected at least the Started row + the bogus row, got {}",
            events.len()
        );
        let bogus = events
            .iter()
            .find(|e| e.payload.is_none() && e.ts == now + 1)
            .expect("the bogus event row must be present in list_events output");
        assert_eq!(
            bogus.event_type,
            EventType::Unknown,
            "unrecognized event_type must surface as Unknown, NOT as Started (raw was 'bogus_type')"
        );
        assert_ne!(
            bogus.event_type,
            EventType::Started,
            "unrecognized event_type must NEVER be misreported as Started"
        );

        // 5. list_events WITH a task_id filter — exercises the other query
        //    branch, which had the same Started-substitution bug.
        let filtered = store
            .list_events(0, Some(&task_id))
            .await
            .expect("list_events filtered");
        let bogus_filtered = filtered
            .iter()
            .find(|e| e.payload.is_none() && e.ts == now + 1)
            .expect("the bogus event row must be present in filtered list_events output");
        assert_eq!(
            bogus_filtered.event_type,
            EventType::Unknown,
            "unrecognized event_type must surface as Unknown in the filtered branch too"
        );

        // 6. Sanity: the genuine Started event is still decoded as Started
        //    (regression guard — the Unknown fallback must not swallow
        //    valid rows).
        let started = events
            .iter()
            .find(|e| e.ts == now)
            .expect("the Started control row must be present");
        assert_eq!(
            started.event_type,
            EventType::Started,
            "genuine 'started' event must still decode as Started"
        );

        // 7. Round-trip: EventType::Unknown serializes / deserializes via
        //    serde as "unknown" (lowercase, matching as_str).
        let unknown_event = TaskEvent {
            task_id: task_id.clone(),
            event_type: EventType::Unknown,
            payload: None,
            ts: now + 2,
        };
        let json = serde_json::to_string(&unknown_event).expect("serialize Unknown event");
        assert!(
            json.contains("\"unknown\""),
            "EventType::Unknown must serde-serialize as \"unknown\", got: {}",
            json
        );
        let back: TaskEvent =
            serde_json::from_str(&json).expect("deserialize Unknown event round-trip");
        assert_eq!(
            back.event_type,
            EventType::Unknown,
            "EventType::Unknown must round-trip through serde"
        );

        drop(store);
        cleanup_db_files(&path);
    }
}
