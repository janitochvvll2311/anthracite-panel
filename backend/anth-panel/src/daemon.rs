//! The daemon's per-connection loop.
//!
//! Requests are served one at a time, in order, as before. The terminal is the
//! reason this file splits the socket: once a session is open, output must be
//! written while further input is still being read.
//!
//! Each session owns a thread that reads the PTY and forwards bytes into a
//! channel. The connection loop merges those channels and never touches the PTY
//! directly, so a shell sitting idle at a prompt cannot stall the socket.
//!
//! Request dispatch lives in `crate::handler`. This module owns the connection,
//! the sessions and the uploads, and asks the handler chain about everything
//! that is stateless.

use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use anth_proto::{AnthReader, AnthWriter, DEFAULT_TERMINAL_SIZE, Request, Response};
use futures::{Stream, StreamExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::accounts_admin;
use crate::config::{Config, FilesConfig};
use crate::files::{FileService, PendingUpload};
use crate::handler::{self, CoreHandler, Dispatch};
use crate::terminal::{OpenError, Output, Sessions, supports_line_editing};
use crate::volumes;

/// One session's output as a stream tagged with its id.
type OutputStream = Pin<Box<dyn Stream<Item = (u32, Output)> + Send>>;

/// Deployment policy for the operations exposed by the socket.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// The `[process]` policy, resolved once so the snapshot and both gates
    /// read the same values.
    pub process: crate::process::ProcessPolicy,
    pub network_control: bool,
    pub firewall_control: bool,
    pub volume_control: bool,
    /// The `[accounts]` policy the guards and the snapshot read. The control
    /// switches live here too, so a snapshot can never disagree with the gate.
    pub accounts: crate::config::AccountsConfig,
    /// The account the daemon runs as, which it may not delete or lock.
    pub daemon_user: String,
    /// `None` disables the mount target ceiling.
    pub mount_root: Option<std::path::PathBuf>,
    /// Paths no unmount may cover: the socket, the config and the served root.
    pub protected_paths: Vec<std::path::PathBuf>,
    /// Shared by network, firewall and volume control commands.
    pub sudo: bool,
}

pub fn handle_request(request: Request) -> Response {
    let files = handler::default_files();
    handle_request_with(request, &Policy::default(), &files)
}

/// Handles a request with the daemon policy that is not part of the wire type.
///
/// Process, network, firewall and volume control and file writes are deployment
/// policy: the socket protocol itself does not know whether the host allows
/// them. This synchronous path is used by the line helper and unit tests.
/// The async connection loop sends blocking requests to a worker instead.
pub fn handle_request_with(request: Request, policy: &Policy, files: &FileService) -> Response {
    let core = CoreHandler;
    handler::dispatch(&[&core], &request, &Dispatch { policy, files })
}

/// An empty `mount_root` disables the target ceiling; anything else is the
/// directory every mount target must resolve inside.
fn mount_root(configured: &str) -> Option<std::path::PathBuf> {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(std::path::PathBuf::from(trimmed))
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
    let files = Arc::new(
        FileService::new(&FilesConfig::default())
            .expect("the default filesystem root must be usable"),
    );
    serve_connection_with(socket, sessions, program, Policy::default(), files).await
}

/// Serves one connection with the deployment policy applied to requests.
pub async fn serve_connection_with(
    socket: UnixStream,
    sessions: Arc<Sessions>,
    program: Arc<str>,
    policy: Policy,
    files: Arc<FileService>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut owned: Vec<u32> = Vec::new();

    // The pump can exit early on a write error, which is exactly what happens
    // when a client disappears. Cleanup lives outside it so a dropped socket
    // cannot skip it: that would leave a shell running with no owner.
    let outcome = pump(
        socket,
        Arc::clone(&sessions),
        program,
        policy,
        files,
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

/// Answers one request that carries no connection state.
async fn answer(
    core: &CoreHandler,
    request: Request,
    policy: &Policy,
    files: &Arc<FileService>,
) -> Response {
    if !handler::blocks(&[core], &request) {
        return handler::dispatch(
            &[core],
            &request,
            &Dispatch {
                policy,
                files: files.as_ref(),
            },
        );
    }

    let policy = policy.clone();
    let files = Arc::clone(files);
    match tokio::task::spawn_blocking(move || {
        handler::dispatch(
            &[&CoreHandler],
            &request,
            &Dispatch {
                policy: &policy,
                files: files.as_ref(),
            },
        )
    })
    .await
    {
        Ok(response) => response,
        Err(error) => Response::Error {
            detail: format!("the request worker did not finish: {error}"),
        },
    }
}

async fn pump(
    socket: UnixStream,
    sessions: Arc<Sessions>,
    program: Arc<str>,
    policy: Policy,
    files: Arc<FileService>,
    owned: &mut Vec<u32>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (read_half, write_half) = socket.into_split();
    let mut reader = AnthReader::from_read_half(read_half);
    let mut writer = AnthWriter::from_write_half(write_half);

    let core = CoreHandler;

    let mut output = futures::stream::select_all(Vec::<OutputStream>::new());
    let mut uploads: HashMap<u32, PendingUpload> = HashMap::new();
    let mut next_upload_id: u32 = 1;

    loop {
        // With no sessions open there is nothing to watch but the socket, which
        // keeps the common case identical to a request/response loop.
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
            Request::FileWriteStart {
                path,
                overwrite,
                require_existing,
                expected_modified_ms,
            } => {
                match files.start_upload(&path, overwrite, require_existing, expected_modified_ms) {
                    Ok(upload) => {
                        let id = allocate_upload_id(&mut next_upload_id, &uploads);
                        uploads.insert(id, upload);
                        Response::FileWriteStarted { id }
                    }
                    Err(error) => Response::FileError(error),
                }
            }
            Request::FileWriteChunk { id, bytes } => match uploads.get_mut(&id) {
                Some(upload) => match upload.write_chunk(&bytes) {
                    Ok(received) => Response::FileWriteAccepted { id, received },
                    Err(error) => {
                        // A failed chunk invalidates the upload; dropping it
                        // removes the temporary file.
                        uploads.remove(&id);
                        Response::FileError(error)
                    }
                },
                None => Response::FileError(anth_proto::FileError::new(
                    anth_proto::FileErrorKind::NotFound,
                    format!("upload {id} is not open"),
                )),
            },
            Request::FileWriteFinish { id } => match uploads.remove(&id) {
                Some(upload) => match upload.finish() {
                    Ok(written) => Response::FileWritten(written),
                    Err(error) => Response::FileError(error),
                },
                None => Response::FileError(anth_proto::FileError::new(
                    anth_proto::FileErrorKind::NotFound,
                    format!("upload {id} is not open"),
                )),
            },
            Request::FileWriteAbort { id } => {
                // Removing the PendingUpload drops it, which removes the temp
                // file. Aborting an unknown id is idempotent.
                uploads.remove(&id);
                Response::FileWriteAborted { id }
            }
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
            other => answer(&core, other, &policy, &files).await,
        };

        writer.send_response(&response).await?;
    }

    Ok(())
}

fn session_stream(id: u32, receiver: mpsc::Receiver<Output>) -> OutputStream {
    Box::pin(ReceiverStream::new(receiver).map(move |event| (id, event)))
}

fn allocate_upload_id(next: &mut u32, uploads: &HashMap<u32, PendingUpload>) -> u32 {
    loop {
        let id = *next;
        *next = next.wrapping_add(1);
        if *next == 0 {
            *next = 1;
        }
        if !uploads.contains_key(&id) {
            return id;
        }
    }
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

pub fn policy_for(config: &Config, config_path: &Path) -> Policy {
    Policy {
        process: crate::process::ProcessPolicy::new(&config.process),
        network_control: config.network.control,
        firewall_control: config.firewall.control,
        volume_control: config.volumes.control,
        accounts: config.accounts.clone(),
        daemon_user: accounts_admin::daemon_user(),
        mount_root: mount_root(&config.volumes.mount_root),
        protected_paths: volumes::protected_paths(
            Some(config_path),
            Path::new(&config.files.root),
            Path::new(&config.socket_path),
        ),
        sudo: config.sudo,
    }
}

/// `config_path` is the file the daemon actually loaded, which `main` resolves
/// from `--config`. It cannot be re-derived from `ANTH_CONFIG` here: the shipped
/// entrypoint always passes `--config` and only sets the variable when the
/// operator did.
pub async fn serve(
    config: &Config,
    config_path: &Path,
) -> Result<(), Box<dyn Error + Send + Sync>> {
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
    let policy = policy_for(config, config_path);
    let files = Arc::new(
        FileService::new(&config.files).map_err(|e| format!("invalid files configuration: {e}"))?,
    );

    loop {
        let (socket, _) = listener.accept().await?;
        let sessions = Arc::clone(&sessions);
        let program = Arc::clone(&program);
        let files = Arc::clone(&files);
        let policy = policy.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_connection_with(socket, sessions, program, policy, files).await {
                eprintln!("Error handling connection: {e}");
            }
        });
    }
}
