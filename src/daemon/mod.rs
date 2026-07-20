//! Cross-platform daemon process management.

use std::path::PathBuf;
use std::time::Duration;
use crate::errors::{Result, XhjobError};

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

<<<<<<< Updated upstream
/// Default PID file path (overridable by XHJOB_PID_FILE env var).
pub fn pid_file_path() -> PathBuf {
    if let Ok(p) = std::env::var("XHJOB_PID_FILE") {
        return PathBuf::from(p);
    }
    #[cfg(unix)]
    { PathBuf::from("/tmp/xhjob.pid") }
    #[cfg(windows)]
    { std::env::temp_dir().join("xhjob.pid") }
}

/// Default log file path.
pub fn log_file_path() -> PathBuf {
    if let Ok(p) = std::env::var("XHJOB_LOG_FILE") {
        return PathBuf::from(p);
    }
    #[cfg(unix)]
    { PathBuf::from("/tmp/xhjob.log") }
    #[cfg(windows)]
    { std::env::temp_dir().join("xhjob.log") }
}

/// Read PID from file. Returns None if not present or stale.
pub fn read_pid() -> Option<u32> {
    let path = pid_file_path();
=======
/// PID file path derived from `service_name`.
///
/// Unix: `${XHJOB_PID_DIR:-/tmp}/xhjob.{name}.pid`
/// Windows: `%TEMP%\xhjob.{name}.pid`
pub fn pid_file_path(service_name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        let dir = std::env::var("XHJOB_PID_DIR").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(dir).join(format!("xhjob.{}.pid", service_name))
    }
    #[cfg(windows)]
    {
        std::env::temp_dir().join(format!("xhjob.{}.pid", service_name))
    }
}

/// Log file path derived from `service_name`.
///
/// Unix: `${XHJOB_LOG_DIR:-/tmp}/xhjob.{name}.log`
/// Windows: `%TEMP%\xhjob.{name}.log`
pub fn log_file_path(service_name: &str) -> PathBuf {
    #[cfg(unix)]
    {
        let dir = std::env::var("XHJOB_LOG_DIR").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(dir).join(format!("xhjob.{}.log", service_name))
    }
    #[cfg(windows)]
    {
        std::env::temp_dir().join(format!("xhjob.{}.log", service_name))
    }
}

/// Read PID from file. Returns None if not present or stale.
pub fn read_pid(service_name: &str) -> Option<u32> {
    let path = pid_file_path(service_name);
>>>>>>> Stashed changes
    let content = std::fs::read_to_string(&path).ok()?;
    let pid: u32 = content.trim().parse().ok()?;
    if is_process_alive(pid) {
        Some(pid)
    } else {
        // stale pid file
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Write PID file atomically.
<<<<<<< Updated upstream
pub fn write_pid(pid: u32) -> Result<()> {
    let path = pid_file_path();
=======
pub fn write_pid(pid: u32, service_name: &str) -> Result<()> {
    let path = pid_file_path(service_name);
>>>>>>> Stashed changes
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, pid.to_string())?;
    Ok(())
}

/// Remove PID file if exists.
<<<<<<< Updated upstream
pub fn remove_pid_file() {
    let _ = std::fs::remove_file(pid_file_path());
=======
pub fn remove_pid_file(service_name: &str) {
    let _ = std::fs::remove_file(pid_file_path(service_name));
>>>>>>> Stashed changes
}

/// Check if a process is alive (cross-platform).
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) returns 0 if process exists
        unsafe { libc_kill(pid as i32, 0) == 0 }
    }
    #[cfg(windows)]
    {
        // OpenProcess returns handle if process exists
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            let h: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() { return false; }
            CloseHandle(h);
            true
        }
    }
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
unsafe fn libc_kill(pid: i32, sig: i32) -> i32 {
    kill(pid, sig)
}

/// Daemon status info returned to PHP.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DaemonStatus {
    pub running: bool,
    pub pid: Option<u32>,
}

impl DaemonStatus {
    pub fn not_running() -> Self {
        Self { running: false, pid: None }
    }
    pub fn running(pid: u32) -> Self {
        Self { running: true, pid: Some(pid) }
    }
}

<<<<<<< Updated upstream
/// Query current daemon status.
pub fn status() -> DaemonStatus {
    match read_pid() {
=======
/// Query current daemon status for `service_name`.
pub fn status(service_name: &str) -> DaemonStatus {
    match read_pid(service_name) {
>>>>>>> Stashed changes
        Some(pid) if is_process_alive(pid) => DaemonStatus::running(pid),
        _ => DaemonStatus::not_running(),
    }
}

/// Send SIGTERM (Unix) or TerminateProcess (Windows) to the daemon.
<<<<<<< Updated upstream
pub fn send_terminate(pid: u32) -> Result<()> {
=======
pub fn send_terminate(pid: u32, service_name: &str) -> Result<()> {
>>>>>>> Stashed changes
    #[cfg(unix)]
    {
        let rc = unsafe { kill(pid as i32, 15 /* SIGTERM */) };
        if rc == 0 {
            // wait up to 10 seconds for the process to exit
            for _ in 0..100 {
                if !is_process_alive(pid) {
<<<<<<< Updated upstream
                    remove_pid_file();
=======
                    remove_pid_file(service_name);
>>>>>>> Stashed changes
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            // force kill if still alive
            let _ = unsafe { kill(pid as i32, 9 /* SIGKILL */) };
<<<<<<< Updated upstream
            remove_pid_file();
=======
            remove_pid_file(service_name);
>>>>>>> Stashed changes
            Ok(())
        } else {
            Err(XhjobError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("failed to send SIGTERM to pid {}", pid),
            )))
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if h.is_null() {
                return Err(XhjobError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("OpenProcess failed for pid {}", pid),
                )));
            }
            let rc = TerminateProcess(h, 1);
            CloseHandle(h);
            if rc == 0 {
                return Err(XhjobError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "TerminateProcess failed",
                )));
            }
            // wait for exit
            for _ in 0..100 {
                if !is_process_alive(pid) {
<<<<<<< Updated upstream
                    remove_pid_file();
=======
                    remove_pid_file(service_name);
>>>>>>> Stashed changes
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
<<<<<<< Updated upstream
            remove_pid_file();
=======
            remove_pid_file(service_name);
>>>>>>> Stashed changes
            Ok(())
        }
    }
}

<<<<<<< Updated upstream
/// Spawn the daemon process.
///
/// Re-executes the current executable (the PHP process running the extension)
/// is NOT what we want; instead we re-exec the xhjob daemon binary if available,
/// or fall back to spawning a helper. For this extension, the daemon logic lives
/// inside the .so itself and is invoked via an env var sentinel.
///
/// Strategy: re-exec the `php` binary with a special `-d` flag set to run a
/// built-in daemon entrypoint. We do this by setting `XHJOB_DAEMON_MODE=1` and
/// running `php -r 'xhjob_start();'` style — but simpler: re-exec the parent
/// php binary path with `-r 'echo "xhjob daemon";'` and `XHJOB_DAEMON_MODE=1`.
///
/// In practice: the PHP C-layer start function calls `spawn_daemon()` which
/// forks the current PHP process; the child calls `daemon_main()` (defined in
/// this module) and never returns. This is the simplest cross-platform path
/// because PHP itself is a single binary that knows how to load the extension.
///
/// For cross-platform compatibility, we use a different mechanism on Windows:
/// re-launch the parent process (the PHP binary) with `XHJOB_DAEMON_MODE=1`.
pub fn spawn_daemon(daemon_main: fn() -> ()) -> Result<()> {
    if let Some(pid) = read_pid() {
=======
/// Spawn the daemon process for `service_name`.
///
/// See `spawn_daemon` for the cross-platform strategy.
pub fn spawn_daemon(daemon_main: fn() -> (), service_name: &str) -> Result<()> {
    if let Some(pid) = read_pid(service_name) {
>>>>>>> Stashed changes
        if is_process_alive(pid) {
            return Err(XhjobError::DaemonAlreadyRunning);
        }
    }
    #[cfg(unix)]
    {
<<<<<<< Updated upstream
        unix::spawn_via_double_fork(daemon_main)
    }
    #[cfg(windows)]
    {
        windows::spawn_via_create_process(daemon_main)
    }
}

/// Public entry point: called by PHP `xhjob_start()`.
///
/// Returns true if daemon is now running (either already running, or just started).
pub fn start(daemon_main: fn() -> ()) -> Result<bool> {
    if let Some(pid) = read_pid() {
=======
        unix::spawn_via_double_fork(daemon_main, service_name)
    }
    #[cfg(windows)]
    {
        windows::spawn_via_create_process(daemon_main, service_name)
    }
}

/// Public entry point: called by PHP `xhjob_start($name)`.
///
/// Returns true if daemon is now running (either already running, or just started).
pub fn start(daemon_main: fn() -> (), service_name: &str) -> Result<bool> {
    if let Some(pid) = read_pid(service_name) {
>>>>>>> Stashed changes
        if is_process_alive(pid) {
            return Ok(true);
        }
    }
<<<<<<< Updated upstream
    spawn_daemon(daemon_main)?;
=======
    spawn_daemon(daemon_main, service_name)?;
>>>>>>> Stashed changes
    // Wait until BOTH the PID file is written AND the IPC socket is accepting
    // connections. Waiting only on the PID file is racy: the daemon writes its
    // PID before binding the IPC listener, so an immediate dispatch would fail.
    for _ in 0..100 {
<<<<<<< Updated upstream
        if let Some(pid) = read_pid() {
            if is_process_alive(pid) && ipc_socket_ready() {
=======
        if let Some(pid) = read_pid(service_name) {
            if is_process_alive(pid) && ipc_socket_ready(service_name) {
>>>>>>> Stashed changes
                return Ok(true);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(false)
}

/// Probe whether the IPC socket is accepting connections (Unix domain socket
/// or Windows named pipe). Best-effort: returns false on any error.
<<<<<<< Updated upstream
fn ipc_socket_ready() -> bool {
    let path = crate::ipc::ipc_path();
=======
fn ipc_socket_ready(service_name: &str) -> bool {
    let path = crate::ipc::ipc_path(service_name);
>>>>>>> Stashed changes
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        UnixStream::connect(&path)
            .map(|_| true)
            .unwrap_or(false)
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::NamedPipeClient;
        // Blocking connect; named pipe Client::connect is sync on Windows.
        NamedPipeClient::connect(&path)
            .map(|_| true)
            .unwrap_or(false)
    }
}

<<<<<<< Updated upstream
/// Public entry point: called by PHP `xhjob_stop()`.
pub fn stop() -> Result<bool> {
    match read_pid() {
        Some(pid) if is_process_alive(pid) => {
            send_terminate(pid)?;
            Ok(true)
        }
        _ => {
            remove_pid_file();
=======
/// Public entry point: called by PHP `xhjob_stop($name)`.
pub fn stop(service_name: &str) -> Result<bool> {
    match read_pid(service_name) {
        Some(pid) if is_process_alive(pid) => {
            send_terminate(pid, service_name)?;
            Ok(true)
        }
        _ => {
            remove_pid_file(service_name);
>>>>>>> Stashed changes
            Ok(false)
        }
    }
}

<<<<<<< Updated upstream
/// Public entry point: called by PHP `xhjob_restart()`.
pub fn restart(daemon_main: fn() -> ()) -> Result<bool> {
    let _ = stop();
    std::thread::sleep(Duration::from_millis(500));
    start(daemon_main)
=======
/// Public entry point: called by PHP `xhjob_restart($name)`.
pub fn restart(daemon_main: fn() -> (), service_name: &str) -> Result<bool> {
    let _ = stop(service_name);
    std::thread::sleep(Duration::from_millis(500));
    start(daemon_main, service_name)
>>>>>>> Stashed changes
}
