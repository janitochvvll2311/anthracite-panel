//! Command-line interface: a thin client over the socket.
//!
//! `main` decides the exit code from [`ErrorKind`](anth_wire::ErrorKind), so a
//! feature does not map outcomes to codes itself — v0.1.0 had that mapping in
//! three places that could drift apart.
//!
//! `--interactive` runs the same dispatcher over stdin, so a typed command and
//! a subcommand cannot disagree about what a command does.

mod api;
mod dispatch;
mod plugins;
mod render;
mod socket;

use std::process::ExitCode;
use std::sync::Arc;

use anth_wire::{PluginPipe, Request, Response};
use clap::{Parser, Subcommand};

use dispatch::Daemon;

#[derive(Parser, Debug)]
#[command(name = "anth-panel", version, about = "Host inspection panel")]
struct Cli {
    /// Socket path. Falls back to $ANTH_SOCKET, then /var/tmp/anth.sock.
    #[arg(long, global = true)]
    socket: Option<String>,

    /// Read commands from stdin instead of running one and exiting.
    #[arg(long, short, global = true)]
    interactive: bool,

    /// The command. Optional because `--interactive` alone is valid.
    #[command(subcommand)]
    command: Option<Command>,
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
    /// Inspect and run the plugins the daemon loaded.
    Plugin {
        #[command(subcommand)]
        action: PluginAction,
    },
}

#[derive(Subcommand, Debug)]
enum PluginAction {
    /// List every plugin the daemon found, including the disabled ones.
    List,
    /// Run one plugin.
    Call {
        /// The plugin id, as `plugin list` prints it.
        id: String,

        /// The command line to hand the plugin, as its own CLI would read it.
        ///
        /// Defaults to empty, which asks for whatever the plugin does with
        /// nothing, and that is usually its default action.
        #[arg(default_value = "")]
        line: String,

        /// Stand in for the HTTP pipe instead: `line` is then a JSON body.
        #[arg(long, conflicts_with = "direct")]
        http: bool,

        /// Stand in for `direct` instead: `line` is `{"action":..,"value":..}`.
        #[arg(long)]
        direct: bool,
    },
}

impl PluginAction {
    /// The pipe this invocation stands in for.
    ///
    /// `cli` is the default because the argument is a command line: someone
    /// typing `plugin call org.anth.hello greet` is standing where the plugin's
    /// own CLI would stand. `--direct` is the escape hatch for the plugin's own
    /// vocabulary, and `--http` for an anchor body.
    fn pipe(&self) -> PluginPipe {
        match self {
            PluginAction::Call { http: true, .. } => PluginPipe::Http,
            PluginAction::Call { direct: true, .. } => PluginPipe::Direct,
            PluginAction::Call { .. } => PluginPipe::Cli,
            PluginAction::List => PluginPipe::Cli,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let socket = cli
        .socket
        .map(std::path::PathBuf::from)
        .unwrap_or_else(socket::default_socket_path);

    if cli.interactive {
        let Some(command) = cli.command else {
            return repl::run(&socket).await;
        };
        // A command *and* `--interactive` one-shot the command, then keep the
        // prompt: starting the daemon and then typing into it is the useful
        // combination, so refusing it would help nobody.
        let code = dispatch_command(&socket, Some(command)).await;
        if code != ExitCode::SUCCESS {
            eprintln!("anth-panel: the startup command failed, staying interactive");
        }
        return repl::run(&socket).await;
    }

    match cli.command {
        Some(command) => dispatch_command(&socket, Some(command)).await,
        None => {
            eprintln!("anth-panel: a command is required unless --interactive is given");
            ExitCode::from(2)
        }
    }
}

/// Runs one subcommand, which is also what the REPL runs per line.
async fn dispatch_command(socket: &std::path::Path, command: Option<Command>) -> ExitCode {
    let Some(command) = command else {
        return ExitCode::from(2);
    };

    match command {
        Command::Daemon => serve_with(socket, None).await,
        Command::Run { addr } => serve_with(socket, Some(addr)).await,
        Command::HostInfo => client(socket, Request::HostInfo).await,
        Command::Plugin { action } => match &action {
            PluginAction::List => client(socket, Request::PluginList).await,
            PluginAction::Call { id, line, .. } => {
                client(
                    socket,
                    Request::PluginCall {
                        id: id.clone(),
                        pipe: action.pipe(),
                        payload: line.clone(),
                    },
                )
                .await
            }
        },
    }
}

/// Serves the socket, and the HTTP API too when an address is given.
async fn serve_with(socket: &std::path::Path, addr: Option<String>) -> ExitCode {
    let daemon = Arc::new(Daemon::new(plugins::registry::Registry::load(
        &plugins::registry::default_root(),
    )));

    let result = match addr {
        Some(addr) => serve_all(socket, &addr, daemon).await,
        None => serve(socket, daemon).await,
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("anth-panel: {error}");
            ExitCode::from(error.kind.exit_code())
        }
    }
}

/// Serves the socket and the HTTP API together.
///
/// Both must be listening before either is awaited, so a port already in use
/// fails at startup instead of leaving a daemon that only speaks one transport.
/// When either loop ends the other is cancelled: a half-serving process is not
/// a state an operator can act on.
async fn serve_all(
    socket: &std::path::Path,
    addr: &str,
    daemon: Arc<Daemon>,
) -> Result<(), anth_wire::WireError> {
    let listener = socket::bind(socket)?;
    let http = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anth_wire::WireError::internal(format!("binding {addr}: {e}")))?;

    eprintln!(
        "anth-panel: listening on {} and http://{addr}",
        socket.display()
    );

    let socket_loop = socket::serve(listener, Arc::clone(&daemon));
    let http_loop = async {
        axum::serve(http, api::router(daemon))
            .await
            .map_err(|e| anth_wire::WireError::internal(format!("serving http: {e}")))
    };

    tokio::select! {
        result = socket_loop => result.map_err(|e| anth_wire::WireError::internal(format!("serving: {e}"))),
        result = http_loop => result,
    }
}

async fn serve(socket: &std::path::Path, daemon: Arc<Daemon>) -> Result<(), anth_wire::WireError> {
    let listener = socket::bind(socket)?;
    eprintln!("anth-panel: listening on {}", socket.display());
    socket::serve(listener, daemon)
        .await
        .map_err(|e| anth_wire::WireError::internal(format!("serving: {e}")))
}

/// Sends one request and renders the answer, or reports why it failed.
async fn client(socket: &std::path::Path, request: Request) -> ExitCode {
    match run_client(socket, request).await {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err((kind, message)) => {
            eprintln!("anth-panel: {message}");
            ExitCode::from(kind.exit_code())
        }
    }
}

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

    render_response(response)
}

fn render_response(response: Response) -> Result<String, (anth_wire::ErrorKind, String)> {
    match response {
        Response::HostInfo(info) => Ok(render::render_host_info(&info)),
        Response::Welcome { protocol_version } => Ok(format!("protocol {protocol_version}")),
        Response::PluginList { plugins } => Ok(render::render_plugin_list(&plugins)),
        Response::PluginReply { status, value, .. } => {
            let body = render::render_plugin_reply(&value);
            if (200..300).contains(&status) {
                Ok(body)
            } else {
                // A plugin's own refusal is a failure for a CLI caller, so it
                // exits non-zero while still printing the plugin's message.
                Err((
                    anth_wire::ErrorKind::Invalid,
                    if body.is_empty() {
                        format!("the plugin answered {status}")
                    } else {
                        body
                    },
                ))
            }
        }
        Response::Error(error) => Err((error.kind, error.message)),
    }
}

mod repl;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_parses_the_host_info_subcommand() {
        let cli = Cli::try_parse_from(["anth-panel", "host-info"]).expect("parsing");
        assert!(matches!(cli.command, Some(Command::HostInfo)));
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

    /// `--interactive` alone is valid, which is why `command` is optional.
    #[test]
    fn interactive_alone_needs_no_subcommand() {
        let cli = Cli::try_parse_from(["anth-panel", "-i"]).expect("parsing");
        assert!(cli.interactive);
        assert!(cli.command.is_none());
    }

    #[test]
    fn a_bare_invocation_parses_and_is_refused_by_main() {
        // Parsing must succeed so `main` can print a message instead of clap's
        // usage dump, which says nothing about `--interactive`.
        let cli = Cli::try_parse_from(["anth-panel"]).expect("parsing");
        assert!(cli.command.is_none());
        assert!(!cli.interactive);
    }

    /// A caller typing a line is standing where the plugin's CLI stands, so that
    /// is the pipe it gets without asking. Defaulting to `direct` made every
    /// `plugin call <id> greet` fail as "not a request".
    #[test]
    fn a_plugin_call_defaults_to_the_cli_pipe() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            r#"{"action":"greeting","value":null}"#,
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        assert_eq!(action.pipe(), PluginPipe::Cli);
    }

    #[test]
    fn the_direct_flag_selects_the_direct_pipe() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            r#"{"action":"greeting","value":null}"#,
            "--direct",
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        assert_eq!(action.pipe(), PluginPipe::Direct);
    }

    #[test]
    fn the_http_flag_selects_the_http_pipe() {
        let cli = Cli::try_parse_from([
            "anth-panel",
            "plugin",
            "call",
            "org.anth.hello",
            "{}",
            "--http",
        ])
        .expect("parsing");

        let Some(Command::Plugin { action }) = cli.command else {
            panic!("expected a plugin command");
        };
        assert_eq!(action.pipe(), PluginPipe::Http);
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

        let daemon = Arc::new(Daemon::new(plugins::registry::Registry::empty()));
        let error = serve(&path, daemon).await.expect_err("must refuse");
        assert_eq!(ExitCode::from(error.kind.exit_code()), ExitCode::from(2));
        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);

        std::fs::remove_file(&path).expect("removing the file");
    }
}
