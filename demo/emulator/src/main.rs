//! The emulator binary: one process, both fixture interfaces.
//!
//! The supervisor passes `--socket` and `--state`; `anth-service` owns the
//! socket, `/health` and `/__anth/call`, and this binary supplies the handlers
//! and the shared host snapshot.

use std::path::PathBuf;
use std::sync::Arc;

fn state_argument() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--state" {
            return args.next().map(PathBuf::from);
        }
        if argument == "--socket" {
            args.next();
        }
    }
    None
}

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let emulator = match anth_host_emulator::Emulator::open(state_argument()) {
        Ok(emulator) => emulator,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let handler = Arc::new(emulator);
    if let Err(error) = anth_service::serve(&socket, handler).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
