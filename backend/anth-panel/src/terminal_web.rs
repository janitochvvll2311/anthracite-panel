//! Browser-facing terminal protocol and the bridge to the dispatcher.
//!
//! The browser speaks JSON over one WebSocket; the daemon exposes handle
//! requests through the same dispatcher as every other transport. The bridge
//! runs a long-poll read next to the input loop, so a quiet shell does not stop
//! keystrokes.

use std::sync::Arc;

use anth_wire::{
    MAX_TERMINAL_WAIT_MS, TerminalClientMessage, TerminalServerMessage, TerminalSize, WireError,
};

use crate::rpc::{Request, Response};
use axum::extract::ws::WebSocket;

use crate::dispatch::Daemon;
use crate::ws::JsonWebSocket;

/// WebSocket frame cap. Input chunks are 16 KiB; base64 and JSON leave plenty
/// of room under this ceiling.
pub(crate) const MAX_WEB_MESSAGE: usize = 128 * 1024;

/// Runs one terminal session over an upgraded WebSocket.
pub(crate) async fn serve_socket(
    socket: WebSocket,
    daemon: Arc<Daemon>,
    owner: String,
    size: TerminalSize,
) {
    let mut ws = JsonWebSocket::new(socket, MAX_WEB_MESSAGE);
    if let Err(detail) = bridge(&mut ws, &daemon, &owner, size).await {
        let _ = ws.send(&TerminalServerMessage::Error { detail }).await;
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

    ws.send(&TerminalServerMessage::Ready {
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
            incoming = ws.next::<TerminalClientMessage>() => {
                match incoming.map_err(|error| format!("invalid client message: {error}"))? {
                    None => break Ok(()),
                    Some(TerminalClientMessage::Input { data }) => {
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
                    Some(TerminalClientMessage::Resize { cols, rows }) => {
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
                    Some(TerminalClientMessage::Ping) => {
                        ws.send(&TerminalServerMessage::Pong)
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
                            ws.send(&TerminalServerMessage::Output {
                                data: chunk.bytes_base64,
                            })
                            .await
                            .map_err(|error| format!("failed to send output: {error}"))?;
                        }
                        if chunk.eof {
                            ws.send(&TerminalServerMessage::Exit {
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
    ws.send(&TerminalServerMessage::Error {
        detail: error.message,
    })
    .await
    .map_err(|error| format!("failed to send terminal error: {error}"))
}
