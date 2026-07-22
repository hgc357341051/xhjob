//! Cross-platform IPC: Unix domain socket (Unix) + Named Pipe (Windows).

use serde::{Serialize, Deserialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::errors::{Result, XhjobError};

#[cfg(unix)]
pub mod unix_socket;
#[cfg(windows)]
pub mod named_pipe;

/// IPC path derived from `service_name` and optional `data_dir`.
///
/// Path resolution priority (highest first):
///   1. `data_dir` argument (if `Some`)
///   2. `XHJOB_SOCK_DIR` env var (Unix only, fine-grained override)
///   3. `XHJOB_DATA_DIR` env var (unified data directory)
///   4. Platform default (`/tmp` on Unix, named pipe on Windows)
///
/// Unix: `<dir>/xhjob.{name}.sock`
/// Windows: `\\.\pipe\xhjob-{name}` (data_dir is ignored on Windows since
/// named pipes live in their own kernel namespace, not the filesystem)
pub fn ipc_path(service_name: &str, data_dir: Option<&str>) -> String {
    #[cfg(unix)]
    {
        let dir = if let Some(d) = data_dir {
            if !d.is_empty() { d.to_string() } else { fallback_sock_dir() }
        } else if let Ok(d) = std::env::var("XHJOB_SOCK_DIR") {
            if !d.is_empty() { d } else { fallback_sock_dir() }
        } else if let Ok(d) = std::env::var("XHJOB_DATA_DIR") {
            if !d.is_empty() { d } else { fallback_sock_dir() }
        } else {
            fallback_sock_dir()
        };
        std::path::PathBuf::from(dir)
            .join(format!("xhjob.{}.sock", service_name))
            .to_string_lossy()
            .to_string()
    }
    #[cfg(windows)]
    {
        let _ = data_dir;  // named pipes don't use filesystem paths
        format!(r"\\.\pipe\xhjob-{}", service_name)
    }
}

/// Fallback sock directory when no explicit dir is provided (Unix only).
/// P0 fix: changed from `/tmp` to `/run/xhjob` (or `/var/run/xhjob` on
/// systems without `/run`). `/tmp` is world-writable and sticky-bitted,
/// making the socket path vulnerable to symlink attacks / name squatting
/// by other local users. `/run/xhjob` is root-owned (daemon runs as root
/// or a dedicated user) with 0o755 perms, which combined with the
/// per-directory 0o700 set in bind() makes the socket path non-accessible
/// to other users. Falls back to `/tmp` only if `/run` does not exist.
#[cfg(unix)]
fn fallback_sock_dir() -> String {
    if std::path::Path::new("/run").exists() {
        // /run is typically a tmpfs mounted by systemd; create xhjob subdir.
        let candidate = "/run/xhjob";
        let _ = std::fs::create_dir_all(candidate);
        // Set 0o700 on the directory we created.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(candidate, std::fs::Permissions::from_mode(0o700));
        candidate.to_string()
    } else if std::path::Path::new("/var/run").exists() {
        let candidate = "/var/run/xhjob";
        let _ = std::fs::create_dir_all(candidate);
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(candidate, std::fs::Permissions::from_mode(0o700));
        candidate.to_string()
    } else {
        // Last resort: /tmp (less secure, but better than failing to start).
        "/tmp".to_string()
    }
}

/// Frame protocol: length-prefixed JSON.
/// Wire format: [4 bytes big-endian length][JSON payload]

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub op: String,
    pub payload: serde_json::Value,
    /// Optional trace id for request correlation (P0-12).
    /// Defaults to `None` when omitted by the client (backward compatible).
    #[serde(default)]
    pub trace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    pub ok: bool,
    pub data: serde_json::Value,
    pub err: Option<String>,
}

impl Response {
    pub fn success(id: u64, data: serde_json::Value) -> Self {
        Self { id, ok: true, data, err: None }
    }
    pub fn error(id: u64, msg: impl Into<String>) -> Self {
        Self { id, ok: false, data: serde_json::Value::Null, err: Some(msg.into()) }
    }
}

/// IPC 事件结构（保留为未来扩展接口）。
/// 未来用于 daemon → client 的事件流推送（如任务完成通知、cron tick 事件等），当前未启用。
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub kind: String,
    pub payload: serde_json::Value,
}

/// Write a length-prefixed JSON frame.
pub async fn write_frame<W: AsyncWriteExt + Unpin, T: Serialize>(w: &mut W, msg: &T) -> Result<()> {
    let json = serde_json::to_vec(msg)
        .map_err(|e| XhjobError::Ipc(format!("serialize: {}", e)))?;
    let len = json.len() as u32;
    w.write_all(&len.to_be_bytes()).await
        .map_err(|e| XhjobError::Ipc(format!("write len: {}", e)))?;
    w.write_all(&json).await
        .map_err(|e| XhjobError::Ipc(format!("write body: {}", e)))?;
    w.flush().await
        .map_err(|e| XhjobError::Ipc(format!("flush: {}", e)))?;
    Ok(())
}

/// Read a length-prefixed JSON frame.
pub async fn read_frame<R: AsyncReadExt + Unpin, T: for<'de> Deserialize<'de>>(r: &mut R) -> Result<T> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await
        .map_err(|e| XhjobError::Ipc(format!("read len: {}", e)))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    // P0 fix: lowered from 64 MB to 8 MB. A legitimate IPC request (dispatch
    // / chain / group / chord) is typically < 10 KB. 8 MB is generous enough
    // for large payloads while preventing a single malicious connection from
    // allocating 64 MB of memory per frame.
    if len > 8 * 1024 * 1024 {
        return Err(XhjobError::Ipc(format!("frame too large: {} (max 8MB)", len)));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await
        .map_err(|e| XhjobError::Ipc(format!("read body: {}", e)))?;
    serde_json::from_slice(&buf)
        .map_err(|e| XhjobError::Ipc(format!("deserialize: {}", e)))
}

/// Server-side listener abstraction.
/// Uses boxed futures so the trait remains dyn-compatible.
pub trait IpcListener: Send + Sync {
    /// Accept one connection. Returns an owned AsyncRead+AsyncWrite stream.
    fn accept<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn IpcStream>>> + Send + 'a>>;
}

/// Stream abstraction (read + write).
pub trait IpcStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Unpin {}

/// Spawn the appropriate listener for the current platform.
///
/// The daemon-side listener binds to the path derived from
/// `service::current()` and `service::current_data_dir()`. The data_dir is
/// set via `XHJOB_DATA_DIR` (or the `-r` code string) by the spawning parent
/// process, allowing all runtime files to be relocated to a user-specified
/// directory for backup / migration / restore.
pub async fn bind_listener() -> Result<Box<dyn IpcListener>> {
    let service_name = crate::service::current();
    let data_dir = crate::service::current_data_dir();
    let data_dir_ref = data_dir.as_deref();
    #[cfg(unix)]
    {
        Ok(Box::new(unix_socket::UnixListenerWrapper::bind(&service_name, data_dir_ref).await?))
    }
    #[cfg(windows)]
    {
        Ok(Box::new(named_pipe::NamedPipeListenerWrapper::bind(&service_name)?))
    }
}

/// Connect to the daemon (client side) for `service_name` with optional
/// `data_dir`. Returns a stream.
pub async fn connect(service_name: &str, data_dir: Option<&str>) -> Result<Box<dyn IpcStream>> {
    #[cfg(unix)]
    {
        unix_socket::UnixStreamWrapper::connect(service_name, data_dir).await
    }
    #[cfg(windows)]
    {
        let _ = data_dir;  // named pipe path doesn't depend on filesystem dir
        named_pipe::NamedPipeClientWrapper::connect(service_name)
    }
}

/// Helper: send one request and receive one response (short connection)
/// targeting the daemon for `service_name` with optional `data_dir`.
///
/// P0 fix: the timeout that used to be baked in here has been moved to the
/// call sites in `lib.rs` (the `ipc_request` helper) so each PHP-FPM-worker
/// entry point explicitly owns its own bounded wait. Without a timeout at
/// the call site, a daemon that accepted the connection but then
/// deadlocked / got SIGSTOP'd / crashed after accept would leave the
/// PHP-FPM worker blocked forever in `read_exact` — `max_execution_time`
/// does not interrupt C-level blocking calls, so the worker pool would be
/// drained one by one until the site returns 502/504 with no self-heal.
///
/// Keeping this transport function unbounded lets internal daemon-to-daemon
/// callers (and tests) opt out of the timeout when they need long-running
/// control ops. Call sites reachable from PHP-FPM should use
/// `lib.rs::ipc_request`, which wraps this in `tokio::time::timeout`.
pub async fn request(
    op: &str,
    payload: serde_json::Value,
    service_name: &str,
    data_dir: Option<&str>,
) -> Result<Response> {
    let mut stream = connect(service_name, data_dir).await?;
    let req = Request {
        id: rand_id(),
        op: op.to_string(),
        payload,
        trace_id: Some(rand_trace_id()),
    };
    write_frame(&mut stream, &req).await?;
    let resp: Response = read_frame(&mut stream).await?;
    Ok(resp)
}

/// Default IPC request timeout in seconds. Tunable via XHJOB_IPC_TIMEOUT_SECS.
/// 5s is enough for any local IPC op (SQLite write / cron scan / dispatch).
pub fn default_ipc_timeout_secs() -> u64 {
    std::env::var("XHJOB_IPC_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(5)
}

fn rand_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos
}

/// Generate a short trace id (8 hex chars derived from the current timestamp
/// in nanoseconds) for request correlation (P0-12).
fn rand_trace_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{:08x}", (nanos & 0xffff_ffff) as u32)
}
