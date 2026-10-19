//! The daemon's per-connection loop.
//!
//! Requests are served one at a time, in order, as before. The terminal is the
//! reason this file splits the socket: once a session is open, output must be
//! written while further input is still being read.
//!
//! Each session owns a thread that reads the PTY and forwards bytes into a
//! channel. The connection loop merges those channels and never touches the PTY
//! directly, so a shell sitting idle at a prompt cannot stall the socket.

use std::error::Error;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use anth_proto::{AnthReader, AnthWriter, DEFAULT_TERMINAL_SIZE, Request, Response};
use futures::{Stream, StreamExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::config::Config;
use crate::hostinfo;
use crate::process;
use crate::terminal::{OpenError, Output, Sessions, supports_line_editing};

/// One session's output as a stream tagged with its id.
type OutputStream = Pin<Box<dyn Stream<Item = (u32, Output)> + Send>>;

pub fn handle_request(request: Request) -> Response {
    handle_request_with(request, false)
}

/// Handles a request with the daemon policy that is not part of the wire type.
///
/// Process control is the only policy today: the socket protocol itself does
/// not know whether the deployment allows signals.
pub fn handle_request_with(request: Request, process_control: bool) -> Response {
    match request {
        Request::Echo { message } => Response::Ok { message },
        Request::HostInfo => Response::HostInfo(Box::new(hostinfo::collect())),
        Request::ProcessList => Response::ProcessSnapshot(process::collect(process_control)),
        Request::ProcessSignal { pid, signal } => {
            match process::send_signal(pid, signal, process_control) {
                Ok(()) => Response::ProcessSignalled { pid, signal },
                Err(error) => Response::Error {
                    detail: error.detail(),
                },
            }
        }
        // Terminal operations need a registry and a live socket; they are
        // served by `serve_connection`, which owns both.
        other => Response::Error {
            detail: format!("{other:?} requires an open connection"),
        },
    }
}

pub fn handle_line(line: &str) -> Response {
    match Request::from_json(line) {
        Ok(request) => handle_request(request),
        Err(e) => Response::Error {
            detail: format!("Error parsing request: {e}"),
        },
    }
}

/// Serves one connection until the peer disconnects.
///
/// `sessions` is shared across connections so the cap applies to the daemon as
/// a whole. Sessions are tracked per connection and closed when it ends, so a
/// dropped client never leaves a shell behind.
pub async fn serve_connection(
    socket: UnixStream,
    sessions: Arc<Sessions>,
    program: Arc<str>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    serve_connection_with(socket, sessions, program, false).await
}

/// Serves one connection with the process-control policy applied to requests.
pub async fn serve_connection_with(
    socket: UnixStream,
    sessions: Arc<Sessions>,
    program: Arc<str>,
    process_control: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut owned: Vec<u32> = Vec::new();

    // The pump can exit early on a write error, which is exactly what happens
    // when a client disappears. Cleanup lives outside it so a dropped socket
    // cannot skip it: that would leave a shell running with no owner.
    let outcome = pump(
        socket,
        Arc::clone(&sessions),
        program,
        process_control,
        &mut owned,
    )
    .await;

    for id in owned {
        sessions.close(id).await;
    }

    outcome
}

/// One output event's effect on the connection.
struct OutputReply {
    /// Frame to send to the client, if any.
    response: Option<Response>,
    /// Whether the session is now finished and should leave `owned`.
    ended: bool,
}

async fn pump(
    socket: UnixStream,
    sessions: Arc<Sessions>,
    program: Arc<str>,
    process_control: bool,
    owned: &mut Vec<u32>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (read_half, write_half) = socket.into_split();
    let mut reader = AnthReader::from_read_half(read_half);
    let mut writer = AnthWriter::from_write_half(write_half);

    let mut output = futures::stream::select_all(Vec::<OutputStream>::new());

    loop {
        // With no sessions open there is nothing to watch but the socket, which
        // keeps the common case identical to the old request/response loop.
        let next = if owned.is_empty() {
            reader.receive_request().await
        } else {
            tokio::select! {
                request = reader.receive_request() => request,
                event = output.next() => {
                    if let Some((id, event)) = event {
                        let reply = exit_response(&sessions, id, event).await;
                        if reply.ended {
                            // A finished session must leave `owned`, otherwise
                            // the empty-SelectAll branch below becomes a busy
                            // loop until the client disconnects.
                            owned.retain(|&open| open != id);
                        }
                        if let Some(response) = reply.response {
                            writer.send_response(&response).await?;
                        }
                    } else {
                        // SelectAll only ends when every output stream has
                        // ended. Close anything still tracked and return to
                        // the socket-only path.
                        for id in owned.drain(..) {
                            sessions.close(id).await;
                        }
                    }
                    continue;
                }
            }
        };

        let request = match next {
            Some(Ok(request)) => request,
            Some(Err(e)) => {
                writer
                    .send_response(&Response::Error {
                        detail: format!("Error receiving request: {e}"),
                    })
                    .await?;
                break;
            }
            None => break,
        };

        let response = match request {
            Request::TerminalOpen(args) => {
                let size = args.size.unwrap_or(DEFAULT_TERMINAL_SIZE);
                match sessions.open(&program, size).await {
                    Ok((id, size, program)) => {
                        owned.push(id);
                        if let Some(receiver) = sessions.take_output(id).await {
                            output.push(session_stream(id, receiver));
                        }
                        Response::TerminalOpened(anth_proto::TerminalOpened {
                            id,
                            size,
                            line_editing: supports_line_editing(&program),
                            program,
                        })
                    }
                    Err(OpenError::Spawn(detail)) => Response::Error { detail },
                    Err(e) => Response::Error { detail: e.detail() },
                }
            }
            Request::TerminalInput { id, bytes } => {
                if !owned.contains(&id) {
                    Response::Error {
                        detail: format!("session {id} was not opened on this connection"),
                    }
                } else {
                    match sessions.write(id, &bytes).await {
                        // Success is silent. An `Ok` per keystroke would double
                        // the traffic of a typing session, and it could not be
                        // correlated anyway: responses carry no request id, and
                        // output frames interleave with them.
                        Some(Ok(())) => continue,
                        Some(Err(detail)) => Response::Error { detail },
                        None => Response::Error {
                            detail: format!("session {id} is closed"),
                        },
                    }
                }
            }
            Request::TerminalResize { id, size } => {
                if !owned.contains(&id) {
                    Response::Error {
                        detail: format!("session {id} was not opened on this connection"),
                    }
                } else {
                    match sessions.resize(id, size).await {
                        Some(Ok(())) => continue,
                        Some(Err(detail)) => Response::Error { detail },
                        None => Response::Error {
                            detail: format!("session {id} is closed"),
                        },
                    }
                }
            }
            Request::TerminalClose { id } => {
                sessions.close(id).await;
                owned.retain(|&open| open != id);
                // Silent for the same reason as input, and because the client
                // asked for the session to end: there is nothing to confirm.
                continue;
            }
            other => handle_request_with(other, process_control),
        };

        writer.send_response(&response).await?;
    }

    Ok(())
}

fn session_stream(id: u32, receiver: mpsc::Receiver<Output>) -> OutputStream {
    Box::pin(ReceiverStream::new(receiver).map(move |event| (id, event)))
}

/// Turns one output event into the frame to send and whether the session ended.
///
/// Data becomes `TerminalOutput`. An exit closes the session and reports the
/// real status once, so the client can tell a finished shell from a dropped
/// connection.
async fn exit_response(sessions: &Arc<Sessions>, id: u32, event: Output) -> OutputReply {
    match event {
        Output::Data(bytes) if !bytes.is_empty() => OutputReply {
            response: Some(Response::TerminalOutput { id, bytes }),
            ended: false,
        },
        Output::Data(_) => OutputReply {
            response: None,
            ended: false,
        },
        Output::Exited(_) | Output::Failed => {
            let status = match sessions.with(id, |session| session.try_exit()).await {
                Some(Ok(Some(code))) => code,
                // No status available: say failure rather than invent a success.
                _ => -1,
            };
            sessions.close(id).await;
            OutputReply {
                response: Some(Response::TerminalExited { id, status }),
                ended: true,
            }
        }
    }
}

pub fn clear_socket_path(path: impl AsRef<Path>) -> std::io::Result<()> {
    match std::fs::remove_file(path.as_ref()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub async fn serve(config: &Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let socket_path = Path::new(&config.socket_path);
    clear_socket_path(socket_path)?;

    let listener = UnixListener::bind(socket_path)?;
    println!("Listening on {}", socket_path.display());

    let sessions = Sessions::new(config.terminal.max_sessions);
    let shell = config.terminal.resolved_shell();
    eprintln!(
        "Terminal shell: {shell} (line editing: {})",
        supports_line_editing(&shell)
    );
    let program: Arc<str> = Arc::from(shell);
    let process_control = config.process.control;

    loop {
        let (socket, _) = listener.accept().await?;
        let sessions = Arc::clone(&sessions);
        let program = Arc::clone(&program);
        tokio::spawn(async move {
            if let Err(e) = serve_connection_with(socket, sessions, program, process_control).await
            {
                eprintln!("Error handling connection: {e}");
            }
        });
    }
}
