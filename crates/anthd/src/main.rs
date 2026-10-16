use anth_conf::{Config, DEFAULT_CONFIG_PATH};
use anthd::cli::Cli;
use anthd::serve;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);
    let config = match Config::load(config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            println!("Using default config.");
            let new_config = Config::default();
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
