//! Task model + Builder pattern chainable API.

use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::errors::{Result, XhjobError};
use crate::store::{Task, TaskType, HttpPayload, ShellPayload};
use crate::ipc::{Request, request as ipc_request};
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
}

fn default_service_name() -> String {
    "default".to_string()
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
            proxy: None,
            encoding: None,
            timezone: None,
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

    /// Set max concurrent instances (only meaningful when allow_overlap = true).
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
        // If cron, compute initial next_fire
        if let Some(expr) = &task.cron {
            match crate::scheduler::cron::next_fire(expr, false, now_ts(), task.timezone.as_deref()) {
                Ok(t) => task.next_fire = Some(t),
                Err(e) => {
                    tracing::warn!(cron = %expr, error = %e, "failed to compute initial next_fire");
                }
            }
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
    /// Routes to the daemon for `self.service_name`.
    pub async fn dispatch(self) -> Result<String> {
        // Validate timezone up front: fail fast on bad input rather than
        // enqueuing an IPC request that the daemon cannot honor correctly.
        if let Some(tz) = &self.timezone {
            crate::scheduler::cron::validate_timezone(tz)?;
        }
        let json = serde_json::to_value(&self)
            .map_err(|e| XhjobError::InvalidTask(format!("serialize: {}", e)))?;
        let resp = ipc_request("dispatch", json, &self.service_name).await?;
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
