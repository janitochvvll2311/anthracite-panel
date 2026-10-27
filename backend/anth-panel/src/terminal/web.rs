//! Browser-facing terminal protocol and the bridge to the dispatcher.
//!
//! The browser speaks JSON over one WebSocket; the daemon exposes handle
//! requests through the same dispatcher as every other transport. The bridge
//! runs a long-poll read next to the input loop, so a quiet shell does not stop
//! keystrokes.

use std::sync::Arc;

use anth_wire::{MAX_TERMINAL_WAIT_MS, Request, Response, TerminalSize, WireError};
use axum::extract::ws::WebSocket;
use serde::{Deserialize, Serialize};

use crate::dispatch::Daemon;
use crate::ws::JsonWebSocket;

/// WebSocket frame cap. Input chunks are 16 KiB; base64 and JSON leave plenty
/// of room under this ceiling.
pub(crate) const MAX_WEB_MESSAGE: usize = 128 * 1024;

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ClientMessage {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
    Ping,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
enum ServerMessage {
    Ready {
        shell: String,
        cols: u16,
        rows: u16,
        line_editing: bool,
    },
    Output {
        data: String,
    },
    Exit {
        code: i32,
    },
    Error {
        detail: String,
    },
    Pong,
}

/// Runs one terminal session over an upgraded WebSocket.
pub(crate) async fn serve_socket(
    socket: WebSocket,
    daemon: Arc<Daemon>,
    owner: String,
    size: TerminalSize,
) {
    let mut ws = JsonWebSocket::new(socket, MAX_WEB_MESSAGE);
    if let Err(detail) = bridge(&mut ws, &daemon, &owner, size).await {
        let _ = ws.send(&ServerMessage::Error { detail }).await;
    }
    ws.close().await;
}

async fn bridge(
    ws: &mut JsonWebSocket,
    daemon: &Arc<Daemon>,
    owner: &str,
    size: TerminalSize,
) -> Result<(), String> {
    let opened = match daemon
        .execute_as(
            owner,
            Request::TerminalOpen {
                cols: size.cols,
                rows: size.rows,
            },
        )
        .await
    {
        Response::TerminalOpened(opened) => opened,
        Response::Error(error) => return send_error(ws, error).await,
        other => return Err(format!("unexpected terminal open answer: {other:?}")),
    };

    ws.send(&ServerMessage::Ready {
        shell: opened.program.clone(),
        cols: opened.cols,
        rows: opened.rows,
        line_editing: opened.line_editing,
    })
    .await
    .map_err(|error| format!("failed to send ready: {error}"))?;

    let id = opened.id;
    let result = loop {
        tokio::select! {
            incoming = ws.next::<ClientMessage>() => {
                match incoming.map_err(|error| format!("invalid client message: {error}"))? {
                    None => break Ok(()),
                    Some(ClientMessage::Input { data }) => {
                        // The dispatcher decodes and bounds the bytes. Passing the
                        // frame through avoids a decode/encode round trip on every
                        // keystroke.
                        if let Response::Error(error) = daemon
                            .execute_as(owner, Request::TerminalWrite {
                                id: id.clone(),
                                bytes_base64: data,
                            })
                            .await
                        {
                            send_error(ws, error).await?;
                            break Ok(());
                        }
                    }
                    Some(ClientMessage::Resize { cols, rows }) => {
                        let size = TerminalSize { cols, rows };
                        size.validate()?;
                        if let Response::Error(error) = daemon
                            .execute_as(owner, Request::TerminalResize {
                                id: id.clone(),
                                cols,
                                rows,
                            })
                            .await
                        {
                            send_error(ws, error).await?;
                            break Ok(());
                        }
                    }
                    Some(ClientMessage::Ping) => {
                        ws.send(&ServerMessage::Pong)
                            .await
                            .map_err(|error| format!("failed to send pong: {error}"))?;
                    }
                }
            }
            response = daemon.execute_as(owner, Request::TerminalRead {
                id: id.clone(),
                wait_ms: MAX_TERMINAL_WAIT_MS,
            }) => {
                match response {
                    Response::TerminalOutput(chunk) => {
                        if !chunk.bytes_base64.is_empty() {
                            ws.send(&ServerMessage::Output {
                                data: chunk.bytes_base64,
                            })
                            .await
                            .map_err(|error| format!("failed to send output: {error}"))?;
                        }
                        if chunk.eof {
                            ws.send(&ServerMessage::Exit {
                                code: chunk.exit_code.unwrap_or(-1),
                            })
                            .await
                            .map_err(|error| format!("failed to send exit: {error}"))?;
                            break Ok(());
                        }
                    }
                    Response::Error(error) => return send_error(ws, error).await,
                    other => break Err(format!("unexpected terminal read answer: {other:?}")),
                }
            }
        }
    };

    let _ = daemon
        .execute_as(owner, Request::TerminalClose { id })
        .await;
    result
}

async fn send_error(ws: &mut JsonWebSocket, error: WireError) -> Result<(), String> {
    ws.send(&ServerMessage::Error {
        detail: error.message,
    })
    .await
    .map_err(|error| format!("failed to send terminal error: {error}"))
}
