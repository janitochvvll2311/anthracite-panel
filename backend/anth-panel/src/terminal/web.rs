//! Browser-facing terminal protocol and the bridge to the daemon.
//!
//! The panel speaks JSON over a WebSocket; the daemon speaks the line protocol
//! over a Unix socket. This module owns the translation and nothing else. The
//! route sits behind the global JWT middleware like every other `/api` path, so
//! there is no terminal-specific token here.

use anth_proto::{
    AnthReader, AnthWriter, MAX_TERMINAL_INPUT, Request, Response, TerminalOpenArgs, TerminalSize,
};
use axum::extract::ws::WebSocket;
use serde::{Deserialize, Serialize};

use crate::app::AppState;
use crate::ws::JsonWebSocket;

/// Websocket frame cap. A 64 KiB input becomes ~87 KiB of base64 plus JSON
/// overhead; 128 KiB leaves room for the envelope and future fields.
pub(crate) const MAX_WEB_MESSAGE: usize = 128 * 1024;

/// Message sent by the browser.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMessage {
    /// Bytes typed into the terminal, base64 encoded.
    Input { data: String },
    /// New window size in character cells.
    Resize { cols: u16, rows: u16 },
    /// Application-level liveness check.
    Ping,
}

/// Message sent to the browser.
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ServerMessage {
    /// Sent once, before any output.
    Ready {
        shell: String,
        cols: u16,
        rows: u16,
        line_editing: bool,
    },
    /// Terminal bytes, base64 encoded.
    Output { data: String },
    /// The program ended. Sent immediately before the socket closes.
    Exit { code: i32 },
    /// A failure. The socket closes after this frame.
    Error { detail: String },
    /// Reply to an application-level ping.
    Pong,
}

/// Runs one terminal session over an upgraded WebSocket.
pub(crate) async fn serve_socket(socket: WebSocket, state: AppState, size: TerminalSize) {
    let mut ws = JsonWebSocket::new(socket, MAX_WEB_MESSAGE);
    if let Err(detail) = bridge(&mut ws, &state, size).await {
        let _ = ws.send(&ServerMessage::Error { detail }).await;
    }
    ws.close().await;
}

async fn bridge(
    ws: &mut JsonWebSocket,
    state: &AppState,
    size: TerminalSize,
) -> Result<(), String> {
    let stream = state
        .connect_stream()
        .await
        .map_err(|_| "daemon unavailable".to_string())?;
    let (read_half, write_half) = stream.into_split();
    let mut reader = AnthReader::from_read_half(read_half);
    let mut writer = AnthWriter::from_write_half(write_half);

    writer
        .send_request(&Request::TerminalOpen(TerminalOpenArgs {
            size: Some(size),
        }))
        .await
        .map_err(|e| format!("failed to open the terminal: {e}"))?;

    let opened = match reader.receive_response().await {
        Some(Ok(Response::TerminalOpened(opened))) => opened,
        Some(Ok(Response::Error { detail })) => return send_error(ws, detail).await,
        Some(Ok(other)) => {
            return send_error(ws, format!("unexpected daemon response: {other:?}")).await;
        }
        Some(Err(e)) => {
            return send_error(ws, format!("failed to read from the daemon: {e}")).await;
        }
        None => return send_error(ws, "daemon closed the connection".to_string()).await,
    };

    ws.send(&ServerMessage::Ready {
        shell: opened.program,
        cols: opened.size.cols,
        rows: opened.size.rows,
        line_editing: opened.line_editing,
    })
    .await
    .map_err(|e| format!("failed to send ready: {e}"))?;

    loop {
        tokio::select! {
            incoming = ws.next::<ClientMessage>() => {
                match incoming.map_err(|e| format!("invalid client message: {e}"))? {
                    None => {
                        let _ = writer
                            .send_request(&Request::TerminalClose { id: opened.id })
                            .await;
                        return Ok(());
                    }
                    Some(ClientMessage::Input { data }) => {
                        let bytes = anth_proto::b64::decode(&data)
                            .map_err(|e| format!("invalid base64 input: {e}"))?;
                        if bytes.len() > MAX_TERMINAL_INPUT {
                            return Err(format!(
                                "terminal input is {} bytes, over the {} byte limit",
                                bytes.len(),
                                MAX_TERMINAL_INPUT
                            ));
                        }
                        writer
                            .send_request(&Request::TerminalInput {
                                id: opened.id,
                                bytes,
                            })
                            .await
                            .map_err(|e| format!("failed to write terminal input: {e}"))?;
                    }
                    Some(ClientMessage::Resize { cols, rows }) => {
                        let size = TerminalSize { cols, rows };
                        if !size.is_sane() {
                            return Err(format!(
                                "terminal size {cols}x{rows} is outside the allowed range"
                            ));
                        }
                        writer
                            .send_request(&Request::TerminalResize { id: opened.id, size })
                            .await
                            .map_err(|e| format!("failed to resize the terminal: {e}"))?;
                    }
                    Some(ClientMessage::Ping) => {
                        ws.send(&ServerMessage::Pong)
                            .await
                            .map_err(|e| format!("failed to send pong: {e}"))?;
                    }
                }
            }
            daemon = reader.receive_response() => {
                match daemon {
                    Some(Ok(Response::TerminalOutput { bytes, .. })) => {
                        ws.send(&ServerMessage::Output {
                            data: anth_proto::b64::encode(&bytes),
                        })
                        .await
                        .map_err(|e| format!("failed to send terminal output: {e}"))?;
                    }
                    Some(Ok(Response::TerminalExited { status, .. })) => {
                        ws.send(&ServerMessage::Exit { code: status })
                            .await
                            .map_err(|e| format!("failed to send terminal exit: {e}"))?;
                        return Ok(());
                    }
                    Some(Ok(Response::Error { detail })) => {
                        return send_error(ws, detail).await;
                    }
                    Some(Ok(other)) => {
                        return Err(format!("unexpected daemon response: {other:?}"));
                    }
                    Some(Err(e)) => {
                        return Err(format!("failed to read from the daemon: {e}"));
                    }
                    None => return Err("daemon closed the connection".to_string()),
                }
            }
        }
    }
}

/// Sends a terminal error frame and ends the session normally from the bridge's
/// point of view, so the caller does not send a second error.
async fn send_error(ws: &mut JsonWebSocket, detail: String) -> Result<(), String> {
    ws.send(&ServerMessage::Error { detail })
        .await
        .map_err(|e| format!("failed to send terminal error: {e}"))
}
