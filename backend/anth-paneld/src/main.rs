//! Command-line interface: a thin client over the socket.
//!
//! `main` decides the exit code from [`ErrorKind`](anth_wire::ErrorKind), so a
//! feature does not map outcomes to codes itself — v0.1.0 had that mapping in
//! three places that could drift apart.

mod api;
mod dispatch;
mod render;
mod socket;

use std::process::ExitCode;

use anth_wire::{Request, Response};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "anth-panel", version, about = "Host inspection panel")]
struct Cli {
    /// Socket path. Falls back to $ANTH_SOCKET, then /var/tmp/anth.sock.
    #[arg(long, global = true)]
    socket: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run the daemon and the HTTP API.
    Run {
        /// HTTP bind address.
        #[arg(long, default_value = "0.0.0.0:8080")]
        addr: String,
    },
    /// Serve the Unix socket only.
    Daemon,
    /// Print a snapshot of the host the daemon runs on.
    HostInfo,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let socket = cli
        .socket
        .map(std::path::PathBuf::from)
        .unwrap_or_else(socket::default_socket_path);

    match cli.command {
        Command::Daemon => match serve(&socket).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("anth-panel: {error}");
                ExitCode::from(error.kind.exit_code())
            }
        },
        Command::Run { addr } => match serve_all(&socket, &addr).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("anth-panel: {error}");
                ExitCode::from(error.kind.exit_code())
            }
        },
        Command::HostInfo => match run_client(&socket, Request::HostInfo).await {
            Ok(output) => {
                println!("{output}");
                ExitCode::SUCCESS
            }
            Err((kind, message)) => {
                eprintln!("anth-panel: {message}");
                ExitCode::from(kind.exit_code())
            }
        },
    }
}

/// Serves the socket and the HTTP API together.
///
/// Both must be listening before either is awaited, so a port already in use
/// fails at startup instead of leaving a daemon that only speaks one transport.
/// When either loop ends the other is cancelled: a half-serving process is not
/// a state an operator can act on.
async fn serve_all(socket: &std::path::Path, addr: &str) -> Result<(), anth_wire::WireError> {
    let listener = socket::bind(socket)?;
    let http = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anth_wire::WireError::internal(format!("binding {addr}: {e}")))?;

    eprintln!(
        "anth-panel: listening on {} and http://{addr}",
        socket.display()
    );

    let socket_loop = socket::serve(listener);
    let http_loop = async {
        axum::serve(http, api::router())
            .await
            .map_err(|e| anth_wire::WireError::internal(format!("serving http: {e}")))
    };

    tokio::select! {
        result = socket_loop => result.map_err(|e| anth_wire::WireError::internal(format!("serving: {e}"))),
        result = http_loop => result,
    }
}

async fn serve(socket: &std::path::Path) -> Result<(), anth_wire::WireError> {
    let listener = socket::bind(socket)?;
    eprintln!("anth-panel: listening on {}", socket.display());
    socket::serve(listener)
        .await
        .map_err(|e| anth_wire::WireError::internal(format!("serving: {e}")))
}

/// Sends one request and renders the answer, or reports why it failed.
async fn run_client(
    socket: &std::path::Path,
    request: Request,
) -> Result<String, (anth_wire::ErrorKind, String)> {
    let mut client = socket::Client::connect(socket)
        .await
        .map_err(|e| (e.kind, e.message))?;

    let response = client
        .call(&request)
        .await
        .map_err(|e| (e.kind, e.message))?;

    match response {
        Response::HostInfo(info) => Ok(render::render_host_info(&info)),
        Response::Welcome { protocol_version } => Ok(format!("protocol {protocol_version}")),
        Response::Error(error) => Err((error.kind, error.message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_parses_the_host_info_subcommand() {
        let cli = Cli::try_parse_from(["anth-panel", "host-info"]).expect("parsing");
        assert!(matches!(cli.command, Command::HostInfo));
        assert!(cli.socket.is_none());
    }

    #[test]
    fn the_socket_flag_is_global_so_it_works_after_the_subcommand() {
        let cli = Cli::try_parse_from(["anth-panel", "host-info", "--socket", "/tmp/x.sock"])
            .expect("parsing");
        assert_eq!(cli.socket.as_deref(), Some("/tmp/x.sock"));
    }

    #[test]
    fn an_unknown_subcommand_is_refused() {
        assert!(Cli::try_parse_from(["anth-panel", "nonsense"]).is_err());
    }

    #[tokio::test]
    async fn binding_a_regular_file_exits_two_because_the_caller_asked_wrong() {
        // Pins the wiring from ErrorKind to the process exit code. The daemon
        // path must derive its code from the kind instead of a literal `1`.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("anth-exit-{}-{nanos}.file", std::process::id()));
        std::fs::write(&path, b"not a socket").expect("writing the file");

        let error = serve(&path).await.expect_err("must refuse");
        assert_eq!(ExitCode::from(error.kind.exit_code()), ExitCode::from(2));
        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);

        std::fs::remove_file(&path).expect("removing the file");
    }
}
