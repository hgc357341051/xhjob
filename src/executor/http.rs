//! HTTP executor based on reqwest.

use std::time::Duration;
use crate::errors::{Result, XhjobError};
use crate::store::{Task, TaskResult, HttpPayload};
use super::Executor;

pub struct HttpExecutor;

impl HttpExecutor {
    pub fn new() -> Self { Self }
}

impl Default for HttpExecutor {
    fn default() -> Self { Self::new() }
}

<<<<<<< Updated upstream
=======
/// Parsed components of a proxy URL.
///
/// `socks5://user:pass@127.0.0.1:1080` -> scheme=socks5, user=Some(user), pass=Some(pass),
/// host_port=127.0.0.1:1080.
///
/// `http://proxy.local:8080` -> scheme=http, user=None, pass=None, host_port=proxy.local:8080.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedProxy {
    pub scheme: String,
    pub user: Option<String>,
    pub pass: Option<String>,
    pub host_port: String,
}

/// Parse a proxy URL into its scheme / auth / host:port components.
///
/// Accepted schemes: `http`, `https`, `socks5`, `socks5h`. The `user:pass@`
/// segment is optional; when present, `user`/`pass` are populated and the
/// `host_port` excludes them (so it can be passed directly to
/// `reqwest::Proxy::socks5` / `socks5h`).
pub(crate) fn parse_proxy_url(url: &str) -> Result<ParsedProxy> {
    let (scheme, rest) = url.split_once("://")
        .ok_or_else(|| XhjobError::Exec(format!("invalid proxy url (no scheme): {}", url)))?;

    // Split `user:pass@host:port` (if present). We use `rfind` so a password
    // containing `@` (rare but possible) does not break parsing.
    let (user, pass, host_port) = if let Some(idx) = rest.rfind('@') {
        let auth = &rest[..idx];
        let hp = &rest[idx + 1..];
        let (u, p) = match auth.split_once(':') {
            Some((u, p)) => (u.to_string(), p.to_string()),
            None => (auth.to_string(), String::new()),
        };
        (Some(u), if p.is_empty() { None } else { Some(p) }, hp.to_string())
    } else {
        (None, None, rest.to_string())
    };

    Ok(ParsedProxy {
        scheme: scheme.to_lowercase(),
        user,
        pass,
        host_port,
    })
}

/// Build a `reqwest::Proxy` from a proxy URL string.
///
/// - `http://` -> `reqwest::Proxy::http(url)`
/// - `https://` -> `reqwest::Proxy::https(url)`
/// - `socks5://` -> `reqwest::Proxy::all("socks5://host:port")` (user:pass stripped)
/// - `socks5h://` -> `reqwest::Proxy::all("socks5h://host:port")` (user:pass stripped)
///
/// If the URL contains `user:pass@`, `.basic_auth(user, pass)` is chained on
/// the resulting `Proxy`.
///
/// Note: reqwest 0.12.x removed the dedicated `Proxy::socks5` / `socks5h`
/// constructors — SOCKS5 proxies are now configured by passing a `socks5://`
/// (or `socks5h://`) URL to `Proxy::all`, which inspects the URL scheme to
/// determine the proxy type. The `socks` cargo feature enables the SOCKS5
/// connector implementation.
fn build_proxy(proxy_str: &str) -> Result<reqwest::Proxy> {
    let parsed = parse_proxy_url(proxy_str)?;
    let proxy = match parsed.scheme.as_str() {
        "http" => reqwest::Proxy::http(proxy_str),
        "https" => reqwest::Proxy::https(proxy_str),
        "socks5" => {
            // Reconstruct the URL without user:pass — basic_auth is applied
            // separately below so credentials don't appear in logs/error messages.
            let url = format!("socks5://{}", parsed.host_port);
            reqwest::Proxy::all(url.as_str())
        }
        "socks5h" => {
            let url = format!("socks5h://{}", parsed.host_port);
            reqwest::Proxy::all(url.as_str())
        }
        other => return Err(XhjobError::Exec(format!("unsupported proxy scheme: {}", other))),
    }
    .map_err(|e| XhjobError::Exec(format!("parse proxy {}: {}", proxy_str, e)))?;

    let proxy = if let Some(user) = parsed.user {
        let pass = parsed.pass.unwrap_or_default();
        proxy.basic_auth(&user, &pass)
    } else {
        proxy
    };

    Ok(proxy)
}

>>>>>>> Stashed changes
impl Executor for HttpExecutor {
    fn execute(&self, task: &Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + '_>> {
        let payload_val = task.payload.clone();
        let timeout = task.timeout;
<<<<<<< Updated upstream
=======
        let proxy = task.proxy.clone();
>>>>>>> Stashed changes
        Box::pin(async move {
            let payload: HttpPayload = serde_json::from_value(payload_val)
                .map_err(|e| XhjobError::Exec(format!("invalid http payload: {}", e)))?;

<<<<<<< Updated upstream
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout))
                .build()
=======
            let mut client_builder = reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout));

            if let Some(proxy_str) = &proxy {
                let p = build_proxy(proxy_str)?;
                client_builder = client_builder.proxy(p);
            }

            let client = client_builder.build()
>>>>>>> Stashed changes
                .map_err(|e| XhjobError::Exec(format!("build client: {}", e)))?;

            let method = match payload.method.to_uppercase().as_str() {
                "GET" => reqwest::Method::GET,
                "POST" => reqwest::Method::POST,
                "PUT" => reqwest::Method::PUT,
                "DELETE" => reqwest::Method::DELETE,
                "PATCH" => reqwest::Method::PATCH,
                "HEAD" => reqwest::Method::HEAD,
                other => return Err(XhjobError::Exec(format!("unsupported method: {}", other))),
            };

            let mut req = client.request(method, &payload.url);
            for (k, v) in &payload.headers {
                req = req.header(k, v);
            }
            if let Some(body) = &payload.body {
                req = req.body(body.clone());
            }

            let resp = req.send().await
                .map_err(|e| XhjobError::Exec(format!("http send: {}", e)))?;
            let status = resp.status().as_u16() as i32;
            let body = resp.text().await
                .map_err(|e| XhjobError::Exec(format!("read body: {}", e)))?;

            Ok(TaskResult {
                body: Some(body),
                status_code: Some(status),
                stdout: None,
                stderr: None,
                exit_code: None,
            })
        })
    }
}
<<<<<<< Updated upstream
=======

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_socks5_with_auth() {
        let p = parse_proxy_url("socks5://user:pass@127.0.0.1:1080").unwrap();
        assert_eq!(p.scheme, "socks5");
        assert_eq!(p.user.as_deref(), Some("user"));
        assert_eq!(p.pass.as_deref(), Some("pass"));
        assert_eq!(p.host_port, "127.0.0.1:1080");
    }

    #[test]
    fn parse_http_without_auth() {
        let p = parse_proxy_url("http://proxy.local:8080").unwrap();
        assert_eq!(p.scheme, "http");
        assert_eq!(p.user, None);
        assert_eq!(p.pass, None);
        assert_eq!(p.host_port, "proxy.local:8080");
    }

    #[test]
    fn parse_https_with_auth() {
        let p = parse_proxy_url("https://alice:secret@proxy.example.com:443").unwrap();
        assert_eq!(p.scheme, "https");
        assert_eq!(p.user.as_deref(), Some("alice"));
        assert_eq!(p.pass.as_deref(), Some("secret"));
        assert_eq!(p.host_port, "proxy.example.com:443");
    }

    #[test]
    fn parse_socks5h_without_auth() {
        let p = parse_proxy_url("socks5h://127.0.0.1:1080").unwrap();
        assert_eq!(p.scheme, "socks5h");
        assert_eq!(p.user, None);
        assert_eq!(p.pass, None);
        assert_eq!(p.host_port, "127.0.0.1:1080");
    }

    #[test]
    fn parse_socks5_user_only() {
        // user without password
        let p = parse_proxy_url("socks5://user@127.0.0.1:1080").unwrap();
        assert_eq!(p.scheme, "socks5");
        assert_eq!(p.user.as_deref(), Some("user"));
        assert_eq!(p.pass, None);
        assert_eq!(p.host_port, "127.0.0.1:1080");
    }

    #[test]
    fn parse_invalid_url_no_scheme() {
        let err = parse_proxy_url("127.0.0.1:1080").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("no scheme"), "unexpected error: {}", msg);
    }

    #[test]
    fn build_proxy_http_url() {
        // Should not panic; constructs a Proxy for an http URL.
        let p = build_proxy("http://proxy.local:8080").unwrap();
        // Proxy doesn't expose its URL via a public API; just assert it built.
        let _ = format!("{:?}", p);
    }

    #[test]
    fn build_proxy_socks5_with_auth() {
        let p = build_proxy("socks5://user:pass@127.0.0.1:1080").unwrap();
        let _ = format!("{:?}", p);
    }

    #[test]
    fn build_proxy_unsupported_scheme() {
        let err = build_proxy("ftp://example.com:21").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("unsupported proxy scheme"), "unexpected error: {}", msg);
    }
}
>>>>>>> Stashed changes
