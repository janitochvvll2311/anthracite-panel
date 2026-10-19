use anth_proto::{Request, Response};

use crate::cli::{Command, FileCommand, ProcessCommand};
use crate::render;

/// Maps a client-only subcommand to the request it sends over the socket.
/// Returns `None` for subcommands that run locally (`daemon`, `api`, `run`);
/// those never travel as requests.
pub fn request_for(command: &Command) -> Option<Request> {
    match command {
        Command::Echo { message } => Some(Request::Echo {
            message: message.clone(),
        }),
        Command::HostInfo(_) => Some(Request::HostInfo),
        Command::Process(args) => match &args.command {
            ProcessCommand::List(_) => Some(Request::ProcessList),
            ProcessCommand::Signal(signal) => Some(Request::ProcessSignal {
                pid: signal.pid,
                signal: signal.signal,
            }),
        },
        Command::File(args) => match &args.command {
            FileCommand::List(list) => Some(Request::FileList {
                path: list.path.clone(),
            }),
            FileCommand::Read(_) | FileCommand::Upload(_) => None,
            FileCommand::Mkdir(args) => Some(Request::FileMkdir {
                path: args.path.clone(),
                recursive: args.parents,
            }),
            FileCommand::Rename(args) => Some(Request::FileRename {
                from: args.from.clone(),
                to: args.to.clone(),
            }),
            FileCommand::Delete(args) => Some(Request::FileDelete {
                path: args.path.clone(),
                recursive: args.recursive,
            }),
        },
        Command::Terminal(_) | Command::Daemon | Command::Api(_) | Command::Run(_) => None,
    }
}

pub fn handle_response(response: &Response) {
    match response {
        Response::Ok { message } => {
            println!("[OK] {}", message);
        }
        Response::Error { detail } => {
            eprintln!("[ERROR] {}", detail);
        }
        Response::HostInfo(info) => {
            println!("{}", render::render_host_info(info));
        }
        Response::ProcessSnapshot(snapshot) => {
            println!("{}", render::render_process_snapshot(snapshot));
        }
        Response::ProcessSignalled { pid, signal } => {
            println!("[OK] sent {signal} to process {pid}");
        }
        Response::FileListing(listing) => {
            println!("{}", render::render_file_listing(listing));
        }
        Response::FileChunk(chunk) => {
            eprintln!(
                "[ERROR] unexpected file chunk for {} ({} bytes)",
                chunk.path,
                chunk.bytes.len()
            );
        }
        Response::FileWritten(written) => {
            println!("[OK] wrote {} ({} bytes)", written.path, written.size);
        }
        Response::FileCreated { path } => {
            println!("[OK] created {path}");
        }
        Response::FileDeleted { path } => {
            println!("[OK] deleted {path}");
        }
        Response::FileRenamed { from, to } => {
            println!("[OK] renamed {from} to {to}");
        }
        Response::FileWriteStarted { id } => {
            println!("[OK] upload {id} started");
        }
        Response::FileWriteAccepted { id, received } => {
            println!("[OK] upload {id} received {received} bytes");
        }
        Response::FileWriteAborted { id } => {
            println!("[OK] upload {id} aborted");
        }
        Response::FileError(error) => {
            eprintln!("[ERROR] {}", error.detail);
        }
        // The CLI has no terminal subcommand; these only reach it if a daemon
        // volunteers frames nobody asked for.
        Response::TerminalOpened(opened) => {
            eprintln!(
                "[ERROR] unexpected terminal session {} from the daemon",
                opened.id
            );
        }
        Response::TerminalOutput { id, bytes } => {
            eprintln!(
                "[ERROR] unexpected terminal output for session {id} ({} bytes)",
                bytes.len()
            );
        }
        Response::TerminalExited { id, status } => {
            eprintln!("[ERROR] unexpected terminal exit for session {id} (status {status})");
        }
    }
}
