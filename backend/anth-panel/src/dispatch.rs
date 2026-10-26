//! The one place that decides what a request does.
//!
//! The socket and HTTP transports only translate. Every command reaches
//! [`Daemon::execute`], so a transport cannot answer a request differently.

use std::sync::Arc;

use anth_wire::{ErrorKind, PROTOCOL_VERSION, PageRequest, Request, Response, WireError};

use crate::config::{Config, Paths};
use crate::files::FileService;
use crate::lifecycle::{ACTION_GRACE, Lifecycle};
use crate::log::{Level, Logger};

/// Everything a request may need from the process.
pub struct Daemon {
    config: Config,
    paths: Paths,
    logger: Arc<Logger>,
    lifecycle: Arc<Lifecycle>,
    files: Result<FileService, String>,
}

impl Daemon {
    /// A daemon that may restart but never reboot.
    pub fn new(config: Config, paths: Paths, logger: Arc<Logger>) -> Self {
        Self::with_lifecycle(config, paths, logger, Arc::new(Lifecycle::without_reboot()))
    }

    pub fn with_lifecycle(
        config: Config,
        paths: Paths,
        logger: Arc<Logger>,
        lifecycle: Arc<Lifecycle>,
    ) -> Self {
        let root = {
            let root = std::path::PathBuf::from(&config.files.root);
            if root.is_absolute() {
                root
            } else {
                paths.config_dir.join(root)
            }
        };
        let files = FileService::new(
            root,
            config.files.write,
            config.files.max_entries,
            config.files.max_preview_bytes,
            config.files.max_upload_bytes,
        );
        Self {
            config,
            paths,
            logger,
            lifecycle,
            files,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn logger(&self) -> &Arc<Logger> {
        &self.logger
    }

    pub fn lifecycle(&self) -> &Arc<Lifecycle> {
        &self.lifecycle
    }

    fn files(&self) -> Result<&FileService, WireError> {
        self.files
            .as_ref()
            .map_err(|error| WireError::internal(format!("the file root is unavailable: {error}")))
    }

    /// Handles one request.
    pub async fn execute(&self, request: Request) -> Response {
        let operation = operation_of(&request);
        let started = std::time::Instant::now();
        let response = match self.dispatch(request).await {
            Ok(response) => response,
            Err(error) => Response::Error(error),
        };
        let failed = response.error().is_some();
        let ms = started.elapsed().as_millis().to_string();
        self.logger.event(
            if failed { Level::Warn } else { Level::Info },
            "request",
            if failed { "failed" } else { "answered" },
            &[("op", operation), ("ms", &ms)],
        );
        response
    }

    async fn dispatch(&self, request: Request) -> Result<Response, WireError> {
        match request {
            Request::Hello {
                protocol_version,
                client,
                features,
            } => hello(protocol_version, &client, &features),
            Request::HostInfo => {
                let info = tokio::task::spawn_blocking(anth_host::collect)
                    .await
                    .map_err(|error| {
                        WireError::internal(format!("the host-info task failed: {error}"))
                    })?;
                Ok(Response::HostInfo(info))
            }
            Request::Restart => {
                let lifecycle = Arc::clone(&self.lifecycle);
                tokio::spawn(async move {
                    tokio::time::sleep(ACTION_GRACE).await;
                    lifecycle.restart();
                });
                Ok(Response::Restarting {
                    pid: std::process::id(),
                })
            }
            Request::SystemReboot { delay_s } => {
                self.lifecycle.authorize_reboot(delay_s)?;
                let lifecycle = Arc::clone(&self.lifecycle);
                tokio::spawn(async move {
                    tokio::time::sleep(ACTION_GRACE).await;
                    if let Err(error) = lifecycle.perform_reboot(delay_s) {
                        eprintln!("anth-panel: reboot failed: {error}");
                    }
                });
                Ok(Response::Rebooting { delay_s })
            }
            Request::FilesList {
                path,
                limit,
                cursor,
            } => {
                let request = PageRequest { limit, cursor };
                Ok(Response::FilesList(self.files()?.list(&path, &request)?))
            }
            Request::FilesRead {
                path,
                offset,
                max_bytes,
            } => Ok(Response::FilesRead(
                self.files()?.read(&path, offset, max_bytes)?,
            )),
            Request::FilesDelete { path } => Ok(Response::FilesDeleted {
                path: self.files()?.delete(&path)?,
            }),
            Request::FilesWrite {
                path,
                content,
                create,
            } => Ok(Response::FilesWritten(
                self.files()?.write(&path, &content, create)?,
            )),
        }
    }
}

fn hello(protocol_version: u16, client: &str, features: &[String]) -> Result<Response, WireError> {
    if protocol_version != PROTOCOL_VERSION {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "protocol_mismatch",
            format!(
                "this daemon speaks protocol {PROTOCOL_VERSION}, the client speaks {protocol_version}"
            ),
        ));
    }
    if client.is_empty() || client.len() > 64 || features.len() > 32 {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "invalid_hello",
            "the Hello client or feature list is out of bounds",
        ));
    }
    if features.iter().any(|feature| feature.len() > 64) {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "invalid_hello",
            "a Hello feature is longer than 64 characters",
        ));
    }

    Ok(Response::Welcome {
        protocol_version: PROTOCOL_VERSION,
        server: "anth-panel".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        features: Vec::new(),
    })
}

fn operation_of(request: &Request) -> &'static str {
    match request {
        Request::Hello { .. } => "hello",
        Request::HostInfo => "host-info",
        Request::Restart => "restart",
        Request::SystemReboot { .. } => "system-reboot",
        Request::FilesList { .. } => "files-list",
        Request::FilesRead { .. } => "files-read",
        Request::FilesDelete { .. } => "files-delete",
        Request::FilesWrite { .. } => "files-write",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::lifecycle::{Recorded, recorded};
    use crate::log::Settings;

    fn daemon(allow_reboot: bool) -> (Arc<Daemon>, Arc<Recorded>) {
        let config = Config::dev_default();
        let paths = config
            .paths("/tmp/anthracite-panel/tests/dispatch/config.toml")
            .expect("paths");
        let (lifecycle, recorder) = recorded(allow_reboot);
        let logger = Arc::new(Logger::buffer(Settings::default()).0);
        (
            Arc::new(Daemon::with_lifecycle(
                config,
                paths,
                logger,
                Arc::new(lifecycle),
            )),
            recorder,
        )
    }

    #[tokio::test]
    async fn restart_is_answered_before_the_action() {
        let (daemon, recorder) = daemon(false);
        let response = daemon.execute(Request::Restart).await;
        assert!(matches!(response, Response::Restarting { .. }));
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(200)).await;
        assert_eq!(recorder.restarts.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn reboot_without_the_switch_is_refused_and_not_scheduled() {
        let (daemon, recorder) = daemon(false);
        let response = daemon.execute(Request::SystemReboot { delay_s: 0 }).await;
        assert_eq!(response.error().expect("error").kind, ErrorKind::Forbidden);
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(100)).await;
        assert!(recorder.reboots.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_authorized_reboot_is_scheduled() {
        let (daemon, recorder) = daemon(true);
        let response = daemon.execute(Request::SystemReboot { delay_s: 7 }).await;
        assert!(matches!(response, Response::Rebooting { delay_s: 7 }));
        tokio::time::sleep(ACTION_GRACE + Duration::from_millis(200)).await;
        assert_eq!(recorder.reboots.lock().unwrap().as_slice(), &[7]);
    }
}
