//! Cross-platform IPC: Unix domain socket (Unix) + Named Pipe (Windows).

use serde::{Serialize, Deserialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use crate::errors::{Result, XhjobError};

#[cfg(unix)]
pub mod unix_socket;
#[cfg(windows)]
pub mod named_pipe;

/// IPC path derived from `service_name`.
///
/// Unix: `${XHJOB_SOCK_DIR:-/tmp}/xhjob.{name}.sock`
/// Windows: `\\.\pipe\xhjob-{name}`
pub fn ipc_path(service_name: &str) -> String {
    #[cfg(unix)]
    {
        let dir = std::env::var("XHJOB_SOCK_DIR").unwrap_or_else(|_| "/tmp".to_string());
        std::path::PathBuf::from(dir)
            .join(format!("xhjob.{}.sock", service_name))
            .to_string_lossy()
            .to_string()
    }
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\xhjob-{}", service_name)
    }
}

/// Frame protocol: length-prefixed JSON.
/// Wire format: [4 bytes big-endian length][JSON payload]

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub op: String,
    pub payload: serde_json::Value,
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
    if len > 64 * 1024 * 1024 {
        return Err(XhjobError::Ipc(format!("frame too large: {}", len)));
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
/// The daemon-side listener binds to the path derived from `service::current()`,
/// which is set via `XHJOB_SERVICE_NAME` by the spawning parent process.
pub async fn bind_listener() -> Result<Box<dyn IpcListener>> {
    let service_name = crate::service::current();
    #[cfg(unix)]
    {
        Ok(Box::new(unix_socket::UnixListenerWrapper::bind(&service_name).await?))
    }
    #[cfg(windows)]
    {
        Ok(Box::new(named_pipe::NamedPipeListenerWrapper::bind(&service_name)?))
    }
}

/// Connect to the daemon (client side) for `service_name`. Returns a stream.
pub async fn connect(service_name: &str) -> Result<Box<dyn IpcStream>> {
    #[cfg(unix)]
    {
        unix_socket::UnixStreamWrapper::connect(service_name).await
    }
    #[cfg(windows)]
    {
        named_pipe::NamedPipeClientWrapper::connect(service_name)
    }
}

/// Helper: send one request and receive one response (short connection)
/// targeting the daemon for `service_name`.
pub async fn request(
    op: &str,
    payload: serde_json::Value,
    service_name: &str,
) -> Result<Response> {
    let mut stream = connect(service_name).await?;
    let req = Request {
        id: rand_id(),
        op: op.to_string(),
        payload,
    };
    write_frame(&mut stream, &req).await?;
    let resp: Response = read_frame(&mut stream).await?;
    Ok(resp)
}

fn rand_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos
}
