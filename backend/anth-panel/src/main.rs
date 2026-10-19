use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use anth_panel::cli::{Cli, Command, EndpointArgs, WatchArgs};
use anth_panel::config::{Config, DEFAULT_CONFIG_PATH};
use anth_panel::stream;
use anth_panel::{AppState, request_for, router};
use anth_proto::{Request, Response};
use tokio::net::TcpListener;

const DEFAULT_ADDR: &str = "0.0.0.0:8080";

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

/// How a response becomes one printed frame.
fn render_frame(response: &Response) -> String {
    match response {
        Response::HostInfo(info) => anth_panel::render_host_info(info),
        Response::Ok { message } => format!("[OK] {message}"),
        Response::Error { detail } => format!("[ERROR] {detail}"),
    }
}

/// How often a subcommand repeats, or `None` to read once. A subcommand opts
/// into streaming by answering `Some` here; nothing else needs to change.
fn watch_interval(command: &Command) -> Result<Option<Duration>, String> {
    match command {
        Command::HostInfo(WatchArgs { watch }) => match watch.as_deref() {
            None => Ok(None),
            Some(raw) => stream::parse_interval(raw).map(Some),
        },
        _ => Ok(None),
    }
}

/// Runs a client-only subcommand against the daemon. Subcommands that serve
/// locally return `None` so `main` can handle them itself.
async fn send_command(config_path: &str, command: &Command) -> Option<ExitCode> {
    let request: Request = request_for(command)?;
    let interval = match watch_interval(command) {
        Ok(interval) => interval,
        Err(e) => {
            eprintln!("[ERROR] {e}");
            return Some(ExitCode::FAILURE);
        }
    };

    // Client-only subcommands never bootstrap the config file.
    let config = Config::load_or_default(config_path);
    Some(
        stream::follow(
            Path::new(&config.socket_path),
            &request,
            interval,
            render_frame,
        )
        .await,
    )
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);

    if let Some(code) = send_command(config_path, &cli.command).await {
        return code;
    }

    let result = match &cli.command {
        Command::Daemon => anth_panel::daemon::serve(&Config::load_or_create(config_path))
            .await
            .map(|()| ExitCode::SUCCESS)
            .map_err(|e| e.to_string()),
        Command::Api(args) => serve_api(Config::load_or_create(config_path), args).await,
        Command::Run(args) => serve_run(Config::load_or_create(config_path), args).await,
        Command::Echo { .. } | Command::HostInfo(_) => unreachable!("handled by send_command"),
    };

    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[Error] {e}");
            ExitCode::FAILURE
        }
    }
}
