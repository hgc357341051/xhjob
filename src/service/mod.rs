//! Named service identity for multi-instance daemon support.
//!
//! Each xhjob daemon instance is identified by a service name. All runtime
//! paths (PID file, IPC socket, SQLite DB, log file) are derived from this
//! name, allowing multiple independent daemons to coexist on the same host.
//!
//! The active service name is propagated from the PHP parent process to the
//! spawned daemon via two mechanisms:
//!   1. A global once-cell set by `xhjob_run_daemon(Some(name), data_dir)`
//!      (primary path; the name and data_dir are encoded into the `-r` code
//!      string so they survive PHP version-manager shim re-execs that may
//!      scrub env vars).
//!   2. The `XHJOB_SERVICE_NAME` / `XHJOB_DATA_DIR` environment variables
//!      (backward-compatible fallback for callers that spawn the daemon
//!      through other paths).

use std::sync::OnceLock;

use crate::errors::{Result, XhjobError};

/// Process-global storage for the current service name.
///
/// Set once by `set_current()` (called from `xhjob_run_daemon(Some(name), _)`)
/// at daemon startup. Once set, `current()` returns this value, taking
/// precedence over the `XHJOB_SERVICE_NAME` env var.
static CURRENT_SERVICE: OnceLock<String> = OnceLock::new();

/// Process-global storage for the current data directory.
///
/// Set once by `set_current_data_dir()` (called from
/// `xhjob_run_daemon(_, Some(dir))`) at daemon startup. When set, all
/// runtime files (PID/sock/db/log) for this daemon are placed under this
/// directory instead of the system default (`/tmp` on Unix, `%TEMP%` on
/// Windows). This enables users to relocate all service files to a custom
/// directory for backup / migration / restore.
static CURRENT_DATA_DIR: OnceLock<String> = OnceLock::new();

/// Newtype wrapping a validated service name string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServiceName(pub String);

impl ServiceName {
    /// Create a `ServiceName` after validating the input.
    pub fn new(name: impl AsRef<str>) -> Result<Self> {
        let name = validate(name.as_ref())?;
        Ok(Self(name))
    }

    /// Return the underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consume into the owned `String`.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl std::fmt::Display for ServiceName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Validate a service name against the rule `^[a-zA-Z][a-zA-Z0-9_-]{0,31}$`.
///
/// Returns the validated name as an owned `String` on success. Empty input,
/// illegal characters, or over-length names yield `XhjobError::Config`.
pub fn validate(name: &str) -> Result<String> {
    if name.is_empty() || name.len() > 32 {
        return Err(XhjobError::config(format!(
            "invalid service name: {}",
            name
        )));
    }
    let mut chars = name.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => {
            return Err(XhjobError::config(format!(
                "invalid service name: {}",
                name
            )))
        }
    };
    if !first.is_ascii_alphabetic() {
        return Err(XhjobError::config(format!(
            "invalid service name: {}",
            name
        )));
    }
    for c in chars {
        if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
            return Err(XhjobError::config(format!(
                "invalid service name: {}",
                name
            )));
        }
    }
    Ok(name.to_string())
}

/// Set the current service name globally for this process.
///
/// Intended to be called once at daemon startup from
/// `xhjob_run_daemon(Some(name), _)` when the service name is delivered via the
/// `-r` command-line code string (the primary propagation path that survives
/// PHP version-manager shim re-execs). Subsequent calls are no-ops: the first
/// caller wins, which matches the daemon-startup semantics (the service name
/// is fixed for the lifetime of the daemon process).
pub fn set_current(name: String) {
    let _ = CURRENT_SERVICE.set(name);
}

/// Set the current data directory globally for this process.
///
/// Intended to be called once at daemon startup from
/// `xhjob_run_daemon(_, Some(dir))` when the data_dir is delivered via the
/// `-r` command-line code string. Subsequent calls are no-ops: the first
/// caller wins, which matches the daemon-startup semantics (the data_dir is
/// fixed for the lifetime of the daemon process).
pub fn set_current_data_dir(dir: String) {
    let _ = CURRENT_DATA_DIR.set(dir);
}

/// Return the service name for the current process.
///
/// Resolution order:
///   1. A name explicitly set via `set_current()` (e.g. by
///      `xhjob_run_daemon(Some(name), _)` when the name was passed as a
///      command-line argument).
///   2. The `XHJOB_SERVICE_NAME` environment variable (set by the spawner
///      when env-var propagation works).
///   3. `default_name()` (`"default"`).
pub fn current() -> String {
    if let Some(name) = CURRENT_SERVICE.get() {
        return name.clone();
    }
    match std::env::var("XHJOB_SERVICE_NAME")
        .ok()
        .filter(|s| !s.is_empty())
    {
        Some(raw) => {
            // P1 fix: the env-var fallback path previously returned `raw`
            // without calling validate(). Since the returned value is used
            // to build filesystem paths (socket, pid, log, db), an
            // attacker-controlled env var containing `../` or `/` could
            // cause path traversal (writing logs/sockets outside the
            // intended directory). Validate here and fall back to the
            // default name if the env value is illegal.
            match validate(&raw) {
                Ok(validated) => validated,
                Err(e) => {
                    // tracing may not be initialized in all callers; use
                    // eprintln as a fallback so the misconfiguration is
                    // at least visible.
                    eprintln!(
                        "xhjob: WARNING XHJOB_SERVICE_NAME='{}' is invalid \
                         ({}); falling back to default service name",
                        raw, e
                    );
                    default_name().to_string()
                }
            }
        }
        None => default_name().to_string(),
    }
}

/// Return the data directory for the current process, if explicitly set.
///
/// Resolution order:
///   1. A directory explicitly set via `set_current_data_dir()` (e.g. by
///      `xhjob_run_daemon(_, Some(dir))` when the dir was passed as a
///      command-line argument).
///   2. The `XHJOB_DATA_DIR` environment variable (set by the spawner
///      when env-var propagation works).
///   3. `None` (caller should fall back to the platform default temp dir).
///
/// Returns `None` when no data directory has been configured, so callers
/// can distinguish "user did not specify" from "user specified empty string".
pub fn current_data_dir() -> Option<String> {
    if let Some(dir) = CURRENT_DATA_DIR.get() {
        return Some(dir.clone());
    }
    std::env::var("XHJOB_DATA_DIR")
        .ok()
        .filter(|s| !s.is_empty())
}

/// The canonical default service name used when no explicit name is provided.
pub fn default_name() -> &'static str {
    "default"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_default() {
        assert_eq!(validate("default").unwrap(), "default");
    }

    #[test]
    fn validate_accepts_letters_digits_underscore_dash() {
        assert_eq!(validate("cron-svc").unwrap(), "cron-svc");
        assert_eq!(validate("queue_svc").unwrap(), "queue_svc");
        assert_eq!(validate("a1B2c3").unwrap(), "a1B2c3");
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate("").is_err());
    }

    #[test]
    fn validate_rejects_leading_digit() {
        assert!(validate("1abc").is_err());
    }

    #[test]
    fn validate_rejects_leading_dash() {
        assert!(validate("-abc").is_err());
    }

    #[test]
    fn validate_rejects_leading_underscore() {
        assert!(validate("_abc").is_err());
    }

    #[test]
    fn validate_rejects_illegal_chars() {
        assert!(validate("abc.def").is_err());
        assert!(validate("abc/def").is_err());
        assert!(validate("abc def").is_err());
        assert!(validate("中").is_err());
    }

    #[test]
    fn validate_rejects_too_long() {
        let name = "a".to_string() + &"b".repeat(32);
        assert!(validate(&name).is_err());
    }

    #[test]
    fn validate_accepts_max_length() {
        let name = "a".to_string() + &"b".repeat(31);
        assert_eq!(validate(&name).unwrap(), name);
    }

    #[test]
    fn current_returns_default_when_unset() {
        // Save and clear, then restore.
        let saved = std::env::var("XHJOB_SERVICE_NAME").ok();
        std::env::remove_var("XHJOB_SERVICE_NAME");
        assert_eq!(current(), "default");
        if let Some(v) = saved {
            std::env::set_var("XHJOB_SERVICE_NAME", v);
        }
    }

    #[test]
    fn current_reads_env_when_set() {
        let saved = std::env::var("XHJOB_SERVICE_NAME").ok();
        std::env::set_var("XHJOB_SERVICE_NAME", "cron-svc");
        assert_eq!(current(), "cron-svc");
        match saved {
            Some(v) => std::env::set_var("XHJOB_SERVICE_NAME", v),
            None => std::env::remove_var("XHJOB_SERVICE_NAME"),
        }
    }

    #[test]
    fn default_name_is_default() {
        assert_eq!(default_name(), "default");
    }

    #[test]
    fn service_name_new_roundtrips() {
        let sn = ServiceName::new("cron-svc").unwrap();
        assert_eq!(sn.as_str(), "cron-svc");
        assert_eq!(sn.to_string(), "cron-svc");
        assert_eq!(sn.into_string(), "cron-svc");
    }

    #[test]
    fn service_name_new_rejects_invalid() {
        assert!(ServiceName::new("").is_err());
        assert!(ServiceName::new("1abc").is_err());
    }
}
