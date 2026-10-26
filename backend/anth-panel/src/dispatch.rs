//! The one place that decides what a request does.
//!
//! The socket and HTTP transports only translate. Every command reaches
//! [`Daemon::execute`], so a transport cannot answer a request differently.

use std::sync::Arc;

use anth_wire::{ErrorKind, PROTOCOL_VERSION, Request, Response, WireError};

use crate::config::{Config, Paths};
use crate::log::{Level, Logger};

/// Everything a request may need from the process.
pub struct Daemon {
    config: Config,
    paths: Paths,
    logger: Arc<Logger>,
}

impl Daemon {
    pub fn new(config: Config, paths: Paths, logger: Arc<Logger>) -> Self {
        Self {
            config,
            paths,
            logger,
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
    }
}
