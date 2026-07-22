//! Minimal configuration file support (P0-13).
//! 
//! Reads a simple KEY=VALUE config file (like .env) from the path
//! specified by XHJOB_CONFIG_FILE env var (default: /etc/xhjob/config).
//! Values are loaded into the process environment so existing
//! std::env::var("XHJOB_") calls pick them up automatically.
//!
//! The config file format is one KEY=VALUE per line, with # for comments.
//! Example:
//!   # Xhjob daemon configuration
//!   XHJOB_PERSIST=1
//!   XHJOB_SOCK_DIR=/run/xhjob
//!   XHJOB_MAX_PENDING=5000
//!   RUST_LOG=xhjob=debug

use std::collections::HashMap;

/// Load the config file into the process environment.
/// Called once at daemon startup (before any env::var reads).
/// If the config file doesn't exist, this is a no-op (backward compatible).
pub fn load_config_file() {
    let path = std::env::var("XHJOB_CONFIG_FILE")
        .unwrap_or_else(|_| "/etc/xhjob/config".to_string());
    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return, // file doesn't exist — backward compatible
    };
    for (key, value) in parse_config(&contents) {
        // Only set if not already in the environment (env vars take precedence
        // over config file values, so command-line -e overrides still work).
        if std::env::var_os(&key).is_none() {
            std::env::set_var(&key, &value);
        }
    }
    tracing::info!(config_path = %path, "config file loaded");
}

/// Parse a simple KEY=VALUE config file.
/// - Lines starting with # are comments
/// - Empty lines are ignored
/// - KEY=VALUE sets an environment variable
/// - Whitespace around KEY and VALUE is trimmed
/// - Quotes around VALUE are stripped
fn parse_config(contents: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(eq_pos) = line.find('=') {
            let key = line[..eq_pos].trim().to_string();
            let mut value = line[eq_pos + 1..].trim().to_string();
            // Strip surrounding quotes
            if value.len() >= 2 {
                let bytes = value.as_bytes();
                if (bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
                    || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'')
                {
                    value = value[1..value.len() - 1].to_string();
                }
            }
            if !key.is_empty() {
                map.insert(key, value);
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_config() {
        let contents = r#"
# Xhjob config
XHJOB_PERSIST=1
XHJOB_SOCK_DIR=/run/xhjob
XHJOB_MAX_PENDING = 5000
RUST_LOG="xhjob=debug"
"#;
        let map = parse_config(contents);
        assert_eq!(map.get("XHJOB_PERSIST"), Some(&"1".to_string()));
        assert_eq!(map.get("XHJOB_SOCK_DIR"), Some(&"/run/xhjob".to_string()));
        assert_eq!(map.get("XHJOB_MAX_PENDING"), Some(&"5000".to_string()));
        assert_eq!(map.get("RUST_LOG"), Some(&"xhjob=debug".to_string()));
    }

    #[test]
    fn test_parse_ignores_comments_and_empty() {
        let contents = r#"
# comment
   # indented comment

KEY=val
"#;
        let map = parse_config(contents);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("KEY"), Some(&"val".to_string()));
    }
}
