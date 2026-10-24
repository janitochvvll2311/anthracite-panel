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
mod table;
mod www;

#[cfg(test)]
mod tests;

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

    /// The plugin directory. Falls back to $ANTH_PLUGINS, then
    /// /var/lib/anth/plugins.
    ///
    /// Read by the daemon and by every command that edits the registry, so
    /// `install`, `remove`, `enable` and `disable` act on the same directory
    /// instead of requiring an environment variable that is easy to forget.
    #[arg(long, global = true)]
    plugins: Option<String>,

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
        ///
        /// Everything after `--` belongs to the plugin, hyphens included, so
        /// `plugin call hello -- greet --name Ana` passes `--name` through.
        /// Without the separator clap reads a hyphenated word as the panel's
        /// own flag, which is why the panel's `--http` and `--direct` keep
        /// working after the payload. The words are rejoined into one line,
        /// quoting any word that needs it, so `--name "Ana Sol"` stays a single
        /// word for the plugin.
        #[arg(default_value = "")]
        line: Vec<String>,

        /// Stand in for the HTTP pipe instead: `line` is then a JSON body.
        #[arg(long, conflicts_with = "direct")]
        http: bool,

        /// Stand in for `direct` instead: `line` is `{"action":..,"value":..}`.
        #[arg(long)]
        direct: bool,
    },
    /// Install a bundle produced by `package.sh`.
    ///
    /// Local, not a daemon request: see `plugins::install` for why the socket is
    /// not allowed to add code. The daemon reads its registry at startup, so a
    /// running one does not see the change until it restarts.
    Install {
        /// Path to the `.zip` `package.sh` wrote.
        bundle: String,

        /// Replace a bundle already installed under the same id.
        ///
        /// Off by default so installing the same file twice is a refusal rather
        /// than a silent overwrite of what the operator approved.
        #[arg(long)]
        force: bool,
    },
    /// Remove an installed bundle.
    ///
    /// Local, like `install`. The directory must hold a `plugin.toml`, so a
    /// registry root that also holds something else is not pruned by accident.
    Remove {
        /// The plugin's id, as `plugin list` shows it.
        id: String,
    },
    /// Switch an installed plugin off without removing it.
    ///
    /// Local, like `install`: it records a decision in the registry's
    /// `state.toml`, which the daemon reads at startup. The bundle stays on disk
    /// and `enable` switches it back on.
    Disable {
        /// The plugin's id, as `plugin list` shows it.
        id: String,
    },
    /// Switch a disabled plugin back on.
    ///
    /// Does not make a broken bundle work: a plugin whose digest does not match
    /// its manifest stays disabled and says why.
    Enable {
        /// The plugin's id, as `plugin list` shows it.
        id: String,
    },
}

impl PluginAction {
    /// The pipe this invocation stands in for.
    ///
    /// `cli` is the default because the argument is a command line: someone
    /// typing `plugin call org.anth.hello greet` is standing where the plugin's
    /// own CLI would stand. `--direct` is the escape hatch for the plugin's own
    /// vocabulary, and `--http` for an anchor body.
    fn pipe(&self) -> Option<PluginPipe> {
        match self {
            PluginAction::Call { http: true, .. } => Some(PluginPipe::Http),
            PluginAction::Call { direct: true, .. } => Some(PluginPipe::Direct),
            PluginAction::Call { .. } => Some(PluginPipe::Cli),
            // An install never reaches the daemon. `None` rather than a default
            // keeps that visible in the type instead of pretending a filesystem
            // action has a transport.
            PluginAction::Install { .. }
            | PluginAction::Remove { .. }
            | PluginAction::Disable { .. }
            | PluginAction::Enable { .. }
            | PluginAction::List => None,
        }
    }

    /// Whether the install path may replace an existing bundle.
    fn force(&self) -> bool {
        matches!(self, PluginAction::Install { force: true, .. })
    }
}

/// The payload for a `plugin call`, from the words clap left after `id`.
///
/// One word is already the line, so it is passed through untouched: someone who
/// typed `plugin call hello "greet --name 'Ana Sol'"` has done the quoting and
/// re-quoting it would hand the plugin one giant word. Several words are an
/// argv that has to be serialized back into a line, so each word is quoted when
/// leaving it bare would change how it splits.
///
/// The HTTP and direct pipes take a JSON body the host never parses, so there
/// the words are always joined with a single space and nothing is quoted.
fn payload(words: &[String], pipe: PluginPipe) -> String {
    match pipe {
        PluginPipe::Cli if words.len() > 1 => words
            .iter()
            .map(|word| quote(word))
            .collect::<Vec<_>>()
            .join(" "),
        PluginPipe::Cli | PluginPipe::Http | PluginPipe::Direct => words.join(" "),
    }
}

/// One word, quoted only when leaving it bare would change how it splits.
fn quote(word: &str) -> String {
    let bare = !word.is_empty()
        && word
            .chars()
            .all(|c| !c.is_whitespace() && !matches!(c, '\'' | '"' | '\\'));
    if bare {
        return word.to_string();
    }
    // Single quotes make everything but a single quote literal, so a word that
    // contains one gets the shell's own answer: close, escape, reopen.
    format!("'{}'", word.replace('\'', r"'\''"))
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let socket = cli
        .socket
        .map(std::path::PathBuf::from)
        .unwrap_or_else(socket::default_socket_path);

    // Resolved once, here, so the daemon and every editing command agree on the
    // directory without each re-reading the environment.
    let root = cli
        .plugins
        .map(std::path::PathBuf::from)
        .unwrap_or_else(plugins::registry::default_root);

    if cli.interactive {
        return interactive(&socket, &root, cli.command).await;
    }

    match cli.command {
        Some(command) => dispatch_command(&socket, &root, Some(command)).await,
        None => {
            eprintln!("anth-panel: a command is required unless --interactive is given");
            ExitCode::from(2)
        }
    }
}

/// Runs one subcommand, which is also what the REPL runs per line.
async fn dispatch_command(
    socket: &std::path::Path,
    root: &std::path::Path,
    command: Option<Command>,
) -> ExitCode {
    let Some(command) = command else {
        return ExitCode::from(2);
    };

    match command {
        Command::Daemon => serve_with(socket, root, None).await,
        Command::Run { addr } => serve_with(socket, root, Some(addr)).await,
        Command::HostInfo => client(socket, Request::HostInfo).await,
        Command::Plugin { action } => match &action {
            PluginAction::List => client(socket, Request::PluginList).await,
            PluginAction::Install { bundle, .. } => {
                install(root, std::path::Path::new(bundle), action.force())
            }
            PluginAction::Remove { id } => remove(root, id),
            PluginAction::Disable { id } => switch(root, id, false),
            PluginAction::Enable { id } => switch(root, id, true),
            PluginAction::Call { id, line, .. } => {
                // `pipe` is `None` only for the arms matched above, so a call
                // always has one; the fallback keeps that out of a panic.
                let Some(pipe) = action.pipe() else {
                    eprintln!("anth-panel: internal: a plugin call without a transport");
                    return ExitCode::from(2);
                };
                client(
                    socket,
                    Request::PluginCall {
                        id: id.clone(),
                        pipe,
                        payload: payload(line, pipe),
                    },
                )
                .await
            }
        },
    }
}

/// Installs a bundle and says where it went.
///
/// Local, so it ignores `--socket`: there is no daemon to ask. The message names
/// the restart because a running daemon read its registry at startup and will
/// not show the new plugin until it is started again.
fn install(root: &std::path::Path, bundle: &std::path::Path, force: bool) -> ExitCode {
    match plugins::install::install(bundle, root, force) {
        Ok(installed) => {
            println!(
                "installed {} {} into {}",
                installed.id,
                installed.version,
                installed.dir.display()
            );
            println!(
                "  {} anchor(s), {} ui slot(s)",
                installed.anchors, installed.slots
            );
            println!("restart the daemon to load it");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("anth-panel: {}", error.message);
            ExitCode::from(error.kind.exit_code())
        }
    }
}

/// Removes an installed bundle.
///
/// Local, so it ignores `--socket`. The path goes to stdout and everything else
/// to stderr, so a script can capture what was removed without parsing prose.
fn remove(root: &std::path::Path, id: &str) -> ExitCode {
    match plugins::install::remove(root, id) {
        Ok(dir) => {
            println!("{}", dir.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("anth-panel: {}", error.message);
            ExitCode::from(error.kind.exit_code())
        }
    }
}

/// Switches one plugin off or on, and says what the next start will do.
///
/// The message is explicit about the restart because this writes a file the
/// daemon read at startup: a running one keeps serving whatever it loaded.
fn switch(root: &std::path::Path, id: &str, enabled: bool) -> ExitCode {
    match plugins::registry::set_enabled(root, id, enabled) {
        Ok(true) => {
            println!(
                "{} {id} in {}; restart the daemon to apply",
                if enabled { "enabled" } else { "disabled" },
                root.display()
            );
            ExitCode::SUCCESS
        }
        // Already in that state is success: the operator asked for an outcome,
        // not for a write, and a script that re-runs this should not fail.
        Ok(false) => {
            println!(
                "{id} was already {}",
                if enabled { "enabled" } else { "disabled" }
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("anth-panel: {}", error.message);
            ExitCode::from(error.kind.exit_code())
        }
    }
}

/// Serves the socket, and the HTTP API too when an address is given.
async fn serve_with(
    socket: &std::path::Path,
    root: &std::path::Path,
    addr: Option<String>,
) -> ExitCode {
    let daemon = Arc::new(Daemon::new(plugins::registry::Registry::load(root)));

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
async fn interactive(socket: &Path, root: &Path, command: Option<Command>) -> ExitCode {
    let addr = match startup(command) {
        Startup::Prompt => return repl::run(socket, root).await,
        Startup::RunFirst(command) => {
            // A client command still runs first and still keeps the prompt: a
            // failed one is reported, not fatal, because the prompt is where the
            // user fixes it.
            let code = dispatch_command(socket, root, Some(*command)).await;
            if code != ExitCode::SUCCESS {
                eprintln!("anth-panel: the startup command failed, staying interactive");
            }
            return repl::run(socket, root).await;
        }
        Startup::Serve { addr } => addr,
    };

    let daemon = Arc::new(Daemon::new(plugins::registry::Registry::load(root)));

    let listeners = match bind(socket, addr.as_deref()).await {
        Ok(listeners) => listeners,
        Err(error) => {
            eprintln!("anth-panel: {error}");
            return ExitCode::from(error.kind.exit_code());
        }
    };

    let serving = tokio::spawn(serve_bound(listeners, daemon));
    let code = repl::run(socket, root).await;
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
