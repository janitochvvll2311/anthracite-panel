//! Entry point: argv, the data-plane listeners and the admin socket.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use proxy_service::config::Endpoints;
use proxy_service::service::Service;
use proxy_service::{plane, tls};

fn argument(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == name {
            return args.next();
        }
    }
    None
}

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = run(socket).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run(socket: PathBuf) -> Result<(), String> {
    let data = argument("--data")
        .map(PathBuf::from)
        .ok_or_else(|| "`--data` is required".to_string())?;
    // The manifest may substitute `{shared}` only once, so both published
    // directories hang off the same root here (certs does the same).
    let shared = argument("--shared")
        .map(PathBuf::from)
        .ok_or_else(|| "`--shared` is required".to_string())?;
    let broker = argument("--broker").map(PathBuf::from);
    let http = argument("--http").unwrap_or_else(|| ":8080".to_string());
    let https = argument("--https").unwrap_or_else(|| ":8443".to_string());

    tls::install_provider();
    let service = Service::new(
        data,
        shared.join("tls"),
        shared.join("acme/http-01"),
        broker,
        Endpoints { http, https },
    );
    service.watch_certificates(Duration::from_secs(10));
    plane::start(Arc::clone(&service)).await;
    anth_service::serve(&socket, service)
        .await
        .map_err(|error| format!("serving the admin socket: {error}"))
}
