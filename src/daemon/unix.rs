//! Unix daemon implementation: re-exec PHP binary in a detached grandchild.
//!
//! We do NOT use in-process `fork + setsid + fork` because the PHP process
//! may already have a tokio runtime initialized (from `xhjob_dispatch()` /
//! `xhjob_state()` calls), and tokio runtime state is not fork-safe.
//! Instead, we spawn a fresh PHP process with the sentinel env var
//! `XHJOB_DAEMON_MODE=1`; the extension startup detects this and invokes
//! `daemon_main()` directly.

use std::os::unix::process::CommandExt;
use std::process::Command;
<<<<<<< Updated upstream
=======
use std::path::PathBuf;
>>>>>>> Stashed changes
use crate::errors::{Result, XhjobError};
use super::{write_pid, remove_pid_file};

/// Spawn the daemon by re-executing the PHP binary in a double-forked,
<<<<<<< Updated upstream
/// detached grandchild. The grandchild re-runs PHP with `XHJOB_DAEMON_MODE=1`,
/// which the extension startup detects and uses to invoke `daemon_main()`.
pub fn spawn_via_double_fork(_daemon_main: fn() -> ()) -> Result<()> {
=======
/// detached grandchild. The grandchild re-runs PHP with `XHJOB_DAEMON_MODE=1`
/// and `XHJOB_SERVICE_NAME=<name>`, which the extension startup detects and
/// uses to invoke `daemon_main()`.
pub fn spawn_via_double_fork(_daemon_main: fn() -> (), service_name: &str) -> Result<()> {
>>>>>>> Stashed changes
    // Locate the PHP binary that loaded us. We can't always read /proc/self/exe
    // reliably across Unix variants, so prefer $_, then /proc/self/exe, then
    // PATH lookup of `php`.
    let exe = std::env::current_exe()
        .or_else(|_| std::env::var("_").map(PathBuf::from))
        .or_else(|_| std::env::current_exe())
        .map_err(XhjobError::Io)?;

<<<<<<< Updated upstream
    // Re-execute with `-r 'xhjob_run_daemon();'` and the sentinel env var.
    // The xhjob extension exposes this hidden function for exactly this purpose.
=======
    tracing::info!(?exe, service_name, "spawn_via_double_fork invoking");

    // Re-execute with `-r 'xhjob_run_daemon();'` and the sentinel env var.
    // The xhjob extension exposes this hidden function for exactly this purpose.
    //
    // Note: in some environments (e.g. phpenv shims) the parent PHP process
    // may scrub env vars before re-exec'ing the real PHP binary, which can
    // drop vars set via `Command::env()`. The service name is therefore also
    // available via the default-name fallback in `service::current()` when the
    // env var is missing; multi-service tests rely on the env-var path which
    // works under standard PHP SAPIs.
>>>>>>> Stashed changes
    let mut cmd = Command::new(&exe);
    cmd.arg("-d").arg("extension=xhjob.so");
    cmd.arg("-r").arg("xhjob_run_daemon();");
    cmd.env("XHJOB_DAEMON_MODE", "1");
<<<<<<< Updated upstream
=======
    cmd.env("XHJOB_SERVICE_NAME", service_name);
>>>>>>> Stashed changes
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
<<<<<<< Updated upstream
    cmd.spawn().map_err(XhjobError::Io)?;
    Ok(())
}

use std::path::PathBuf;

=======
    let _child = cmd.spawn().map_err(XhjobError::Io)?;
    Ok(())
}

>>>>>>> Stashed changes
extern "C" {
    fn setsid() -> i32;
}

unsafe fn libc_setsid() -> i32 {
    setsid()
}

/// Called by daemon_main on startup (already in daemon process).
<<<<<<< Updated upstream
pub fn daemon_started() -> Result<()> {
    // Write PID file (we are the daemon now)
    write_pid(std::process::id())
=======
/// Reads the service name from `service::current()` (set via XHJOB_SERVICE_NAME
/// by the spawner) and writes the PID file for that service.
pub fn daemon_started() -> Result<()> {
    let service_name = crate::service::current();
    write_pid(std::process::id(), &service_name)
>>>>>>> Stashed changes
}

/// Called by daemon_main on exit: cleanup.
pub fn daemon_stopping() {
<<<<<<< Updated upstream
    remove_pid_file();
=======
    let service_name = crate::service::current();
    remove_pid_file(&service_name);
>>>>>>> Stashed changes
}
