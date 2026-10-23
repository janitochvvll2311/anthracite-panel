//! Unix-socket server and client for the panel protocol.
//!
//! One request per connection turn, answered in order, which is what v0.1.0 did
//! and what the CLI and HTTP layers expect. The difference is the framing: see
//! `anth_wire::frame`.

use std::path::{Path, PathBuf};
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

/// Default socket path, matching v0.1.0 so an upgraded host needs no migration.
pub fn default_socket_path() -> PathBuf {
    match std::env::var("ANTH_SOCKET") {
        Ok(path) if !path.is_empty() => PathBuf::from(path),
        _ => PathBuf::from("/var/tmp/anth.sock"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A socket path unique to this test, removed on drop.
    struct SocketPath(PathBuf);

    impl SocketPath {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            Self(
                std::env::temp_dir()
                    .join(format!("anth-{label}-{}-{nanos}.sock", std::process::id())),
            )
        }
    }

    impl Drop for SocketPath {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Starts a daemon on a unique path and returns the path once it is bound.
    async fn spawn_daemon(label: &str) -> SocketPath {
        let path = SocketPath::new(label);
        let listener = bind(&path.0).expect("binding the test socket");
        let daemon = Arc::new(Daemon::new(crate::plugins::registry::Registry::empty()));
        tokio::spawn(async move {
            let _ = serve(listener, daemon).await;
        });
        path
    }

    #[tokio::test]
    async fn a_request_reaches_the_daemon_over_the_socket() {
        let path = spawn_daemon("round-trip").await;
        let mut client = Client::connect(&path.0).await.expect("connecting");

        let response = client.call(&Request::HostInfo).await.expect("calling");

        match response {
            Response::HostInfo(info) => {
                assert!(!info.hostname.is_empty(), "the real host has a hostname");
                assert!(!info.os.arch.is_empty());
            }
            other => panic!("expected a snapshot, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn one_connection_can_carry_several_requests() {
        let path = spawn_daemon("pipeline").await;
        let mut client = Client::connect(&path.0).await.expect("connecting");

        let first = client
            .call(&Request::Hello {
                protocol_version: anth_wire::PROTOCOL_VERSION,
            })
            .await
            .expect("calling");
        assert!(matches!(first, Response::Welcome { .. }));

        let second = client.call(&Request::HostInfo).await.expect("calling");
        assert!(matches!(second, Response::HostInfo(_)));
    }

    #[tokio::test]
    async fn an_unreadable_request_is_answered_and_the_connection_survives() {
        let path = spawn_daemon("bad-request").await;

        let mut raw = UnixStream::connect(&path.0).await.expect("connecting");
        write_frame(&mut raw, b"{\"type\":\"nonsense\"}")
            .await
            .expect("writing");
        let reply = read_frame(&mut raw)
            .await
            .expect("reading")
            .expect("a reply");
        let response: Response = serde_json::from_slice(&reply).expect("decoding");
        assert_eq!(
            response.error().map(|e| e.kind),
            Some(anth_wire::ErrorKind::Invalid)
        );

        // The same connection must still work: a bad frame is not a hang-up.
        write_frame(&mut raw, br#"{"type":"host_info"}"#)
            .await
            .expect("writing");
        let reply = read_frame(&mut raw)
            .await
            .expect("reading")
            .expect("a reply");
        let response: Response = serde_json::from_slice(&reply).expect("decoding");
        assert!(matches!(response, Response::HostInfo(_)));
    }

    #[tokio::test]
    async fn connecting_to_a_missing_socket_says_so() {
        let path = SocketPath::new("absent");
        let error = Client::connect(&path.0).await.err().expect("must fail");
        assert_eq!(error.kind, anth_wire::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn binding_replaces_a_stale_socket() {
        let path = SocketPath::new("stale");
        let first = bind(&path.0).expect("first bind");
        drop(first);
        // The file is still on disk after the listener is dropped.
        assert!(path.0.exists(), "the stale socket file should remain");
        let _second = bind(&path.0).expect("a second bind must replace the stale file");
    }

    #[tokio::test]
    async fn binding_refuses_a_regular_file_instead_of_deleting_it() {
        let path = SocketPath::new("regular");
        std::fs::write(&path.0, b"important").expect("writing the file");

        let error = bind(&path.0).expect_err("must refuse");

        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
        assert_eq!(
            std::fs::read(&path.0).expect("the file must survive"),
            b"important"
        );
    }
}
