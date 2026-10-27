//! Unix-socket listener and connection loop.

use std::io;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;
use std::sync::Arc;

use anth_wire::{Request, Response, WireError, read_frame, write_frame};
use tokio::net::{UnixListener, UnixStream};

use crate::dispatch::Daemon;

/// Serves connections until the listener fails.
pub async fn serve(listener: UnixListener, daemon: Arc<Daemon>) -> io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let daemon = Arc::clone(&daemon);
        tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, daemon).await {
                eprintln!("connection ended: {error}");
            }
        });
    }
}

async fn serve_connection(mut stream: UnixStream, daemon: Arc<Daemon>) -> Result<(), WireError> {
    let credentials = stream
        .peer_cred()
        .map_err(|error| WireError::internal(format!("reading peer credentials: {error}")))?;
    let daemon_uid = unsafe { libc::getuid() };
    if credentials.uid() != daemon_uid && credentials.uid() != 0 {
        return Err(WireError::forbidden(format!(
            "socket peer uid {} is not the daemon uid {daemon_uid} or root",
            credentials.uid()
        )));
    }
    let owner = format!("socket:{}", credentials.uid());

    let Some(payload) = read_frame(&mut stream).await? else {
        return Ok(());
    };
    let request = serde_json::from_slice::<Request>(&payload).map_err(|error| {
        WireError::new(
            anth_wire::ErrorKind::Invalid,
            "invalid_hello",
            format!("the first frame is not a Hello: {error}"),
        )
    })?;
    if !matches!(request, Request::Hello { .. }) {
        let response = Response::Error(WireError::new(
            anth_wire::ErrorKind::Invalid,
            "invalid_hello",
            "the first frame must be Hello",
        ));
        write_response(&mut stream, &response).await?;
        return Ok(());
    }

    let response = daemon.execute_as(&owner, request).await;
    let close = response
        .error()
        .is_some_and(|error| error.code == "protocol_mismatch" || error.code == "invalid_hello");
    write_response(&mut stream, &response).await?;
    if close {
        return Ok(());
    }

    while let Some(payload) = read_frame(&mut stream).await? {
        let request = match serde_json::from_slice::<Request>(&payload) {
            Ok(request) => request,
            Err(error) => {
                let response = Response::Error(WireError::new(
                    anth_wire::ErrorKind::Invalid,
                    "invalid_request",
                    format!("unreadable request: {error}"),
                ));
                write_response(&mut stream, &response).await?;
                continue;
            }
        };
        if matches!(request, Request::Hello { .. }) {
            let response = Response::Error(WireError::new(
                anth_wire::ErrorKind::Invalid,
                "invalid_hello",
                "Hello is only valid as the first frame",
            ));
            write_response(&mut stream, &response).await?;
            continue;
        }
        let response = daemon.execute_as(&owner, request).await;
        write_response(&mut stream, &response).await?;
    }
    Ok(())
}

async fn write_response(stream: &mut UnixStream, response: &Response) -> Result<(), WireError> {
    let encoded = serde_json::to_vec(response)
        .map_err(|error| WireError::internal(format!("encoding the response: {error}")))?;
    write_frame(stream, &encoded).await
}

/// Binds the socket, removing a stale socket left by a previous run.
pub fn bind(path: impl AsRef<Path>) -> Result<UnixListener, WireError> {
    let path = path.as_ref();
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path).map_err(|error| {
            WireError::internal(format!("inspecting {}: {error}", path.display()))
        })?;
        if !metadata.file_type().is_socket() {
            return Err(WireError::invalid(format!(
                "{} exists and is not a socket",
                path.display()
            )));
        }
        std::fs::remove_file(path).map_err(|error| {
            WireError::internal(format!("removing {}: {error}", path.display()))
        })?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            WireError::internal(format!("creating {}: {error}", parent.display()))
        })?;
    }
    let listener = UnixListener::bind(path)
        .map_err(|error| WireError::internal(format!("binding {}: {error}", path.display())))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))
        .map_err(|error| WireError::internal(format!("setting socket mode: {error}")))?;
    Ok(listener)
}
