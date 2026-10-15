pub mod cli;

use anth_conf::Config;
use anth_proto::{AnthServer, Request, Response};
use std::error::Error;
use std::path::Path;
use tokio::net::{UnixListener, UnixStream};

pub fn handle_request(request: Request) -> Response {
    match request {
        Request::Echo { message } => Response::Ok { message },
    }
}

pub fn handle_line(line: &str) -> Response {
    match Request::from_json(line) {
        Ok(request) => handle_request(request),
        Err(e) => Response::Error {
            detail: format!("Error parsing request: {e}"),
        },
    }
}

pub async fn serve_connection(socket: UnixStream) -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut server = AnthServer::new(socket);

    while let Some(result) = server.receive_request().await {
        match result {
            Ok(request) => {
                let response = handle_request(request);
                if let Err(e) = server.send_response(&response).await {
                    eprintln!("Error writing to socket: {e}");
                    return Err(e);
                }
            }
            Err(e) => {
                eprintln!("Error receiving request: {e}");
                let response = Response::Error {
                    detail: format!("Error receiving request: {e}"),
                };
                if let Err(e) = server.send_response(&response).await {
                    eprintln!("Error writing error response to socket: {e}");
                }
                return Err(e);
            }
        }
    }

    Ok(())
}

pub fn clear_socket_path(path: impl AsRef<Path>) -> std::io::Result<()> {
    match std::fs::remove_file(path.as_ref()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub async fn serve(config: &Config) -> Result<(), Box<dyn Error + Send + Sync>> {
    let socket_path = Path::new(&config.socket_path);
    clear_socket_path(&socket_path)?;

    let listener = UnixListener::bind(&socket_path)?;
    println!("Listening on {}", socket_path.display());

    loop {
        let (socket, _) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = serve_connection(socket).await {
                eprintln!("Error handling connection: {e}");
            }
        });
    }
}
