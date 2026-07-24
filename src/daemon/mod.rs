//! Cross-platform daemon process management.

use crate::errors::{Result, XhjobError};
use std::path::PathBuf;
use std::time::Duration;

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
    resolve_dir_path(data_dir, "XHJOB_PID_DIR").join(format!("xhjob.{}.pid", service_name))
}

/// Log file path derived from `service_name` and optional `data_dir`.
///
/// Path resolution priority (highest first):
///   1. `data_dir` argument (if `Some`)
///   2. `XHJOB_LOG_DIR` env var (Unix only, fine-grained override)
///   3. `XHJOB_DATA_DIR` env var (unified data directory)
///   4. Platform default (`/tmp` on Unix, `%TEMP%` on Windows)
pub fn log_file_path(service_name: &str, data_dir: Option<&str>) -> PathBuf {
    resolve_dir_path(data_dir, "XHJOB_LOG_DIR").join(format!("xhjob.{}.log", service_name))
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

/// Read PID (and optional starttime) from file.
///
/// Returns `Some((pid, starttime))` if the PID file exists and the recorded
/// process is still alive (with matching starttime when present).
/// Returns `None` (and removes the stale PID file) if the file is missing,
/// unparseable, or points at a dead/reused PID.
///
/// File format:
///   - New (two lines): `pid\nstarttime`
///   - Legacy (one line): `pid` — parsed with `starttime = None`
pub fn read_pid(service_name: &str, data_dir: Option<&str>) -> Option<(u32, Option<u64>)> {
    let path = pid_file_path(service_name, data_dir);
    let content = std::fs::read_to_string(&path).ok()?;
    let mut lines = content.lines();
    let pid_str = lines.next()?;
    let pid: u32 = pid_str.trim().parse().ok()?;
    // Optional second line: starttime. Absent or unparseable -> None (legacy).
    let starttime: Option<u64> = lines.next().and_then(|s| s.trim().parse::<u64>().ok());
    if is_process_alive_with_starttime(pid, starttime) {
        Some((pid, starttime))
    } else {
        // stale pid file (dead, or PID reused with mismatched starttime)
        let _ = std::fs::remove_file(&path);
        None
    }
}

/// Write PID file atomically.
///
/// When `starttime` is `Some`, writes the new two-line format `pid\nstarttime`
/// so future readers can detect PID reuse. When `None`, writes the legacy
/// single-line format `pid` for backward compatibility.
pub fn write_pid(
    pid: u32,
    starttime: Option<u64>,
    service_name: &str,
    data_dir: Option<&str>,
) -> Result<()> {
    let path = pid_file_path(service_name, data_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = match starttime {
        Some(st) => format!("{}\n{}", pid, st),
        None => pid.to_string(),
    };
    std::fs::write(&path, content)?;
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

/// Read the starttime of a process (Linux: `/proc/<pid>/stat` field 22,
/// in clock ticks). Returns `None` on non-Linux platforms or any read/parse
/// failure, so callers can gracefully degrade to plain liveness checks.
pub fn process_starttime(pid: u32) -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
        // /proc/<pid>/stat format: `pid (comm) state ppid ... starttime ...`
        // `comm` may contain spaces and parentheses, so we cannot simply
        // split the whole line on whitespace — we must start parsing after
        // the *last* ')' (the closing paren of the comm field).
        let after_comm = stat.rsplit_once(')')?.1;
        let fields: Vec<&str> = after_comm.split_whitespace().collect();
        // After the closing ')', fields are: state(0) ppid(1) pgrp(2) session(3)
        // tty_nr(4) tpgid(5) flags(6) minflt(7) cminflt(8) majflt(9) cmajflt(10)
        // utime(11) stime(12) cutime(13) cstime(14) priority(15) nice(16)
        // num_threads(17) itrealvalue(18) starttime(19).
        // This maps to field 22 of the full /proc/<pid>/stat (1-indexed),
        // since `pid`(1) + `comm`(2) precede the ')' — starttime is the 20th
        // field after the ')' (0-indexed 19).
        let st: u64 = fields.get(19)?.parse().ok()?;
        Some(st)
    }
    #[cfg(not(target_os = "linux"))]
    {
        // No /proc filesystem on Windows/macOS — starttime unavailable.
        let _ = pid;
        None
    }
}

/// Check if a process is alive (cross-platform).
///
/// Backward-compatible wrapper: equivalent to `is_process_alive_with_starttime(pid, None)`.
pub fn is_process_alive(pid: u32) -> bool {
    is_process_alive_with_starttime(pid, None)
}

/// Check if a process is alive, optionally verifying its starttime to guard
/// against PID reuse.
///
/// When `expected_starttime` is `Some(expected)`, returns `true` only if the
/// PID is alive AND its current starttime matches `expected` — so a stale
/// PID file pointing at a recycled PID (same numeric PID, different process)
/// is correctly rejected. When `None`, degrades to plain `kill(pid, 0)`
/// liveness (legacy behavior).
pub fn is_process_alive_with_starttime(pid: u32, expected_starttime: Option<u64>) -> bool {
    // P0/P2 fix: pid==0 is never a real daemon process. On Unix,
    // kill(0, 0) tests "can we signal the caller's process group" and
    // always returns 0, so a PID file containing "0" would cause a
    // false "running" report. Reject it up front. Also reject pids that
    // exceed i32::MAX — they cannot be represented as a positive pid_t
    // and `as i32` would produce a negative value (e.g. u32::MAX -> -1,
    // which means "signal all processes I can reach" — catastrophic if
    // running as root).
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    #[cfg(unix)]
    {
        // kill(pid, 0) returns 0 if process exists (EAGAIN/EPERM/ESRCH
        // mean not-plainly-alive from our perspective). We treat any
        // non-zero return as "not alive" for simplicity; the subsequent
        // starttime check would have to be skipped anyway.
        let alive = unsafe { libc_kill(pid as i32, 0) == 0 };
        if !alive {
            return false;
        }
        match expected_starttime {
            None => true,
            Some(expected) => process_starttime(pid) == Some(expected),
        }
    }
    #[cfg(windows)]
    {
        // Windows has no starttime concept — degrade to OpenProcess-based
        // liveness, ignoring expected_starttime.
        let _ = expected_starttime;
        use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let h: HANDLE = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
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
        Self {
            running: false,
            pid: None,
        }
    }
    pub fn running(pid: u32) -> Self {
        Self {
            running: true,
            pid: Some(pid),
        }
    }
}

/// Query current daemon status for `service_name` with optional `data_dir`.
pub fn status(service_name: &str, data_dir: Option<&str>) -> DaemonStatus {
    match read_pid(service_name, data_dir) {
        Some((pid, starttime)) if is_process_alive_with_starttime(pid, starttime) => {
            DaemonStatus::running(pid)
        }
        _ => DaemonStatus::not_running(),
    }
}

/// Decide whether SIGKILL escalation is safe after the SIGTERM poll loop
/// times out.
///
/// Returns `true` only when `expected_starttime` is `Some` — i.e. the PID
/// file used the modern two-line format and `is_process_alive_with_starttime`
/// has been continuously verifying the PID against its recorded starttime
/// throughout the poll. With a validated starttime, a still-alive PID after
/// the timeout is provably the same daemon process, so SIGKILL is safe.
///
/// Returns `false` when `expected_starttime` is `None` (legacy single-line
/// PID file, or no starttime recorded). In that case the poll loop only
/// checked `kill(pid, 0)` liveness — it cannot prove the PID was not reused
/// by an unrelated process while we waited. Escalating to SIGKILL would risk
/// killing the innocent reused process (catastrophic when run as root), so
/// we fail SAFE: refuse SIGKILL and require manual operator investigation.
fn should_sigkill(expected_starttime: Option<u64>) -> bool {
    expected_starttime.is_some()
}

/// Send SIGTERM (Unix) or TerminateProcess (Windows) to the daemon.
///
/// `expected_starttime` (when `Some`) is used to verify the target PID has not
/// been recycled between the caller reading the PID file and us sending the
/// signal — if the daemon died and an unrelated process reused the PID, the
/// starttime check causes the loop to exit early without raising SIGKILL on
/// the innocent process.
///
/// When `expected_starttime` is `None` (legacy PID file with no recorded
/// starttime), the SIGKILL escalation is suppressed: the poll loop only had
/// plain `kill(pid, 0)` liveness to go on, so a still-alive PID after the
/// timeout could be an unrelated reused process. Killing it with SIGKILL
/// (especially when running as root) would be fail-DEADLY, so we instead
/// fail-SAFE: log a warning and return without SIGKILL, leaving manual
/// investigation to the operator. The initial SIGTERM was already sent.
///
/// The SIGKILL grace period aligns with the daemon-side drain timeout
/// (`XHJOB_SHUTDOWN_DRAIN_SECS`, default 30s): we wait `max(10, drain + 5)`
/// seconds for the daemon to drain in-flight tasks and exit cleanly before
/// escalating to SIGKILL. The +5 buffer covers shutdown bookkeeping after the
/// drain deadline fires inside the daemon.
pub fn send_terminate(
    pid: u32,
    service_name: &str,
    data_dir: Option<&str>,
    expected_starttime: Option<u64>,
) -> Result<()> {
    // P0 fix: defensive validation mirroring is_process_alive. A pid of 0
    // or > i32::MAX must never reach kill(): kill(0, sig) would signal the
    // caller's whole process group, and kill(-1, sig) (from u32::MAX as i32)
    // would signal every process the caller can reach — catastrophic when
    // the daemon runs as root. is_process_alive() already rejects these,
    // but send_terminate can be reached independently, so guard here too.
    if pid == 0 || pid > i32::MAX as u32 {
        return Err(XhjobError::Io(std::io::Error::other(format!(
            "refusing to signal invalid pid {}",
            pid
        ))));
    }
    // Task 5: SIGKILL wait time aligned with the daemon's drain timeout so
    // in-flight tasks are not preemptively killed. drain_secs + 5 covers the
    // post-drain shutdown bookkeeping; never go below the original 10s.
    let drain_secs = std::env::var("XHJOB_SHUTDOWN_DRAIN_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(30);
    let wait_secs = std::cmp::max(10u64, drain_secs.saturating_add(5));
    let polls = wait_secs.saturating_mul(10);
    #[cfg(unix)]
    {
        // TOCTOU hardening: between the caller's read_pid (which validated
        // the PID + starttime) and this kill(), a microsecond window exists
        // in which the daemon could exit and the OS could reuse the PID for
        // an unrelated process. Re-validate immediately before signalling;
        // if the PID no longer matches (dead or reused with a mismatched
        // starttime), do not send SIGTERM to a stranger.
        if !is_process_alive_with_starttime(pid, expected_starttime) {
            remove_pid_file(service_name, data_dir);
            return Ok(());
        }
        let rc = unsafe {
            kill(pid as i32, 15 /* SIGTERM */)
        };
        if rc == 0 {
            // Poll for the process to exit. Using is_process_alive_with_starttime
            // means: if the daemon exited and its PID was reused by an unrelated
            // process, the starttime mismatch makes us return Ok(()) early
            // WITHOUT escalating to SIGKILL on the innocent reused PID.
            for _ in 0..polls {
                if !is_process_alive_with_starttime(pid, expected_starttime) {
                    remove_pid_file(service_name, data_dir);
                    return Ok(());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            // Poll loop timed out and the PID is still "alive". Only
            // escalate to SIGKILL when we have a validated starttime (modern
            // two-line PID file): that proves the still-alive PID is the
            // same daemon, not a reused unrelated process. When
            // expected_starttime is None (legacy PID file), the poll loop
            // only checked plain liveness and cannot rule out PID reuse, so
            // SIGKILL is refused to avoid killing an innocent reused PID
            // (fail-SAFE). The operator must investigate manually; the
            // SIGTERM was already delivered.
            if should_sigkill(expected_starttime) {
                let _ = unsafe {
                    kill(pid as i32, 9 /* SIGKILL */)
                };
                remove_pid_file(service_name, data_dir);
            } else {
                tracing::warn!(
                    pid,
                    "PID file has no starttime (legacy format); refusing SIGKILL to avoid \
                     killing an unrelated reused PID. Manual investigation required."
                );
                // Do NOT SIGKILL — return without escalating. The SIGTERM
                // was already sent. Leave PID file removal to the operator
                // since we cannot prove the live PID is still our daemon.
            }
            Ok(())
        } else {
            Err(XhjobError::Io(std::io::Error::other(format!(
                "failed to send SIGTERM to pid {}",
                pid
            ))))
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };
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
            for _ in 0..polls {
                if !is_process_alive_with_starttime(pid, expected_starttime) {
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
pub fn spawn_daemon(
    daemon_main: fn() -> (),
    service_name: &str,
    data_dir: Option<&str>,
) -> Result<()> {
    if let Some((pid, starttime)) = read_pid(service_name, data_dir) {
        if is_process_alive_with_starttime(pid, starttime) {
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
    if let Some((pid, starttime)) = read_pid(service_name, data_dir) {
        if is_process_alive_with_starttime(pid, starttime)
            && ipc_socket_ready(service_name, data_dir)
        {
            return Ok(true);
        }
    }
    spawn_daemon(daemon_main, service_name, data_dir)?;
    // Wait until BOTH the PID file is written AND the IPC socket is accepting
    // connections. Waiting only on the PID file is racy: the daemon writes its
    // PID before binding the IPC listener, so an immediate dispatch would fail.
    for _ in 0..100 {
        if let Some((pid, starttime)) = read_pid(service_name, data_dir) {
            if is_process_alive_with_starttime(pid, starttime)
                && ipc_socket_ready(service_name, data_dir)
            {
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
        UnixStream::connect(&path).map(|_| true).unwrap_or(false)
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
        Some((pid, starttime)) if is_process_alive_with_starttime(pid, starttime) => {
            send_terminate(pid, service_name, data_dir, starttime)?;
            Ok(true)
        }
        _ => {
            remove_pid_file(service_name, data_dir);
            Ok(false)
        }
    }
}

/// Public entry point: called by PHP `xhjob_restart($name, $data_dir)`.
pub fn restart(
    daemon_main: fn() -> (),
    service_name: &str,
    data_dir: Option<&str>,
) -> Result<bool> {
    let _ = stop(service_name, data_dir);
    std::thread::sleep(Duration::from_millis(500));
    start(daemon_main, service_name, data_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a unique temp dir for a test invocation so concurrent
    /// test runs (and the real daemon) cannot collide on PID file paths.
    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "xhjob_test_{}_{}_{}",
            label,
            std::process::id(),
            nanos
        ))
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_process_starttime_returns_some_for_self() {
        // The current test process must be readable via /proc/self/stat on
        // Linux, so process_starttime should yield a concrete tick count.
        let pid = std::process::id();
        let st = process_starttime(pid);
        assert!(
            st.is_some(),
            "expected Some(starttime) for self on Linux, got {:?}",
            st
        );
    }

    #[test]
    fn test_process_starttime_returns_none_for_invalid_pid() {
        // u32::MAX is never a valid PID; /proc/u32::MAX/stat does not exist
        // on Linux (so the read fails), and non-Linux platforms always
        // return None.
        let st = process_starttime(u32::MAX);
        assert_eq!(st, None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_is_process_alive_with_starttime_rejects_mismatch() {
        let pid = std::process::id();
        let real_starttime = process_starttime(pid).expect("self starttime on Linux");
        // Same PID, but a starttime off by one tick must NOT be considered
        // alive — this is exactly the PID-reuse scenario we are guarding
        // against (the numeric PID exists, but it is a different process).
        let mismatched = real_starttime.wrapping_add(1);
        assert_ne!(mismatched, real_starttime, "starttime wraparound collided");
        assert!(
            !is_process_alive_with_starttime(pid, Some(mismatched)),
            "PID + mismatched starttime must be rejected"
        );
        // Sanity: passing the real starttime back should accept the process.
        assert!(
            is_process_alive_with_starttime(pid, Some(real_starttime)),
            "PID + matching starttime must be accepted"
        );
    }

    #[test]
    fn test_legacy_pid_file_backward_compat() {
        // Old single-line PID file format: just the PID number on one line.
        // read_pid must still parse it and report starttime=None.
        let dir = unique_test_dir("legacy");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        let dir_str = dir.to_string_lossy().into_owned();
        let service = "test_legacy_pid_compat";
        let pid = std::process::id();
        let path = pid_file_path(service, Some(&dir_str));
        // Write the legacy single-line format (no newline, no starttime).
        std::fs::write(&path, pid.to_string()).expect("write legacy pid file");
        let result = read_pid(service, Some(&dir_str));
        assert_eq!(
            result,
            Some((pid, None)),
            "legacy single-line PID file must parse with starttime=None"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_new_pid_file_format() {
        // New two-line PID file format: `pid\nstarttime`.
        // read_pid must parse both and verify the live process's starttime.
        let dir = unique_test_dir("newfmt");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        let dir_str = dir.to_string_lossy().into_owned();
        let service = "test_new_pid_format";
        let pid = std::process::id();
        let starttime = process_starttime(pid).expect("self starttime on Linux");
        let path = pid_file_path(service, Some(&dir_str));
        std::fs::write(&path, format!("{}\n{}", pid, starttime)).expect("write new pid file");
        let result = read_pid(service, Some(&dir_str));
        assert_eq!(
            result,
            Some((pid, Some(starttime))),
            "two-line PID file must parse with pid + starttime"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn repro_send_terminate_no_sigkill_on_legacy_pid() {
        // Regression for the High-severity PID-reuse bug: when the PID file
        // is in legacy single-line format, read_pid yields starttime=None and
        // send_terminate's poll loop degrades to plain kill(pid,0) liveness.
        // After the poll timeout, escalating to SIGKILL on a still-alive PID
        // would kill an UNRELATED reused process (catastrophic when run as
        // root). The fix extracts the SIGKILL-escalation DECISION into
        // should_sigkill(), which must refuse SIGKILL when starttime is None
        // (fail-SAFE) and only permit it when a starttime was recorded (modern
        // two-line PID file, where starttime validation proves the PID is
        // still the daemon).
        assert!(
            !should_sigkill(None),
            "legacy PID (no starttime) must not SIGKILL — possible PID reuse"
        );
        assert!(
            should_sigkill(Some(123_456_789)),
            "modern PID with starttime may SIGKILL after validation"
        );
    }
}
