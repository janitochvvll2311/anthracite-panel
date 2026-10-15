use std::process::ExitCode;

use anth_cli::{cli::Cli, handle_response, request_for};
use anth_conf::{Config, DEFAULT_CONFIG_PATH};
use anth_proto::AnthClient;
use std::path::Path;
use tokio::net::UnixStream;

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let request = request_for(&cli.command);

    let config_path = cli.config_path.as_deref().unwrap_or(DEFAULT_CONFIG_PATH);
    let config = match Config::load(config_path) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Failed to load config: {e}");
            Config::default()
        }
    };

    let socket_path = Path::new(&config.socket_path);
    let socket = match UnixStream::connect(&socket_path).await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("Failed to connect to socket: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut client = AnthClient::new(socket);
    match client.send_request(&request).await {
        Ok(_) => {
            println!("Request sent successfully.");
            if let Some(result) = client.receive_response().await {
                match result {
                    Ok(response) => handle_response(&response),
                    Err(e) => {
                        eprintln!("Failed to receive response: {e}");
                        return ExitCode::FAILURE;
                    }
                }
            } else {
                eprintln!("Stream closed while waiting for response");
                return ExitCode::FAILURE;
            }
        }
        Err(e) => {
            eprintln!("Failed to send request: {e}");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
