use anth_proto::{Request, Response};

use crate::cli::Command;
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
        Command::Daemon | Command::Api(_) | Command::Run(_) => None,
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
    }
}
