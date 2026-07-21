//! Cross-platform shell executor.
//!
//! Unix: runs `bash -c "<cmd>"` via tokio::process::Command.
//! Windows: runs `cmd /C "<cmd>"` via tokio::process::Command.

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
    fn execute(&self, task: &Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + '_>> {
        let payload_val = task.payload.clone();
        let timeout = task.timeout;
        let encoding = task.encoding.clone();
        let soft_timeout = task.soft_timeout;
        Box::pin(async move {
            let payload: ShellPayload = serde_json::from_value(payload_val)
                .map_err(|e| XhjobError::Exec(format!("invalid shell payload: {}", e)))?;

            let mut cmd = build_command(&payload.cmd);

            // Spawn the child process
            let mut child = cmd.stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .stdin(std::process::Stdio::null())
                .spawn()
                .map_err(|e| XhjobError::Exec(format!("spawn: {}", e)))?;

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

            let status_result = if let Some(soft_secs) = soft {
                let grace = timeout - soft_secs;
                // Phase 1: wait soft_secs for graceful completion (no signal yet).
                match tokio::time::timeout(Duration::from_secs(soft_secs), child.wait()).await {
                    Ok(s) => Ok(s), // completed before soft_timeout
                    Err(_) => {
                        // soft_timeout elapsed — send SIGTERM to the child.
                        #[cfg(unix)]
                        if let Some(pid) = child.id() {
                            use nix::sys::signal::{kill, Signal};
                            use nix::unistd::Pid;
                            let _ = kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
                        }
                        // Phase 2: wait grace period for graceful exit after SIGTERM.
                        match tokio::time::timeout(Duration::from_secs(grace), child.wait()).await {
                            Ok(s) => Ok(s), // graceful exit after SIGTERM
                            Err(_) => {
                                // SIGKILL after grace period — reap the child
                                // so it doesn't become a zombie.
                                let _ = child.start_kill();
                                let _ = child.wait().await;
                                return Err(XhjobError::Exec(format!(
                                    "timeout after {}s (soft={}s, grace={}s, SIGKILL)",
                                    timeout, soft_secs, grace
                                )));
                            }
                        }
                    }
                }
            } else {
                // No soft_timeout: existing logic, just wait timeout then SIGKILL.
                tokio::time::timeout(Duration::from_secs(timeout), child.wait()).await
            };

            let (stdout_text, stderr_text, exit_code) = match status_result {
                Ok(Ok(status)) => {
                    // Read stdout/stderr from the captured pipes
                    let stdout_text = if let Some(mut s) = stdout_fut {
                        use tokio::io::AsyncReadExt;
                        let mut buf = Vec::new();
                        let _ = s.read_to_end(&mut buf).await;
                        decode_output(&buf, encoding.as_ref())?
                    } else { String::new() };
                    let stderr_text = if let Some(mut s) = stderr_fut {
                        use tokio::io::AsyncReadExt;
                        let mut buf = Vec::new();
                        let _ = s.read_to_end(&mut buf).await;
                        decode_output(&buf, encoding.as_ref())?
                    } else { String::new() };
                    let code = status.code().unwrap_or(-1);
                    (stdout_text, stderr_text, code)
                }
                Ok(Err(e)) => {
                    return Err(XhjobError::Exec(format!("wait: {}", e)));
                }
                Err(_) => {
                    // timeout: kill the child
                    let _ = child.start_kill();
                    return Err(XhjobError::Exec(format!("timeout after {}s", timeout)));
                }
            };

            Ok(TaskResult {
                body: None,
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
/// labels produce `XhjobError::Exec("unsupported encoding: ...")`.
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
        .ok_or_else(|| XhjobError::Exec(format!("unsupported encoding: {}", encoding)))?;
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
fn build_command(cmd: &str) -> tokio::process::Command {
    #[cfg(unix)]
    {
        let mut c = tokio::process::Command::new("bash");
        c.arg("-c").arg(cmd);
        c
    }
    #[cfg(windows)]
    {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
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

    /// Unknown encoding labels must surface as `Err(XhjobError::Exec(...))`
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
}
