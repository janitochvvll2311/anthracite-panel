//! The terminal WebSocket, shared by the browser protocol and the CLI.
//!
//! The daemon exposes one endpoint, `GET /api/terminal` with an HTTP upgrade.
//! A Unix socket carries the same upgrade, so the CLI speaks the same messages
//! the browser does instead of a second long-poll API.

use anth_wire::{ErrorKind, TerminalClientMessage, TerminalServerMessage, TerminalSize, WireError};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use hyper::http::header;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{WebSocketStream, client_async};

use crate::transport::{Endpoint, Stream, open_stream};

/// One open terminal session.
pub struct TerminalSession {
    socket: WebSocketStream<Stream>,
}

impl std::fmt::Debug for TerminalSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TerminalSession")
            .finish_non_exhaustive()
    }
}

impl TerminalSession {
    /// Opens a session with the requested initial size.
    pub(crate) async fn open(
        endpoint: &Endpoint,
        token: Option<&str>,
        size: TerminalSize,
    ) -> Result<Self, WireError> {
        size.validate().map_err(WireError::invalid)?;
        let stream = open_stream(endpoint).await?;
        let url = format!(
            "ws://localhost/api/terminal?cols={}&rows={}",
            size.cols, size.rows
        );
        let mut request = url.into_client_request().map_err(|error| {
            WireError::internal(format!("building the terminal request: {error}"))
        })?;
        if let Some(token) = token {
            let value = hyper::http::HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|error| WireError::invalid(format!("invalid bearer token: {error}")))?;
            request.headers_mut().insert(header::AUTHORIZATION, value);
        }
        let (socket, _) = client_async(request, stream).await.map_err(upgrade_error)?;
        Ok(Self { socket })
    }

    /// Sends one message.
    pub async fn send(&mut self, message: &TerminalClientMessage) -> Result<(), WireError> {
        let text = serde_json::to_string(message)
            .map_err(|error| WireError::internal(format!("encoding the message: {error}")))?;
        self.socket
            .send(Message::Text(text.into()))
            .await
            .map_err(|error| WireError::internal(format!("sending to the terminal: {error}")))
    }

    /// Sends keystrokes, base64 encoded on the wire.
    pub async fn input(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        use base64::Engine as _;
        self.send(&TerminalClientMessage::Input {
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
        .await
    }

    /// Updates the window size.
    pub async fn resize(&mut self, size: TerminalSize) -> Result<(), WireError> {
        size.validate().map_err(WireError::invalid)?;
        self.send(&TerminalClientMessage::Resize {
            cols: size.cols,
            rows: size.rows,
        })
        .await
    }

    /// Reads the next message, or `None` when the daemon closes the socket.
    pub async fn next(&mut self) -> Result<Option<TerminalServerMessage>, WireError> {
        read_message(&mut self.socket).await
    }

    /// Splits the session so one task reads while another writes.
    pub fn split(self) -> (TerminalWriter, TerminalReader) {
        let (writer, reader) = self.socket.split();
        (TerminalWriter { writer }, TerminalReader { reader })
    }

    /// Closes the socket; the daemon ends the PTY when it goes away.
    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }
}

/// The writing half of a split session.
pub struct TerminalWriter {
    writer: SplitSink<WebSocketStream<Stream>, Message>,
}

impl TerminalWriter {
    /// Sends one message.
    pub async fn send(&mut self, message: &TerminalClientMessage) -> Result<(), WireError> {
        let text = serde_json::to_string(message)
            .map_err(|error| WireError::internal(format!("encoding the message: {error}")))?;
        self.writer
            .send(Message::Text(text.into()))
            .await
            .map_err(|error| WireError::internal(format!("sending to the terminal: {error}")))
    }

    /// Sends keystrokes, base64 encoded on the wire.
    pub async fn input(&mut self, bytes: &[u8]) -> Result<(), WireError> {
        use base64::Engine as _;
        self.send(&TerminalClientMessage::Input {
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
        .await
    }

    /// Updates the window size.
    pub async fn resize(&mut self, size: TerminalSize) -> Result<(), WireError> {
        size.validate().map_err(WireError::invalid)?;
        self.send(&TerminalClientMessage::Resize {
            cols: size.cols,
            rows: size.rows,
        })
        .await
    }

    /// Closes the socket.
    pub async fn close(mut self) {
        let _ = self.writer.close().await;
    }
}

/// The reading half of a split session.
pub struct TerminalReader {
    reader: SplitStream<WebSocketStream<Stream>>,
}

impl TerminalReader {
    /// Reads the next message, or `None` when the daemon closes the socket.
    pub async fn next(&mut self) -> Result<Option<TerminalServerMessage>, WireError> {
        read_message(&mut self.reader).await
    }
}

async fn read_message<S>(stream: &mut S) -> Result<Option<TerminalServerMessage>, WireError>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        match stream.next().await {
            None => return Ok(None),
            Some(Err(error)) => {
                return Err(WireError::internal(format!(
                    "reading from the terminal: {error}"
                )));
            }
            Some(Ok(Message::Text(text))) => {
                let message = serde_json::from_str(text.as_str()).map_err(|error| {
                    WireError::internal(format!("the terminal sent invalid JSON: {error}"))
                })?;
                return Ok(Some(message));
            }
            Some(Ok(Message::Binary(_))) => {
                return Err(WireError::internal(
                    "the terminal sent a binary frame the protocol does not use",
                ));
            }
            Some(Ok(Message::Close(_))) => return Ok(None),
            Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => continue,
        }
    }
}

fn upgrade_error(error: tokio_tungstenite::tungstenite::Error) -> WireError {
    use tokio_tungstenite::tungstenite::Error as WsError;
    match error {
        WsError::Http(response) => {
            let status = response.status();
            // The daemon answers a refused upgrade with its normal error body,
            // so the caller gets `terminal_disabled` instead of a bare status.
            if let Some(body) = response.body()
                && let Ok(envelope) = serde_json::from_slice::<ErrorEnvelope>(body)
            {
                return envelope.error;
            }
            let kind = if status.as_u16() == 401 {
                ErrorKind::Unauthorized
            } else if status.as_u16() == 403 {
                ErrorKind::Forbidden
            } else {
                ErrorKind::Internal
            };
            WireError::new(
                kind,
                "terminal_upgrade",
                format!("the daemon refused the terminal upgrade with HTTP {status}"),
            )
        }
        other => WireError::internal(format!("opening the terminal: {other}")),
    }
}

#[derive(serde::Deserialize)]
struct ErrorEnvelope {
    error: WireError,
}
