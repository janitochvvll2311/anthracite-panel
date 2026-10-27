//! Generic JSON-over-WebSocket plumbing.
//!
//! This module knows nothing about terminals, routes or application messages.
//! It carries `Serialize`/`Deserialize` values as text frames, handles control
//! frames, and enforces one size limit in both directions.

use std::error::Error;

use axum::extract::ws::{Message, WebSocket};
use futures_util::SinkExt;
use serde::{Serialize, de::DeserializeOwned};

pub(crate) type WsResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub(crate) struct JsonWebSocket {
    socket: WebSocket,
    max_message_size: usize,
}

impl JsonWebSocket {
    pub fn new(socket: WebSocket, max_message_size: usize) -> Self {
        Self {
            socket,
            max_message_size,
        }
    }

    /// Sends one JSON value as a text frame.
    pub async fn send<T: Serialize>(&mut self, value: &T) -> WsResult<()> {
        let text = serde_json::to_string(value)?;
        if text.len() > self.max_message_size {
            return Err(format!(
                "outgoing message is {} bytes, over the {} byte limit",
                text.len(),
                self.max_message_size
            )
            .into());
        }
        self.socket
            .send(Message::Text(text.into()))
            .await
            .map_err(|error| Box::new(error) as _)
    }

    /// Reads the next JSON value. `Ok(None)` means the peer closed.
    pub async fn next<T: DeserializeOwned>(&mut self) -> WsResult<Option<T>> {
        loop {
            match self.socket.recv().await {
                Some(Ok(Message::Text(text))) => {
                    if text.as_str().len() > self.max_message_size {
                        return Err(format!(
                            "incoming message is {} bytes, over the {} byte limit",
                            text.as_str().len(),
                            self.max_message_size
                        )
                        .into());
                    }
                    return Ok(Some(serde_json::from_str(text.as_str())?));
                }
                Some(Ok(Message::Binary(_))) => {
                    return Err("binary WebSocket frames are not supported".into());
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                Some(Ok(Message::Close(_))) | None => return Ok(None),
                Some(Err(error)) => return Err(Box::new(error)),
            }
        }
    }

    pub async fn close(&mut self) {
        let _ = self.socket.close().await;
    }
}
