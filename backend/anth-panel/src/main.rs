//! `anth-panel` binary.
//!
//! Phase 0 exposes the config seam so the file format and atomic edits are
//! exercised before the daemon and HTTP layers arrive.

use std::path::PathBuf;
use std::process::ExitCode;

use anth_panel::config::Config;
use anth_panel::log::Settings;
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "anth-panel", version, about = "Anthracite Panel")]
struct Cli {
    /// Path to the configuration file.
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Log level: error, warn, info, debug or trace.
    #[arg(long, global = true)]
    log_level: Option<String>,

    /// Log format: human, json or off.
    #[arg(long, global = true)]
    log_format: Option<String>,

    /// Log destination: stdout or stderr.
    #[arg(long, global = true)]
    log_out: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Read, validate or change the configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Print the configuration file.
    Get,
    /// Parse and validate the configuration without writing.
    Validate,
    /// Change one dotted key, preserving comments and key order.
    Set {
        /// Dotted key, e.g. files.write or auth.username.
        key: String,
        /// New value, parsed as a TOML literal.
        value: String,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("{message}");
            ExitCode::from(code)
        }
    }
}

fn run() -> Result<(), (u8, String)> {
    let cli = Cli::parse();
    let path = cli.config.unwrap_or_else(Config::default_config_path);

    let (settings, problems) = Settings::from_flags(
        cli.log_format.as_deref(),
        cli.log_level.as_deref(),
        cli.log_out.as_deref(),
    );
    for problem in problems {
        eprintln!("{problem}");
    }
    let _logger = anth_panel::Logger::with_out(settings);

    match cli.command {
        Command::Config { command } => match command {
            ConfigCommand::Get => {
                let text = std::fs::read_to_string(&path)
                    .map_err(|error| (1, format!("reading {}: {error}", path.display())))?;
                print!("{text}");
                Ok(())
            }
            ConfigCommand::Validate => {
                Config::load(&path).map_err(|error| (1, error))?;
                println!("{} is valid", path.display());
                Ok(())
            }
            ConfigCommand::Set { key, value } => {
                Config::set_key(&path, &key, &value).map_err(|error| (2, error))?;
                println!("{} updated", path.display());
                Ok(())
            }
        },
    }
}
