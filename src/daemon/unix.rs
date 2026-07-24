//! Unix daemon implementation: re-exec PHP binary in a detached grandchild.
//!
//! We do NOT use in-process `fork + setsid + fork` because the PHP process
//! may already have a tokio runtime initialized (from `xhjob_dispatch()` /
//! `xhjob_state()` calls), and tokio runtime state is not fork-safe.
//! Instead, we spawn a fresh PHP process with `XHJOB_DAEMON_MODE=1`; the
//! extension startup detects this and invokes `daemon_main()` directly.

use super::{remove_pid_file, write_pid};
use crate::errors::{Result, XhjobError};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

/// Spawn the daemon by re-executing the PHP binary in a double-forked,
/// detached grandchild. The grandchild re-runs PHP with `-r` invoking
/// `xhjob_run_daemon('<service_name>', '<data_dir>')`, which sets the active
/// service name + data directory and runs `daemon_main()`.
///
/// Both the service name and the data directory are passed via the `-r` code
/// string (a command-line argument) rather than relying solely on env vars,
/// because some PHP SAPI / version-manager (e.g. phpenv) setups scrub env
/// vars set via `Command::env()` before they reach the spawned child. The env
/// vars are still set as a backward-compatible fallback for callers that
/// spawn the daemon through other paths.
pub fn spawn_via_double_fork(
    _daemon_main: fn() -> (),
    service_name: &str,
    data_dir: Option<&str>,
) -> Result<()> {
    // Locate the PHP binary that loaded us. We can't always read /proc/self/exe
    // reliably across Unix variants, so prefer $_, then /proc/self/exe, then
    // PATH lookup of `php`.
    let exe = std::env::current_exe()
        .or_else(|_| std::env::var("_").map(PathBuf::from))
        .or_else(|_| std::env::current_exe())
        .map_err(XhjobError::Io)?;

    tracing::info!(
        ?exe,
        service_name,
        data_dir,
        "spawn_via_double_fork invoking"
    );

    // Encode the service name and data_dir directly into the `-r` code string
    // so they are delivered as command-line arguments. Command-line args are
    // preserved across re-exec by PHP version-manager shims, unlike env vars
    // set via `Command::env()` which can be dropped. Single quotes in the
    // values are escaped using a PHP-safe `\\'` sequence; service names are
    // validated by `service::validate` to be `[a-zA-Z][a-zA-Z0-9_-]{0,31}`,
    // and data_dir is shell-escaped here for safety.
    let escaped_name = service_name.replace('\'', "\\'");
    let code = if let Some(dir) = data_dir {
        if !dir.is_empty() {
            // Escape backslashes first, then single quotes, for PHP single-quoted strings.
            let escaped_dir = dir.replace('\\', "\\\\").replace('\'', "\\'");
            format!("xhjob_run_daemon('{}', '{}');", escaped_name, escaped_dir)
        } else {
            format!("xhjob_run_daemon('{}');", escaped_name)
        }
    } else {
        format!("xhjob_run_daemon('{}');", escaped_name)
    };

    let mut cmd = Command::new(&exe);
    cmd.arg("-d").arg("extension=xhjob.so");
    cmd.arg("-r").arg(&code);
    // Env vars are kept as a backward-compatible fallback: if a caller spawns
    // the daemon through a path that does preserve env vars, the daemon will
    // still see them.
    cmd.env("XHJOB_DAEMON_MODE", "1");
    cmd.env("XHJOB_SERVICE_NAME", service_name);
    if let Some(dir) = data_dir {
        if !dir.is_empty() {
            cmd.env("XHJOB_DATA_DIR", dir);
        }
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    // Fix 3: redirect stderr to a log file so tracing_subscriber output
    // is actually visible for debugging. Without this, all tracing logs
    // (daemon startup, task dispatch, errors) go to /dev/null, making
    // production debugging impossible.
    // Log path follows the same pattern as sock/db: <dir>/xhjob.<name>.log
    let log_dir = data_dir
        .filter(|d| !d.is_empty())
        .map(|d| d.to_string())
        .or_else(|| {
            std::env::var("XHJOB_DATA_DIR")
                .ok()
                .filter(|d| !d.is_empty())
        })
        .unwrap_or_else(|| {
            // Same fallback as ipc::fallback_sock_dir
            std::env::var("XHJOB_SOCK_DIR")
                .ok()
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| "/tmp".to_string())
        });
    let log_path = std::path::PathBuf::from(&log_dir).join(format!("xhjob.{}.log", service_name));
    // Try to open the log file for appending. If it fails (e.g. dir doesn't
    // exist yet), fall back to /dev/null to avoid blocking daemon startup.
    let log_stdio = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map(|f| {
            // Harden log file permissions (may contain task payloads / secrets).
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&log_path, std::fs::Permissions::from_mode(0o600));
            std::process::Stdio::from(f)
        })
        .unwrap_or(std::process::Stdio::null());
    cmd.stderr(log_stdio);

    // Double-fork pattern via `pre_exec`: the spawned process calls setsid
    // before exec'ing, so it becomes a session leader detached from any tty.
    unsafe {
        cmd.pre_exec(|| {
            if libc_setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    // Spawn the detached child. The child will write its own PID file when
    // daemon_main() runs.
    let _child = cmd.spawn().map_err(XhjobError::Io)?;
    Ok(())
}

extern "C" {
    fn setsid() -> i32;
}

unsafe fn libc_setsid() -> i32 {
    setsid()
}

/// Called by daemon_main on startup (already in daemon process).
/// Reads the service name from `service::current()` and data_dir from
/// `service::current_data_dir()`, then writes the PID file (with current
/// process starttime, so future readers can detect PID reuse) for that
/// service in the resolved directory.
pub fn daemon_started() -> Result<()> {
    let service_name = crate::service::current();
    let data_dir = crate::service::current_data_dir();
    // Capture our own starttime so a future reader can detect that this
    // specific daemon instance is the PID-file owner (vs. an unrelated process
    // that later recycled the same PID). Returns None on non-Linux, in which
    // case write_pid emits the legacy single-line format.
    let starttime = super::process_starttime(std::process::id());
    write_pid(
        std::process::id(),
        starttime,
        &service_name,
        data_dir.as_deref(),
    )
}

/// Called by daemon_main on exit: cleanup.
pub fn daemon_stopping() {
    let service_name = crate::service::current();
    let data_dir = crate::service::current_data_dir();
    remove_pid_file(&service_name, data_dir.as_deref());
}
