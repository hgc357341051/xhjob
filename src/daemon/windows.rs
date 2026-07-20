//! Windows daemon implementation via CreateProcessW + DETACHED_PROCESS.
//!
//! On Windows, we cannot in-process detach like Unix fork. Instead we
//! re-launch the current PHP binary with `-r 'xhjob_run_daemon("<name>");'`
//! and the DETACHED_PROCESS + CREATE_NEW_PROCESS_GROUP flags. The relaunched
//! process runs `daemon_main()` directly.

use crate::errors::{Result, XhjobError};
use super::{write_pid, remove_pid_file};

pub fn spawn_via_create_process(_daemon_main: fn() -> (), service_name: &str) -> Result<()> {
    // On Windows we cannot pass a function pointer across processes.
    // Strategy: re-launch PHP with `-r 'xhjob_run_daemon("<name>");'`. The
    // service name is encoded into the code string so it survives any env-var
    // scrubbing by PHP version-manager shims (consistent with the Unix path).
    //
    // We use std::process::Command with creation_flags for detachment.
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const DETACHED_PROCESS: u32 = 0x00000008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

    let exe = std::env::current_exe().map_err(XhjobError::Io)?;

    // PHP single-quoted strings escape `'` as `\'` and `\` as `\\`. Service
    // names are validated to be `[a-zA-Z][a-zA-Z0-9_-]{0,31}` so neither
    // character is legal, but we escape defensively anyway.
    let escaped = service_name
        .replace('\\', "\\\\")
        .replace('\'', "\\'");
    let code = format!("xhjob_run_daemon('{}');", escaped);

    let mut cmd = Command::new(&exe);
    cmd.arg("-d").arg("extension=xhjob.so");
    cmd.arg("-r").arg(&code);
    // Env vars kept as a backward-compatible fallback.
    cmd.env("XHJOB_DAEMON_MODE", "1");
    cmd.env("XHJOB_SERVICE_NAME", service_name);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);

    cmd.spawn().map_err(XhjobError::Io)?;
    Ok(())
}

/// Called by daemon_main on startup: write PID file (Windows path).
/// Reads the service name from `service::current()`.
pub fn daemon_started() -> Result<()> {
    let service_name = crate::service::current();
    write_pid(std::process::id(), &service_name)
}

/// Called by daemon_main on exit.
pub fn daemon_stopping() {
    let service_name = crate::service::current();
    remove_pid_file(&service_name);
}

// daemon_main is invoked by the extension startup when XHJOB_DAEMON_MODE=1
// (Windows branch). The fn pointer is passed from the C-layer startup.
#[allow(dead_code)]
pub fn run_in_daemon_mode(daemon_main: fn() -> ()) {
    let _ = daemon_started();
    daemon_main();
    daemon_stopping();
}
