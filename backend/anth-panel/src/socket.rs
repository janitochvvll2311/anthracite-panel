//! Unix-socket HTTP listener.
//!
//! The same router serves TCP and the local socket. The socket adds one thing:
//! it checks `peer_cred` on every connection and installs a local owner, so the
//! CLI and a `bin/` app get a normal HTTP session without a cookie or token.
//! The extension is set by this listener only; a TCP request cannot carry one.

use std::io;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;

use axum::Router;
use axum::extract::Extension;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tokio::net::UnixListener;

use crate::api::LocalOwner;

/// Serves the router on the socket until the listener fails.
pub async fn serve(listener: UnixListener, router: Router) -> io::Result<()> {
    let daemon_uid = unsafe { libc::getuid() };
    loop {
        let (stream, _) = listener.accept().await?;
        let credentials = match stream.peer_cred() {
            Ok(credentials) => credentials,
            Err(error) => {
                eprintln!("rejecting a socket connection: {error}");
                continue;
            }
        };
        // A client from another uid could still be root; anything else is
        // refused before a single byte reaches the router.
        if credentials.uid() != daemon_uid && credentials.uid() != 0 {
            eprintln!(
                "rejecting socket peer uid {}; the daemon runs as {daemon_uid}",
                credentials.uid()
            );
            continue;
        }
        let owner = LocalOwner(format!("socket:{}", credentials.uid()));
        let router = router.clone().layer(Extension(owner));
        tokio::spawn(async move {
            let connection = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), TowerToHyperService::new(router))
                .with_upgrades();
            if let Err(error) = connection.await {
                eprintln!("socket connection ended: {error}");
            }
        });
    }
}

/// Binds the socket, removing a stale socket left by a previous run.
pub fn bind(path: impl AsRef<Path>) -> Result<UnixListener, anth_wire::WireError> {
    let path = path.as_ref();
    if path.exists() {
        let metadata = std::fs::symlink_metadata(path).map_err(|error| {
            anth_wire::WireError::internal(format!("inspecting {}: {error}", path.display()))
        })?;
        if !metadata.file_type().is_socket() {
            return Err(anth_wire::WireError::invalid(format!(
                "{} exists and is not a socket",
                path.display()
            )));
        }
        std::fs::remove_file(path).map_err(|error| {
            anth_wire::WireError::internal(format!("removing {}: {error}", path.display()))
        })?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            anth_wire::WireError::internal(format!("creating {}: {error}", parent.display()))
        })?;
    }
    let listener = UnixListener::bind(path).map_err(|error| {
        anth_wire::WireError::internal(format!("binding {}: {error}", path.display()))
    })?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))
        .map_err(|error| anth_wire::WireError::internal(format!("setting socket mode: {error}")))?;
    Ok(listener)
}
