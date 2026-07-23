//! Cross-platform shell executor.
//!
//! Unix: runs `bash -c "<cmd>"` via tokio::process::Command.
//! Windows: runs `cmd /C "<cmd>"` via tokio::process::Command.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use crate::errors::{Result, XhjobError};
use crate::store::{Task, TaskResult, ShellPayload};
use super::Executor;

pub struct ShellExecutor;

impl ShellExecutor {
    pub fn new() -> Self { Self }
}

impl Default for ShellExecutor {
    fn default() -> Self { Self::new() }
}

impl Executor for ShellExecutor {
    fn execute<'a>(&'a self, task: &'a Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>> {
        // 委托给 execute_with_cancel，不传 cancel 标志（向后兼容）。
        self.execute_with_cancel(task, None)
    }

    fn execute_with_cancel<'a>(&'a self, task: &'a Task, cancel_flag: Option<Arc<AtomicBool>>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>> {
        let payload_val = task.payload.clone();
        let timeout = task.timeout;
        let encoding = task.encoding.clone();
        let soft_timeout = task.soft_timeout;
        Box::pin(async move {
            let payload: ShellPayload = serde_json::from_value(payload_val)
                .map_err(|e| XhjobError::exec(format!("invalid shell payload: {}", e)))?;

            let mut cmd = build_command(&payload);

            // stdin: pipe only when the payload supplies input bytes,
            // otherwise /dev/null (the historical default).
            let stdin_bytes = payload.stdin.as_ref().map(|s| s.as_bytes().to_vec());
            let stdin_cfg = if stdin_bytes.is_some() {
                std::process::Stdio::piped()
            } else {
                std::process::Stdio::null()
            };

            // Spawn the child process
            let mut child = cmd.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(stdin_cfg)
                .spawn()
                .map_err(|e| XhjobError::exec(format!("spawn: {}", e)))?;

            // Feed stdin to the child in its own task so it does not block
            // the stdout/stderr drain tasks or the wait future. Best-effort:
            // if the child exits before reading all stdin, the write may
            // fail and we simply ignore the error. Dropping the handle
            // closes the pipe, signalling EOF to the child.
            if let (Some(bytes), Some(mut child_stdin)) = (stdin_bytes, child.stdin.take()) {
                tokio::spawn(async move {
                    use tokio::io::AsyncWriteExt;
                    let _ = child_stdin.write_all(&bytes).await;
                    drop(child_stdin);
                });
            }

            // Wait with timeout
            let stdout_fut = child.stdout.take();
            let stderr_fut = child.stderr.take();

            // softTimeout (C11): when set and strictly less than `timeout`,
            // send SIGTERM at `soft_timeout` seconds. If the child does not
            // exit within (timeout - soft_timeout) seconds after SIGTERM,
            // send SIGKILL. When None or >= timeout, fall back to the
            // existing hard-kill-at-timeout behavior.
            // Reference: Celery soft_time_limit.
            let soft = soft_timeout.filter(|&s| s > 0 && s < timeout);

            // 主等待逻辑封装为 async 块，便于与 cancel 检查并发执行。
            // 返回 Result<ExitStatus, XhjobError>，错误已包含 timeout / wait
            // 失败的语义，且内部完成 SIGTERM / SIGKILL 与 reap。
            let wait_fut = async {
                if let Some(soft_secs) = soft {
                    let grace = timeout - soft_secs;
                    // Phase 1: wait soft_secs for graceful completion (no signal yet).
                    match tokio::time::timeout(Duration::from_secs(soft_secs), child.wait()).await {
                        Ok(s) => s.map_err(|e| XhjobError::exec(format!("wait: {}", e))),
                        Err(_) => {
                            // soft_timeout elapsed — send SIGTERM to the child.
                            // P1: target the whole process group (negative pid)
                            // so grandchildren spawned by the shell are also
                            // signaled. process_group(0) in build_command
                            // made the child the group leader, so -pid == pgid.
                            #[cfg(unix)]
                            if let Some(pid) = child.id() {
                                use nix::sys::signal::{kill, Signal};
                                use nix::unistd::Pid;
                                let _ = kill(Pid::from_raw(-(pid as i32)), Signal::SIGTERM);
                            }
                            // Phase 2: wait grace period for graceful exit after SIGTERM.
                            match tokio::time::timeout(Duration::from_secs(grace), child.wait()).await {
                                Ok(s) => s.map_err(|e| XhjobError::exec(format!("wait: {}", e))),
                                Err(_) => {
                                    // SIGKILL after grace period — reap the child
                                    // so it doesn't become a zombie.
                                    let _ = child.start_kill();
                                    let _ = child.wait().await;
                                    Err(XhjobError::exec(format!(
                                        "timeout after {}s (soft={}s, grace={}s, SIGKILL)",
                                        timeout, soft_secs, grace
                                    )))
                                }
                            }
                        }
                    }
                } else {
                    // No soft_timeout: existing logic, just wait timeout then SIGKILL.
                    match tokio::time::timeout(Duration::from_secs(timeout), child.wait()).await {
                        Ok(s) => s.map_err(|e| XhjobError::exec(format!("wait: {}", e))),
                        Err(_) => {
                            // timeout: kill the child and reap.
                            let _ = child.start_kill();
                            let _ = child.wait().await;
                            Err(XhjobError::exec(format!("timeout after {}s", timeout)))
                        }
                    }
                }
            };

            // P0 fix: drain stdout/stderr CONCURRENTLY with the wait, so that
            // a high-volume child cannot deadlock on a full OS pipe buffer.
            // Previously the code did `child.wait().await` first and only
            // then read the pipes — if the child produced more than ~64KB
            // of output the pipe buffer filled, the child blocked on write,
            // and `wait()` never returned (classic pipe deadlock).
            //
            // We spawn two read tasks and race the wait against the cancel
            // watcher; on completion/cancel we collect whatever was captured.
            // P1 fix: cap captured stdout/stderr at MAX_OUTPUT_BYTES so a
            // runaway child cannot exhaust daemon memory by streaming GBs.
            // We use `take(MAX)` so reads stop at the cap; the remainder is
            // simply discarded (the child may then block on a full pipe,
            // which is fine — we already have enough to surface a result and
            // the wait/timeout path will reap it).
            const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024; // 64 MiB per stream
            let stdout_task = tokio::spawn(async move {
                if let Some(s) = stdout_fut {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let mut limited = s.take(MAX_OUTPUT_BYTES as u64);
                    let _ = limited.read_to_end(&mut buf).await;
                    Some(buf)
                } else {
                    None
                }
            });
            let stderr_task = tokio::spawn(async move {
                if let Some(s) = stderr_fut {
                    use tokio::io::AsyncReadExt;
                    let mut buf = Vec::new();
                    let mut limited = s.take(MAX_OUTPUT_BYTES as u64);
                    let _ = limited.read_to_end(&mut buf).await;
                    Some(buf)
                } else {
                    None
                }
            });

            // 与 cancel 检查循环并发：当提供 cancel_flag 时，每 200ms 检查一次；
            // 检测到取消则终止子进程并返回 cancelled 错误。
            // Reference: Celery revoke (terminate=true).
            let status_result: std::result::Result<std::process::ExitStatus, XhjobError> = if let Some(flag) = cancel_flag.as_ref() {
                let flag = Arc::clone(flag);
                let cancel_watcher = async move {
                    loop {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        if flag.load(Ordering::SeqCst) {
                            return;
                        }
                    }
                };
                tokio::select! {
                    r = wait_fut => r,
                    _ = cancel_watcher => {
                        // cancel 触发：发送 SIGTERM，给 500ms grace 等待优雅退出；
                        // 超时则 SIGKILL 并 reap，避免僵尸进程。
                        // P1: signal the whole process group (negative pid) so
                        // grandchildren are also terminated.
                        #[cfg(unix)]
                        if let Some(pid) = child.id() {
                            use nix::sys::signal::{kill, Signal};
                            use nix::unistd::Pid;
                            let _ = kill(Pid::from_raw(-(pid as i32)), Signal::SIGTERM);
                        }
                        #[cfg(not(unix))]
                        { let _ = child.start_kill(); }
                        match tokio::time::timeout(Duration::from_millis(500), child.wait()).await {
                            Ok(_) => {}
                            Err(_) => {
                                let _ = child.start_kill();
                                let _ = child.wait().await;
                            }
                        }
                        Err(XhjobError::exec("cancelled".to_string()))
                    }
                }
            } else {
                wait_fut.await
            };

            // Collect whatever the read tasks captured (they may still be
            // running if the child was killed before EOF; await them with a
            // short grace so we don't block forever on a closed pipe).
            //
            // P1 fix: previously a drain timeout returned Err unconditionally
            // via `?`, which marked an exit-0 success task as failed when a
            // grandchild process (e.g. `nohup x &`, daemonizing scripts)
            // inherited the pipe and kept it open past the 500ms grace. Now
            // we treat drain timeout/join errors as non-fatal when the child
            // already exited successfully — we just take whatever partial
            // output was captured. On the error path (status_result is Err)
            // we already tolerate partial output below.
            let stdout_buf = match tokio::time::timeout(
                Duration::from_millis(500),
                stdout_task,
            ).await {
                Ok(Ok(Some(buf))) => buf,
                _ => Vec::new(), // timeout, join error, or None: no captured output
            };
            let stderr_buf = match tokio::time::timeout(
                Duration::from_millis(500),
                stderr_task,
            ).await {
                Ok(Ok(Some(buf))) => buf,
                _ => Vec::new(),
            };
            if status_result.is_ok() {
                // Log when we dropped output due to drain timeout on a
                // successful task, so operators can diagnose missing stdout.
                if stdout_buf.is_empty() || stderr_buf.is_empty() {
                    tracing::debug!(
                        stdout_len = stdout_buf.len(),
                        stderr_len = stderr_buf.len(),
                        "successful task had partial/empty drained output (grandchild may hold the pipe)"
                    );
                }
            }

            let (stdout_text, stderr_text, exit_code) = match status_result {
                Ok(status) => {
                    let stdout_text = decode_output(&stdout_buf, encoding.as_ref())?;
                    let stderr_text = decode_output(&stderr_buf, encoding.as_ref())?;
                    let code = status.code().unwrap_or(-1);
                    (stdout_text, stderr_text, code)
                }
                Err(e) => {
                    // MEDIUM fix: on timeout / cancel / soft_timeout kill, the
                    // child may have already produced partial stdout/stderr
                    // before being killed. Decode and log them so operators
                    // can diagnose why the task timed out (instead of the
                    // previous behavior of silently dropping all captured
                    // output). Best-effort: ignore decode errors here since
                    // we're already in an error path.
                    if !stdout_buf.is_empty() || !stderr_buf.is_empty() {
                        let stdout_partial = decode_output(&stdout_buf, encoding.as_ref())
                            .unwrap_or_else(|_| String::from_utf8_lossy(&stdout_buf).to_string());
                        let stderr_partial = decode_output(&stderr_buf, encoding.as_ref())
                            .unwrap_or_else(|_| String::from_utf8_lossy(&stderr_buf).to_string());
                        tracing::warn!(
                            error = %e,
                            stdout_len = stdout_partial.len(),
                            stderr_len = stderr_partial.len(),
                            "task ended with error; partial output captured (logged for diagnostics)"
                        );
                        if !stdout_partial.is_empty() {
                            tracing::info!(target: "xhjob_shell_partial", "PARTIAL STDOUT:\n{}", stdout_partial);
                        }
                        if !stderr_partial.is_empty() {
                            tracing::info!(target: "xhjob_shell_partial", "PARTIAL STDERR:\n{}", stderr_partial);
                        }
                    }
                    return Err(e);
                }
            };

            Ok(TaskResult {
                body: None,
                body_b64: None,
                status_code: None,
                stdout: Some(stdout_text),
                stderr: Some(stderr_text),
                exit_code: Some(exit_code),
            })
        })
    }
}

/// Decode captured shell output bytes into a UTF-8 `String`.
///
/// If `encoding` is `None`, falls back to the historical lossy UTF-8 behavior.
/// Otherwise delegates to [`decode_bytes`] which honors the special `"auto"`
/// label and any encoding label accepted by `encoding_rs`.
fn decode_output(bytes: &[u8], encoding: Option<&String>) -> Result<String> {
    match encoding {
        Some(enc) => decode_bytes(bytes, enc),
        None => Ok(String::from_utf8_lossy(bytes).to_string()),
    }
}

/// Decode `bytes` from the requested encoding into a UTF-8 `String`.
///
/// `encoding` is matched case-insensitively. Special values:
/// - `"auto"`: invokes [`detect_encoding`] to pick an encoding (OEM code page
///   on Windows, UTF-8 on Unix) and then recursively decodes with that label.
/// - `"utf-8"` / `"utf8"`: lossy UTF-8 passthrough (same as the None path).
///
/// Any other value is forwarded to `encoding_rs::Encoding::for_label`. Unknown
/// labels produce `XhjobError::exec("unsupported encoding: ...")`.
pub(crate) fn decode_bytes(bytes: &[u8], encoding: &str) -> Result<String> {
    let enc_lower = encoding.to_lowercase();
    if enc_lower == "auto" {
        let detected = detect_encoding();
        return decode_bytes(bytes, detected);
    }
    if enc_lower == "utf-8" || enc_lower == "utf8" {
        return Ok(String::from_utf8_lossy(bytes).to_string());
    }
    let enc = encoding_rs::Encoding::for_label(encoding.as_bytes())
        .ok_or_else(|| XhjobError::exec(format!("unsupported encoding: {}", encoding)))?;
    let (text, _encoding_used) = enc.decode_without_bom_handling(bytes);
    Ok(text.to_string())
}

/// Best-effort auto-detection of the platform's preferred shell output encoding.
///
/// Windows: queries the OEM code page via `GetOEMCP` and maps the common
/// East Asian + ANSI code pages to labels understood by `encoding_rs`.
/// Falls back to `"UTF-8"` for unknown code pages.
///
/// Unix: always returns `"UTF-8"` since shells are conventionally UTF-8.
fn detect_encoding() -> &'static str {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::WindowsProgramming::GetOEMCP;
        let cp = unsafe { GetOEMCP() };
        match cp {
            936 => "GBK",
            950 => "Big5",
            932 => "Shift_JIS",
            949 => "EUC-KR",
            874 => "windows-874",
            1250 => "windows-1250",
            1251 => "windows-1251",
            1252 => "windows-1252",
            1253 => "windows-1253",
            1254 => "windows-1254",
            1255 => "windows-1255",
            1256 => "windows-1256",
            1257 => "windows-1257",
            1258 => "windows-1258",
            _ => "UTF-8",
        }
    }
    #[cfg(not(windows))]
    {
        "UTF-8"
    }
}

/// Build the platform-specific command.
///
/// P1 hardening (all applied here so every spawn path benefits):
/// - `kill_on_drop(true)`: if the `Child` handle is dropped without an
///   explicit `kill()` (e.g. panic, early return, daemon SIGKILL), tokio
///   sends SIGKILL to the child so it cannot be orphaned. Without this,
///   a daemon killed mid-task leaves `bash` + its grandchildren running.
/// - `process_group(0)` (Unix): puts the child in its own process group
///   (pgid == child pid), so a `kill(-pgid, SIGTERM)` reaches the whole
///   tree (grandchildren spawned by the shell included), not just the
///   top-level `bash`. Combined with the negative-pid kills in
///   `execute_with_cancel`, this prevents grandchildren from surviving
///   a timeout/cancel.
/// - `env_clear()` + minimal env: starts from a known-clean environment
///   (no leakage of daemon's PATH / HOME / secrets into user tasks) and
///   re-adds only `PATH` and `HOME`. This is defence-in-depth against
///   tasks that happen to share the daemon's uid.
fn build_command(payload: &ShellPayload) -> tokio::process::Command {
    #[cfg(unix)]
    {
        let mut c = tokio::process::Command::new("bash");
        c.arg("-c").arg(&payload.cmd);
        // Put the child in its own process group so we can signal the
        // whole tree on timeout/cancel. (tokio re-exports this from
        // std::os::unix::process::CommandExt via its Command type.)
        c.process_group(0);
        // kill_on_drop: orphan-safety net for panic / early-return paths.
        c.kill_on_drop(true);
        // env_clear + minimal env: avoid leaking daemon env into user tasks.
        // L3 fix: re-inject XHJOB_OWNER so shell tasks can perform owner-
        // scoped actions (logging, audit). Other XHJOB_* daemon-internal
        // vars (XHJOB_DAEMON_MODE, XHJOB_DATA_DIR, etc.) are intentionally
        // NOT passed — they are daemon bookkeeping, not task context.
        c.env_clear();
        if let Ok(path) = std::env::var("PATH") {
            c.env("PATH", path);
        } else {
            c.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                c.env("HOME", home);
            }
        }
        if let Ok(owner) = std::env::var("XHJOB_OWNER") {
            if !owner.is_empty() {
                c.env("XHJOB_OWNER", owner);
            }
        }
        // User-supplied env vars are injected last so they take precedence
        // over the defaults above (e.g. a custom PATH).
        if let Some(user_env) = payload.env.as_ref() {
            for (k, v) in user_env {
                c.env(k, v);
            }
        }
        // working_dir: when Some and non-empty, chdir before exec.
        if let Some(dir) = payload.working_dir.as_ref() {
            if !dir.is_empty() {
                c.current_dir(dir);
            }
        }
        c
    }
    #[cfg(windows)]
    {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(&payload.cmd);
        c.kill_on_drop(true);
        if let Some(user_env) = payload.env.as_ref() {
            for (k, v) in user_env {
                c.env(k, v);
            }
        }
        if let Some(dir) = payload.working_dir.as_ref() {
            if !dir.is_empty() {
                c.current_dir(dir);
            }
        }
        c
    }
}

/// Read default shell timeout from env (used by daemon config; not enforced here).
pub fn configured_timeout() -> u64 {
    if let Ok(s) = std::env::var("XHJOB_SHELL_TIMEOUT") {
        if let Ok(n) = s.parse::<u64>() {
            if n > 0 { return n; }
        }
    }
    300
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "中文" encoded as GBK produces the byte sequence `0xD6 0xD0 0xCE 0xC4`.
    /// Verifying `decode_bytes` round-trips this back to the original UTF-8 string.
    #[test]
    fn decode_bytes_gbk_to_utf8() {
        let bytes = [0xD6u8, 0xD0, 0xCE, 0xC4];
        let s = decode_bytes(&bytes, "GBK").expect("GBK decode should succeed");
        assert_eq!(s, "中文");
    }

    /// UTF-8 passthrough: feeding UTF-8 bytes with `"UTF-8"` (case-insensitive)
    /// should return the original string without re-encoding.
    #[test]
    fn decode_bytes_utf8_passthrough() {
        let original = "hello 世界 — UTF-8 ✓";
        let bytes = original.as_bytes();
        let s = decode_bytes(bytes, "UTF-8").expect("UTF-8 passthrough should succeed");
        assert_eq!(s, original);

        // Lowercase alias should also be accepted.
        let s2 = decode_bytes(bytes, "utf8").expect("utf8 alias should work");
        assert_eq!(s2, original);
    }

    /// On Unix, `auto` resolves to UTF-8 — so a UTF-8 input should round-trip.
    /// On Windows this test still works as long as the bytes are valid UTF-8
    /// and the OEM code page resolves to UTF-8 (rare in CI). We assert
    /// behavior on Unix only to keep the test deterministic.
    #[test]
    fn decode_bytes_auto_unix_returns_utf8() {
        let original = "auto-mode 测试";
        let bytes = original.as_bytes();
        let s = decode_bytes(bytes, "auto").expect("auto decode should succeed");
        #[cfg(unix)]
        assert_eq!(s, original);
        #[cfg(not(unix))]
        {
            // Just make sure it did not error and produced some non-empty text.
            assert!(!s.is_empty(), "auto decode produced empty output");
        }
    }

    /// Unknown encoding labels must surface as `Err(XhjobError::exec(...))`
    /// rather than silently falling back to lossy UTF-8.
    #[test]
    fn decode_bytes_invalid_encoding_returns_error() {
        let result = decode_bytes(&[0x41u8], "INVALID");
        assert!(result.is_err(), "expected Err for unsupported encoding");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("unsupported encoding"), "unexpected error: {}", msg);
    }

    /// softTimeout (C11) SubTask 41.9 — SIGTERM graceful exit:
    /// `softTimeout(2) + timeout(5)`. The shell script traps SIGTERM and
    /// exits cleanly via the trap (echo CAUGHT; exit 0). The executor sends
    /// SIGTERM at 2s; the trap fires, prints "CAUGHT", and the child exits
    /// with code 0 → execute returns Ok with exit_code=0 and stdout
    /// containing "CAUGHT".
    /// Reference: Celery soft_time_limit.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_soft_timeout_sigterm_graceful_exit() {
        use crate::executor::Executor;
        use crate::store::{Task, TaskType};
        // Bash script: trap SIGTERM, print CAUGHT, exit 0; otherwise loop
        // sleeping so the child is still alive when SIGTERM arrives at 2s.
        let script = "trap 'echo CAUGHT; exit 0' TERM; echo STARTED; for i in $(seq 1 30); do sleep 0.5; done";
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": script}));
        task.timeout = 5;
        task.soft_timeout = Some(2);

        let result = ShellExecutor.execute(&task).await
            .expect("execute should succeed (graceful SIGTERM exit)");
        assert_eq!(result.exit_code, Some(0), "exit_code should be 0 (graceful exit via trap)");
        let stdout = result.stdout.as_deref().unwrap_or("");
        assert!(stdout.contains("CAUGHT"), "stdout should contain CAUGHT: {}", stdout);
    }

    /// softTimeout (C11) SubTask 41.10 — SIGKILL after grace period:
    /// `softTimeout(2) + timeout(4)`. The shell script explicitly ignores
    /// SIGTERM (`trap '' TERM`). The executor sends SIGTERM at 2s (no
    /// effect), then SIGKILL at 4s (grace period = 2s) → execute returns
    /// Err mentioning SIGKILL.
    /// Reference: Celery soft_time_limit.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_soft_timeout_sigkill_after_grace_period() {
        use crate::executor::Executor;
        use crate::store::{Task, TaskType};
        // Bash script: explicitly ignore SIGTERM; keep sleeping so SIGKILL
        // is required to terminate the child.
        let script = "trap '' TERM; echo STARTED; for i in $(seq 1 30); do sleep 0.5; done";
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": script}));
        task.timeout = 4;
        task.soft_timeout = Some(2);

        let result = ShellExecutor.execute(&task).await;
        assert!(result.is_err(), "expected Err (SIGKILL after grace period)");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("SIGKILL"), "error message should mention SIGKILL: {}", msg);
    }

    /// cancel 修复验证：对一个长时间运行的 shell 任务设置 cancel_flag 后，
    /// executor 应在 ~200ms 轮询窗口内检测到取消，终止子进程并返回
    /// "cancelled" 错误，而不是等任务自然结束（30s）。
    ///
    /// 修复前：executor 不检查 cancel_flag，任务运行到 timeout（30s）才返回。
    /// 修复后：executor 每 200ms 检查 cancel_flag，检测到后立即 SIGTERM + reap。
    /// Reference: Celery revoke (terminate=true).
    #[cfg(unix)]
    #[tokio::test]
    async fn test_cancel_flag_terminates_running_child() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicBool;
        use std::time::{Duration, Instant};
        use crate::executor::Executor;
        use crate::store::{Task, TaskType};

        // sleep 30 — 如果 cancel 不生效，测试会因 timeout=10s 而失败（耗时 10s）；
        // 如果 cancel 生效，应在 ~1s 内返回。
        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "sleep 30"}));
        task.timeout = 10; // 硬超时兜底，防止 cancel 失效时挂住整个测试套件

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let flag_clone = Arc::clone(&cancel_flag);

        // 500ms 后设置 cancel 标志，模拟 xhjob_cancel 调用
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(500)).await;
            flag_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });

        let start = Instant::now();
        let result = ShellExecutor.execute_with_cancel(&task, Some(cancel_flag)).await;
        let elapsed = start.elapsed();

        // 应返回 Err 且错误信息包含 "cancelled"
        assert!(result.is_err(), "expected Err after cancel");
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("cancelled"), "error should mention cancelled: {}", msg);

        // 应在远小于 30s 内返回（cancel 轮询间隔 200ms + 500ms 延迟 + grace 500ms）。
        // 上限给 5s 足够宽松，只要远小于 timeout=10s 即说明 cancel 生效。
        assert!(elapsed.as_secs() < 5,
            "cancel should terminate child quickly, took {:?}s", elapsed);
    }

    /// 验证不提供 cancel_flag 时，execute_with_cancel 行为与 execute 一致
    /// （正常完成、不受 cancel 轮询影响）。
    #[cfg(unix)]
    #[tokio::test]
    async fn test_execute_with_cancel_none_completes_normally() {
        use crate::executor::Executor;
        use crate::store::{Task, TaskType};

        let mut task = Task::new(TaskType::Shell, serde_json::json!({"cmd": "echo hello"}));
        task.timeout = 5;

        let result = ShellExecutor.execute_with_cancel(&task, None).await
            .expect("execute_with_cancel(None) should succeed");
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.as_deref().unwrap_or("").contains("hello"),
            "stdout should contain hello: {:?}", result.stdout);
    }
}
