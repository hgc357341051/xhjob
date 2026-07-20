//! Windows daemon implementation via CreateProcessW + DETACHED_PROCESS.
//!
//! On Windows, we cannot in-process detach like Unix fork. Instead we
//! re-launch the current PHP binary with XHJOB_DAEMON_MODE=1 and the
//! DETACHED_PROCESS + CREATE_NEW_PROCESS_GROUP flags. The relaunched process
//! detects the env var and runs daemon_main().

use crate::errors::{Result, XhjobError};
use super::{write_pid, remove_pid_file};

<<<<<<< Updated upstream
pub fn spawn_via_create_process(daemon_main: fn() -> ()) -> Result<()> {
=======
pub fn spawn_via_create_process(daemon_main: fn() -> (), service_name: &str) -> Result<()> {
>>>>>>> Stashed changes
    // On Windows we cannot pass a function pointer across processes.
    // Strategy: the PHP extension's startup detects XHJOB_DAEMON_MODE=1 and
    // invokes daemon_main() directly. Here we just spawn a new PHP process
    // with that env var set.
    //
    // We use std::process::Command with creation_flags for detachment.
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const DETACHED_PROCESS: u32 = 0x00000008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

    let exe = std::env::current_exe().map_err(XhjobError::Io)?;
    let argv: Vec<String> = std::env::args().collect();

    let mut cmd = Command::new(&exe);
    for arg in argv.iter().skip(1) {
        cmd.arg(arg);
    }
    cmd.env("XHJOB_DAEMON_MODE", "1");
<<<<<<< Updated upstream
=======
    cmd.env("XHJOB_SERVICE_NAME", service_name);
>>>>>>> Stashed changes
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::null());
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);

    cmd.spawn().map_err(XhjobError::Io)?;
    Ok(())
}

/// Called by daemon_main on startup: write PID file (Windows path).
<<<<<<< Updated upstream
pub fn daemon_started() -> Result<()> {
    write_pid(std::process::id())
=======
/// Reads the service name from `service::current()`.
pub fn daemon_started() -> Result<()> {
    let service_name = crate::service::current();
    write_pid(std::process::id(), &service_name)
>>>>>>> Stashed changes
}

/// Called by daemon_main on exit.
pub fn daemon_stopping() {
<<<<<<< Updated upstream
    remove_pid_file();
=======
    let service_name = crate::service::current();
    remove_pid_file(&service_name);
>>>>>>> Stashed changes
}

// daemon_main is invoked by the extension startup when XHJOB_DAEMON_MODE=1
// (Windows branch). The fn pointer is passed from the C-layer startup.
#[allow(dead_code)]
pub fn run_in_daemon_mode(daemon_main: fn() -> ()) {
    let _ = daemon_started();
    daemon_main();
    daemon_stopping();
}
