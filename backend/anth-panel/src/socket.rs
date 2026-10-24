//! Unix-socket server and client for the panel protocol.
//!
//! One request per connection turn, answered in order, which is what v0.1.0 did
//! and what the CLI and HTTP layers expect. The difference is the framing: see
//! `anth_wire::frame`.

use std::path::Path;
use std::sync::Arc;

use anth_wire::{Request, Response, WireError, read_frame, write_frame};
use tokio::net::{UnixListener, UnixStream};

use crate::dispatch::Daemon;

/// Serves requests until the listener fails.
///
/// Each connection is its own task, so a slow request cannot stall another
/// client. A request that cannot be parsed is answered with an error and the
/// connection stays open, because a client typo is not a reason to hang up.
pub async fn serve(listener: UnixListener, daemon: Arc<Daemon>) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let daemon = Arc::clone(&daemon);
        tokio::spawn(async move {
            if let Err(error) = serve_connection(stream, &daemon).await {
                eprintln!("connection ended: {error}");
            }
        });
    }
}

async fn serve_connection(mut stream: UnixStream, daemon: &Daemon) -> Result<(), WireError> {
    while let Some(payload) = read_frame(&mut stream).await? {
        let response = match serde_json::from_slice::<Request>(&payload) {
            Ok(request) => daemon.handle(request).await,
            Err(e) => Response::Error(WireError::invalid(format!("unreadable request: {e}"))),
        };
        let encoded = serde_json::to_vec(&response)
            .map_err(|e| WireError::internal(format!("encoding the response: {e}")))?;
        write_frame(&mut stream, &encoded).await?;
    }
    Ok(())
}

/// Binds the socket, removing a stale file left by a previous run.
///
/// Refuses to remove something that is not a socket: a config typo pointing at a
/// regular file must not delete it.
pub fn bind(path: impl AsRef<Path>) -> Result<UnixListener, WireError> {
    let path = path.as_ref();
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path)
            .map_err(|e| WireError::internal(format!("inspecting {}: {e}", path.display())))?;
        if !is_socket(&metadata) {
            return Err(WireError::invalid(format!(
                "{} exists and is not a socket",
                path.display()
            )));
        }
        std::fs::remove_file(path)
            .map_err(|e| WireError::internal(format!("removing {}: {e}", path.display())))?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| WireError::internal(format!("creating {}: {e}", parent.display())))?;
    }
    UnixListener::bind(path)
        .map_err(|e| WireError::internal(format!("binding {}: {e}", path.display())))
}

fn is_socket(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    metadata.file_type().is_socket()
}

/// A client connection that sends one request and reads one response.
pub struct Client {
    stream: UnixStream,
}

impl Client {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, WireError> {
        let path = path.as_ref();
        let stream = UnixStream::connect(path)
            .await
            .map_err(|e| WireError::not_found(format!("connecting to {}: {e}", path.display())))?;
        Ok(Self { stream })
    }

    /// Sends a request and returns the raw response, including an error response.
    pub async fn call(&mut self, request: &Request) -> Result<Response, WireError> {
        let payload = serde_json::to_vec(request)
            .map_err(|e| WireError::internal(format!("encoding the request: {e}")))?;
        write_frame(&mut self.stream, &payload).await?;

        let reply = read_frame(&mut self.stream)
            .await?
            .ok_or_else(|| WireError::internal("the daemon closed without answering"))?;
        serde_json::from_slice(&reply)
            .map_err(|e| WireError::internal(format!("unreadable response: {e}")))
    }
}
