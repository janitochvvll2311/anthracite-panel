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
    HostInfo,
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
