pub mod cli;

use anth_proto::{Request, Response};

use crate::cli::Command;

pub fn request_for(command: &Command) -> Request {
    match command {
        Command::Echo { message } => Request::Echo {
            message: message.clone(),
        },
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
    }
}
