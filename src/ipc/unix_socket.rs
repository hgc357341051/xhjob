//! Unix domain socket implementation.

use std::os::unix::net::UnixStream as StdUnixStream;
use tokio::net::{UnixListener, UnixStream};
use crate::errors::{Result, XhjobError};
use super::{IpcListener, IpcStream, ipc_path};

pub struct UnixListenerWrapper {
    inner: UnixListener,
}

impl UnixListenerWrapper {
<<<<<<< Updated upstream
    pub async fn bind() -> Result<Self> {
        let path = ipc_path();
=======
    pub async fn bind(service_name: &str) -> Result<Self> {
        let path = ipc_path(service_name);
>>>>>>> Stashed changes
        // remove stale socket file
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)
            .map_err(|e| XhjobError::Ipc(format!("bind {}: {}", path, e)))?;
        // set socket file permissions to 0o660
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o660));
        Ok(Self { inner: listener })
    }
}

impl IpcListener for UnixListenerWrapper {
    fn accept<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn IpcStream>>> + Send + 'a>> {
        Box::pin(async move {
            let (stream, _addr) = self.inner.accept().await
                .map_err(|e| XhjobError::Ipc(format!("accept: {}", e)))?;
            Ok(Box::new(stream) as Box<dyn IpcStream>)
        })
    }
}

pub struct UnixStreamWrapper;

impl UnixStreamWrapper {
<<<<<<< Updated upstream
    pub async fn connect() -> Result<Box<dyn IpcStream>> {
        let path = ipc_path();
=======
    pub async fn connect(service_name: &str) -> Result<Box<dyn IpcStream>> {
        let path = ipc_path(service_name);
>>>>>>> Stashed changes
        // try tokio UnixStream first
        match UnixStream::connect(&path).await {
            Ok(s) => Ok(Box::new(s)),
            Err(e) => {
                // fallback to blocking connect with timeout
                let _ = e;
                let s = StdUnixStream::connect(&path)
                    .map_err(|e| XhjobError::Ipc(format!("connect {}: {}", path, e)))?;
                s.set_nonblocking(true)
                    .map_err(|e| XhjobError::Ipc(format!("set_nonblocking: {}", e)))?;
                let s = tokio::net::UnixStream::from_std(s)
                    .map_err(|e| XhjobError::Ipc(format!("from_std: {}", e)))?;
                Ok(Box::new(s))
            }
        }
    }
}

impl IpcStream for UnixStream {}
