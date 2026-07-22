#![cfg_attr(windows, feature(abi_vectorcall))]

use ext_php_rs::prelude::*;

pub mod daemon;
pub mod daemon_main;
pub mod errors;
pub mod executor;
pub mod ipc;
pub mod outcome;
pub mod pool;
pub mod retry;
pub mod scheduler;
pub mod service;
pub mod store;
pub mod task;
pub mod utils;

pub use service::ServiceName;

// =========================================================================
// PHP functions
// =========================================================================

/// Resolve and validate a PHP-supplied service name. `None` falls back to the
/// default service. Returns the validated name as a `String`.
fn resolve_service_name(name: Option<String>) -> Result<String, String> {
    let raw = name.unwrap_or_else(|| service::default_name().to_string());
    service::validate(&raw).map_err(|e| format!("{}", e))
}

/// Normalize a PHP-supplied data_dir: empty string becomes None.
fn normalize_data_dir(dir: Option<String>) -> Option<String> {
    dir.and_then(|d| if d.is_empty() { None } else { Some(d) })
}

#[php_function]
pub fn xhjob_start(name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_start invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    // Check if daemon is already running
    let status = daemon::status(&service_name, data_dir.as_deref());
    if status.running {
        return true;
    }
    // Spawn daemon. daemon_main is the function the daemon will run.
    match daemon::start(daemon_main::daemon_main, &service_name, data_dir.as_deref()) {
        Ok(true) => true,
        Ok(false) => {
            // Failed to start within timeout
            false
        }
        Err(e) => {
            tracing::error!("xhjob_start failed: {}", e);
            false
        }
    }
}

#[php_function]
pub fn xhjob_stop(name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_stop invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    match daemon::stop(&service_name, data_dir.as_deref()) {
        Ok(_) => true,
        Err(e) => {
            tracing::error!("xhjob_stop failed: {}", e);
            false
        }
    }
}

#[php_function]
pub fn xhjob_restart(name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_restart invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    match daemon::restart(daemon_main::daemon_main, &service_name, data_dir.as_deref()) {
        Ok(_) => true,
        Err(e) => {
            tracing::error!("xhjob_restart failed: {}", e);
            false
        }
    }
}

#[php_function]
pub fn xhjob_status(name: Option<String>, data_dir: Option<String>) -> Vec<(String, String)> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            let mut out: Vec<(String, String)> = Vec::new();
            out.push(("running".to_string(), "false".to_string()));
            out.push(("error".to_string(), e));
            return out;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let status = daemon::status(&service_name, data_dir.as_deref());
    let mut out: Vec<(String, String)> = Vec::new();
    out.push(("running".to_string(), status.running.to_string()));
    if let Some(pid) = status.pid {
        out.push(("pid".to_string(), pid.to_string()));
    }
    out
}

#[php_function]
pub fn xhjob_dispatch(task_json: String, name: Option<String>, data_dir: Option<String>) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let data_dir = normalize_data_dir(data_dir);
    // Build a one-shot request to the daemon and return the task_id (or error string).
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    let result: std::result::Result<String, String> = rt.block_on(async move {
        let payload: serde_json::Value = serde_json::from_str(&task_json)
            .map_err(|e| format!("invalid json: {}", e))?;
        let resp = ipc::request("dispatch", payload, &service_name, data_dir.as_deref()).await
            .map_err(|e| format!("{}", e))?;
        if !resp.ok {
            return Err(resp.err.unwrap_or_else(|| "unknown".to_string()));
        }
        let task_id = resp.data.get("task_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing task_id".to_string())?
            .to_string();
        Ok(task_id)
    });
    match result {
        Ok(id) => id,
        Err(e) => format!("error: {}", e),
    }
}

#[php_function]
pub fn xhjob_state(id: String, name: Option<String>, data_dir: Option<String>) -> Vec<(String, String)> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            let mut out: Vec<(String, String)> = Vec::new();
            out.push(("state".to_string(), "UNKNOWN".to_string()));
            out.push(("error".to_string(), e));
            return out;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    let info = rt.block_on(async move {
        match outcome::query_state(&id, &service_name, data_dir.as_deref()).await {
            Ok(info) => Some(info),
            Err(_) => None,
        }
    });
    let mut out: Vec<(String, String)> = Vec::new();
    if let Some(info) = info {
        out.push(("state".to_string(), info.state));
        out.push(("attempts".to_string(), info.attempts.to_string()));
        out.push(("created_at".to_string(), info.created_at.to_string()));
        if let Some(s) = info.started_at { out.push(("started_at".to_string(), s.to_string())); }
        if let Some(f) = info.finished_at { out.push(("finished_at".to_string(), f.to_string())); }
        if let Some(e) = info.last_error { out.push(("last_error".to_string(), e)); }
        out.push(("execution_count".to_string(), info.execution_count.to_string()));
        out.push(("max_executions".to_string(), info.max_executions.to_string()));
        out.push(("paused".to_string(), info.paused.to_string()));
        out.push(("start_date".to_string(), info.start_date.map(|t| t.to_string()).unwrap_or_else(|| "null".to_string())));
        out.push(("end_date".to_string(), info.end_date.map(|t| t.to_string()).unwrap_or_else(|| "null".to_string())));
        out.push(("meta".to_string(), info.meta.clone().unwrap_or_else(|| "null".to_string())));
        out.push(("interval".to_string(), info.interval.map(|t| t.to_string()).unwrap_or_else(|| "null".to_string())));
        out.push(("run_at".to_string(), info.run_at.map(|t| t.to_string()).unwrap_or_else(|| "null".to_string())));
        out.push(("jitter".to_string(), info.jitter.to_string()));
        out.push(("expires".to_string(), info.expires.to_string()));
        out.push(("retry_backoff".to_string(), info.retry_backoff.to_string()));
        out.push(("ignore_result".to_string(), info.ignore_result.to_string()));
        out.push(("acks_late".to_string(), info.acks_late.to_string()));
        out.push(("soft_timeout".to_string(), info.soft_timeout.map(|t| t.to_string()).unwrap_or_else(|| "null".to_string())));
        // 追加 StateInfo 中已有但之前未透出的字段
        out.push(("misfire_grace_time".to_string(), info.misfire_grace_time.to_string()));
        out.push(("tags".to_string(), serde_json::to_string(&info.tags).unwrap_or_else(|_| "[]".to_string())));
        out.push(("rate_limit_count".to_string(), info.rate_limit_count.to_string()));
        out.push(("rate_limit_window".to_string(), info.rate_limit_window.to_string()));
        out.push(("acks_on_failure".to_string(), info.acks_on_failure.to_string()));
        out.push(("timezone".to_string(), info.timezone.clone().unwrap_or_default()));
        out.push(("coalesce".to_string(), info.coalesce.to_string()));
        out.push(("progress".to_string(), info.progress.map(|p| p.to_string()).unwrap_or_default()));
        out.push(("progress_meta".to_string(), info.progress_meta.clone().unwrap_or_default()));
    } else {
        out.push(("state".to_string(), "UNKNOWN".to_string()));
        out.push(("error".to_string(), "task not found or daemon not running".to_string()));
    }
    out
}

#[php_function]
pub fn xhjob_result(id: String, name: Option<String>, data_dir: Option<String>) -> Vec<(String, String)> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            let mut out: Vec<(String, String)> = Vec::new();
            out.push(("error".to_string(), e));
            return out;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    let result = rt.block_on(async move {
        match outcome::query_result(&id, &service_name, data_dir.as_deref()).await {
            Ok(r) => Some(r),
            Err(_) => None,
        }
    });
    let mut out: Vec<(String, String)> = Vec::new();
    if let Some(r) = result {
        if let Some(b) = r.body { out.push(("body".to_string(), b)); }
        if let Some(c) = r.status_code { out.push(("status_code".to_string(), c.to_string())); }
        if let Some(o) = r.stdout { out.push(("stdout".to_string(), o)); }
        if let Some(e) = r.stderr { out.push(("stderr".to_string(), e)); }
        if let Some(c) = r.exit_code { out.push(("exit_code".to_string(), c.to_string())); }
    } else {
        out.push(("error".to_string(), "no result record for this task (it may have failed before producing output; check xhjob_state() last_error)".to_string()));
    }
    out
}

/// Remove a task definition from the store. Does not affect running instances.
/// Reference: APScheduler remove_job.
/// PHP: `xhjob_remove(string $id, string $name = "default", string $data_dir = null): bool`
#[php_function]
pub fn xhjob_remove(id: String, name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_remove invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "id": id });
        match ipc::request("remove", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => resp.ok,
            Err(e) => {
                tracing::error!("xhjob_remove ipc: {}", e);
                false
            }
        }
    })
}

/// Pause a cron task. The task definition is preserved but cron tick will not fire it.
/// Reference: APScheduler pause_job.
/// PHP: `xhjob_pause(string $id, string $name = "default", string $data_dir = null): bool`
#[php_function]
pub fn xhjob_pause(id: String, name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_pause invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "id": id });
        match ipc::request("pause", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => resp.ok,
            Err(e) => {
                tracing::error!("xhjob_pause ipc: {}", e);
                false
            }
        }
    })
}

/// Resume a paused cron task.
/// Reference: APScheduler resume_job.
/// PHP: `xhjob_resume(string $id, string $name = "default", string $data_dir = null): bool`
#[php_function]
pub fn xhjob_resume(id: String, name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_resume invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "id": id });
        match ipc::request("resume", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => resp.ok,
            Err(e) => {
                tracing::error!("xhjob_resume ipc: {}", e);
                false
            }
        }
    })
}

/// Cancel a task. Pending → Cancelled terminal; Running → no retry, no cron re-trigger.
/// Reference: Celery revoke.
/// PHP: `xhjob_cancel(string $id, string $name = "default", string $data_dir = null): bool`
#[php_function]
pub fn xhjob_cancel(id: String, name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_cancel invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "id": id });
        match ipc::request("cancel", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => resp.ok,
            Err(e) => {
                tracing::error!("xhjob_cancel ipc: {}", e);
                false
            }
        }
    })
}

/// List all tasks in a service, optionally filtered by state.
/// Reference: APScheduler get_jobs.
/// PHP: `xhjob_list(string $name = "default", string $state_filter = null, string $tag = null, string $data_dir = null): string`
/// Returns a JSON string of the form `{"tasks": [...]}`. PHP callers should
/// `json_decode($result, true)` to obtain the associative array. On error the
/// returned string starts with `error:`. The `$tag` parameter (when non-null)
/// is forwarded to the daemon as `tag_filter` for server-side tag filtering.
#[php_function]
pub fn xhjob_list(
    name: Option<String>,
    state_filter: Option<String>,
    tag: Option<String>,
    data_dir: Option<String>,
) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return format!("error: {}", e),
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        // 新增 tag_filter 字段，透传到 daemon 端 handle_list_op
        let payload = serde_json::json!({
            "state_filter": state_filter,
            "tag_filter": tag,
        });
        match ipc::request("list", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return format!("error: {}", resp.err.unwrap_or_else(|| "unknown".to_string()));
                }
                resp.data.get("tasks")
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "[]".to_string())
            }
            Err(e) => format!("error: {}", e),
        }
    })
}

/// Re-queue a terminal task (Cancelled / Failed / Expired) back to Pending so
/// it can be triggered again. Resets attempts to 0 and sets next_fire to now.
/// Returns `true` if requeued, `false` if the task was not in a requeueable
/// terminal state (or does not exist).
/// Reference: Celery requeue.
/// PHP: `xhjob_requeue(string $id, string $name = "default", string $data_dir = null): bool`
#[php_function]
pub fn xhjob_requeue(id: String, name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_requeue invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "id": id });
        match ipc::request("requeue", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return false;
                }
                resp.data.get("requeued")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            }
            Err(e) => {
                tracing::error!("xhjob_requeue ipc: {}", e);
                false
            }
        }
    })
}

/// Reschedule a cron task's cron expression online (A11). Preserves task
/// state, execution_count, attempts, and meta — only `cron` and `next_fire`
/// change. Returns `true` on success. Returns `false` if the task does not
/// exist, is not a cron task (e.g. interval / runAt), is in a terminal state,
/// or the new cron expression is invalid.
/// Reference: APScheduler reschedule_job.
/// PHP: `xhjob_reschedule(string $id, string $cron, string $name = "default", string $data_dir = null): bool`
#[php_function]
pub fn xhjob_reschedule(id: String, cron: String, name: Option<String>, data_dir: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_reschedule invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "id": id, "cron": cron });
        match ipc::request("reschedule", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return false;
                }
                resp.data.get("rescheduled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            }
            Err(e) => {
                tracing::error!("xhjob_reschedule ipc: {}", e);
                false
            }
        }
    })
}

/// Fetch a single task definition by id, returning the full Task JSON (A12).
/// Differs from `xhjob_state` (which returns the trimmed `StateInfo` view):
/// `xhjob_get` returns every persisted field including configuration fields
/// such as `retry_max` / `timeout` / `priority` / `allow_overlap` /
/// `max_instances` / `coalesce` / `cron` / `interval` / `run_at` / etc.
///
/// Returns the JSON string of the task on success, or `null` if the task
/// does not exist (or the daemon is unreachable). The returned string can be
/// decoded with `json_decode($json, true)` to obtain the full associative
/// array.
///
/// Reference: APScheduler get_job.
/// PHP: `xhjob_get(string $id, string $name = "default", string $data_dir = null): ?string`
#[php_function]
pub fn xhjob_get(id: String, name: Option<String>, data_dir: Option<String>) -> Option<String> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_get invalid service name: {}", e);
            return None;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "id": id });
        match ipc::request("get", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return None;
                }
                let ok = resp.data.get("ok")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !ok {
                    return None;
                }
                resp.data.get("data")
                    .map(|d| d.to_string())
            }
            Err(e) => {
                tracing::error!("xhjob_get ipc: {}", e);
                None
            }
        }
    })
}

/// Hidden entry point invoked when the PHP binary is re-executed by
/// `spawn_via_double_fork` (Unix) or `spawn_via_create_process` (Windows).
/// Runs the daemon loop in the current process and never returns.
///
/// The optional `service_name` argument is the primary propagation path for
/// the service identity: the spawner encodes it into the `-r` code string
/// (e.g. `xhjob_run_daemon('cron-svc', '/var/lib/xhjob');`) so it survives
/// PHP version-manager shim re-execs that may scrub env vars set via
/// `Command::env()`. When provided, the name is validated and installed via
/// `service::set_current()` before `daemon_main()` runs, so all derived paths
/// (PID file, IPC socket, log file, store) are keyed correctly.
///
/// The optional `data_dir` argument is the primary propagation path for the
/// data directory: when provided, it is installed via
/// `service::set_current_data_dir()` so all runtime files are placed under
/// that directory. This enables users to relocate all service files to a
/// custom directory for backup / migration / restore.
///
/// When either argument is `None`, the value falls back to `service::current()`
/// / `service::current_data_dir()`'s env-var path (`XHJOB_SERVICE_NAME` /
/// `XHJOB_DATA_DIR`) and finally to the platform default, preserving backward
/// compatibility with callers that spawn the daemon through other paths.
#[php_function]
pub fn xhjob_run_daemon(service_name: Option<String>, data_dir: Option<String>) -> bool {
    if let Some(name) = service_name {
        match service::validate(&name) {
            Ok(validated) => service::set_current(validated),
            Err(e) => {
                tracing::error!("xhjob_run_daemon invalid service name: {}", e);
                return false;
            }
        }
    }
    if let Some(dir) = normalize_data_dir(data_dir) {
        service::set_current_data_dir(dir);
    }
    // Std streams were detached by the spawn (Stdio::null). Reopen them to
    // the log file so tracing output is captured.
    #[cfg(unix)]
    {
        let _ = reopen_std_streams_for_daemon();
    }
    daemon_main::daemon_main();
    true
}

#[cfg(unix)]
fn reopen_std_streams_for_daemon() {
    use std::os::unix::io::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let service_name = crate::service::current();
    let data_dir = crate::service::current_data_dir();
    let log = daemon::log_file_path(&service_name, data_dir.as_deref());
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let f = std::fs::OpenOptions::new()
        .create(true).append(true).read(false).mode(0o644)
        .open(&log);
    if let Ok(f) = f {
        let log_fd = f.as_raw_fd();
        let devnull = std::fs::OpenOptions::new().read(true).open("/dev/null");
        if let Ok(devnull) = devnull {
            unsafe {
                extern "C" { fn dup2(oldfd: i32, newfd: i32) -> i32; }
                dup2(devnull.as_raw_fd(), 0);
                dup2(log_fd, 1);
                dup2(log_fd, 2);
            }
            std::mem::forget(f);
            std::mem::forget(devnull);
        }
    }
}

// =========================================================================
// PHP class Xhjob (chainable API)
// =========================================================================

#[php_class]
#[derive(Default)]
pub struct Xhjob {
    builder: task::TaskBuilder,
}

#[php_impl]
impl Xhjob {
    pub fn task() -> Xhjob {
        Xhjob { builder: task::TaskBuilder::new() }
    }

    /// Bind this Xhjob instance to a named service. Subsequent `dispatch()`
    /// will route to the daemon for that service. Returns `&mut self` for
    /// chaining. Exposed as `service()` in PHP.
    pub fn service(&mut self, name: String) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).service(name);
        self
    }

    /// Set the data directory where the daemon's PID/sock/db/log files live.
    /// When set, `dispatch()` resolves the IPC socket under this directory.
    /// Used for backup / migration / restore scenarios where the user has
    /// relocated all service files to a custom directory.
    /// Exposed as `dataDir()` in PHP (snake→camel auto-conversion).
    pub fn data_dir(&mut self, dir: String) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).data_dir(dir);
        self
    }

    pub fn via_http(&mut self, method: String, url: String) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).via_http(method, url);
        self
    }

    /// Set HTTP request headers (overrides previous). PHP associative array
    /// converts to `Vec<(String, String)>` in ext-php-rs 0.15.
    /// Exposed as `withHeaders()` in PHP (snake→camel auto-conversion).
    pub fn with_headers(&mut self, headers: Vec<(String, String)>) -> &mut Self {
        let builder = std::mem::take(&mut self.builder);
        let mut map = std::collections::HashMap::new();
        for (k, v) in headers {
            map.insert(k, v);
        }
        self.builder = builder.headers(map);
        self
    }

    /// Set HTTP request body. Exposed as `withBody()` in PHP.
    pub fn with_body(&mut self, body: String) -> &mut Self {
        let builder = std::mem::take(&mut self.builder);
        self.builder = builder.body(body);
        self
    }

    /// Set HTTP/SOCKS5 proxy URL for HTTP tasks. Exposed as `withProxy()` in PHP.
    /// Accepted schemes: `http://`, `https://`, `socks5://`, `socks5h://`.
    /// The URL may include `user:pass@` credentials.
    pub fn with_proxy(&mut self, proxy: String) -> &mut Self {
        let builder = std::mem::take(&mut self.builder);
        self.builder = builder.proxy(proxy);
        self
    }

    /// Set output encoding for Shell tasks (e.g. `GBK`, `Big5`, `auto`).
    /// Exposed as `withEncoding()` in PHP. The string is case-insensitive
    /// and forwarded to `encoding_rs::Encoding::for_label` at decode time.
    /// `auto` triggers OEM code page detection on Windows (no-op on Unix).
    pub fn with_encoding(&mut self, from: String) -> &mut Self {
        let builder = std::mem::take(&mut self.builder);
        self.builder = builder.encoding(from);
        self
    }

    /// Set the IANA timezone (e.g. `Asia/Shanghai`, `America/New_York`) used
    /// when evaluating the cron expression. Exposed as `withTimezone()` in PHP.
    /// When set, `next_fire` is computed in this timezone instead of the system
    /// local timezone. Invalid timezone strings cause `dispatch()` to fail.
    pub fn with_timezone(&mut self, tz: String) -> &mut Self {
        let builder = std::mem::take(&mut self.builder);
        self.builder = builder.timezone(tz);
        self
    }

    pub fn via_shell(&mut self, cmd: String) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).via_shell(cmd);
        self
    }

    pub fn with_retry(&mut self, max: i64, delay: i64) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).with_retry(max as u32, delay as u64);
        self
    }

    pub fn cron(&mut self, expr: String) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).cron(expr);
        self
    }

    pub fn timeout(&mut self, secs: i64) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).timeout(secs as u64);
        self
    }

    pub fn priority(&mut self, p: i64) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).priority(p as i32);
        self
    }

    pub fn allow_overlap(&mut self, allow: bool) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).allow_overlap(allow);
        self
    }

    pub fn max_instances(&mut self, n: i64) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).max_instances(n as u32);
        self
    }

    pub fn coalesce(&mut self, c: bool) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).coalesce(c);
        self
    }

    pub fn persist(&mut self, p: bool) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).persist(p);
        self
    }

    /// Set maximum executions for cron task (0 = unlimited).
    /// PHP: `maxExecutions(int $n): $this`
    pub fn max_executions(&mut self, n: i64) -> &mut Self {
        self.builder.max_executions = if n < 0 { 0 } else { n as u32 };
        self
    }

    /// Set start date (Unix ts). Cron triggers before this time are skipped.
    /// PHP: `startAt(int $ts): $this` (snake→camel auto-conversion)
    pub fn start_at(&mut self, ts: i64) -> &mut Self {
        self.builder.start_date = Some(ts);
        self
    }

    /// Set end date (Unix ts). After this time, task state becomes Success terminal.
    /// PHP: `endAt(int $ts): $this` (snake→camel auto-conversion)
    pub fn end_at(&mut self, ts: i64) -> &mut Self {
        self.builder.end_date = Some(ts);
        self
    }

    /// Set result TTL in seconds (0 = keep forever).
    /// PHP: `resultTtl(int $secs): $this` (snake→camel auto-conversion)
    pub fn result_ttl(&mut self, secs: i64) -> &mut Self {
        self.builder.result_ttl = if secs < 0 { 0 } else { secs as u64 };
        self
    }

    /// Attach user metadata (JSON string) to the task.
    /// PHP: `withMeta(string $json): $this` (snake→camel auto-conversion)
    pub fn with_meta(&mut self, json: String) -> &mut Self {
        self.builder.meta = Some(json);
        self
    }

    /// Set IntervalTrigger period in seconds (A7). The task fires every
    /// `secs` seconds. Mutually exclusive with `cron` and `runAt`; if both
    /// are set, `cron` / `runAt` take priority.
    /// PHP: `every(int $secs): $this`
    /// Reference: APScheduler IntervalTrigger.
    pub fn every(&mut self, secs: i64) -> &mut Self {
        self.builder.interval = Some(if secs < 0 { 0 } else { secs as u64 });
        self
    }

    /// Set DateTrigger absolute Unix timestamp (A8). The task fires once at
    /// the given timestamp, then immediately transitions to Success terminal
    /// state. Highest scheduling priority (overrides cron + interval).
    /// PHP: `runAt(int $ts): $this` (snake→camel auto-conversion)
    /// Reference: APScheduler DateTrigger.
    pub fn run_at(&mut self, ts: i64) -> &mut Self {
        self.builder.run_at = Some(ts);
        self
    }

    /// Set jitter (A9): random offset in seconds added to next_fire for cron
    /// / interval tasks to avoid thundering-herd effects. Default 0 = no
    /// jitter. Ignored for runAt tasks (precise one-shot timestamp).
    /// PHP: `jitter(int $secs): $this`
    /// Reference: APScheduler jitter.
    pub fn jitter(&mut self, secs: i64) -> &mut Self {
        self.builder.jitter = if secs < 0 { 0 } else { secs as u64 };
        self
    }

    /// Set task-level expires (C6): if a task remains Pending for longer than
    /// `secs` seconds (measured from `created_at`), it transitions to
    /// `Expired` terminal state. Default 0 = no expiry. Only affects Pending
    /// tasks; Running tasks are not interrupted.
    /// PHP: `expires(int $secs): $this`
    /// Reference: APScheduler expires.
    pub fn expires(&mut self, secs: i64) -> &mut Self {
        self.builder.expires = if secs < 0 { 0 } else { secs as u64 };
        self
    }

    /// Enable/disable retry exponential backoff (C8). When enabled, retry
    /// delays grow exponentially as
    /// `min(retry_delay * 2^(attempts-1), retry_delay * 60)`. When disabled
    /// (default), retry delays are fixed at `retry_delay` seconds.
    /// PHP: `retryBackoff(bool $on): $this` (snake→camel auto-conversion)
    /// Reference: Celery retry_backoff.
    pub fn retry_backoff(&mut self, on: bool) -> &mut Self {
        self.builder.retry_backoff = on;
        self
    }

    /// Enable fire-and-forget mode (C9): when true, the daemon skips
    /// `save_result` for this task so `xhjob_result()` will return null. The
    /// task state machine still runs (Pending → Running → Success/Failed).
    /// Useful for high-throughput tasks whose result is not needed by the
    /// caller. If both `ignoreResult(true)` and `resultTtl(>0)` are set, a
    /// warning is logged and `ignoreResult` takes priority.
    /// PHP: `ignoreResult(bool $on): $this` (snake→camel auto-conversion)
    /// Reference: Celery ignore_result.
    pub fn ignore_result(&mut self, on: bool) -> &mut Self {
        self.builder.ignore_result = on;
        self
    }

    /// Enable late acknowledgment (C10): when true, the task is "acked late"
    /// — on daemon restart, Running tasks with `acksLate=true` are
    /// automatically reset to Pending so they will be re-triggered (crash
    /// recovery semantics). When false (default), Running tasks on daemon
    /// restart stay Running (or, in persist mode, are unconditionally reset
    /// to Pending — `acksLate=true` is reserved for the future
    /// "task is idempotent and safe to re-run" opt-in flag).
    /// PHP: `acksLate(bool $on): $this` (snake→camel auto-conversion)
    /// Reference: Celery acks_late.
    pub fn acks_late(&mut self, on: bool) -> &mut Self {
        self.builder.acks_late = on;
        self
    }

    /// Set soft timeout (C11): graceful exit timeout in seconds. When set
    /// and strictly less than `timeout`, the shell executor sends SIGTERM
    /// at `soft_timeout` seconds; if the child does not exit within
    /// (timeout - soft_timeout) seconds, SIGKILL is sent. A value of 0
    /// clears the soft timeout (None). HTTP tasks ignore this field.
    /// PHP: `softTimeout(int $secs): $this` (snake→camel auto-conversion)
    /// Reference: Celery soft_time_limit.
    pub fn soft_timeout(&mut self, secs: i64) -> &mut Self {
        self.builder.soft_timeout = if secs <= 0 {
            None
        } else {
            Some(secs as u64)
        };
        self
    }

    /// Set per-job misfire_grace_time (A13) in seconds. 0 = use global
    /// default (60s). When `now - next_fire > grace_time`, the trigger is
    /// considered misfired; `coalesce=true` collapses missed triggers into
    /// one fire (still executes once), `coalesce=false` skips the trigger
    /// entirely. Only effective for cron tasks.
    /// PHP: `misfireGraceTime(int $secs): $this` (snake→camel auto-conversion)
    /// Reference: APScheduler misfire_grace_time.
    pub fn misfire_grace_time(&mut self, secs: i64) -> &mut Self {
        self.builder.misfire_grace_time = secs.max(0) as u64;
        self
    }

    /// Set an explicit task id (A14). When set, dispatch will use this id
    /// instead of auto-generating a UUID. If `replaceExisting` is true, an
    /// existing task with the same id is fully replaced.
    /// PHP: `withId(string $id): $this` (snake→camel auto-conversion)
    /// Reference: APScheduler id / replace_existing.
    pub fn id(&mut self, id: String) -> &mut Self {
        self.builder.id = if id.is_empty() { None } else { Some(id) };
        self
    }

    /// Enable replace_existing (A14): when true and `id` is set, dispatch
    /// replaces an existing task with the same id (full overwrite).
    /// PHP: `replaceExisting(bool $on): $this` (snake→camel auto-conversion)
    /// Reference: APScheduler replace_existing.
    pub fn replace_existing(&mut self, on: bool) -> &mut Self {
        self.builder.replace_existing = on;
        self
    }

    /// Add a tag (A15) to this task. Multiple tags can be added by chaining.
    /// Empty tags are silently ignored.
    /// PHP: `tag(string $tag): $this`.
    /// Reference: APScheduler tags.
    pub fn tag(&mut self, tag: String) -> &mut Self {
        if !tag.is_empty() && !self.builder.tags.iter().any(|t| t == &tag) {
            self.builder.tags.push(tag);
        }
        self
    }

    /// Set the rate limit (C12): max `count` triggers within `window`
    /// seconds. 0 count = no rate limiting.
    /// PHP: `rateLimit(int $count, int $window): $this` (snake→camel auto-conversion)
    /// Reference: Celery rate_limit.
    pub fn rate_limit(&mut self, count: i64, window: i64) -> &mut Self {
        self.builder.rate_limit_count = count.max(0) as u32;
        self.builder.rate_limit_window = window.max(0) as u64;
        self
    }

    /// Set acks_on_failure (C13): when true (default), task failures respect
    /// `retry_max`. When false, failures are retried indefinitely until the
    /// task succeeds or is cancelled/removed.
    /// PHP: `acksOnFailure(bool $on): $this` (snake→camel auto-conversion)
    /// Reference: Celery acks_on_failure.
    pub fn acks_on_failure(&mut self, on: bool) -> &mut Self {
        self.builder.acks_on_failure = on;
        self
    }

    pub fn dispatch(&mut self) -> String {
        let rt = match pool::coroutine_pool::global_runtime() {
            Some(rt) => rt,
            None => pool::coroutine_pool::init_global_runtime(),
        };
        let builder = std::mem::take(&mut self.builder);
        let result = rt.block_on(async move {
            builder.dispatch().await
        });
        match result {
            Ok(id) => id,
            Err(e) => format!("error: {}", e),
        }
    }
}

/// List task events since `since_ts` (Unix seconds), optionally filtered by
/// `task_id` (A17). Returns a JSON array of `{task_id, event_type, payload,
/// ts}` objects, or `"error: ..."` on failure.
///
/// Reference: APScheduler EVENT_JOB_*.
/// PHP: `xhjob_events(int $since_ts, ?string $task_id = null, string $name = "default", string $data_dir = null): string`
#[php_function]
pub fn xhjob_events(
    since_ts: i64,
    task_id: Option<String>,
    name: Option<String>,
    data_dir: Option<String>,
) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return format!("error: {}", e),
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({
            "since_ts": since_ts,
            "task_id": task_id,
        });
        match ipc::request("events", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return format!("error: {}", resp.err.unwrap_or_else(|| "unknown".to_string()));
                }
                resp.data.get("events")
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "[]".to_string())
            }
            Err(e) => format!("error: {}", e),
        }
    })
}

/// Report task progress (percent + optional meta JSON). The percent must be
/// in the range 0-100; out-of-range values return `false` without contacting
/// the daemon. Reference: Celery update_state(state='PROGRESS', meta=...).
///
/// PHP: `xhjob_report_progress(string $id, int $percent, ?string $meta_json = null, ?string $name = "default", ?string $data_dir = null): bool`
#[php_function]
pub fn xhjob_report_progress(
    id: String,
    percent: i64,
    meta_json: Option<String>,
    name: Option<String>,
    data_dir: Option<String>,
) -> bool {
    if percent < 0 || percent > 100 {
        return false;
    }
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_report_progress invalid service name: {}", e);
            return false;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({
            "id": id,
            "percent": percent,
            "meta": meta_json,
        });
        match ipc::request("report_progress", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => resp.ok,
            Err(e) => {
                tracing::error!("xhjob_report_progress ipc: {}", e);
                false
            }
        }
    })
}

/// Pull task events since `since_ts` (Unix seconds), optionally filtered by
/// `event_type` (A17). Returns a JSON array of `{task_id, event_type, payload,
/// ts}` objects, or `"error: ..."` on failure.
///
/// Reference: APScheduler EVENT_JOB_*.
/// PHP: `xhjob_pull_events(int $since_ts, ?string $event_type = null, ?string $name = "default", ?string $data_dir = null): string`
#[php_function]
pub fn xhjob_pull_events(
    since_ts: i64,
    event_type: Option<String>,
    name: Option<String>,
    data_dir: Option<String>,
) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return format!("error: {}", e),
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({
            "since_ts": since_ts,
            "event_type": event_type,
        });
        match ipc::request("pull_events", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return format!("error: {}", resp.err.unwrap_or_else(|| "unknown".to_string()));
                }
                resp.data.get("events")
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "[]".to_string())
            }
            Err(e) => format!("error: {}", e),
        }
    })
}

/// Inspect daemon state. `mode` selects the query:
///   - `"active"`     — currently running tasks
///   - `"registered"` — cron / interval tasks
///   - `"scheduled"`  — tasks with a future `next_fire`
///   - `"stats"`      — aggregate WorkerStats (default)
///
/// Returns a JSON string (array for active/registered/scheduled, object for
/// stats), or `"error: ..."` on failure.
///
/// Reference: Celery inspect active / registered / scheduled / stats.
/// PHP: `xhjob_inspect(string $mode, ?string $name = "default", ?string $data_dir = null): string`
#[php_function]
pub fn xhjob_inspect(
    mode: String,
    name: Option<String>,
    data_dir: Option<String>,
) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return format!("error: {}", e),
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "mode": mode });
        match ipc::request("inspect", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return format!("error: {}", resp.err.unwrap_or_else(|| "unknown".to_string()));
                }
                resp.data.get("data")
                    .map(|t| t.to_string())
                    .unwrap_or_else(|| "null".to_string())
            }
            Err(e) => format!("error: {}", e),
        }
    })
}

/// Create a chain of tasks (C15). Sequential pipeline: each task's stdout is
/// fed into the next task's input. The chain record is persisted with the
/// ordered list of task configs and the daemon dispatches them one at a
/// time, advancing only after the previous step succeeds. On any step
/// failure the chain state becomes "failed" and remaining steps are
/// skipped.
///
/// `tasks_json` is a JSON array of TaskBuilder config objects, e.g.
/// `[{"type":"shell","cmd":"echo a"},{"type":"shell","cmd":"grep a"}]`.
///
/// Returns the chain_id on success, or `"error: ..."` on failure.
///
/// Reference: Celery `chain(t1, t2, t3)`.
/// PHP: `xhjob_chain(string $tasks_json, string $name = "default", string $data_dir = null): string`
#[php_function]
pub fn xhjob_chain(tasks_json: String, name: Option<String>, data_dir: Option<String>) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return format!("error: {}", e),
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let tasks: serde_json::Value = match serde_json::from_str(&tasks_json) {
            Ok(v) => v,
            Err(e) => return format!("error: invalid json: {}", e),
        };
        let payload = serde_json::json!({ "tasks": tasks });
        match ipc::request("chain", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return format!("error: {}", resp.err.unwrap_or_else(|| "unknown".to_string()));
                }
                resp.data.get("chain_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "error: missing chain_id".to_string())
            }
            Err(e) => format!("error: {}", e),
        }
    })
}

/// Get the chain state (C15). Returns the full ChainRecord JSON
/// (`{chain_id, tasks, current_step, state, created_at, updated_at}`)
/// or `null` if the chain does not exist / daemon unreachable.
///
/// Reference: Celery chain inspection.
/// PHP: `xhjob_chain_state(string $chain_id, string $name = "default", string $data_dir = null): ?string`
#[php_function]
pub fn xhjob_chain_state(chain_id: String, name: Option<String>, data_dir: Option<String>) -> Option<String> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_chain_state invalid service name: {}", e);
            return None;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "chain_id": chain_id });
        match ipc::request("chain_state", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return None;
                }
                let ok = resp.data.get("ok")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !ok {
                    return None;
                }
                resp.data.get("data").map(|d| d.to_string())
            }
            Err(e) => {
                tracing::error!("xhjob_chain_state ipc: {}", e);
                None
            }
        }
    })
}

/// Create a group of tasks (C16). Parallel batch: all tasks are dispatched
/// concurrently. As each task completes the daemon updates the group
/// state. Final group state is "success" (all ok) / "partial_failed"
/// (some failed) / "failed" (all failed).
///
/// `tasks_json` is a JSON array of TaskBuilder config objects.
///
/// Returns the group_id on success, or `"error: ..."` on failure.
///
/// Reference: Celery `group(t1, t2, t3)`.
/// PHP: `xhjob_group(string $tasks_json, string $name = "default", string $data_dir = null): string`
#[php_function]
pub fn xhjob_group(tasks_json: String, name: Option<String>, data_dir: Option<String>) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return format!("error: {}", e),
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let tasks: serde_json::Value = match serde_json::from_str(&tasks_json) {
            Ok(v) => v,
            Err(e) => return format!("error: invalid json: {}", e),
        };
        let payload = serde_json::json!({ "tasks": tasks });
        match ipc::request("group", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return format!("error: {}", resp.err.unwrap_or_else(|| "unknown".to_string()));
                }
                resp.data.get("group_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "error: missing group_id".to_string())
            }
            Err(e) => format!("error: {}", e),
        }
    })
}

/// Get the group state (C16). Returns the full GroupRecord JSON
/// (`{group_id, tasks, state, created_at, updated_at}`) plus a live
/// `summary` field `{total, succeeded, failed, pending}`, or `null` if
/// the group does not exist / daemon unreachable.
///
/// Reference: Celery group inspection.
/// PHP: `xhjob_group_state(string $group_id, string $name = "default", string $data_dir = null): ?string`
#[php_function]
pub fn xhjob_group_state(group_id: String, name: Option<String>, data_dir: Option<String>) -> Option<String> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_group_state invalid service name: {}", e);
            return None;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "group_id": group_id });
        match ipc::request("group_state", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return None;
                }
                let ok = resp.data.get("ok")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !ok {
                    return None;
                }
                resp.data.get("data").map(|d| d.to_string())
            }
            Err(e) => {
                tracing::error!("xhjob_group_state ipc: {}", e);
                None
            }
        }
    })
}

/// Create a chord (C16+). A chord = header (parallel tasks) + body
/// (callback). All header tasks are dispatched concurrently. When every
/// header task succeeds, the body is dispatched with its `meta` set to a
/// JSON array of `{ id, result }` objects carrying each header task's
/// result. If any header task fails, the chord flips to `partial_failed`
/// and the body is NOT dispatched.
///
/// `header_json` is a JSON array of TaskBuilder config objects.
/// `callback_json` is a single TaskBuilder config object (the body).
///
/// Returns the chord_id on success, or `"error: ..."` on failure.
///
/// Reference: Celery `chord(header, body)`.
/// PHP: `xhjob_chord(string $header_json, string $callback_json, string $name = "default", string $data_dir = null): string`
#[php_function]
pub fn xhjob_chord(header_json: String, callback_json: String, name: Option<String>, data_dir: Option<String>) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return format!("error: {}", e),
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let header: serde_json::Value = match serde_json::from_str(&header_json) {
            Ok(v) => v,
            Err(e) => return format!("error: invalid header json: {}", e),
        };
        let callback: serde_json::Value = match serde_json::from_str(&callback_json) {
            Ok(v) => v,
            Err(e) => return format!("error: invalid callback json: {}", e),
        };
        let payload = serde_json::json!({ "header": header, "callback": callback });
        match ipc::request("chord", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return format!("error: {}", resp.err.unwrap_or_else(|| "unknown".to_string()));
                }
                resp.data.get("chord_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "error: missing chord_id".to_string())
            }
            Err(e) => format!("error: {}", e),
        }
    })
}

/// Get the chord state (C16+). Returns the full ChordRecord JSON
/// (`{id, header_task_ids, callback_json, callback_task_id, state,
/// created_at, updated_at}`) or `null` if the chord does not exist /
/// daemon unreachable.
///
/// Reference: Celery chord inspection.
/// PHP: `xhjob_chord_state(string $chord_id, string $name = "default", string $data_dir = null): ?string`
#[php_function]
pub fn xhjob_chord_state(chord_id: String, name: Option<String>, data_dir: Option<String>) -> Option<String> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_chord_state invalid service name: {}", e);
            return None;
        }
    };
    let data_dir = normalize_data_dir(data_dir);
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    rt.block_on(async move {
        let payload = serde_json::json!({ "chord_id": chord_id });
        match ipc::request("chord_state", payload, &service_name, data_dir.as_deref()).await {
            Ok(resp) => {
                if !resp.ok {
                    return None;
                }
                let ok = resp.data.get("ok")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !ok {
                    return None;
                }
                resp.data.get("data").map(|d| d.to_string())
            }
            Err(e) => {
                tracing::error!("xhjob_chord_state ipc: {}", e);
                None
            }
        }
    })
}

// =========================================================================
// Module entry
// =========================================================================

#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module
        .class::<Xhjob>()
        .function(wrap_function!(xhjob_start))
        .function(wrap_function!(xhjob_stop))
        .function(wrap_function!(xhjob_restart))
        .function(wrap_function!(xhjob_status))
        .function(wrap_function!(xhjob_dispatch))
        .function(wrap_function!(xhjob_state))
        .function(wrap_function!(xhjob_result))
        .function(wrap_function!(xhjob_remove))
        .function(wrap_function!(xhjob_pause))
        .function(wrap_function!(xhjob_resume))
        .function(wrap_function!(xhjob_cancel))
        .function(wrap_function!(xhjob_list))
        .function(wrap_function!(xhjob_requeue))
        .function(wrap_function!(xhjob_reschedule))
        .function(wrap_function!(xhjob_get))
        .function(wrap_function!(xhjob_run_daemon))
        .function(wrap_function!(xhjob_events))
        .function(wrap_function!(xhjob_chain))
        .function(wrap_function!(xhjob_chain_state))
        .function(wrap_function!(xhjob_group))
        .function(wrap_function!(xhjob_group_state))
        .function(wrap_function!(xhjob_chord))
        .function(wrap_function!(xhjob_chord_state))
        .function(wrap_function!(xhjob_report_progress))
        .function(wrap_function!(xhjob_pull_events))
        .function(wrap_function!(xhjob_inspect))
}
