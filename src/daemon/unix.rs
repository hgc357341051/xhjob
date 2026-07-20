//! Unix daemon implementation: re-exec PHP binary in a detached grandchild.
//!
//! We do NOT use in-process `fork + setsid + fork` because the PHP process
//! may already have a tokio runtime initialized (from `xhjob_dispatch()` /
//! `xhjob_state()` calls), and tokio runtime state is not fork-safe.
//! Instead, we spawn a fresh PHP process with `XHJOB_DAEMON_MODE=1`; the
//! extension startup detects this and invokes `daemon_main()` directly.

use std::os::unix::process::CommandExt;
use std::process::Command;
use std::path::PathBuf;
use crate::errors::{Result, XhjobError};
use super::{write_pid, remove_pid_file};

/// Spawn the daemon by re-executing the PHP binary in a double-forked,
/// detached grandchild. The grandchild re-runs PHP with `-r` invoking
/// `xhjob_run_daemon('<service_name>')`, which sets the active service name
/// and runs `daemon_main()`.
///
/// The service name is passed via the `-r` code string (a command-line
/// argument) rather than relying solely on the `XHJOB_SERVICE_NAME` env var,
/// because some PHP SAPI / version-manager (e.g. phpenv) setups scrub env
/// vars set via `Command::env()` before they reach the spawned child. The env
/// var is still set as a backward-compatible fallback for callers that spawn
/// the daemon through other paths.
pub fn spawn_via_double_fork(_daemon_main: fn() -> (), service_name: &str) -> Result<()> {
    // Locate the PHP binary that loaded us. We can't always read /proc/self/exe
    // reliably across Unix variants, so prefer $_, then /proc/self/exe, then
    // PATH lookup of `php`.
    let exe = std::env::current_exe()
        .or_else(|_| std::env::var("_").map(PathBuf::from))
        .or_else(|_| std::env::current_exe())
        .map_err(XhjobError::Io)?;

    tracing::info!(?exe, service_name, "spawn_via_double_fork invoking");

    // Encode the service name directly into the `-r` code string so it is
    // delivered as a command-line argument. Command-line args are preserved
    // across re-exec by PHP version-manager shims, unlike env vars set via
    // `Command::env()` which can be dropped. Single quotes in the name are
    // escaped using a PHP-safe `\\'` sequence; service names are validated by
    // `service::validate` to be `[a-zA-Z][a-zA-Z0-9_-]{0,31}` so this is
    // belt-and-braces.
    let escaped = service_name.replace('\'', "\\'");
    let code = format!("xhjob_run_daemon('{}');", escaped);

    let mut cmd = Command::new(&exe);
    cmd.arg("-d").arg("extension=xhjob.so");
    cmd.arg("-r").arg(&code);
    // Env vars are kept as a backward-compatible fallback: if a caller spawns
    // the daemon through a path that does preserve env vars, the daemon will
    // still see them.
    cmd.env("XHJOB_DAEMON_MODE", "1");
    cmd.env("XHJOB_SERVICE_NAME", service_name);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());

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
/// Reads the service name from `service::current()` (set via XHJOB_SERVICE_NAME
/// by the spawner) and writes the PID file for that service.
pub fn daemon_started() -> Result<()> {
    let service_name = crate::service::current();
    write_pid(std::process::id(), &service_name)
}

/// Called by daemon_main on exit: cleanup.
pub fn daemon_stopping() {
    let service_name = crate::service::current();
    remove_pid_file(&service_name);
}
