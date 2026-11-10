//! Shared boot for the Home service binaries.
//!
//! `home-service` answers with real host facts; `home-demo` answers with the
//! deterministic fixture the demo instance installs. The wire contract and the
//! frontend are identical, so only the packaged binary changes.

use std::sync::Arc;

use anth_service::Handler;

/// Serves `handler` on the `--socket` the supervisor passed.
pub fn serve(handler: Arc<dyn Handler>) {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = runtime.block_on(anth_service::serve(&socket, handler)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
