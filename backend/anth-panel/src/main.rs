use std::process::ExitCode;

use anth_panel::cli::{Cli, Command, EndpointArgs};
use anth_panel::config::{Config, DEFAULT_CONFIG_PATH};
use anth_panel::{AppState, handle_response, request_for, router};
use anth_proto::AnthClient;
use std::path::Path;
use tokio::net::{TcpListener, UnixStream};

const DEFAULT_ADDR: &str = "0.0.0.0:8080";

fn load_config(path: &str) -> Config {
    match Config::load(path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            Config::default()
        }
    }
}

fn resolve_addr(args: &EndpointArgs) -> String {
    args.addr
        .clone()
        .or_else(|| std::env::var("ANTH_PANEL_ADDR").ok())
        .unwrap_or_else(|| DEFAULT_ADDR.to_string())
}

async fn serve_api(config: Config, args: &EndpointArgs) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = TcpListener::bind(&address)
        .await
        .map_err(|e| format!("Failed to bind to address {address}: {e}"))?;

    println!("Server is listening on {address}");
    axum::serve(listener, router(AppState::new(config)))
        .await
        .map_err(|e| format!("Server error: {e}"))?;
    Ok(ExitCode::SUCCESS)
}

/// Keeps the daemon task alive for as long as the API runs: binding the HTTP
/// port first surfaces address-in-use errors before the socket is touched.
async fn serve_run(config: Config, args: &EndpointArgs) -> Result<ExitCode, String> {
    let address = resolve_addr(args);
    let listener = TcpListener::bind(&address)
        .await
        .map_err(|e| format!("Failed to bind to address {address}: {e}"))?;

    let daemon_config = config.clone();
    let daemon = tokio::spawn(async move {
        if let Err(e) = anth_panel::daemon::serve(&daemon_config).await {
            eprintln!("Daemon stopped: {e}");
        }
    });

    println!("Daemon and server are running (http://{address})");
    let result = axum::serve(listener, router(AppState::new(config)))
        .await
        .map_err(|e| format!("Server error: {e}"));

    daemon.abort();
    let _ = daemon.await;
    result.map(|_| ExitCode::SUCCESS)
}

async fn send(config: &Config, request: &anth_proto::Request) -> ExitCode {
    let socket_path = Path::new(&config.socket_path);
    let socket = match UnixStream::connect(socket_path).await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Failed to connect to socket: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut client = AnthClient::new(socket);
    if let Err(e) = client.send_request(request).await {
        eprintln!("Failed to send request: {e}");
        return ExitCode::FAILURE;
    }

    match client.receive_response().await {
        Some(Ok(response)) => {
            handle_response(&response);
            match response {
                anth_proto::Response::Error { .. } => ExitCode::FAILURE,
                _ => ExitCode::SUCCESS,
            }
        }
        Some(Err(e)) => {
            eprintln!("Failed to receive response: {e}");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("Stream closed while waiting for response");
            ExitCode::FAILURE
        }
    }
}

/// Runs a client-only subcommand against the daemon. Subcommands that serve
/// locally return `None` so `main` can handle them itself.
async fn send_command(config: &Config, command: &Command) -> Option<ExitCode> {
    let request = request_for(command)?;
    Some(send(config, &request).await)
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);

    if let Some(code) = send_command(&load_config(config_path), &cli.command).await {
        return code;
    }

    let result = match &cli.command {
        Command::Daemon => anth_panel::daemon::serve(&load_config(config_path))
            .await
            .map(|()| ExitCode::SUCCESS)
            .map_err(|e| e.to_string()),
        Command::Api(args) => serve_api(load_config(config_path), args).await,
        Command::Run(args) => serve_run(load_config(config_path), args).await,
        Command::Echo { .. } | Command::HostInfo => unreachable!("handled by send_command"),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[Error] {e}");
            ExitCode::FAILURE
        }
    }
}
