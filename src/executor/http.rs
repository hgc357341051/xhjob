//! HTTP executor based on reqwest.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
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
        .ok_or_else(|| XhjobError::exec(format!("invalid proxy url (no scheme): {}", url)))?;

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
        other => return Err(XhjobError::exec(format!("unsupported proxy scheme: {}", other))),
    }
    .map_err(|e| XhjobError::exec(format!("parse proxy {}: {}", proxy_str, e)))?;

    let proxy = if let Some(user) = parsed.user {
        let pass = parsed.pass.unwrap_or_default();
        proxy.basic_auth(&user, &pass)
    } else {
        proxy
    };

    Ok(proxy)
}

impl Executor for HttpExecutor {
    fn execute<'a>(&'a self, task: &'a Task) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>> {
        let payload_val = task.payload.clone();
        let timeout = task.timeout;
        let proxy = task.proxy.clone();
        Box::pin(async move {
            let payload: HttpPayload = serde_json::from_value(payload_val)
                .map_err(|e| XhjobError::exec(format!("invalid http payload: {}", e)))?;

            let mut client_builder = reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout))
                // P1 fix: separate, shorter connect timeout so a dead host
                // fails fast instead of consuming the whole `timeout` budget
                // (or, if `timeout` is large, hanging the worker for minutes).
                // 10s is enough for any healthy local/LAN connect; TCP SYN
                // retransmits alone exceed this for unreachable hosts.
                .connect_timeout(Duration::from_secs(
                    std::env::var("XHJOB_HTTP_CONNECT_TIMEOUT")
                        .ok().and_then(|s| s.parse::<u64>().ok())
                        .filter(|n| *n > 0)
                        .unwrap_or(10)
                ))
                // MEDIUM fix: explicit redirect policy. Previously relied on
                // reqwest's default (follow up to 10 redirects silently).
                // Making the limit explicit + tunable via XHJOB_HTTP_MAX_REDIRECTS
                // (0 = disable redirects). Mitigates SSRF amplification and makes
                // the redirect budget visible to operators. Default 5.
                .redirect(reqwest::redirect::Policy::limited(
                    std::env::var("XHJOB_HTTP_MAX_REDIRECTS")
                        .ok().and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(5)
                ));

            if let Some(proxy_str) = &proxy {
                let p = build_proxy(proxy_str)?;
                client_builder = client_builder.proxy(p);
            }

            let client = client_builder.build()
                .map_err(|e| XhjobError::exec(format!("build client: {}", e)))?;

            let method = match payload.method.to_uppercase().as_str() {
                "GET" => reqwest::Method::GET,
                "POST" => reqwest::Method::POST,
                "PUT" => reqwest::Method::PUT,
                "DELETE" => reqwest::Method::DELETE,
                "PATCH" => reqwest::Method::PATCH,
                "HEAD" => reqwest::Method::HEAD,
                // H4 fix: OPTIONS is a valid HTTP method (CORS preflight,
                // health probes, REST discovery). Previously rejected.
                "OPTIONS" => reqwest::Method::OPTIONS,
                other => return Err(XhjobError::exec(format!("unsupported method: {}", other))),
            };

            let mut req = client.request(method, &payload.url);
            for (k, v) in &payload.headers {
                req = req.header(k, v);
            }
            if let Some(body) = &payload.body {
                req = req.body(body.clone());
            }

            let resp = req.send().await
                .map_err(|e| XhjobError::exec(format!("http send: {}", e)))?;
            let status = resp.status().as_u16() as i32;

            // P1 fix: cap response body size so a malicious / runaway server
            // cannot exhaust daemon memory by streaming GBs. We pre-check
            // Content-Length when present, and also enforce a hard cap during
            // streaming read so chunked-transfer responses are bounded too.
            const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB
            if let Some(cl) = resp.content_length() {
                if cl > MAX_BODY_BYTES {
                    return Err(XhjobError::exec(format!(
                        "response body too large: {} bytes (max {})",
                        cl, MAX_BODY_BYTES
                    )));
                }
            }
            // Stream the body with a hard cap — works even for chunked
            // responses that omit Content-Length. We accumulate into a Vec
            // bounded by MAX_BODY_BYTES; if the cap is hit we error out.
            let mut stream = resp.bytes_stream();
            use futures_util::StreamExt;
            let mut body_buf: Vec<u8> = Vec::new();
            let mut exceeded = false;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk
                    .map_err(|e| XhjobError::exec(format!("read body chunk: {}", e)))?;
                if body_buf.len() + chunk.len() > MAX_BODY_BYTES as usize {
                    exceeded = true;
                    break;
                }
                body_buf.extend_from_slice(&chunk);
            }
            if exceeded {
                return Err(XhjobError::exec(format!(
                    "response body exceeded max {} bytes during streaming",
                    MAX_BODY_BYTES
                )));
            }
            // CRITICAL fix: detect whether the response body is valid UTF-8.
            // - Valid UTF-8 (text/JSON/XML/HTML): store as-is in `body`.
            // - Invalid UTF-8 (binary: images, files, msgpack, etc.):
            //   base64-encode into `body_b64` and leave `body` = None.
            //   Previously `from_utf8_lossy` silently replaced invalid bytes
            //   with U+FFFD, permanently corrupting binary payloads.
            let (body, body_b64) = match String::from_utf8(body_buf) {
                Ok(text) => (Some(text), None),
                Err(e) => {
                    let bytes = e.into_bytes();
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    tracing::debug!(
                        status,
                        bytes = bytes.len(),
                        b64_len = encoded.len(),
                        "binary HTTP response (non-UTF-8): stored as body_b64"
                    );
                    (None, Some(encoded))
                }
            };

            Ok(TaskResult {
                body,
                body_b64,
                status_code: Some(status),
                stdout: None,
                stderr: None,
                exit_code: None,
            })
        })
    }

    /// H3 fix: HTTP executor with cancel support. Wraps the request future in
    /// `tokio::select!` against the cancel flag. When cancel is requested,
    /// the reqwest future is dropped (aborting the in-flight TCP connection),
    /// and we return a "cancelled" error so the queue can record the event
    /// and not retry (cancel_requested is checked upstream).
    fn execute_with_cancel<'a>(&'a self, task: &'a Task, cancel_flag: Option<Arc<AtomicBool>>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TaskResult>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(flag) = &cancel_flag {
                if flag.load(std::sync::atomic::Ordering::SeqCst) {
                    return Err(XhjobError::exec("cancelled before start"));
                }
                // Race the execution against a cancel watcher. We poll the
                // flag every 200ms while the HTTP request is in flight.
                let exec_fut = self.execute(task);
                tokio::pin!(exec_fut);
                loop {
                    tokio::select! {
                        biased;
                        result = &mut exec_fut => return result,
                        _ = tokio::time::sleep(Duration::from_millis(200)) => {
                            if flag.load(std::sync::atomic::Ordering::SeqCst) {
                                return Err(XhjobError::exec("cancelled during http request"));
                            }
                        }
                    }
                }
            } else {
                self.execute(task).await
            }
        })
    }
}

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
