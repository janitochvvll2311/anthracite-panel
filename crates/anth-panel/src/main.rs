pub mod app;

use std::process::ExitCode;

use anth_conf::{Config, DEFAULT_CONFIG_PATH};
use anth_panel::{app::AppState, cli::Cli, router};
use axum::serve;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);
    let config = match Config::load(config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            Config::default()
        }
    };

    let state = AppState::new(config);
    let address = std::env::var("ANTH_PANEL_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    match TcpListener::bind(&address).await {
        Ok(listener) => {
            println!("Server is listening on {address}");
            if let Err(e) = serve(listener, router(state)).await {
                eprintln!("Server error: {e}");
            };
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("Failed to bind to address: {e}");
            return ExitCode::FAILURE;
        }
    };
}
