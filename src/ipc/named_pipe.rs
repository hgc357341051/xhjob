//! Windows Named Pipe implementation.

use super::{ipc_path, IpcListener, IpcStream};
use crate::errors::{Result, XhjobError};
use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, ServerOptions};
use tokio::sync::Mutex;

pub struct NamedPipeListenerWrapper {
    pipe_name: String,
    first: Mutex<Option<NamedPipeServer>>,
}

impl NamedPipeListenerWrapper {
    pub fn bind(service_name: &str) -> Result<Self> {
        let pipe_name = ipc_path(service_name);
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .create(&pipe_name)
            .map_err(|e| XhjobError::ipc(format!("create pipe {}: {}", pipe_name, e)))?;
        Ok(Self {
            pipe_name,
            first: Mutex::new(Some(first)),
        })
    }
}

impl IpcListener for NamedPipeListenerWrapper {
    fn accept<'a>(
        &'a self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Box<dyn IpcStream>>> + Send + 'a>>
    {
        Box::pin(async move {
            // Use the first pre-created instance if available, otherwise create new.
            let server = {
                let mut guard = self.first.lock().await;
                guard.take()
            };
            let server = match server {
                Some(s) => s,
                None => ServerOptions::new()
                    .create(&self.pipe_name)
                    .map_err(|e| XhjobError::ipc(format!("create pipe instance: {}", e)))?,
            };
            server
                .connect()
                .await
                .map_err(|e| XhjobError::ipc(format!("pipe connect: {}", e)))?;
            Ok(Box::new(server) as Box<dyn IpcStream>)
        })
    }
}

pub struct NamedPipeClientWrapper;

impl NamedPipeClientWrapper {
    pub fn connect(service_name: &str) -> Result<Box<dyn IpcStream>> {
        let pipe_name = ipc_path(service_name);
        let client = NamedPipeClient::connect(&pipe_name)
            .map_err(|e| XhjobError::ipc(format!("client connect {}: {}", pipe_name, e)))?;
        Ok(Box::new(client))
    }
}

impl IpcStream for NamedPipeServer {}
impl IpcStream for NamedPipeClient {}
