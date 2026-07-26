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
pub(crate) fn resolve_dir_path(data_dir: Option<&str>, fine_grained_var: &str) -> PathBuf {
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

// =========================================================================
// PHP runtime symbol lookup (for extension-load dedup)
// =========================================================================
//
// PHP exposes the SAPI name and loaded ini paths as GLOBAL VARIABLES (not
// linkable C functions): `sapi_module.name`, `php_ini_opened_path`, and
// `php_ini_scanned_files`. The userland `php_sapi_name()` / `php_ini_loaded_file()`
// functions are `static inline` wrappers in PHP headers, so they have no
// linkable symbol entry — we must read the underlying globals directly.
//
// To keep the unit-test binary linkable (it does NOT link against the PHP
// runtime), we resolve these symbols at RUNTIME via `dlsym(RTLD_DEFAULT, ...)`
// on Unix and `GetModuleHandleA(NULL)` + `GetProcAddress(...)` on Windows.
// When the symbols are unavailable (test binary, or a non-PHP host), every
// lookup returns null and `xhjob_loaded_via_php_ini()` conservatively
// returns false (caller keeps `-d extension=xhjob.so`).

/// Minimal `repr(C)` view of PHP's `sapi_module_struct`. We only read the
/// first field (`name: *mut c_char`), so the remaining fields are omitted —
/// the struct layout is irrelevant for a single-field prefix read.
#[repr(C)]
struct SapiModuleStruct {
    name: *mut std::os::raw::c_char,
}

/// `RTLD_DEFAULT` for `dlsym`. The libc crate does not expose this constant
/// on every Unix target (notably Linux glibc and macOS), so define it here.
/// Values: glibc/Linux = `NULL`; macOS/BSD = `(void*)-2`.
#[cfg(unix)]
const RTLD_DEFAULT: *mut std::ffi::c_void = {
    #[cfg(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
    ))]
    {
        -2isize as *mut std::ffi::c_void
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
    )))]
    {
        std::ptr::null_mut()
    }
};

/// Look up a PHP runtime symbol at runtime (not link-time). Returns null if
/// the symbol isn't available (e.g., in the unit-test binary which doesn't
/// link against PHP).
///
/// # Safety
///
/// `name` MUST be a NUL-terminated C string (e.g. `b"php_ini_opened_path\0"`).
unsafe fn lookup_php_symbol(name: &[u8]) -> *mut std::ffi::c_void {
    debug_assert!(
        name.ends_with(b"\0"),
        "lookup_php_symbol: name must be NUL-terminated"
    );
    #[cfg(unix)]
    {
        // SAFETY: `dlsym` with `RTLD_DEFAULT` searches the global symbol
        // table of the current process. The `name` is a NUL-terminated C
        // string. The lookup is read-only and has no side effects. When the
        // symbol is not found, `dlsym` returns NULL and sets `dlerror`.
        libc::dlsym(RTLD_DEFAULT, name.as_ptr() as *const _)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        // SAFETY: `GetModuleHandleA(NULL)` returns a borrowed handle to the
        // main executable (php.exe / php-fpm.exe) without incrementing its
        // refcount. Read-only, no side effects. Returns 0 (NULL) on failure.
        let hmod = GetModuleHandleA(std::ptr::null());
        if hmod == 0 {
            return std::ptr::null_mut();
        }
        // SAFETY: `name` is a NUL-terminated C string. `GetProcAddress` does
        // not mutate it. Returns `None` when the symbol is not exported by
        // the module.
        let proc = GetProcAddress(hmod, name.as_ptr() as *const u8);
        match proc {
            Some(f) => f as *mut std::ffi::c_void,
            None => std::ptr::null_mut(),
        }
    }
}

/// Read the SAPI name (e.g. `"cli"`, `"fpm-fcgi"`, `"apache2handler"`) from
/// the PHP `sapi_module` global. Returns `None` on any FFI/error (e.g., when
/// the PHP runtime is not loaded, as in unit tests).
///
/// Exposed as `pub(crate)` so `xhjob_diag()` can include the SAPI name in
/// the diagnostic JSON. The function is safe to call from any context: all
/// FFI pointer dereferences are guarded by null checks, and the underlying
/// PHP globals are read-only (populated once at SAPI startup, kept valid for
/// the process lifetime).
pub(crate) fn read_sapi_name() -> Option<String> {
    // SAFETY: `lookup_php_symbol` is read-only and returns null gracefully
    // when the symbol is unavailable (test binary, non-PHP host). The
    // returned pointer is null-checked before dereferencing; PHP guarantees
    // the `sapi_module.name` field is a stable NUL-terminated C string for
    // the process lifetime once SAPI startup completes.
    unsafe {
        let sym = lookup_php_symbol(b"sapi_module\0");
        if sym.is_null() {
            return None;
        }
        let sapi_module_ptr = sym as *mut SapiModuleStruct;
        let name_ptr = (*sapi_module_ptr).name;
        if name_ptr.is_null() {
            return None;
        }
        let cstr = std::ffi::CStr::from_ptr(name_ptr);
        Some(cstr.to_string_lossy().into_owned())
    }
}

/// Read a PHP global `char *` variable (e.g. `php_ini_opened_path`,
/// `php_ini_scanned_files`). Returns `None` on any FFI/error or when the
/// variable itself is NULL (e.g. no ini file loaded).
///
/// # Safety
///
/// The function dereferences raw FFI pointers obtained via `lookup_php_symbol`.
unsafe fn read_php_cstring_global(symbol: &[u8]) -> Option<String> {
    let sym = lookup_php_symbol(symbol);
    if sym.is_null() {
        return None;
    }
    // SAFETY: `sym` is the address of a `char *` global variable. We
    // dereference once to read the `char *` value, then read the C string it
    // points to. PHP owns the string memory and keeps it alive for the
    // process lifetime.
    let char_ptr_ptr = sym as *mut *mut std::os::raw::c_char;
    let char_ptr = *char_ptr_ptr;
    if char_ptr.is_null() {
        return None;
    }
    let cstr = std::ffi::CStr::from_ptr(char_ptr);
    Some(cstr.to_string_lossy().into_owned())
}

/// Detect whether the xhjob extension is loaded via php.ini (vs. via `-d
/// extension=` on the CLI). When loaded via php.ini, the spawned daemon PHP
/// process will automatically load xhjob by reading the same php.ini, so we
/// must NOT pass `-d extension=xhjob.so` again (would trigger PHP's
/// "Module already loaded" warning).
///
/// Heuristic:
///   1. FPM (`fpm-fcgi`) and Apache (`apache2handler`, `apache2filter`)
///      SAPIs always read php.ini and scan conf.d directories, so xhjob is
///      loaded via ini — return true.
///   2. CLI (`cli`) SAPI may or may not have xhjob in php.ini — fall back to
///      reading the loaded php.ini path + scanned files and grepping their
///      contents for `extension=...xhjob`.
///   3. On any FFI/error: return false (conservative — keep `-d extension=`).
pub fn xhjob_loaded_via_php_ini() -> bool {
    let sapi = read_sapi_name().unwrap_or_default();
    // FPM / Apache SAPIs always read php.ini + scan conf.d, and the
    // "Module already loaded" warning in production proves xhjob is in there.
    if sapi.starts_with("fpm") || sapi.contains("apache") {
        return true;
    }
    // For CLI (and any other SAPI, conservatively) — read the loaded ini
    // file path + scanned files and grep their contents for an
    // `extension=...xhjob` directive.
    let opened = unsafe { read_php_cstring_global(b"php_ini_opened_path\0") }.unwrap_or_default();
    let scanned = unsafe { read_php_cstring_global(b"php_ini_scanned_files\0") }.unwrap_or_default();
    ini_references_xhjob_extension(&opened, &scanned)
}

/// Strip a trailing PHP ini inline comment from a directive value. PHP
/// treats ` ;` (whitespace + semicolon) as the start of an inline comment
/// within a value, but NOT a leading `;` (which is part of the value). We
/// also handle ` #` for robustness, since PHP 8+ accepts `#` as a comment
/// marker in some contexts.
fn strip_ini_inline_comment(value: &str) -> &str {
    let cut = value
        .find(" ;")
        .or_else(|| value.find(" #"))
        .unwrap_or(value.len());
    value[..cut].trim_end()
}

/// Grep the loaded php.ini file (and scanned ini files) for an
/// `extension=...xhjob` (or `zend_extension=...xhjob`) directive.
///
/// `opened` is the path returned by PHP's `php_ini_opened_path` global
/// (the primary php.ini file). `scanned` is the comma-and-newline-separated
/// list of paths returned by `php_ini_scanned_files` (the conf.d scan
/// result). Both are read best-effort; missing/unreadable files are skipped.
fn ini_references_xhjob_extension(opened: &str, scanned: &str) -> bool {
    let mut haystack = String::new();
    // Best-effort: read the primary loaded php.ini file.
    if !opened.is_empty() {
        if let Ok(contents) = std::fs::read_to_string(opened) {
            haystack.push_str(&contents);
            haystack.push('\n');
        }
    }
    // Best-effort: read each scanned ini file. The `php_ini_scanned_files`
    // format is "path1,\npath2,\npath3\n" (comma + newline separator).
    for entry in scanned.split([',', '\n']) {
        let path = entry.trim();
        if !path.is_empty() {
            if let Ok(contents) = std::fs::read_to_string(path) {
                haystack.push_str(&contents);
                haystack.push('\n');
            }
        }
    }
    // Grep for `extension=...xhjob` directives. PHP ini directive names are
    // case-insensitive; we match the extension basename case-insensitively
    // for cross-platform robustness (Unix is case-sensitive, Windows isn't).
    // PHP's ini parser also tolerates whitespace around the `=` separator, so
    // `extension = xhjob.so` is equivalent to `extension=xhjob.so`.
    for line in haystack.lines() {
        let trimmed = line.trim();
        // Skip comments and blank lines.
        if trimmed.starts_with(';') || trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_ascii_lowercase();
        // Split on the first `=` — the key is everything before, the value
        // everything after (PHP does not allow `=` in directive names).
        let (key, value) = match lower.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim()),
            None => continue,
        };
        if key != "extension" && key != "zend_extension" {
            continue;
        }
        // Strip trailing inline comments. PHP ini treats ` ;` (space-semicolon)
        // as a comment start within a value, but NOT a leading `;` (which is
        // part of the value). For robustness, strip from the first ` ;` and
        // also handle ` #`.
        let value = strip_ini_inline_comment(value);
        // Match `xhjob`, `xhjob.so`, or any path ending in `xhjob.so`.
        // (PHP normalizes `extension=xhjob` and `extension=xhjob.so` to the
        // same load on Unix; absolute paths are also valid.)
        if value == "xhjob"
            || value == "xhjob.so"
            || value.ends_with("/xhjob.so")
            || value.ends_with("\\xhjob.so")
        {
            return true;
        }
    }
    false
}

/// Test-friendly helper: returns the inverse of `xhjob_loaded_via_php_ini()`,
/// i.e. whether the caller SHOULD inject `-d extension=xhjob.so`. Extracted
/// as a named function so the spawn paths (`unix.rs` / `windows.rs`) and the
/// unit tests share a single decision point — no risk of the two diverging.
pub(crate) fn should_inject_extension_arg() -> bool {
    !xhjob_loaded_via_php_ini()
}

// =========================================================================
// spawn-after-wait diagnostic
// =========================================================================

/// Read the last 2KB of a file as a UTF-8-lossy `String`. Used to capture
/// the daemon log file tail when the spawned daemon child exits prematurely
/// (setsid error, missing PHP binary, `-r` parse error, etc.), so the
/// diagnostic error returned to the caller carries the actual failure cause
/// instead of a generic "spawn ok but daemon never came up".
///
/// On any error (file missing, permission denied, seek past start, ...) the
/// function returns the placeholder `"<no log file or empty>"` so the caller
/// can unconditionally format the result into an error message without an
/// extra branch.
pub(crate) fn read_log_tail(path: &std::path::Path) -> String {
    const TAIL_BYTES: u64 = 2048;
    // Open the file read-only. If it doesn't exist (daemon never got far
    // enough to create it) or cannot be read, return the placeholder.
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return "<no log file or empty>".to_string(),
    };
    let len = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return "<no log file or empty>".to_string(),
    };
    // If the file is smaller than TAIL_BYTES, just read the whole thing.
    let offset = len.saturating_sub(TAIL_BYTES);
    use std::io::{Read, Seek, SeekFrom};
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return "<no log file or empty>".to_string();
    }
    let mut buf = Vec::with_capacity(TAIL_BYTES as usize);
    if file.read_to_end(&mut buf).is_err() {
        return "<no log file or empty>".to_string();
    }
    if buf.is_empty() {
        return "<no log file or empty>".to_string();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Read the last 2KB of the daemon log file for `service_name` + optional
/// `data_dir`, resolving the log path via the same priority chain as
/// `log_file_path`. Returns the placeholder string when the log file does
/// not exist or cannot be read.
///
/// Wraps `log_file_path` + `read_log_tail` so `xhjob_start` can build a
/// diagnostic message in one call without re-deriving the path.
pub(crate) fn read_daemon_log_tail(service_name: &str, data_dir: Option<&str>) -> String {
    let path = log_file_path(service_name, data_dir);
    read_log_tail(&path)
}

/// Resolve the final data directory for `data_dir` (using the same priority
/// chain as `pid_file_path` / `log_file_path`) and return it as a string.
/// Used by `xhjob_diag()` so the diagnostic JSON shows the user the actual
/// resolved directory instead of just the raw `data_dir` argument.
pub(crate) fn resolve_data_dir(data_dir: Option<&str>) -> String {
    resolve_dir_path(data_dir, "XHJOB_PID_DIR")
        .to_string_lossy()
        .into_owned()
}

/// Read the PHP `open_basedir` ini entry at runtime via the
/// `zend_ini_string_ex` FFI symbol. Returns `None` on any failure (test
/// binary not linked against PHP, symbol missing, ini not set, ...).
///
/// The function looks up `zend_ini_string_ex` via dlsym/GetProcAddress and
/// calls it with `"open_basedir"`. The returned `zend_string*` is read via
/// a `#[repr(C)]` mirror of PHP's struct layout; the `val` field is at
/// offset `sizeof(gc) + sizeof(h) + sizeof(len)`.
///
/// Used by `xhjob_diag()` so the diagnostic JSON reports the active
/// `open_basedir` restriction (a common cause of daemon spawn failures in
/// PHP-FPM contexts where the worker is sandboxed to specific paths).
pub(crate) fn read_open_basedir() -> Option<String> {
    // SAFETY: the FFI lookup is read-only and returns null gracefully when
    // the symbol is unavailable. The returned zend_string* is null-checked
    // before reading. PHP ini entries are stable for the process lifetime
    // after MINIT, so reading the value at any point after startup is safe.
    unsafe {
        let sym = lookup_php_symbol(b"zend_ini_string_ex\0");
        if sym.is_null() {
            return None;
        }
        // Signature: zend_string* zend_ini_string_ex(const char* name,
        //                                           size_t name_length,
        //                                           zend_bool orig,
        //                                           zend_bool *exists);
        // Pass exists=NULL so the function does not try to write the
        // "found" flag back through our (uninitialized) pointer.
        let func: extern "C" fn(
            *const std::os::raw::c_char,
            usize,
            std::os::raw::c_int,
            *mut std::os::raw::c_uchar,
        ) -> *mut ZendString = std::mem::transmute(sym);
        let name = b"open_basedir\0";
        let zs_ptr = func(name.as_ptr() as *const _, 12, 0, std::ptr::null_mut());
        if zs_ptr.is_null() {
            return None;
        }
        // Read the `len` field at the repr(C) offset, then read `len` bytes
        // from `val` (which immediately follows `len` in the layout).
        let len = (*zs_ptr).len as usize;
        if len == 0 {
            return Some(String::new());
        }
        // Cap the read at 4KB to guard against a corrupt len field causing
        // an outsized allocation. The real `open_basedir` value is always a
        // short path list well under 4KB.
        let cap = len.min(4096);
        let val_ptr = (&(*zs_ptr).val) as *const std::os::raw::c_char;
        let slice = std::slice::from_raw_parts(val_ptr as *const u8, cap);
        Some(String::from_utf8_lossy(slice).into_owned())
    }
}

/// `#[repr(C)]` mirror of PHP's `zend_string` struct used by
/// `read_open_basedir`. Only the fields we read (`len`, `val`) are
/// declared; the layout is determined by `repr(C)` so the compiler matches
/// PHP's C-side layout.
///
/// Layout (PHP 8.x on 64-bit):
///   - `gc`: zend_refcounted_h = { uint32_t refcount; union { uint32_t
///     type_info; } u; } = 8 bytes
///   - `h`: zend_ulong = size_t = 8 bytes
///   - `len`: size_t = 8 bytes
///   - `val`: char[1] flexible array member — accessed via pointer
///     arithmetic from the address of `val[0]`
///
/// `usize` is used for `h` and `len` so the layout is correct on both
/// 32-bit and 64-bit targets (matching PHP's `size_t`).
#[repr(C)]
struct ZendString {
    _gc: [u32; 2], // zend_refcounted_h (8 bytes on all platforms)
    _h: usize,     // zend_ulong (= size_t)
    len: usize,    // size_t
    val: std::os::raw::c_char, // flexible array member — address taken only
}

/// Wait briefly (500ms, polled every 50ms) for `child` to exit. Used by the
/// daemon spawn paths (`unix::spawn_via_double_fork` /
/// `windows::spawn_via_create_process`) right after `cmd.spawn()`: if the
/// child PHP process exits immediately (setsid failure, missing PHP binary,
/// `-r` parse error), we surface the failure as an `Err` carrying the exit
/// status + daemon log tail — instead of letting the parent time out after
/// 10s returning `Ok(false)` with no diagnostic.
///
/// Returns:
/// - `Ok(())` if the child is still running after 500ms (normal detach path).
///   The child handle is `mem::forget`-en so the detached process is NOT
///   killed when the `Child` drop runs in the parent — we explicitly want
///   the daemon to keep running.
/// - `Err(XhjobError::Io(...))` if the child exited within 500ms. The error
///   message includes the exit status, service name, log file path, and the
///   last 2KB of the log file for diagnostics.
/// - `Err(...)` if `try_wait` itself fails.
///
/// Extracted as a `pub(crate)` helper so unit tests can exercise the
/// "child-exits-immediately" branch directly by spawning `/bin/false` (Unix)
/// or `cmd /c exit 1` (Windows) without mocking the entire daemon spawn.
pub(crate) fn check_child_alive(
    mut child: std::process::Child,
    log_path: &std::path::Path,
    service_name: &str,
) -> Result<()> {
    // Poll try_wait() every 50ms for 500ms (10 polls). If the child has not
    // exited by then, treat it as the normal detach path. try_wait() is in
    // std (unlike wait_timeout which is Unix-only); this approach works
    // cross-platform with no extra dependencies.
    let poll_interval = Duration::from_millis(50);
    let poll_count = 10u32;
    for _ in 0..poll_count {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Child already exited — definitely failed. Read the log tail
                // for diagnostics and return a structured error.
                let log_tail = read_log_tail(log_path);
                return Err(XhjobError::Io(std::io::Error::other(format!(
                    "daemon child exited prematurely with status {:?}; service={}; \
                     log file: {}; log tail (last 2KB):\n{}",
                    status,
                    service_name,
                    log_path.display(),
                    log_tail,
                ))));
            }
            Ok(None) => {
                // Child still running — keep polling until the deadline.
                std::thread::sleep(poll_interval);
            }
            Err(e) => {
                // try_wait itself failed — propagate.
                return Err(XhjobError::Io(e));
            }
        }
    }
    // 500ms elapsed and the child is still running — normal detach path.
    // Drop the handle WITHOUT killing the child: std::process::Child does
    // not auto-kill on drop in std (only some platform-specific Drop
    // implementations do, and we explicitly want the daemon to survive).
    // `mem::forget` ensures the platform Drop (which on some Rust versions
    // attempts to close handles / reap the child) does not run, leaving the
    // daemon fully detached.
    std::mem::forget(child);
    Ok(())
}

// =========================================================================
// data_dir writability pre-check
// =========================================================================

/// Current real user ID (Unix). On Windows returns 0 (no concept of uid).
#[cfg(unix)]
pub(crate) fn current_uid() -> u32 {
    // SAFETY: `getuid()` is always safe to call and has no side effects.
    unsafe { libc::getuid() }
}

/// Current real group ID (Unix). On Windows returns 0 (no concept of gid).
#[cfg(unix)]
pub(crate) fn current_gid() -> u32 {
    // SAFETY: `getgid()` is always safe to call and has no side effects.
    unsafe { libc::getgid() }
}

/// Stub on non-Unix (Windows) — no uid concept. Kept for cross-platform
/// diagnostic formatting.
#[cfg(not(unix))]
pub(crate) fn current_uid() -> u32 {
    0
}

/// Stub on non-Unix (Windows) — no gid concept. Kept for cross-platform
/// diagnostic formatting.
#[cfg(not(unix))]
pub(crate) fn current_gid() -> u32 {
    0
}

/// Pre-flight check: ensure the resolved `data_dir` is writable by the
/// current user. Returns `Ok(())` if writable, `Err(XhjobError::Io(...))`
/// with a detailed message including the path, current uid/gid, PHP binary
/// path, and log file path — so the user can fix permissions without
/// grepping logs.
///
/// Resolution follows the same priority chain as `resolve_dir_path`:
///   1. Explicit `data_dir` argument
///   2. `XHJOB_PID_DIR` env var (Unix only)
///   3. `XHJOB_DATA_DIR` env var
///   4. Platform default (`/tmp` on Unix, `%TEMP%` on Windows)
pub(crate) fn check_data_dir_writable(service_name: &str, data_dir: Option<&str>) -> Result<()> {
    // Resolve the final dir using the same priority chain as pid_file_path /
    // log_file_path so the pre-check reflects exactly where the daemon will
    // try to write its PID file + IPC socket + log.
    let dir = resolve_dir_path(data_dir, "XHJOB_PID_DIR");

    // Ensure the directory exists (create_dir_all). If that fails, return
    // an error with the path + uid/gid + PHP binary path so the operator
    // can fix permissions without grepping logs.
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Err(XhjobError::Io(std::io::Error::other(format!(
            "data_dir '{}' is not writable by current user (uid={}, gid={}); \
             failed to create_dir_all: {}; PHP binary: {:?}; log file: {}",
            dir.display(),
            current_uid(),
            current_gid(),
            e,
            std::env::current_exe(),
            log_file_path(service_name, data_dir).display(),
        ))));
    }

    // Write a probe file to confirm write permission. create_dir_all
    // succeeding does not guarantee writability (e.g. on read-only
    // filesystems, or when the dir was created by a more-privileged
    // pre-existing path component).
    let probe = dir.join(".xhjob_write_test");
    if let Err(e) = std::fs::write(&probe, b"test") {
        return Err(XhjobError::Io(std::io::Error::other(format!(
            "data_dir '{}' is not writable by current user (uid={}, gid={}); \
             write probe failed: {}; PHP binary: {:?}; log file: {}",
            dir.display(),
            current_uid(),
            current_gid(),
            e,
            std::env::current_exe(),
            log_file_path(service_name, data_dir).display(),
        ))));
    }
    // Clean up the probe file. Best-effort: ignore errors (the file is tiny
    // and named uniquely enough not to collide with real daemon files).
    let _ = std::fs::remove_file(&probe);
    Ok(())
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
    // Pre-flight: verify data_dir is writable before spawning, so we can
    // return a clear, actionable error (with path/uid/gid/PHP binary/log
    // file) instead of a silent daemon spawn failure that the PHP side can
    // only observe as a false return.
    check_data_dir_writable(service_name, data_dir)?;

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

    // =====================================================================
    // Task 1 tests: dedup `-d extension=xhjob.so` loading
    // =====================================================================

    #[test]
    fn test_xhjob_loaded_via_php_ini_returns_bool() {
        // In the unit-test binary the PHP runtime is NOT loaded, so all FFI
        // symbol lookups return null and `xhjob_loaded_via_php_ini` must
        // conservatively return false (do not skip `-d extension=`). We can't
        // fully mock the FFI here, so this is a smoke test that the function
        // does not panic and returns a bool.
        let loaded = xhjob_loaded_via_php_ini();
        assert!(
            !loaded,
            "in test binary (no PHP runtime) the detector must return false so \
             the daemon spawn path keeps `-d extension=xhjob.so` (fail-safe)"
        );
    }

    #[test]
    fn test_dedup_logic_in_command_args() {
        // Smoke test the extracted decision helper. In the test binary the
        // helper returns true (inject `-d extension=`), since the FFI lookup
        // fails and `xhjob_loaded_via_php_ini` returns false.
        let should_inject = should_inject_extension_arg();
        // Just assert it returns a bool without panicking; the actual value
        // depends on whether the PHP runtime is loaded (it isn't in tests).
        let _ = should_inject;
        assert!(
            should_inject,
            "in test binary (no PHP runtime) the helper must return true so \
             `-d extension=xhjob.so` is injected (fail-safe)"
        );
    }

    #[test]
    fn test_ini_references_xhjob_extension_detects_directive() {
        // Pure-logic test of the ini-content grep. We write a temp ini file
        // containing `extension=xhjob.so` and verify the helper detects it.
        let dir = unique_test_dir("ini_detect");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        let ini = dir.join("xhjob.ini");
        std::fs::write(
            &ini,
            "; comment line\n\
             extension=grpc.so\n\
             extension=xhjob.so\n\
             zend_extension=opcache.so\n",
        )
        .expect("write test ini");
        let ini_str = ini.to_string_lossy().into_owned();
        assert!(
            ini_references_xhjob_extension(&ini_str, ""),
            "extension=xhjob.so in loaded php.ini must be detected"
        );

        // Variants — case-insensitive, basename-only, full path.
        assert!(
            ini_references_xhjob_extension("", &format!("{},\n", ini_str)),
            "extension=xhjob.so in scanned ini files must be detected"
        );

        // Negative: ini without xhjob.
        let other_ini = dir.join("other.ini");
        std::fs::write(&other_ini, "extension=grpc.so\nextension=opcache.so\n")
            .expect("write other ini");
        let other_str = other_ini.to_string_lossy().into_owned();
        assert!(
            !ini_references_xhjob_extension(&other_str, ""),
            "ini without xhjob must NOT be reported as containing xhjob"
        );

        // Empty inputs.
        assert!(
            !ini_references_xhjob_extension("", ""),
            "empty ini inputs must not be reported as containing xhjob"
        );

        // Various forms of the directive (lowercase, .so omitted, full path).
        let variants = [
            "extension=xhjob",
            "EXTENSION=XHJOB.SO",
            "zend_extension=/usr/lib/php/xhjob.so",
            "extension = xhjob.so",
            "extension=xhjob.so ; trailing comment",
        ];
        for v in variants {
            let v_ini = dir.join(format!("v_{}.ini", hash_str(v)));
            std::fs::write(&v_ini, format!("{}\n", v)).expect("write variant ini");
            let v_str = v_ini.to_string_lossy().into_owned();
            assert!(
                ini_references_xhjob_extension(&v_str, ""),
                "directive form {:?} must be detected as xhjob",
                v
            );
        }

        // Commented-out directives must NOT be detected.
        let commented = dir.join("commented.ini");
        std::fs::write(&commented, "; extension=xhjob.so\n# extension=xhjob\n")
            .expect("write commented ini");
        let commented_str = commented.to_string_lossy().into_owned();
        assert!(
            !ini_references_xhjob_extension(&commented_str, ""),
            "commented-out extension=xhjob must NOT be detected"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Tiny deterministic string hasher for generating unique filenames in
    /// tests (avoids pulling in a hashing crate). Not cryptographically
    /// secure — just a collision-resistant-enough mixing for test labels.
    fn hash_str(s: &str) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in s.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    }

    // =====================================================================
    // Task 2 tests: data_dir writability pre-check
    // =====================================================================

    #[test]
    fn test_check_data_dir_writable_ok() {
        // A freshly-created temp dir must pass the writability check.
        let dir = unique_test_dir("writable");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        let dir_str = dir.to_string_lossy().into_owned();
        let result = check_data_dir_writable("test_writable", Some(&dir_str));
        assert!(
            result.is_ok(),
            "freshly created temp dir must pass writability check, got: {:?}",
            result
        );
        // The probe file must have been cleaned up.
        let probe = dir.join(".xhjob_write_test");
        assert!(
            !probe.exists(),
            "check_data_dir_writable must clean up its .xhjob_write_test probe"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_check_data_dir_writable_fails_on_unwritable() {
        // Skip when running as root: root bypasses Unix permission checks,
        // so a 0o555 dir remains writable and the test cannot exercise the
        // failure path. This is the standard pattern for permission tests.
        if current_uid() == 0 {
            eprintln!(
                "skipping test_check_data_dir_writable_fails_on_unwritable: \
                 running as root (uid=0) bypasses Unix permission checks"
            );
            return;
        }

        let dir = unique_test_dir("unwritable");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        // Strip write permission from the dir (read+execute only).
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))
            .expect("chmod 0o555");
        let dir_str = dir.to_string_lossy().into_owned();
        let result = check_data_dir_writable("test_unwritable", Some(&dir_str));
        assert!(
            result.is_err(),
            "read-only (0o555) data_dir must fail writability check, got: {:?}",
            result
        );
        // Verify the error message is actionable (contains path + uid).
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not writable"),
            "error message must explain the failure, got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("uid="),
            "error message must include uid for diagnostics, got: {}",
            err_msg
        );

        // Restore write permission so cleanup can succeed.
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_check_data_dir_writable_fails_on_path_under_file() {
        // Robust unwritable-path test that works even when running as root
        // (root bypasses Unix permission checks). We create a regular FILE
        // and try to use a path UNDER it as data_dir — create_dir_all fails
        // with ENOTDIR regardless of user privileges.
        let parent = unique_test_dir("file_blocker_parent");
        std::fs::create_dir_all(&parent).expect("mkdir parent");
        let blocker = parent.join("blocker_file");
        std::fs::write(&blocker, b"i am a file, not a dir").expect("write blocker");
        // data_dir = <parent>/blocker_file/sub — parent path component is a
        // file, so create_dir_all cannot create this.
        let bad_dir = blocker.join("sub");
        let bad_dir_str = bad_dir.to_string_lossy().into_owned();
        let result = check_data_dir_writable("test_file_blocker", Some(&bad_dir_str));
        assert!(
            result.is_err(),
            "data_dir path under a regular file must fail writability check, got: {:?}",
            result
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("not writable"),
            "error message must explain the failure, got: {}",
            err_msg
        );
        let _ = std::fs::remove_dir_all(&parent);
    }

    // =====================================================================
    // Task 3 tests: spawn-after-wait diagnostic
    // =====================================================================

    #[test]
    fn test_read_log_tail_returns_placeholder_when_file_missing() {
        // Non-existent path must yield the placeholder, not a panic.
        let path = unique_test_dir("no_log").join("missing.log");
        let tail = read_log_tail(&path);
        assert_eq!(
            tail, "<no log file or empty>",
            "missing log file must yield the placeholder"
        );
    }

    #[test]
    fn test_read_log_tail_reads_last_2kb() {
        // Write a file larger than 2KB and verify read_log_tail returns
        // exactly the last 2KB (UTF-8 lossy). We write a deterministic
        // 4KB file where the last 2KB is uniquely identifiable.
        let dir = unique_test_dir("log_tail");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        let path = dir.join("daemon.log");
        // Build a 4KB payload: first 2KB = 'A' * 2048, last 2KB = 'B' * 2048.
        let mut payload = Vec::with_capacity(4096);
        payload.extend(std::iter::repeat_n(b'A', 2048));
        payload.extend(std::iter::repeat_n(b'B', 2048));
        std::fs::write(&path, &payload).expect("write log file");
        let tail = read_log_tail(&path);
        assert_eq!(
            tail.len(),
            2048,
            "read_log_tail must return exactly the last 2KB, got {} bytes",
            tail.len()
        );
        assert!(
            tail.chars().all(|c| c == 'B'),
            "tail must contain only 'B' chars (last 2KB), got: {:?}",
            &tail[..tail.len().min(64)]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_log_tail_reads_whole_small_file() {
        // A file smaller than 2KB must be returned in full (not padded).
        let dir = unique_test_dir("log_small");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        let path = dir.join("small.log");
        let body = "short log line\n";
        std::fs::write(&path, body).expect("write log file");
        let tail = read_log_tail(&path);
        assert_eq!(tail, body, "small log file must be returned in full");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Spawn a child process that exits immediately with a non-zero status
    /// (`/bin/false` on Unix, `cmd /c exit 1` on Windows). The
    /// `check_child_alive` helper must catch the immediate exit and return
    /// an `Err` carrying the exit status + the (placeholder) log tail.
    ///
    /// This exercises the same code path that fires when the real daemon
    /// spawn fails because the spawned PHP process exited prematurely
    /// (setsid error, missing PHP binary, `-r` parse error, etc.).
    #[test]
    fn test_spawn_returns_err_when_child_exits_immediately() {
        let dir = unique_test_dir("child_exits");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        let log_path = dir.join("daemon.log");

        #[cfg(unix)]
        let mut cmd = {
            let mut c = std::process::Command::new("/bin/false");
            // /bin/false exits with status 1 immediately.
            c.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = std::process::Command::new("cmd");
            c.arg("/c").arg("exit").arg("1");
            c.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            c
        };

        let child = cmd.spawn().expect("spawn test child");
        let result = check_child_alive(child, &log_path, "test_child_exits");
        assert!(
            result.is_err(),
            "check_child_alive must return Err when the child exits within 500ms, got: {:?}",
            result
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("exited prematurely"),
            "error message must mention premature exit, got: {}",
            err_msg
        );
        assert!(
            err_msg.contains("service=test_child_exits"),
            "error message must include service name, got: {}",
            err_msg
        );
        // The log file does not exist, so the placeholder must appear.
        assert!(
            err_msg.contains("<no log file or empty>"),
            "error message must include the placeholder when log file is missing, got: {}",
            err_msg
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Spawn a long-running child (`sleep 5` on Unix, `ping -n 5 127.0.0.1`
    /// on Windows) and verify `check_child_alive` returns `Ok(())` without
    /// killing the child. This is the normal-detach path.
    ///
    /// IMPORTANT: the child is `mem::forget`-en inside the helper, so it
    /// survives the test. We explicitly kill + wait it afterwards to avoid
    /// leaking a stray process into the test runner.
    #[test]
    fn test_check_child_alive_returns_ok_for_long_running_child() {
        #[cfg(unix)]
        let mut cmd = {
            let mut c = std::process::Command::new("sleep");
            c.arg("5");
            c.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            // ping on Windows waits 1s between pings; `-n 5` ≈ 4s.
            let mut c = std::process::Command::new("ping");
            c.arg("-n").arg("5").arg("127.0.0.1");
            c.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            c
        };

        // We need the Child handle for cleanup, but check_child_alive
        // consumes + forgets it. So we capture the PID before calling the
        // helper, and kill the process directly via the OS afterwards.
        // (mem::forget inside the helper means the handle is gone — we cannot
        // wait() on it normally.)
        let child = cmd.spawn().expect("spawn long-running child");
        let pid = child.id();
        let dir = unique_test_dir("child_alive");
        std::fs::create_dir_all(&dir).expect("mkdir test dir");
        let log_path = dir.join("daemon.log");

        let result = check_child_alive(child, &log_path, "test_long_running");
        assert!(
            result.is_ok(),
            "check_child_alive must return Ok for a long-running child, got: {:?}",
            result
        );

        // Cleanup the leaked (forgotten) child so the test runner does not
        // accumulate stray processes. Best-effort: ignore kill errors.
        #[cfg(unix)]
        {
            // SAFETY: kill(pid, SIGTERM) is a standard libc call. The pid is
            // the freshly-spawned sleep child, well within i32::MAX.
            unsafe {
                libc::kill(pid as i32, 15 /* SIGTERM */);
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
                if !h.is_null() {
                    TerminateProcess(h, 1);
                    CloseHandle(h);
                }
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
