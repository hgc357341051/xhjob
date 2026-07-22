//! Unix domain socket implementation.

use std::os::unix::net::UnixStream as StdUnixStream;
use tokio::net::{UnixListener, UnixStream};
use crate::errors::{Result, XhjobError};
use super::{IpcListener, IpcStream, ipc_path};

pub struct UnixListenerWrapper {
    inner: UnixListener,
}

impl UnixListenerWrapper {
    pub async fn bind(service_name: &str, data_dir: Option<&str>) -> Result<Self> {
        let path = ipc_path(service_name, data_dir);
        // ensure parent dir exists (e.g. user-specified data_dir may not exist yet)
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
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
            // Fix 5: SO_PEERCRED peer authentication.
            //
            // Without this check, any local user who can reach the socket
            // file (e.g. via a world-readable parent directory, or because
            // the socket was accidentally chmod'd to 0o666) can issue IPC
            // commands — dispatching arbitrary shell/HTTP tasks, cancelling
            // tasks, reading task results (which may contain secrets), etc.
            //
            // We verify the peer's uid/gid via SO_PEERCRED (a kernel-level
            // credential that cannot be spoofed) and reject the connection
            // unless the peer is:
            //   - root (uid 0) — always allowed, root can do anything anyway
            //   - the same uid as the daemon process (common case: daemon +
            //     PHP-FPM both run as www-data)
            //   - in the same group as the daemon process (matches the
            //     0o660 socket file permission we set in bind())
            //
            // Set XHJOB_IPC_NO_PEERCRED=1 to bypass the check (for tests
            // running as a different user than the daemon).
            if std::env::var("XHJOB_IPC_NO_PEERCRED").as_deref() != Ok("1") {
                if let Err(e) = verify_peer_cred(&stream) {
                    tracing::warn!(error = %e, "rejecting IPC connection: peer credential check failed");
                    return Err(e);
                }
            }
            Ok(Box::new(stream) as Box<dyn IpcStream>)
        })
    }
}

/// Verify the peer credentials of a Unix domain socket connection via
/// SO_PEERCRED. Returns Ok if the peer is allowed, Err otherwise.
///
/// Allows: root (uid 0), same uid as daemon, same gid as daemon.
fn verify_peer_cred(stream: &UnixStream) -> Result<()> {
    let cred = stream.peer_cred()
        .map_err(|e| XhjobError::Ipc(format!("peer_cred: {}", e)))?;
    let peer_uid = cred.uid();
    let peer_gid = cred.gid();
    // Use nix::unistd for geteuid/getegid (nix is already a unix dependency).
    let my_uid = nix::unistd::geteuid().as_raw();
    let my_gid = nix::unistd::getegid().as_raw();
    // root is always allowed
    if peer_uid == 0 {
        return Ok(());
    }
    // same uid as daemon (common case)
    if peer_uid == my_uid {
        return Ok(());
    }
    // same gid as daemon (matches the 0o660 socket permission)
    if peer_gid == my_gid {
        return Ok(());
    }
    Err(XhjobError::Ipc(format!(
        "peer credential rejected: peer uid={} gid={} vs daemon uid={} gid={} (set XHJOB_IPC_NO_PEERCRED=1 to bypass for testing)",
        peer_uid, peer_gid, my_uid, my_gid
    )))
}

pub struct UnixStreamWrapper;

impl UnixStreamWrapper {
    pub async fn connect(service_name: &str, data_dir: Option<&str>) -> Result<Box<dyn IpcStream>> {
        let path = ipc_path(service_name, data_dir);
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
