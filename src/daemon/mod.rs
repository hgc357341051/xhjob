//! Cross-platform daemon process management.

use std::path::PathBuf;
use std::time::Duration;
use crate::errors::{Result, XhjobError};

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

/// PID file path derived from `service_name` and optional `data_dir`.
///
/// Path resolution priority (highest first):
///   1. `data_dir` argument (if `Some`)
///   2. `XHJOB_PID_DIR` env var (Unix only, fine-grained override)
///   3. `XHJOB_DATA_DIR` env var (unified data directory)
///   4. Platform default (`/tmp` on Unix, `%TEMP%` on Windows)
///
/// Unix: `<dir>/xhjob.{name}.pid`
/// Windows: `%TEMP%\xhjob.{name}.pid` (data_dir is also honored when provided)
pub fn pid_file_path(service_name: &str, data_dir: Option<&str>) -> PathBuf {
    resolve_dir_path(data_dir, "XHJOB_PID_DIR")
        .join(format!("xhjob.{}.pid", service_name))
}

/// Log file path derived from `service_name` and optional `data_dir`.
///
/// Path resolution priority (highest first):
///   1. `data_dir` argument (if `Some`)
///   2. `XHJOB_LOG_DIR` env var (Unix only, fine-grained override)
///   3. `XHJOB_DATA_DIR` env var (unified data directory)
///   4. Platform default (`/tmp` on Unix, `%TEMP%` on Windows)
pub fn log_file_path(service_name: &str, data_dir: Option<&str>) -> PathBuf {
    resolve_dir_path(data_dir, "XHJOB_LOG_DIR")
        .join(format!("xhjob.{}.log", service_name))
}

/// Resolve a runtime directory path from the priority chain.
///
/// Order:
///   1. Explicit `data_dir` argument (if `Some` and non-empty)
///   2. Fine-grained env var `fine_grained_var` (if set and non-empty)
///   3. Unified `XHJOB_DATA_DIR` env var (if set and non-empty)
///   4. Platform default (`/tmp` on Unix, `%TEMP%` on Windows)
fn resolve_dir_path(data_dir: Option<&str>, fine_grained_var: &str) -> PathBuf {
    if let Some(d) = data_dir {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    if let Ok(d) = std::env::var(fine_grained_var) {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    if let Ok(d) = std::env::var("XHJOB_DATA_DIR") {
        if !d.is_empty() {
            return PathBuf::from(d);
        }
    }
    #[cfg(unix)]
    {
        PathBuf::from("/tmp")
    }
    #[cfg(windows)]
    {
        std::env::temp_dir()
    }
}

/// Read PID from file. Returns None if not present or stale.
pub fn read_pid(service_name: &str, data_dir: Option<&str>) -> Option<u32> {
    let path = pid_file_path(service_name, data_dir);
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
pub fn write_pid(pid: u32, service_name: &str, data_dir: Option<&str>) -> Result<()> {
    let path = pid_file_path(service_name, data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, pid.to_string())?;
    Ok(())
}

/// Remove PID file if exists.
pub fn remove_pid_file(service_name: &str, data_dir: Option<&str>) {
    let _ = std::fs::remove_file(pid_file_path(service_name, data_dir));
    // Also clean up the IPC socket file on Unix (Named Pipe on Windows has no file).
    // This is best-effort: the daemon should clean up on exit, but if it crashed
    // the stale socket file would otherwise block future starts.
    #[cfg(unix)]
    {
        let sock_path = crate::ipc::ipc_path(service_name, data_dir);
        let _ = std::fs::remove_file(&sock_path);
    }
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

/// Query current daemon status for `service_name` with optional `data_dir`.
pub fn status(service_name: &str, data_dir: Option<&str>) -> DaemonStatus {
    match read_pid(service_name, data_dir) {
        Some(pid) if is_process_alive(pid) => DaemonStatus::running(pid),
        _ => DaemonStatus::not_running(),
    }
}

/// Send SIGTERM (Unix) or TerminateProcess (Windows) to the daemon.
pub fn send_terminate(pid: u32, service_name: &str, data_dir: Option<&str>) -> Result<()> {
    #[cfg(unix)]
    {
        let rc = unsafe { kill(pid as i32, 15 /* SIGTERM */) };
        if rc == 0 {
            // wait up to 10 seconds for the process to exit
            for _ in 0..100 {
                if !is_process_alive(pid) {
                    remove_pid_file(service_name, data_dir);
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            // force kill if still alive
            let _ = unsafe { kill(pid as i32, 9 /* SIGKILL */) };
            remove_pid_file(service_name, data_dir);
            Ok(())
        } else {
            Err(XhjobError::Io(std::io::Error::other(
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
                    remove_pid_file(service_name, data_dir);
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            remove_pid_file(service_name, data_dir);
            Ok(())
        }
    }
}

/// Spawn the daemon process for `service_name` with optional `data_dir`.
///
/// See `spawn_daemon` for the cross-platform strategy.
pub fn spawn_daemon(daemon_main: fn() -> (), service_name: &str, data_dir: Option<&str>) -> Result<()> {
    if let Some(pid) = read_pid(service_name, data_dir) {
        if is_process_alive(pid) {
            return Err(XhjobError::DaemonAlreadyRunning);
        }
    }
    #[cfg(unix)]
    {
        unix::spawn_via_double_fork(daemon_main, service_name, data_dir)
    }
    #[cfg(windows)]
    {
        windows::spawn_via_create_process(daemon_main, service_name, data_dir)
    }
}

/// Public entry point: called by PHP `xhjob_start($name, $data_dir)`.
///
/// Returns true if daemon is now running (either already running, or just started).
pub fn start(daemon_main: fn() -> (), service_name: &str, data_dir: Option<&str>) -> Result<bool> {
    // Fast path: a daemon is already running. Check BOTH PID liveness AND IPC
    // socket readiness — checking only PID is racy when the previous daemon is
    // shutting down (PID still alive, socket already closed). Without the
    // socket check, an immediate dispatch would fail with "Connection refused".
    if let Some(pid) = read_pid(service_name, data_dir) {
        if is_process_alive(pid) && ipc_socket_ready(service_name, data_dir) {
            return Ok(true);
        }
    }
    spawn_daemon(daemon_main, service_name, data_dir)?;
    // Wait until BOTH the PID file is written AND the IPC socket is accepting
    // connections. Waiting only on the PID file is racy: the daemon writes its
    // PID before binding the IPC listener, so an immediate dispatch would fail.
    for _ in 0..100 {
        if let Some(pid) = read_pid(service_name, data_dir) {
            if is_process_alive(pid) && ipc_socket_ready(service_name, data_dir) {
                return Ok(true);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(false)
}

/// Probe whether the IPC socket is accepting connections (Unix domain socket
/// or Windows named pipe). Best-effort: returns false on any error.
fn ipc_socket_ready(service_name: &str, data_dir: Option<&str>) -> bool {
    let path = crate::ipc::ipc_path(service_name, data_dir);
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

/// Public entry point: called by PHP `xhjob_stop($name, $data_dir)`.
pub fn stop(service_name: &str, data_dir: Option<&str>) -> Result<bool> {
    match read_pid(service_name, data_dir) {
        Some(pid) if is_process_alive(pid) => {
            send_terminate(pid, service_name, data_dir)?;
            Ok(true)
        }
        _ => {
            remove_pid_file(service_name, data_dir);
            Ok(false)
        }
    }
}

/// Public entry point: called by PHP `xhjob_restart($name, $data_dir)`.
pub fn restart(daemon_main: fn() -> (), service_name: &str, data_dir: Option<&str>) -> Result<bool> {
    let _ = stop(service_name, data_dir);
    std::thread::sleep(Duration::from_millis(500));
    start(daemon_main, service_name, data_dir)
}
