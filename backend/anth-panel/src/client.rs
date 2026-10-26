//! The CLI's socket client.

use std::path::Path;

use anth_wire::{PROTOCOL_VERSION, Request, Response, WireError, read_frame, write_frame};
use tokio::net::UnixStream;

pub struct Client {
    stream: UnixStream,
}

impl Client {
    pub async fn connect(path: impl AsRef<Path>) -> Result<Self, WireError> {
        let path = path.as_ref();
        let stream = UnixStream::connect(path).await.map_err(|error| {
            WireError::not_found(format!("connecting to {}: {error}", path.display()))
        })?;
        Ok(Self { stream })
    }

    /// Sends the mandatory Hello and checks the daemon's answer.
    pub async fn hello(&mut self) -> Result<Response, WireError> {
        let response = self
            .call(&Request::Hello {
                protocol_version: PROTOCOL_VERSION,
                client: format!("cli/{}", env!("CARGO_PKG_VERSION")),
                features: Vec::new(),
            })
            .await?;
        match response {
            Response::Welcome { .. } => Ok(response),
            other => Err(WireError::invalid(format!(
                "the daemon answered Hello with {other:?}"
            ))),
        }
    }

    /// Sends one request and returns its response.
    pub async fn call(&mut self, request: &Request) -> Result<Response, WireError> {
        let payload = serde_json::to_vec(request)
            .map_err(|error| WireError::internal(format!("encoding the request: {error}")))?;
        write_frame(&mut self.stream, &payload).await?;
        let reply = read_frame(&mut self.stream)
            .await?
            .ok_or_else(|| WireError::internal("the daemon closed without answering"))?;
        let response: Response = serde_json::from_slice(&reply)
            .map_err(|error| WireError::internal(format!("unreadable response: {error}")))?;
        response.into_result()
    }
}
