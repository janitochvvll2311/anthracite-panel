//! Command-line interface: a thin client over the socket.
//!
//! `main` decides the exit code from [`ErrorKind`](anth_wire::ErrorKind), so a
//! feature does not map outcomes to codes itself — v0.1.0 had that mapping in
//! three places that could drift apart.
//!
//! `--interactive` runs the same dispatcher over stdin, so a typed command and
//! a subcommand cannot disagree about what a command does. With `run` or
//! `daemon` it also serves: the daemon comes up on a task of its own and the
//! prompt stays usable in front of it.

mod api;
mod dispatch;
mod plugins;
mod render;
mod socket;

use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use anth_wire::{PluginPipe, Request, Response};
use clap::{Parser, Subcommand};
use tokio::net::{TcpListener, UnixListener};

use dispatch::Daemon;

#[derive(Parser, Debug)]
#[command(name = "anth-panel", version, about = "Host inspection panel")]
struct Cli {
    /// Socket path. Falls back to $ANTH_SOCKET, then /var/tmp/anth.sock.
    #[arg(long, global = true)]
    socket: Option<String>,

    /// Read commands from stdin instead of running one and exiting.
    ///
    /// With `run` or `daemon` the server keeps running behind the prompt.
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
        return interactive(&socket, cli.command).await;
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

    let result = match bind(socket, addr.as_deref()).await {
        Ok(listeners) => serve_bound(listeners, daemon).await,
        Err(error) => Err(error),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("anth-panel: {error}");
            ExitCode::from(error.kind.exit_code())
        }
    }
}

/// The listeners a server command owns, bound but not yet serving.
///
/// Binding is separate from serving so a caller can fail before it does
/// anything else: an address already in use must not leave a prompt running
/// against a daemon that never started.
struct Listeners {
    socket: UnixListener,
    http: Option<TcpListener>,
}

async fn bind(socket: &Path, addr: Option<&str>) -> Result<Listeners, anth_wire::WireError> {
    let listener = socket::bind(socket)?;
    let http = match addr {
        Some(addr) => Some(
            TcpListener::bind(addr)
                .await
                .map_err(|e| anth_wire::WireError::internal(format!("binding {addr}: {e}")))?,
        ),
        None => None,
    };

    match addr {
        Some(addr) => eprintln!(
            "anth-panel: listening on {} and http://{addr}",
            socket.display()
        ),
        None => eprintln!("anth-panel: listening on {}", socket.display()),
    }

    Ok(Listeners {
        socket: listener,
        http,
    })
}

/// Serves the bound listeners until one of them fails.
///
/// Both must be listening before either is awaited, so a port already in use
/// fails at startup instead of leaving a daemon that only speaks one transport.
/// When either loop ends the other is cancelled: a half-serving process is not
/// a state an operator can act on.
async fn serve_bound(
    listeners: Listeners,
    daemon: Arc<Daemon>,
) -> Result<(), anth_wire::WireError> {
    let Some(http) = listeners.http else {
        return socket::serve(listeners.socket, daemon)
            .await
            .map_err(|e| anth_wire::WireError::internal(format!("serving: {e}")));
    };

    let socket_daemon = Arc::clone(&daemon);
    let socket_loop = async {
        socket::serve(listeners.socket, socket_daemon)
            .await
            .map_err(|e| anth_wire::WireError::internal(format!("serving: {e}")))
    };

    let http_loop = async {
        axum::serve(http, api::router(daemon))
            .await
            .map_err(|e| anth_wire::WireError::internal(format!("serving http: {e}")))
    };

    tokio::select! {
        result = socket_loop => result,
        result = http_loop => result,
    }
}

/// What an interactive session does before the prompt appears.
///
/// A pure decision, so a test asserts that `run -i` serves and keeps the prompt
/// without binding a port or reading stdin.
#[derive(Debug)]
enum Startup {
    /// Serve these listeners while the prompt runs.
    Serve { addr: Option<String> },
    /// Run this command first, then keep the prompt.
    RunFirst(Box<Command>),
    /// Nothing to start: just the prompt.
    Prompt,
}

fn startup(command: Option<Command>) -> Startup {
    match command {
        Some(Command::Daemon) => Startup::Serve { addr: None },
        Some(Command::Run { addr }) => Startup::Serve { addr: Some(addr) },
        Some(command) => Startup::RunFirst(Box::new(command)),
        None => Startup::Prompt,
    }
}

/// Runs the prompt, with a server behind it when the command starts one.
///
/// `daemon` and `run` never return, so dispatching them and then starting the
/// prompt — what this used to do — left the prompt unreachable and the flag
/// silently ignored. They are bound here instead, served on a task of their own,
/// and stopped when the prompt ends, which is what `run -i` promises: a daemon
/// you can type at.
async fn interactive(socket: &Path, command: Option<Command>) -> ExitCode {
    let addr = match startup(command) {
        Startup::Prompt => return repl::run(socket).await,
        Startup::RunFirst(command) => {
            // A client command still runs first and still keeps the prompt: a
            // failed one is reported, not fatal, because the prompt is where the
            // user fixes it.
            let code = dispatch_command(socket, Some(*command)).await;
            if code != ExitCode::SUCCESS {
                eprintln!("anth-panel: the startup command failed, staying interactive");
            }
            return repl::run(socket).await;
        }
        Startup::Serve { addr } => addr,
    };

    let daemon = Arc::new(Daemon::new(plugins::registry::Registry::load(
        &plugins::registry::default_root(),
    )));

    let listeners = match bind(socket, addr.as_deref()).await {
        Ok(listeners) => listeners,
        Err(error) => {
            eprintln!("anth-panel: {error}");
            return ExitCode::from(error.kind.exit_code());
        }
    };

    let serving = tokio::spawn(serve_bound(listeners, daemon));
    let code = repl::run(socket).await;
    // The prompt owns the process; a listener outliving it would keep the
    // terminal held by a daemon the operator thought they had quit.
    serving.abort();
    code
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

        let error = bind(&path, None).await.err().expect("must refuse");
        assert_eq!(ExitCode::from(error.kind.exit_code()), ExitCode::from(2));
        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);

        std::fs::remove_file(&path).expect("removing the file");
    }

    /// `run -i` and `daemon -i` must serve *and* keep the prompt. Dispatching
    /// them instead started a loop that never returns, so the prompt was
    /// unreachable and `-i` was silently ignored.
    #[test]
    fn a_server_command_under_interactive_serves_behind_the_prompt() {
        let run = startup(Some(Command::Run {
            addr: "127.0.0.1:0".into(),
        }));
        let Startup::Serve { addr } = run else {
            panic!("`run -i` must serve, not run the daemon and block");
        };
        assert_eq!(addr.as_deref(), Some("127.0.0.1:0"));

        let daemon = startup(Some(Command::Daemon));
        let Startup::Serve { addr } = daemon else {
            panic!("`daemon -i` must serve, not block");
        };
        assert!(addr.is_none());
    }

    /// A client command still runs first, so `-i host-info` prints a snapshot
    /// and then keeps the prompt.
    #[test]
    fn a_client_command_under_interactive_runs_first_and_keeps_the_prompt() {
        let Startup::RunFirst(command) = startup(Some(Command::HostInfo)) else {
            panic!("a client command must run before the prompt");
        };
        assert!(matches!(*command, Command::HostInfo));
    }

    #[test]
    fn interactive_alone_starts_nothing_but_the_prompt() {
        assert!(matches!(startup(None), Startup::Prompt));
    }
}
