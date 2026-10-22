//! `anth-panel`: one binary serving the daemon, the CLI and the HTTP panel.
//!
//! `main` only parses the command line and hands the command to `cli_dispatch`.
//! Each concern lives in its own module there: the client talks to a daemon, the
//! server owns the listeners, `confirm` owns the prompts, and the settings and
//! plugin commands edit the config file locally.

mod cli_dispatch;
mod main_prelude;

use std::process::ExitCode;

use anth_panel::cli::Cli;
use anth_panel::config::DEFAULT_CONFIG_PATH;

use cli_dispatch::dispatch::{dispatch, run_interactive};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);

    if cli.interactive {
        return run_interactive(config_path, cli.command).await;
    }

    match &cli.command {
        Some(command) => dispatch(config_path, command, None).await,
        // A bare invocation has nothing to run; clap cannot require the
        // subcommand because `-i` alone is valid.
        None => {
            eprintln!("[ERROR] a command is required; run `anth-panel --help`");
            ExitCode::FAILURE
        }
    }
}
