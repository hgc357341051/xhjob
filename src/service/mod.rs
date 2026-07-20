//! Named service identity for multi-instance daemon support.
//!
//! Each xhjob daemon instance is identified by a service name. All runtime
//! paths (PID file, IPC socket, SQLite DB, log file) are derived from this
//! name, allowing multiple independent daemons to coexist on the same host.
//!
//! The active service name is propagated from the PHP parent process to the
//! spawned daemon via the `XHJOB_SERVICE_NAME` environment variable.

use crate::errors::{Result, XhjobError};

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
        return Err(XhjobError::Config(format!("invalid service name: {}", name)));
    }
    let mut chars = name.chars();
    let first = match chars.next() {
        Some(c) => c,
        None => return Err(XhjobError::Config(format!("invalid service name: {}", name))),
    };
    if !first.is_ascii_alphabetic() {
        return Err(XhjobError::Config(format!("invalid service name: {}", name)));
    }
    for c in chars {
        if !c.is_ascii_alphanumeric() && c != '_' && c != '-' {
            return Err(XhjobError::Config(format!("invalid service name: {}", name)));
        }
    }
    Ok(name.to_string())
}

/// Return the service name for the current process.
///
/// Reads the `XHJOB_SERVICE_NAME` environment variable. When unset (for
/// example, in a PHP parent process that has not explicitly set a service),
/// returns `default_name()`.
pub fn current() -> String {
    std::env::var("XHJOB_SERVICE_NAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_name().to_string())
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
