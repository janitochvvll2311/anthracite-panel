use anth_proto::{Request, Response};
use futures::{SinkExt, StreamExt};
use std::error::Error;
use std::path::Path;
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Framed, LinesCodec};

pub fn handle_request(request: Request) -> Response {
    match request {
        Request::Echo { message } => Response::Ok { message }
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
    let mut framed = Framed::new(socket, LinesCodec::new());

    while let Some(result) = framed.next().await {
        let response = match result {
            Ok(line) => handle_line(&line),
            Err(e) => {
                eprintln!("Error reading from socket: {e}");
                return Err(Box::new(e));
            }
        };

        let response_json = match response.to_json() {
            Ok(json) => json,
            Err(e) => {
                eprintln!("Error serializing response: {e}");
                return Err(Box::new(e));
            }
        };

        if let Err(e) = framed.send(response_json).await {
            eprintln!("Error writing to socket: {e}");
            return Err(Box::new(e));
        }
    }

    Ok(())
}

pub fn clear_socket_path(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub async fn serve() -> Result<(), Box<dyn Error + Send + Sync>> {
    let socket_path = Path::new("/tmp/anth.sock");
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
