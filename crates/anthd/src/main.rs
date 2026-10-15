use anth_conf::{Config, DEFAULT_CONFIG_PATH};
use anthd::serve;
use clap::Parser;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "anthd")]
#[command(version = "1.0", about = "Anthracite Panel Daemon", long_about = None)]
struct Cli {
    #[arg(
        name = "config",
        short,
        long,
        value_name = "FILE",
        help = "Path to the configuration file"
    )]
    pub config_path: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);
    let config = match Config::load(config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            let new_config = Config::default();
            println!("Using default config.");
            if let Err(e) = new_config.save(config_path) {
                eprintln!("Failed to save default config: {e}");
            }
            new_config
        }
    };

    match serve(&config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[Error] {e}");
            ExitCode::FAILURE
        }
    }
}
