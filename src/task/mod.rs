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
    #[serde(default)]
    pub payload: serde_json::Value,
    pub cron: Option<String>,
    #[serde(default)]
    pub retry_max: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_delay: u64,
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub allow_overlap: bool,
    #[serde(default = "default_max_instances")]
    pub max_instances: u32,
    #[serde(default = "default_coalesce_true")]
    pub coalesce: bool,
    #[serde(default)]
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
    /// misfire_grace_time (A13): per-job override of the global default
    /// 60s misfire grace window. 0 = use global default (60s).
    /// Reference: APScheduler misfire_grace_time.
    #[serde(default)]
    pub misfire_grace_time: u64,
    /// Optional explicit task id (A14). When set, dispatch will use this id
    /// instead of auto-generating a UUID. If `replace_existing` is true, an
    /// existing task with the same id is fully replaced. If false, dispatch
    /// errors out on id conflict.
    #[serde(default)]
    pub id: Option<String>,
    /// replace_existing (A14): when true and `id` is set, dispatch replaces
    /// an existing task with the same id (full overwrite, state /
    /// attempts / execution_count reset). Default false (error on conflict).
    #[serde(default)]
    pub replace_existing: bool,
    /// tags (A15): user-supplied labels for grouping / filtering tasks.
    /// Empty by default. Reference: APScheduler tags.
    #[serde(default)]
    pub tags: Vec<String>,
    /// rate_limit_count (C12): max number of triggers allowed within
    /// `rate_limit_window` seconds. 0 = no rate limiting (default).
    /// Reference: Celery rate_limit.
    #[serde(default)]
    pub rate_limit_count: u32,
    /// rate_limit_window (C12): sliding window length in seconds for
    /// rate limiting. 0 = no rate limiting (default).
    /// Reference: Celery rate_limit.
    #[serde(default)]
    pub rate_limit_window: u64,
    /// acks_on_failure (C13): when true (default), task failures respect
    /// `retry_max`. When false, failures are retried indefinitely until
    /// the task succeeds or is cancelled/removed.
    /// Reference: Celery acks_on_failure.
    #[serde(default = "default_acks_on_failure_true")]
    pub acks_on_failure: bool,
    /// idempotent: when true, declares this HTTP task safe to retry even if
    /// it uses a non-idempotent method (POST/PUT/DELETE/PATCH). When false
    /// (default), such methods are NOT retried on 5xx to prevent duplicate
    /// side effects. GET/HEAD/OPTIONS are always retryable regardless.
    /// Reference: HTTP method safety/idempotency (RFC 7231 §4.2.1-2).
    #[serde(default)]
    pub idempotent: bool,
    /// Countdown (Celery apply_async(countdown=N)): relative delay in
    /// seconds. Equivalent to `run_at(now + countdown)`. When both
    /// `countdown` and `run_at` are set, `run_at` takes precedence and a
    /// warn is logged. None = no countdown.
    /// Reference: Celery apply_async(countdown=N).
    #[serde(default)]
    pub countdown: Option<u64>,
    /// Owner/tenant for multi-tenant isolation. When set at dispatch time,
    /// only the same owner can query/modify this task. P0-17 fix.
    #[serde(default)]
    pub owner: String,
}

fn default_acks_on_failure_true() -> bool { true }

fn default_service_name() -> String {
    "default".to_string()
}

// Per-field serde defaults that mirror `TaskBuilder::default()`. Plain
// `#[serde(default)]` would use the field type's `Default` (e.g. `0` for
// `u64`), which differs from the struct's defaults for `retry_delay`,
// `timeout`, `max_instances`, and `coalesce`. Providing explicit functions
// ensures partial JSON (e.g. only `task_type` + `payload` from PHP users)
// deserializes with the same sensible defaults the Rust builder API uses.
fn default_retry_delay() -> u64 { 1 }
fn default_timeout() -> u64 { 30 }
fn default_max_instances() -> u32 { 1 }
fn default_coalesce_true() -> bool { true }

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
            misfire_grace_time: 0,
            id: None,
            replace_existing: false,
            tags: Vec::new(),
            rate_limit_count: 0,
            rate_limit_window: 0,
            acks_on_failure: true,
            idempotent: false,
            countdown: None,
            owner: String::new(),
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

    /// Set per-job misfire_grace_time (A13) in seconds. 0 = use the global
    /// default (60s). When `now - next_fire > grace_time`, the trigger is
    /// considered misfired; `coalesce=true` collapses missed triggers into
    /// one fire (still executes once), `coalesce=false` skips the trigger
    /// entirely. Only effective for cron tasks; interval / runAt tasks
    /// ignore this field (warn + ignore).
    /// Exposed as `misfireGraceTime(int $secs)` in PHP (snake→camel auto-conversion).
    /// Reference: APScheduler misfire_grace_time.
    pub fn misfire_grace_time(mut self, secs: u64) -> Self {
        self.misfire_grace_time = secs;
        self
    }

    /// Set an explicit task id (A14). When set, dispatch will use this id
    /// instead of auto-generating a UUID. If `replace_existing` is true, an
    /// existing task with the same id is fully replaced (state /
    /// attempts / execution_count reset). If false (default), dispatch
    /// errors out on id conflict.
    /// Exposed as `withId(string $id)` in PHP (snake→camel auto-conversion).
    /// Reference: APScheduler id / replace_existing.
    pub fn id(mut self, id: impl Into<String>) -> Self {
        let id = id.into();
        self.id = if id.is_empty() { None } else { Some(id) };
        self
    }

    /// Enable replace_existing (A14): when true and `id` is set, dispatch
    /// replaces an existing task with the same id (full overwrite, state /
    /// attempts / execution_count reset). When false (default), dispatch
    /// returns an error on id conflict.
    /// Exposed as `replaceExisting(bool $on)` in PHP (snake→camel auto-conversion).
    /// Reference: APScheduler replace_existing.
    pub fn replace_existing(mut self, on: bool) -> Self {
        self.replace_existing = on;
        self
    }

    /// Add a tag (A15) to this task. Tags are user-supplied labels for
    /// grouping / filtering tasks. Multiple tags can be added by chaining.
    /// Empty / duplicate tags are silently ignored.
    /// Exposed as `tags(string ...$tags)` in PHP.
    /// Reference: APScheduler tags.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        let t = tag.into();
        if !t.is_empty() && !self.tags.iter().any(|x| x == &t) {
            self.tags.push(t);
        }
        self
    }

    /// Set the rate limit (C12): max `count` triggers within `window`
    /// seconds. 0 count = no rate limiting (default). A sliding window
    /// algorithm enforces the limit; over-limit triggers record a
    /// `RateLimited` event and are skipped (next_fire advanced by window).
    /// Exposed as `rateLimit(int $count, int $window)` in PHP (snake→camel auto-conversion).
    /// Reference: Celery rate_limit.
    pub fn rate_limit(mut self, count: u32, window: u64) -> Self {
        self.rate_limit_count = count;
        self.rate_limit_window = window;
        self
    }

    /// Set acks_on_failure (C13): when true (default), task failures respect
    /// `retry_max` (transition to Failed terminal after exhausting retries).
    /// When false, failures are retried indefinitely (ignoring retry_max)
    /// until the task succeeds or is cancelled/removed. Complementary to
    /// `acks_late`.
    /// Exposed as `acksOnFailure(bool $on)` in PHP (snake→camel auto-conversion).
    /// Reference: Celery acks_on_failure.
    pub fn acks_on_failure(mut self, on: bool) -> Self {
        self.acks_on_failure = on;
        self
    }

    /// Declare this HTTP task as idempotent (safe to retry even with a
    /// non-idempotent method like POST/PUT/DELETE/PATCH). When false
    /// (default), HTTP tasks using non-idempotent methods are NOT retried
    /// on 5xx to prevent duplicate side effects. GET/HEAD/OPTIONS are
    /// always retryable regardless of this flag.
    /// Exposed as `idempotent(bool $on)` in PHP.
    /// Reference: HTTP method safety/idempotency (RFC 7231 §4.2.1-2).
    pub fn idempotent(mut self, on: bool) -> Self {
        self.idempotent = on;
        self
    }

    /// Countdown (Celery apply_async(countdown=N)): relative delay in seconds.
    /// Equivalent to run_at(now + countdown). When both countdown and run_at
    /// are set, run_at takes precedence and a warn is logged.
    /// Exposed as `countdown(int $secs)` in PHP.
    /// Reference: Celery apply_async(countdown=N).
    pub fn countdown(mut self, secs: u64) -> Self {
        self.countdown = Some(secs);
        self
    }

    /// Build the final Task struct (without dispatching).
    pub fn build(self) -> Result<Task> {
        let task_type = self.task_type.ok_or_else(|| XhjobError::InvalidTask(
            "task type not set; call via_http() or via_shell() first".to_string()
        ))?;
        // P0-2: validate explicit id charset. ids must match
        // `^[A-Za-z0-9_-]{1,64}$`. This prevents two classes of bug:
        //   (1) ids starting with "error:" break the PHP-side dispatch()
        //       error-detection contract (str_starts_with($r, 'error:'));
        //   (2) ids containing SQL meta-chars / JSON quotes could cause
        //       downstream issues. Empty id is allowed (means auto-generate).
        if let Some(ref id) = self.id {
            if id.is_empty() {
                // empty string is treated as "not set" — fall through to
                // auto-generate below.
            } else {
                let valid = id.len() <= 64
                    && id.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
                    });
                if !valid {
                    return Err(XhjobError::InvalidTask(format!(
                        "task id '{}' is invalid: must match ^[A-Za-z0-9_-]{{1,64}}$",
                        id
                    )));
                }
            }
        }
        // MINOR fix: validate payload content before constructing the Task.
        // - HTTP tasks MUST have a non-empty `url` (otherwise the executor
        //   would silently no-op or fail with a confusing reqwest error).
        // - Shell tasks MUST have a non-empty `cmd` (otherwise `bash -c ""`
        //   exits 0 immediately, masking a misconfiguration).
        // These are checked here rather than in via_http()/via_shell() so
        // the validation also covers deserialized TaskBuilder JSON from PHP
        // / chord callbacks / chain steps.
        match task_type {
            TaskType::Http => {
                if let Ok(p) = serde_json::from_value::<HttpPayload>(self.payload.clone()) {
                    if p.url.trim().is_empty() {
                        return Err(XhjobError::InvalidTask(
                            "http task url must not be empty".to_string()
                        ));
                    }
                    if p.method.trim().is_empty() {
                        return Err(XhjobError::InvalidTask(
                            "http task method must not be empty".to_string()
                        ));
                    }
                }
            }
            TaskType::Shell => {
                if let Ok(p) = serde_json::from_value::<ShellPayload>(self.payload.clone()) {
                    if p.cmd.trim().is_empty() {
                        return Err(XhjobError::InvalidTask(
                            "shell task cmd must not be empty".to_string()
                        ));
                    }
                }
            }
        }
        // MINOR fix: when retry_max > 0, retry_delay MUST be > 0. A 0-second
        // delay with retries would cause a tight retry storm (retry_max
        // attempts fired back-to-back within microseconds), which can
        // overwhelm downstream services and is almost certainly a
        // misconfiguration. retry_delay == 0 is only valid when retry_max == 0
        // (no retries — the delay is irrelevant).
        if self.retry_max > 0 && self.retry_delay == 0 {
            return Err(XhjobError::InvalidTask(format!(
                "retry_max={} but retry_delay=0; retry_delay must be > 0 when retries are enabled (set retry_delay >= 1 to avoid a retry storm)",
                self.retry_max
            )));
        }
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
        task.misfire_grace_time = self.misfire_grace_time;
        // 防御性归一化：客户端（特别是 PHP TaskBuilder）可能把 Option 字段
        // 输出为 0 而不是 null，导致 Rust 反序列化为 Some(0)。Some(0) 会让
        // interval / runAt / start_date / end_date / soft_timeout 被误判为
        // 已设置，进而触发 DateTrigger / SoftTimeout 等错误路径。
        // 这里把所有 Option 时间戳字段中的 Some(0) 视为 None。
        if task.interval == Some(0) { task.interval = None; }
        if task.run_at == Some(0) { task.run_at = None; }
        if task.start_date == Some(0) { task.start_date = None; }
        if task.end_date == Some(0) { task.end_date = None; }
        if task.soft_timeout == Some(0) { task.soft_timeout = None; }
        // countdown 归一化：countdown 与 run_at 同时设置时 run_at 优先。
        // countdown==0 视为未设置（无延迟）。当 run_at 未设置但 countdown
        // 已设置时，转换为 run_at = now + countdown，复用既有 DateTrigger 路径。
        if self.countdown == Some(0) {
            // 0 延迟无意义，等同于不设置。
        } else if let Some(cd) = self.countdown {
            if task.run_at.is_none() {
                task.run_at = Some(now_ts() as i64 + cd as i64);
            } else {
                tracing::warn!(
                    "both countdown and run_at set; run_at takes precedence"
                );
            }
        }
        // A14: explicit id — if set, override the auto-generated UUID.
        if let Some(id) = self.id {
            task.id = id;
        }
        task.replace_existing = self.replace_existing;
        task.tags = self.tags;
        task.rate_limit_count = self.rate_limit_count;
        task.rate_limit_window = self.rate_limit_window;
        task.acks_on_failure = self.acks_on_failure;
        task.idempotent = self.idempotent;
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
            return Err(XhjobError::ipc(resp.err.unwrap_or_else(|| "unknown error".to_string())));
        }
        // Expect data = {"task_id": "..."}
        let task_id = resp.data.get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| XhjobError::ipc("missing task_id in response".to_string()))?
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

    /// Partial-JSON deserialization (Round 4 fix): PHP users typically pass
    /// only `task_type` + `payload` to `xhjob_dispatch()`; the remaining
    /// TaskBuilder fields must fall back to their struct-level defaults
    /// (retry_delay=1, timeout=30, max_instances=1, coalesce=true), NOT the
    /// raw type defaults (0, 0, 0, false) that plain `#[serde(default)]`
    /// would produce. This test guards against regressions where a missing
    /// `timeout` field silently produces timeout=0 and every task fails with
    /// "exec: timeout after 0s".
    #[test]
    fn test_partial_json_uses_struct_defaults() {
        let json = r#"{"task_type":"shell","payload":{"cmd":"echo hi"}}"#;
        let b = TaskBuilder::from_json(json).expect("partial JSON should deserialize");
        let task = b.build().expect("build should succeed with defaults");
        assert_eq!(task.task_type, TaskType::Shell);
        assert_eq!(task.retry_max, 0, "retry_max default = 0");
        assert_eq!(task.retry_delay, 1, "retry_delay default = 1 (not 0)");
        assert_eq!(task.timeout, 30, "timeout default = 30 (not 0)");
        assert_eq!(task.priority, 0, "priority default = 0");
        assert_eq!(task.max_instances, 1, "max_instances default = 1 (not 0)");
        assert!(task.coalesce, "coalesce default = true (not false)");
        assert!(!task.persist, "persist default = false");
        assert!(!task.allow_overlap, "allow_overlap default = false");
        assert!(task.acks_on_failure, "acks_on_failure default = true");
    }

    /// P0-2: build() must reject ids that don't match `^[A-Za-z0-9_-]{1,64}$`.
    /// This includes ids containing "error:" prefix (which would break PHP
    /// dispatch() error-detection contract), SQL meta-chars, JSON quotes, etc.
    #[test]
    fn test_build_rejects_invalid_id_charset() {
        let bad_ids = [
            "error: malicious",      // contains ":" and space
            "'; DROP TABLE tasks; --", // SQL injection chars
            "id with spaces",        // spaces
            "id/with/slashes",       // slashes
            &"a".repeat(65),         // too long (>64)
        ];
        for bad in bad_ids {
            let json = format!(
                r#"{{"task_type":"shell","payload":{{"cmd":"echo hi"}},"id":"{}"}}"#,
                bad.replace('"', "\\\"").replace('\\', "\\\\")
            );
            let b = TaskBuilder::from_json(&json).expect("JSON should deserialize");
            let err = b.build().expect_err(
                &format!("id '{}' should be rejected", bad)
            );
            let msg = format!("{}", err);
            assert!(
                msg.contains("invalid") && msg.contains("id"),
                "error message should mention invalid id: {}", msg
            );
        }
    }

    /// P0-2: build() must accept ids that match `^[A-Za-z0-9_-]{1,64}$`.
    #[test]
    fn test_build_accepts_valid_id_charset() {
        let good_ids = [
            "my-task-001_ABC",
            "a",
            &"a".repeat(64),  // max length
            "ABC123-_-",
        ];
        for good in good_ids {
            let json = format!(
                r#"{{"task_type":"shell","payload":{{"cmd":"echo hi"}},"id":"{}"}}"#,
                good
            );
            let b = TaskBuilder::from_json(&json).expect("JSON should deserialize");
            let task = b.build().expect(
                &format!("id '{}' should be accepted", good)
            );
            assert_eq!(task.id, *good);
        }
    }

    /// MINOR fix: build() must reject HTTP tasks with an empty url.
    #[test]
    fn test_build_rejects_empty_http_url() {
        let json = r#"{"task_type":"http","payload":{"method":"POST","url":"","headers":{}}}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let err = b.build().expect_err("empty url must be rejected");
        assert!(
            err.to_string().contains("url must not be empty"),
            "expected url-empty error, got: {}",
            err
        );
    }

    /// MINOR fix: build() must reject HTTP tasks with a whitespace-only url
    /// (a stray space should not slip through `trim().is_empty()`).
    #[test]
    fn test_build_rejects_whitespace_http_url() {
        let json = r#"{"task_type":"http","payload":{"method":"POST","url":"   ","headers":{}}}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let err = b.build().expect_err("whitespace-only url must be rejected");
        assert!(
            err.to_string().contains("url must not be empty"),
            "expected url-empty error, got: {}",
            err
        );
    }

    /// MINOR fix: build() must reject HTTP tasks with an empty method.
    #[test]
    fn test_build_rejects_empty_http_method() {
        let json = r#"{"task_type":"http","payload":{"method":"","url":"https://example.com","headers":{}}}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let err = b.build().expect_err("empty method must be rejected");
        assert!(
            err.to_string().contains("method must not be empty"),
            "expected method-empty error, got: {}",
            err
        );
    }

    /// MINOR fix: build() must reject Shell tasks with an empty cmd.
    #[test]
    fn test_build_rejects_empty_shell_cmd() {
        let json = r#"{"task_type":"shell","payload":{"cmd":""}}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let err = b.build().expect_err("empty cmd must be rejected");
        assert!(
            err.to_string().contains("cmd must not be empty"),
            "expected cmd-empty error, got: {}",
            err
        );
    }

    /// MINOR fix: build() must reject Shell tasks with a whitespace-only cmd.
    #[test]
    fn test_build_rejects_whitespace_shell_cmd() {
        let json = r#"{"task_type":"shell","payload":{"cmd":"   "}}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let err = b.build().expect_err("whitespace-only cmd must be rejected");
        assert!(
            err.to_string().contains("cmd must not be empty"),
            "expected cmd-empty error, got: {}",
            err
        );
    }

    /// MINOR fix: build() must reject retry_max > 0 with retry_delay == 0
    /// (would cause a tight retry storm).
    #[test]
    fn test_build_rejects_retry_max_without_delay() {
        let json = r#"{"task_type":"shell","payload":{"cmd":"echo hi"},"retry_max":3,"retry_delay":0}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let err = b.build().expect_err("retry_max>0 + retry_delay=0 must be rejected");
        assert!(
            err.to_string().contains("retry_delay must be > 0"),
            "expected retry-delay error, got: {}",
            err
        );
    }

    /// MINOR fix: retry_max == 0 with retry_delay == 0 is allowed (no retries,
    /// delay is irrelevant).
    #[test]
    fn test_build_allows_no_retry_with_zero_delay() {
        let json = r#"{"task_type":"shell","payload":{"cmd":"echo hi"},"retry_max":0,"retry_delay":0}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let task = b.build().expect("retry_max=0 + retry_delay=0 must be allowed");
        assert_eq!(task.retry_max, 0);
        assert_eq!(task.retry_delay, 0);
    }

    /// MINOR fix: valid HTTP task with non-empty url + method builds fine.
    #[test]
    fn test_build_accepts_valid_http_payload() {
        let json = r#"{"task_type":"http","payload":{"method":"POST","url":"https://api.example.com","headers":{}}}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let task = b.build().expect("valid http payload must build");
        assert_eq!(task.task_type, crate::store::TaskType::Http);
    }

    /// MINOR fix: valid Shell task with non-empty cmd builds fine.
    #[test]
    fn test_build_accepts_valid_shell_payload() {
        let json = r#"{"task_type":"shell","payload":{"cmd":"echo hi"}}"#;
        let b = TaskBuilder::from_json(json).expect("JSON should deserialize");
        let task = b.build().expect("valid shell payload must build");
        assert_eq!(task.task_type, crate::store::TaskType::Shell);
    }
}
