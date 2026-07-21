//! Task model + Builder pattern chainable API.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::errors::{Result, XhjobError};
use crate::store::{Task, TaskType, HttpPayload, ShellPayload};
use crate::ipc::request as ipc_request;
use crate::store::now_ts;

/// Builder for constructing tasks with a fluent chainable API.
///
/// Example (Rust):
/// ```ignore
/// let id = TaskBuilder::new()
///     .via_http("POST", "https://api.example.com")
///     .with_retry(3, 1)
///     .cron("*/5 * * * *")
///     .allow_overlap(false)
///     .persist(true)
///     .dispatch()
///     .await?;
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskBuilder {
    pub task_type: Option<TaskType>,
    pub payload: serde_json::Value,
    pub cron: Option<String>,
    pub retry_max: u32,
    pub retry_delay: u64,
    pub timeout: u64,
    pub priority: i32,
    pub allow_overlap: bool,
    pub max_instances: u32,
    pub coalesce: bool,
    pub persist: bool,
    /// Service name this builder dispatches to. Defaults to "default".
    #[serde(default = "default_service_name")]
    pub service_name: String,
    /// Optional data directory where the daemon's PID/sock/db/log files live.
    /// When set, the dispatch path resolves the IPC socket under this directory.
    /// Used for backup / migration / restore scenarios where the user has
    /// relocated all service files to a custom directory.
    #[serde(default)]
    pub data_dir: Option<String>,
    /// Optional proxy URL (e.g. `http://host:port`, `socks5://user:pass@host:port`).
    #[serde(default)]
    pub proxy: Option<String>,
    /// Optional output encoding for Shell tasks (e.g. `GBK`, `Big5`, `auto`).
    /// When set, stdout/stderr bytes are decoded from this encoding to UTF-8.
    #[serde(default)]
    pub encoding: Option<String>,
    /// Optional IANA timezone (e.g. `Asia/Shanghai`, `America/New_York`) used
    /// when evaluating the cron expression. When set, `next_fire` is computed
    /// in this timezone instead of the system local timezone. None = Local.
    #[serde(default)]
    pub timezone: Option<String>,
    /// Maximum executions for cron tasks (0 = unlimited, default).
    #[serde(default)]
    pub max_executions: u32,
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
    /// `interval` seconds. Reference: APScheduler IntervalTrigger.
    #[serde(default)]
    pub interval: Option<u64>,
    /// DateTrigger absolute Unix timestamp (A8). When set, the task fires once
    /// at the given timestamp. Reference: APScheduler DateTrigger.
    #[serde(default)]
    pub run_at: Option<i64>,
    /// Jitter (A9): random offset in seconds added to next_fire for cron /
    /// interval tasks. Default 0 = no jitter. Ignored for runAt tasks.
    /// Reference: APScheduler jitter.
    #[serde(default)]
    pub jitter: u64,
    /// Task-level expires (C6): if a task remains Pending for longer than
    /// `expires` seconds (measured from `created_at`), it transitions to
    /// `Expired` terminal state. Default 0 = no expiry.
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
    /// task — fire-and-forget semantics. Default false.
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
    /// None (default) = no soft timeout. HTTP tasks ignore this field.
    /// Reference: Celery soft_time_limit.
    #[serde(default)]
    pub soft_timeout: Option<u64>,
}

fn default_service_name() -> String {
    "default".to_string()
}

/// Compute a random jitter offset in `[0, secs]` using `rand::thread_rng()`.
/// Used to spread out cron / interval task triggers and avoid thundering-herd
/// effects when many tasks share the same fire time. Returns 0 when `secs == 0`.
/// Reference: APScheduler jitter.
fn rand_jitter(secs: u64) -> u64 {
    if secs == 0 {
        return 0;
    }
    use rand::Rng;
    rand::thread_rng().gen_range(0..secs)
}

impl Default for TaskBuilder {
    fn default() -> Self {
        Self {
            task_type: None,
            payload: serde_json::Value::Null,
            cron: None,
            retry_max: 0,
            retry_delay: 1,
            timeout: 30,
            priority: 0,
            allow_overlap: false,
            max_instances: 1,
            coalesce: true,
            persist: false,
            service_name: default_service_name(),
            data_dir: None,
            proxy: None,
            encoding: None,
            timezone: None,
            max_executions: 0,
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
        }
    }
}

impl TaskBuilder {
    pub fn new() -> Self { Self::default() }

    /// Bind this builder to the named service. The dispatch path will resolve
    /// to that service's IPC socket.
    pub fn service(mut self, name: impl Into<String>) -> Self {
        self.service_name = name.into();
        self
    }

    /// Set the data directory where the daemon's PID/sock/db/log files live.
    /// When set, the dispatch path resolves the IPC socket under this directory.
    /// Used for backup / migration / restore scenarios.
    pub fn data_dir(mut self, dir: impl Into<String>) -> Self {
        let dir = dir.into();
        self.data_dir = if dir.is_empty() { None } else { Some(dir) };
        self
    }

    /// Set an HTTP/SOCKS5 proxy URL for HTTP tasks. Accepted schemes:
    /// `http://`, `https://`, `socks5://`, `socks5h://`. The URL may include
    /// `user:pass@` credentials.
    pub fn proxy(mut self, proxy: impl Into<String>) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    /// Set the output encoding for Shell tasks. The string is case-insensitive
    /// and accepts any label supported by `encoding_rs` (e.g. `GBK`, `Big5`,
    /// `windows-1252`) plus the special value `auto` which auto-detects the
    /// OEM code page on Windows (no-op on Unix).
    pub fn encoding(mut self, from: impl Into<String>) -> Self {
        self.encoding = Some(from.into());
        self
    }

    /// Set the IANA timezone (e.g. `Asia/Shanghai`, `America/New_York`) used
    /// when evaluating the cron expression. When set, `next_fire` is computed
    /// in this timezone instead of the system local timezone.
    pub fn timezone(mut self, tz: impl Into<String>) -> Self {
        self.timezone = Some(tz.into());
        self
    }

    /// Set task type to HTTP with method + url.
    pub fn via_http(mut self, method: impl Into<String>, url: impl Into<String>) -> Self {
        let payload = HttpPayload {
            method: method.into(),
            url: url.into(),
            headers: HashMap::new(),
            body: None,
        };
        self.payload = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
        self.task_type = Some(TaskType::Http);
        self
    }

    /// Set HTTP headers (overrides previous).
    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        if self.task_type == Some(TaskType::Http) {
            if let Ok(mut p) = serde_json::from_value::<HttpPayload>(self.payload.clone()) {
                p.headers = headers;
                self.payload = serde_json::to_value(&p).unwrap_or(serde_json::Value::Null);
            }
        }
        self
    }

    /// Set HTTP body.
    pub fn body(mut self, body: impl Into<String>) -> Self {
        if self.task_type == Some(TaskType::Http) {
            if let Ok(mut p) = serde_json::from_value::<HttpPayload>(self.payload.clone()) {
                p.body = Some(body.into());
                self.payload = serde_json::to_value(&p).unwrap_or(serde_json::Value::Null);
            }
        }
        self
    }

    /// Set task type to Shell with command.
    pub fn via_shell(mut self, cmd: impl Into<String>) -> Self {
        let payload = ShellPayload { cmd: cmd.into() };
        self.payload = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
        self.task_type = Some(TaskType::Shell);
        self
    }

    /// Set retry policy: max attempts + base delay (seconds).
    pub fn with_retry(mut self, max: u32, delay: u64) -> Self {
        self.retry_max = max;
        self.retry_delay = delay;
        self
    }

    /// Set cron expression (5-segment standard, or 6-segment with seconds).
    pub fn cron(mut self, expr: impl Into<String>) -> Self {
        self.cron = Some(expr.into());
        self
    }

    /// Set execution timeout in seconds.
    pub fn timeout(mut self, secs: u64) -> Self {
        self.timeout = secs;
        self
    }

    /// Set task priority (higher = first).
    pub fn priority(mut self, p: i32) -> Self {
        self.priority = p;
        self
    }

    /// Set whether to allow overlapping executions of the same task.
    pub fn allow_overlap(mut self, allow: bool) -> Self {
        self.allow_overlap = allow;
        self
    }

    /// Set max concurrent instances. Independent of `allow_overlap` (A10):
    /// when N>1 is set, it takes priority over `allow_overlap` and enforces a
    /// hard concurrency cap of N. When left at the default (1), the concurrency
    /// behavior is governed solely by `allow_overlap` (1 instance if false,
    /// unlimited if true) for backward compatibility.
    /// Exposed as `maxInstances(int $n)` in PHP (snake→camel auto-conversion).
    /// Reference: APScheduler max_instances.
    pub fn max_instances(mut self, n: u32) -> Self {
        self.max_instances = n.max(1);
        self
    }

    /// Set whether to coalesce missed triggers into one (default true).
    pub fn coalesce(mut self, c: bool) -> Self {
        self.coalesce = c;
        self
    }

    /// Enable/disable SQLite persistence for this task.
    pub fn persist(mut self, p: bool) -> Self {
        self.persist = p;
        self
    }

    /// Set the maximum number of executions for a cron task (0 = unlimited).
    /// After reaching the limit, the task state becomes Success and is no longer triggered.
    pub fn max_executions(mut self, n: u32) -> Self {
        self.max_executions = n;
        self
    }

    /// Set the start date (Unix timestamp). Cron triggers before this time are skipped.
    /// Exposed as `startAt()` in PHP (snake→camel auto-conversion).
    pub fn start_at(mut self, ts: i64) -> Self {
        self.start_date = Some(ts);
        self
    }

    /// Set the end date (Unix timestamp). After this time, task state becomes Success terminal.
    /// Exposed as `endAt()` in PHP (snake→camel auto-conversion).
    pub fn end_at(mut self, ts: i64) -> Self {
        self.end_date = Some(ts);
        self
    }

    /// Set result time-to-live in seconds (0 = keep forever).
    /// After task reaches terminal state, result is auto-cleaned after this duration.
    /// Exposed as `resultTtl()` in PHP (snake→camel auto-conversion).
    pub fn result_ttl(mut self, secs: u64) -> Self {
        self.result_ttl = secs;
        self
    }

    /// Attach user metadata (any JSON string) to the task.
    /// Exposed as `meta()` in PHP.
    pub fn meta(mut self, s: impl Into<String>) -> Self {
        self.meta = Some(s.into());
        self
    }

    /// Set the IntervalTrigger period in seconds (A7). The task fires every
    /// `secs` seconds. Mutually exclusive with `cron` and `run_at`; if both
    /// are set, `cron` / `run_at` take priority and `interval` is ignored
    /// (with a runtime warning).
    /// Exposed as `every(int $secs)` in PHP.
    /// Reference: APScheduler IntervalTrigger.
    pub fn every(mut self, secs: u64) -> Self {
        self.interval = Some(secs);
        self
    }

    /// Set the DateTrigger absolute Unix timestamp (A8). The task fires once
    /// at the given timestamp, then immediately transitions to Success
    /// terminal state. Highest scheduling priority (overrides cron + interval).
    /// Exposed as `runAt(int $ts)` in PHP (snake→camel auto-conversion).
    /// Reference: APScheduler DateTrigger.
    pub fn run_at(mut self, ts: i64) -> Self {
        self.run_at = Some(ts);
        self
    }

    /// Set jitter (A9): random offset in seconds added to next_fire for cron
    /// / interval tasks to avoid thundering-herd effects. Default 0 = no
    /// jitter. Ignored for runAt tasks (precise one-shot timestamp).
    /// Exposed as `jitter(int $secs)` in PHP.
    /// Reference: APScheduler jitter.
    pub fn jitter(mut self, secs: u64) -> Self {
        self.jitter = secs;
        self
    }

    /// Set task-level expires (C6): if a task remains Pending for longer than
    /// `secs` seconds (measured from `created_at`), it transitions to
    /// `Expired` terminal state. Default 0 = no expiry. Only affects Pending
    /// tasks; Running tasks are not interrupted.
    /// Exposed as `expires(int $secs)` in PHP.
    /// Reference: APScheduler expires.
    pub fn expires(mut self, secs: u64) -> Self {
        self.expires = secs;
        self
    }

    /// Enable/disable retry exponential backoff (C8). When enabled, retry
    /// delays grow exponentially as
    /// `min(retry_delay * 2^(attempts-1), retry_delay * 60)`. When disabled
    /// (default), retry delays are fixed at `retry_delay` seconds.
    /// Exposed as `retryBackoff(bool $on)` in PHP (snake→camel auto-conversion).
    /// Reference: Celery retry_backoff.
    pub fn retry_backoff(mut self, on: bool) -> Self {
        self.retry_backoff = on;
        self
    }

    /// Enable fire-and-forget mode (C9): when true, the daemon skips
    /// `save_result` for this task so `xhjob_result()` will return null. The
    /// task state machine still runs (Pending → Running → Success/Failed).
    /// Useful for high-throughput tasks whose result is not needed by the
    /// caller. If both `ignore_result=true` and `result_ttl>0` are set, a
    /// warning is logged at dispatch time and `ignore_result` takes priority.
    /// Exposed as `ignoreResult(bool $on)` in PHP (snake→camel auto-conversion).
    /// Reference: Celery ignore_result.
    pub fn ignore_result(mut self, on: bool) -> Self {
        self.ignore_result = on;
        self
    }

    /// Enable late acknowledgment (C10): when true, the task is "acked late"
    /// — on daemon restart, Running tasks with `acks_late=true` are
    /// automatically reset to Pending so they will be re-triggered (crash
    /// recovery semantics). When false (default), Running tasks on daemon
    /// restart are left in the Running state (or, in persist mode, are
    /// unconditionally reset to Pending — `acks_late=true` is reserved for
    /// the future "task is idempotent and safe to re-run" opt-in flag).
    /// Exposed as `acksLate(bool $on)` in PHP (snake→camel auto-conversion).
    /// Reference: Celery acks_late.
    pub fn acks_late(mut self, on: bool) -> Self {
        self.acks_late = on;
        self
    }

    /// Set soft timeout (C11): graceful exit timeout in seconds. When set
    /// and less than `timeout`, the shell executor sends SIGTERM at
    /// `soft_timeout` seconds; if the child does not exit within
    /// (timeout - soft_timeout) seconds after SIGTERM, SIGKILL is sent.
    /// A value of 0 is treated as None (no soft timeout). HTTP tasks
    /// ignore this field (HTTP clients cannot be gracefully interrupted)
    /// — a warning is logged at build time and `soft_timeout` is reset
    /// to None.
    /// Exposed as `softTimeout(int $secs)` in PHP (snake→camel auto-conversion).
    /// Reference: Celery soft_time_limit.
    pub fn soft_timeout(mut self, secs: u64) -> Self {
        // 0 means "not set" — store as None so the executor's `is_some()`
        // check correctly skips the SIGTERM path.
        self.soft_timeout = if secs == 0 { None } else { Some(secs) };
        self
    }

    /// Build the final Task struct (without dispatching).
    pub fn build(self) -> Result<Task> {
        let task_type = self.task_type.ok_or_else(|| XhjobError::InvalidTask(
            "task type not set; call via_http() or via_shell() first".to_string()
        ))?;
        let mut task = Task::new(task_type, self.payload);
        task.cron = self.cron;
        task.retry_max = self.retry_max;
        task.retry_delay = self.retry_delay;
        task.timeout = self.timeout;
        task.priority = self.priority;
        task.allow_overlap = self.allow_overlap;
        task.max_instances = self.max_instances;
        task.coalesce = self.coalesce;
        task.persist = self.persist;
        task.proxy = self.proxy;
        task.encoding = self.encoding;
        task.timezone = self.timezone;
        task.max_executions = self.max_executions;
        task.start_date = self.start_date;
        task.end_date = self.end_date;
        task.result_ttl = self.result_ttl;
        task.meta = self.meta;
        task.interval = self.interval;
        task.run_at = self.run_at;
        task.jitter = self.jitter;
        task.expires = self.expires;
        task.retry_backoff = self.retry_backoff;
        task.ignore_result = self.ignore_result;
        task.acks_late = self.acks_late;
        task.soft_timeout = self.soft_timeout;
        // Warn when both ignore_result and result_ttl are set: they conflict
        // (one says "don't store", the other says "store then auto-clean").
        // ignore_result takes priority — no row is ever written.
        if task.ignore_result && task.result_ttl > 0 {
            tracing::warn!(
                "both ignore_result=true and result_ttl={} are set; ignore_result takes priority (no result row will be stored)",
                task.result_ttl
            );
        }
        // softTimeout (C11) validations:
        // 1. HTTP tasks: warn and reset to None (HTTP clients cannot be
        //    gracefully interrupted via SIGTERM).
        // 2. soft_timeout >= timeout: warn and reset to None (the SIGTERM
        //    phase would never fire — soft must be strictly less than hard
        //    timeout to leave a non-zero grace period for SIGKILL).
        if let Some(st) = task.soft_timeout {
            if task.task_type == TaskType::Http {
                tracing::warn!(
                    "soft_timeout={} is set on an HTTP task; HTTP clients cannot be gracefully interrupted, soft_timeout will be ignored",
                    st
                );
                task.soft_timeout = None;
            } else if st >= task.timeout {
                tracing::warn!(
                    "soft_timeout={} is >= timeout={}; soft_timeout must be strictly less than timeout to leave a SIGKILL grace period, soft_timeout will be ignored",
                    st, task.timeout
                );
                task.soft_timeout = None;
            }
        }
        // Compute the initial next_fire based on scheduling priority:
        //   runAt > cron > interval.
        // Emit warnings when multiple triggers are set simultaneously.
        if task.run_at.is_some() {
            if task.cron.is_some() {
                tracing::warn!(
                    "both runAt and cron are set; runAt takes priority, cron will be ignored"
                );
            }
            if task.interval.is_some() {
                tracing::warn!(
                    "both runAt and every(interval) are set; runAt takes priority, interval will be ignored"
                );
            }
            if task.jitter > 0 {
                tracing::warn!(
                    "jitter is set on a runAt task; jitter is ignored for one-shot triggers"
                );
            }
            task.next_fire = Some(task.run_at.unwrap() as u64);
        } else if let Some(expr) = &task.cron {
            if task.interval.is_some() {
                tracing::warn!(
                    "both cron and every(interval) are set; cron takes priority, interval will be ignored"
                );
            }
            match crate::scheduler::cron::next_fire(expr, now_ts(), task.timezone.as_deref()) {
                Ok(t) => {
                    // Apply jitter: add a random offset in [0, jitter] to the
                    // initial next_fire for cron tasks.
                    if task.jitter > 0 {
                        task.next_fire = Some(t + rand_jitter(task.jitter));
                    } else {
                        task.next_fire = Some(t);
                    }
                }
                Err(e) => {
                    return Err(XhjobError::CronParse(format!("invalid cron '{}': {}", expr, e)));
                }
            }
        } else if let Some(secs) = task.interval {
            let mut nf = now_ts() + secs;
            if task.jitter > 0 {
                nf += rand_jitter(task.jitter);
            }
            task.next_fire = Some(nf);
        }
        Ok(task)
    }

    /// Serialize the builder as JSON (for sending to daemon via IPC).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Deserialize a builder from JSON (used by daemon to reconstruct).
    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s)
            .map_err(|e| XhjobError::InvalidTask(format!("invalid task json: {}", e)))
    }

    /// Dispatch the task to the daemon via IPC. Returns task_id.
    ///
    /// Routes to the daemon for `self.service_name` with `self.data_dir`
    /// (if set) as the directory containing the IPC socket.
    pub async fn dispatch(self) -> Result<String> {
        // Validate timezone up front: fail fast on bad input rather than
        // enqueuing an IPC request that the daemon cannot honor correctly.
        if let Some(tz) = &self.timezone {
            crate::scheduler::cron::validate_timezone(tz)?;
        }
        let json = serde_json::to_value(&self)
            .map_err(|e| XhjobError::InvalidTask(format!("serialize: {}", e)))?;
        let resp = ipc_request("dispatch", json, &self.service_name, self.data_dir.as_deref()).await?;
        if !resp.ok {
            return Err(XhjobError::Ipc(resp.err.unwrap_or_else(|| "unknown error".to_string())));
        }
        // Expect data = {"task_id": "..."}
        let task_id = resp.data.get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| XhjobError::Ipc("missing task_id in response".to_string()))?
            .to_string();
        Ok(task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TaskType;

    /// softTimeout (C11) SubTask 41.11 — HTTP tasks: setting `soft_timeout`
    /// on an HTTP task is invalid because HTTP clients cannot be gracefully
    /// interrupted via SIGTERM. `build()` should log a warning and reset
    /// `soft_timeout` to None (rather than erroring out — the task is still
    /// dispatchable, just without soft-timeout semantics).
    /// Reference: Celery soft_time_limit.
    #[test]
    fn test_soft_timeout_ignored_for_http() {
        let task = TaskBuilder::new()
            .via_http("GET", "https://example.com")
            .timeout(10)
            .soft_timeout(5)
            .build()
            .expect("build should succeed (warn + reset, not error)");
        assert_eq!(task.task_type, TaskType::Http);
        assert_eq!(task.soft_timeout, None, "soft_timeout should be reset to None for HTTP tasks");
    }

    /// softTimeout (C11) SubTask 41.12 — `soft_timeout >= timeout` is invalid
    /// because the SIGTERM phase would never leave a non-zero grace period for
    /// SIGKILL escalation. `build()` should log a warning and reset
    /// `soft_timeout` to None. Covers both the strictly-greater case (15>10)
    /// and the equality boundary case (10==10).
    /// Reference: Celery soft_time_limit.
    #[test]
    fn test_soft_timeout_ge_timeout_ignored() {
        // Case 1: soft_timeout (15) > timeout (10) — strictly greater.
        let task = TaskBuilder::new()
            .via_shell("echo hi")
            .timeout(10)
            .soft_timeout(15)
            .build()
            .expect("build should succeed (warn + reset, not error)");
        assert_eq!(task.task_type, TaskType::Shell);
        assert_eq!(task.soft_timeout, None, "soft_timeout > timeout should be reset to None");

        // Case 2: soft_timeout (10) == timeout (10) — equality boundary.
        let task = TaskBuilder::new()
            .via_shell("echo hi")
            .timeout(10)
            .soft_timeout(10)
            .build()
            .expect("build should succeed (warn + reset, not error)");
        assert_eq!(task.soft_timeout, None, "soft_timeout == timeout should also be reset to None");
    }
}
