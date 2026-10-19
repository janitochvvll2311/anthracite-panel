use anth_proto::{Request, Response};

use crate::cli::{Command, ProcessCommand};
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
