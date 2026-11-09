//! The emulator binary: one process, both fixture interfaces.
//!
//! The supervisor passes `--socket`; `anth-service` owns the socket, `/health`
//! and `/__anth/call`, and this binary only supplies the handlers.

use std::sync::Arc;

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let handler = Arc::new(anth_host_emulator::Emulator::new());
    if let Err(error) = anth_service::serve(&socket, handler).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
