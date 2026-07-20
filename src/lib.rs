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
<<<<<<< Updated upstream
pub mod store;
pub mod task;

=======
pub mod service;
pub mod store;
pub mod task;

pub use service::ServiceName;

>>>>>>> Stashed changes
// =========================================================================
// PHP functions
// =========================================================================

<<<<<<< Updated upstream
#[php_function]
pub fn xhjob_start() -> bool {
    // Check if daemon is already running
    let status = daemon::status();
=======
/// Resolve and validate a PHP-supplied service name. `None` falls back to the
/// default service. Returns the validated name as a `String`.
fn resolve_service_name(name: Option<String>) -> Result<String, String> {
    let raw = name.unwrap_or_else(|| service::default_name().to_string());
    service::validate(&raw).map_err(|e| format!("{}", e))
}

#[php_function]
pub fn xhjob_start(name: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_start invalid service name: {}", e);
            return false;
        }
    };
    // Check if daemon is already running
    let status = daemon::status(&service_name);
>>>>>>> Stashed changes
    if status.running {
        return true;
    }
    // Spawn daemon. daemon_main is the function the daemon will run.
<<<<<<< Updated upstream
    match daemon::start(daemon_main::daemon_main) {
=======
    match daemon::start(daemon_main::daemon_main, &service_name) {
>>>>>>> Stashed changes
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
<<<<<<< Updated upstream
pub fn xhjob_stop() -> bool {
    match daemon::stop() {
=======
pub fn xhjob_stop(name: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_stop invalid service name: {}", e);
            return false;
        }
    };
    match daemon::stop(&service_name) {
>>>>>>> Stashed changes
        Ok(_) => true,
        Err(e) => {
            tracing::error!("xhjob_stop failed: {}", e);
            false
        }
    }
}

#[php_function]
<<<<<<< Updated upstream
pub fn xhjob_restart() -> bool {
    match daemon::restart(daemon_main::daemon_main) {
=======
pub fn xhjob_restart(name: Option<String>) -> bool {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("xhjob_restart invalid service name: {}", e);
            return false;
        }
    };
    match daemon::restart(daemon_main::daemon_main, &service_name) {
>>>>>>> Stashed changes
        Ok(_) => true,
        Err(e) => {
            tracing::error!("xhjob_restart failed: {}", e);
            false
        }
    }
}

#[php_function]
<<<<<<< Updated upstream
pub fn xhjob_status() -> Vec<(String, String)> {
    let status = daemon::status();
=======
pub fn xhjob_status(name: Option<String>) -> Vec<(String, String)> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            let mut out: Vec<(String, String)> = Vec::new();
            out.push(("running".to_string(), "false".to_string()));
            out.push(("error".to_string(), e));
            return out;
        }
    };
    let status = daemon::status(&service_name);
>>>>>>> Stashed changes
    let mut out: Vec<(String, String)> = Vec::new();
    out.push(("running".to_string(), status.running.to_string()));
    if let Some(pid) = status.pid {
        out.push(("pid".to_string(), pid.to_string()));
    }
    out
}

#[php_function]
<<<<<<< Updated upstream
pub fn xhjob_dispatch(task_json: String) -> String {
=======
pub fn xhjob_dispatch(task_json: String, name: Option<String>) -> String {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => return e,
    };
>>>>>>> Stashed changes
    // Build a one-shot request to the daemon and return the task_id (or error string).
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    let result: std::result::Result<String, String> = rt.block_on(async move {
        let payload: serde_json::Value = serde_json::from_str(&task_json)
            .map_err(|e| format!("invalid json: {}", e))?;
<<<<<<< Updated upstream
        let resp = ipc::request("dispatch", payload).await
=======
        let resp = ipc::request("dispatch", payload, &service_name).await
>>>>>>> Stashed changes
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
        Err(e) => e,
    }
}

#[php_function]
<<<<<<< Updated upstream
pub fn xhjob_state(id: String) -> Vec<(String, String)> {
=======
pub fn xhjob_state(id: String, name: Option<String>) -> Vec<(String, String)> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            let mut out: Vec<(String, String)> = Vec::new();
            out.push(("state".to_string(), "UNKNOWN".to_string()));
            out.push(("error".to_string(), e));
            return out;
        }
    };
>>>>>>> Stashed changes
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    let info = rt.block_on(async move {
<<<<<<< Updated upstream
        match outcome::query_state(&id).await {
=======
        match outcome::query_state(&id, &service_name).await {
>>>>>>> Stashed changes
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
    } else {
        out.push(("state".to_string(), "UNKNOWN".to_string()));
        out.push(("error".to_string(), "task not found or daemon not running".to_string()));
    }
    out
}

#[php_function]
<<<<<<< Updated upstream
pub fn xhjob_result(id: String) -> Vec<(String, String)> {
=======
pub fn xhjob_result(id: String, name: Option<String>) -> Vec<(String, String)> {
    let service_name = match resolve_service_name(name) {
        Ok(s) => s,
        Err(e) => {
            let mut out: Vec<(String, String)> = Vec::new();
            out.push(("error".to_string(), e));
            return out;
        }
    };
>>>>>>> Stashed changes
    let rt = match pool::coroutine_pool::global_runtime() {
        Some(rt) => rt,
        None => pool::coroutine_pool::init_global_runtime(),
    };
    let result = rt.block_on(async move {
<<<<<<< Updated upstream
        match outcome::query_result(&id).await {
=======
        match outcome::query_result(&id, &service_name).await {
>>>>>>> Stashed changes
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
        out.push(("error".to_string(), "result not found or daemon not running".to_string()));
    }
    out
}

/// Hidden entry point invoked when the PHP binary is re-executed by
/// `spawn_via_double_fork` with `XHJOB_DAEMON_MODE=1`. Runs the daemon loop
/// in the current process and never returns.
#[php_function]
pub fn xhjob_run_daemon() -> bool {
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
<<<<<<< Updated upstream
    let log = daemon::log_file_path();
=======
    let service_name = crate::service::current();
    let log = daemon::log_file_path(&service_name);
>>>>>>> Stashed changes
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

<<<<<<< Updated upstream
=======
    /// Bind this Xhjob instance to a named service. Subsequent `dispatch()`
    /// will route to the daemon for that service. Returns `&mut self` for
    /// chaining. Exposed as `service()` in PHP.
    pub fn service(&mut self, name: String) -> &mut Self {
        self.builder = std::mem::take(&mut self.builder).service(name);
        self
    }

>>>>>>> Stashed changes
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

<<<<<<< Updated upstream
=======
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

>>>>>>> Stashed changes
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
        .function(wrap_function!(xhjob_run_daemon))
}
