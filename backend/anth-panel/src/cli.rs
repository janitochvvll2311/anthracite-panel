use anth_proto::ProcessSignal;
use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "anth-panel")]
#[command(version = "0.1.0", about = "Anthracite Panel", long_about = None)]
pub struct Cli {
    #[arg(
        name = "config",
        short,
        long,
        global = true,
        value_name = "FILE",
        help = "Path to the configuration file"
    )]
    pub config_path: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn parse() -> Self {
        Parser::parse()
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Serve the Unix-socket daemon.
    Daemon,
    /// Serve the HTTP API and the embedded web UI.
    Api(EndpointArgs),
    /// Serve the daemon and the HTTP API from a single process.
    Run(EndpointArgs),
    /// Send a request to the running daemon and print the response.
    Echo {
        #[arg(help = "Message to echo")]
        message: String,
    },
    /// Print the operating system and resources of the daemon host as a table.
    HostInfo(WatchArgs),
    /// Inspect processes on the daemon host and optionally signal one.
    Process(ProcessArgs),
    /// Open an interactive terminal on the daemon host.
    Terminal(TerminalArgs),
}

#[derive(Args, Debug)]
pub struct ProcessArgs {
    #[command(subcommand)]
    pub command: ProcessCommand,
}

/// `process` subcommands. `list` keeps the short `ls` alias.
#[derive(Subcommand, Debug)]
pub enum ProcessCommand {
    /// List processes as a table.
    #[command(alias = "ls")]
    List(WatchArgs),
    /// Send an allowlisted signal to one process.
    Signal(ProcessSignalArgs),
}

#[derive(Args, Debug)]
pub struct ProcessSignalArgs {
    #[arg(value_name = "PID", help = "Process id to signal")]
    pub pid: u32,

    #[arg(
        value_name = "SIGNAL",
        value_parser = parse_signal,
        help = "Signal name: term, kill, hup, int, quit, stop, cont"
    )]
    pub signal: ProcessSignal,

    #[arg(long, help = "Do not ask for confirmation")]
    pub yes: bool,
}

fn parse_signal(value: &str) -> Result<ProcessSignal, String> {
    value.parse()
}

/// `--watch` takes an optional interval: bare, it repeats every 2s.
#[derive(Args, Debug, Default)]
pub struct WatchArgs {
    #[arg(
        long,
        value_name = "INTERVAL",
        num_args = 0..=1,
        default_missing_value = "2s",
        help = "Keep reading every INTERVAL (500ms, 2s, 1m) instead of printing once"
    )]
    pub watch: Option<String>,
}

/// Initial size for `terminal`: the session starts at the local terminal's
/// size, or at the explicit values when either flag is given.
#[derive(Args, Debug, Default)]
pub struct TerminalArgs {
    #[arg(
        long,
        value_name = "COLS",
        help = "Initial terminal width in columns; defaults to the local terminal width"
    )]
    pub cols: Option<u16>,

    #[arg(
        long,
        value_name = "ROWS",
        help = "Initial terminal height in rows; defaults to the local terminal height"
    )]
    pub rows: Option<u16>,
}

#[derive(Args, Debug, Default)]
pub struct EndpointArgs {
    #[arg(
        long,
        value_name = "ADDR",
        help = "Address the HTTP API binds to; defaults to ANTH_PANEL_ADDR or 0.0.0.0:8080"
    )]
    pub addr: Option<String>,
}
