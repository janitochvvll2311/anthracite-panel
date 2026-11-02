//! Binding and protecting the Unix socket.

use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::Path;

use anth_contract::Error;
use tokio::net::UnixListener;

use crate::log::Logger;

/// Binds the socket, replacing a stale socket but nothing else.
///
/// A regular file at that path is an error: deleting it because a connection
/// failed would be destroying something that is not ours.
pub async fn bind(path: &Path, logger: &Logger) -> Result<UnixListener, Error> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
        if !metadata.file_type().is_socket() {
            return Err(Error::invalid(format!(
                "{} exists and is not a socket",
                path.display()
            )));
        }
        match tokio::net::UnixStream::connect(path).await {
            Ok(_) => {
                return Err(Error::conflict(format!(
                    "another engine is listening on {}",
                    path.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) if is_stale(&error) => {
                logger.warn(&format!("removing the stale socket {}", path.display()));
                std::fs::remove_file(path).map_err(|error| {
                    Error::internal(format!(
                        "removing the stale socket {}: {error}",
                        path.display()
                    ))
                })?;
            }
            Err(error) => {
                // A permission error is not staleness: the socket may be live and
                // owned by someone else, and deleting it would be an attack.
                return Err(Error::conflict(format!(
                    "cannot tell whether another engine is listening on {}: {error}",
                    path.display()
                )));
            }
        }
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::internal(format!("creating {}: {error}", parent.display())))?;
    }
    UnixListener::bind(path)
        .map_err(|error| Error::internal(format!("binding {}: {error}", path.display())))
}

/// The socket is owner-only until peer-credential groups arrive with the auth
/// slice; `bind` alone leaves whatever the umask allows.
pub fn restrict(path: &Path) -> Result<(), Error> {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|error| {
        Error::internal(format!(
            "setting permissions on {}: {error}",
            path.display()
        ))
    })
}

/// Only a refused connection means nobody is listening; every other error is
/// ambiguous and must not lead to deleting the socket.
fn is_stale(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::ConnectionRefused
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LogSection;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("anth-socket-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn logger() -> Logger {
        Logger::new(LogSection::default())
    }

    #[test]
    fn only_a_refused_connection_is_stale() {
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::ConnectionReset,
        ] {
            assert!(
                !is_stale(&std::io::Error::from(kind)),
                "{kind:?} is not stale"
            );
        }
        assert!(is_stale(&std::io::Error::from(
            std::io::ErrorKind::ConnectionRefused
        )));
    }

    #[tokio::test]
    async fn a_regular_file_is_not_removed() {
        let dir = scratch("regular");
        let path = dir.join("engine.sock");
        std::fs::write(&path, b"important").expect("write");
        let error = bind(&path, &logger()).await.expect_err("not a socket");
        assert!(error.message().contains("not a socket"), "{error}");
        assert_eq!(std::fs::read(&path).expect("read"), b"important");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn a_stale_socket_is_replaced() {
        let dir = scratch("stale");
        let path = dir.join("engine.sock");
        let first = UnixListener::bind(&path).expect("bind");
        drop(first);
        let listener = bind(&path, &logger()).await.expect("rebind");
        drop(listener);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn the_socket_is_owner_only() {
        let dir = scratch("mode");
        let path = dir.join("engine.sock");
        let listener = bind(&path, &logger()).await.expect("bind");
        restrict(&path).expect("restrict");
        let mode = std::fs::metadata(&path).expect("metadata");
        assert_eq!(mode.permissions().mode() & 0o777, 0o600);
        drop(listener);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
