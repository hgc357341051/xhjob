//! Windows daemon implementation via CreateProcessW + DETACHED_PROCESS.
//!
//! On Windows, we cannot in-process detach like Unix fork. Instead we
//! re-launch the current PHP binary with `-r 'xhjob_run_daemon("<name>", "<data_dir>");'`
//! and the DETACHED_PROCESS + CREATE_NEW_PROCESS_GROUP flags. The relaunched
//! process runs `daemon_main()` directly.

use super::{remove_pid_file, write_pid};
use crate::errors::{Result, XhjobError};

pub fn spawn_via_create_process(
    _daemon_main: fn() -> (),
    service_name: &str,
    data_dir: Option<&str>,
) -> Result<()> {
    // On Windows we cannot pass a function pointer across processes.
    // Strategy: re-launch PHP with `-r 'xhjob_run_daemon("<name>", "<data_dir>");'`.
    // The service name and data_dir are encoded into the code string so they
    // survive any env-var scrubbing by PHP version-manager shims (consistent
    // with the Unix path).
    //
    // We use std::process::Command with creation_flags for detachment.
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const DETACHED_PROCESS: u32 = 0x00000008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

    // Locate the PHP binary. In PHP-FPM context, `current_exe()` returns
    // `php-fpm` (not the CLI `php`), which rejects `-r`/`-d`. `resolve_php_binary()`
    // returns the resolved CLI binary plus the raw `current_exe()` for diagnostics.
    let (exe, _raw_exe) = super::resolve_php_binary();

    // PHP single-quoted strings escape `'` as `\'` and `\` as `\\`. Service
    // names are validated to be `[a-zA-Z][a-zA-Z0-9_-]{0,31}` so neither
    // character is legal, but we escape defensively anyway. data_dir is also
    // shell-escaped here for safety.
    let escaped_name = service_name.replace('\\', "\\\\").replace('\'', "\\'");
    let code = if let Some(dir) = data_dir {
        if !dir.is_empty() {
            let escaped_dir = dir.replace('\\', "\\\\").replace('\'', "\\'");
            format!("xhjob_run_daemon('{}', '{}');", escaped_name, escaped_dir)
        } else {
            format!("xhjob_run_daemon('{}');", escaped_name)
        }
    } else {
        format!("xhjob_run_daemon('{}');", escaped_name)
    };

    let mut cmd = Command::new(&exe);
    // Only inject `-d extension=xhjob.so` when the current process did NOT
    // load xhjob via php.ini. When xhjob is already in php.ini (FPM/Apache,
    // or CLI with ini-configured xhjob), the spawned daemon will auto-load
    // it from the same php.ini — passing `-d extension=` again would trigger
    // PHP's "Module already loaded" warning.
    if super::should_inject_extension_arg() {
        cmd.arg("-d").arg("extension=xhjob.so");
    }
    cmd.arg("-r").arg(&code);
    // Env vars kept as a backward-compatible fallback.
    cmd.env("XHJOB_DAEMON_MODE", "1");
    cmd.env("XHJOB_SERVICE_NAME", service_name);
    if let Some(dir) = data_dir {
        if !dir.is_empty() {
            cmd.env("XHJOB_DATA_DIR", dir);
        }
    }
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    // H8 fix: redirect stderr to the log file instead of null, so panic
    // traces and tracing output are preserved on Windows (mirroring the
    // Unix daemon path). Without this, production debugging on Windows
    // is impossible — all stderr output is permanently lost.
    let log_path = super::log_file_path(&service_name, data_dir);
    let stderr = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map(std::process::Stdio::from)
        .unwrap_or(std::process::Stdio::null());
    cmd.stderr(stderr);
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);

    let child = cmd.spawn().map_err(XhjobError::Io)?;
    // Detect immediate-exit failures (PHP binary missing, `-r` parse error,
    // etc.) so the caller gets a useful error instead of waiting 10s for a
    // PID file that will never appear. The helper polls try_wait() for 500ms;
    // if the child is still running, it `mem::forget`s the handle (preserving
    // the detached daemon) and returns Ok(()).
    super::check_child_alive(child, &log_path, service_name)
}

/// Called by daemon_main on startup: write PID file (Windows path).
/// Reads the service name from `service::current()` and data_dir from
/// `service::current_data_dir()`.
pub fn daemon_started() -> Result<()> {
    let service_name = crate::service::current();
    let data_dir = crate::service::current_data_dir();
    write_pid(std::process::id(), &service_name, data_dir.as_deref())
}

/// Called by daemon_main on exit.
pub fn daemon_stopping() {
    let service_name = crate::service::current();
    let data_dir = crate::service::current_data_dir();
    remove_pid_file(&service_name, data_dir.as_deref());
}

// daemon_main is invoked by the extension startup when XHJOB_DAEMON_MODE=1
// (Windows branch). The fn pointer is passed from the C-layer startup.
#[allow(dead_code)]
pub fn run_in_daemon_mode(daemon_main: fn() -> ()) {
    let _ = daemon_started();
    daemon_main();
    daemon_stopping();
}
