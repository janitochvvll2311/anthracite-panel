//! The service side of the private WIT-RPC protocol (§19 §4).
//!
//! A native service is a WIT provider, not a second API: it answers
//! `interface#function` over `POST /__anth/call` and reports readiness over
//! `GET /health`. This crate owns the socket, the HTTP framing and the contract
//! error envelope so a service only writes its handlers.
//!
//! The manifest declares the app's WIT, and the handler matches the same
//! `interface` and `function` strings. A typed, generated trait is a later step;
//! this adapter is deliberately small so the emulator can exist before the
//! codegen does.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anth_contract::{Error, ErrorEnvelope, ErrorKind};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// The per-call cap, matching the engine.
pub const CALL_CAP: usize = 4 * 1024 * 1024;
/// Header blocks larger than this are refused before the body is read.
const HEADER_CAP: usize = 64 * 1024;

/// One function of one exported interface.
pub trait Handler: Send + Sync + 'static {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error>;
}

/// Binds `socket` and serves until the process is killed.
pub async fn serve(socket: &Path, handler: Arc<dyn Handler>) -> Result<(), Error> {
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)
        .map_err(|error| Error::unavailable(format!("binding {}: {error}", socket.display())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600));
    }
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(error) => {
                return Err(Error::unavailable(format!(
                    "accepting on {}: {error}",
                    socket.display()
                )));
            }
        };
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            let _ = answer(stream, handler).await;
        });
    }
}

/// A convenient path for a service launched with `--socket`.
pub fn socket_argument() -> Result<PathBuf, Error> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == "--socket" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| Error::invalid("`--socket` needs a path"));
        }
    }
    Err(Error::invalid("`--socket` is required"))
}

async fn answer(mut stream: UnixStream, handler: Arc<dyn Handler>) -> Result<(), Error> {
    match request(&mut stream).await {
        Ok((method, path, body)) => {
            let (status, payload) = route(&method, &path, &body, handler.as_ref());
            respond(&mut stream, status, &payload).await
        }
        Err(error) => {
            let payload = envelope(&error);
            respond(&mut stream, error.status(), &payload).await
        }
    }
}

fn route(method: &str, path: &str, body: &[u8], handler: &dyn Handler) -> (u16, Vec<u8>) {
    if method == "GET" && path == "/health" {
        return (200, json(&serde_json::json!({"ok": true})));
    }
    if path != "/__anth/call" {
        return (
            404,
            envelope(&Error::not_found(format!("`{path}` is not an endpoint"))),
        );
    }
    if method != "POST" {
        return (
            405,
            envelope(&Error::method_not_allowed("calls are `POST /__anth/call`")),
        );
    }
    let call: Call = match serde_json::from_slice(body) {
        Ok(call) => call,
        Err(error) => {
            return (
                400,
                envelope(&Error::invalid(format!(
                    "the call body is not valid: {error}"
                ))),
            );
        }
    };
    if call.interface.is_empty() || call.function.is_empty() {
        return (
            400,
            envelope(&Error::invalid("a call needs `interface` and `function`")),
        );
    }
    match handler.call(&call.interface, &call.function, call.input) {
        Ok(value) => (200, json(&serde_json::json!({"ok": value}))),
        Err(error) => (error.status(), envelope(&error)),
    }
}

async fn request(stream: &mut UnixStream) -> Result<(String, String, Vec<u8>), Error> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 8192];
    let split = loop {
        if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break index;
        }
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| Error::unavailable(format!("reading a request: {error}")))?;
        if read == 0 {
            return Err(Error::invalid("the request ended before its headers"));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.len() > HEADER_CAP {
            return Err(Error::too_large("the request headers are too large"));
        }
    };
    let header = std::str::from_utf8(&buffer[..split])
        .map_err(|_| Error::invalid("the request headers are not UTF-8"))?;
    let mut lines = header.lines();
    let mut request = lines.next().unwrap_or_default().split_whitespace();
    let method = request.next().unwrap_or_default().to_string();
    let path = request.next().unwrap_or_default().to_string();
    let length = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .next()
        .unwrap_or(0);
    if length > CALL_CAP {
        return Err(Error::too_large(format!(
            "the call is {length} bytes; the limit is {CALL_CAP}"
        )));
    }
    let mut body = buffer[split + 4..].to_vec();
    while body.len() < length {
        let read = stream
            .read(&mut chunk)
            .await
            .map_err(|error| Error::unavailable(format!("reading a request body: {error}")))?;
        if read == 0 {
            return Err(Error::invalid(
                "the request body is shorter than `Content-Length`",
            ));
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Ok((method, path, body))
}

async fn respond(stream: &mut UnixStream, status: u16, payload: &[u8]) -> Result<(), Error> {
    let header = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        payload.len()
    );
    stream
        .write_all(header.as_bytes())
        .await
        .map_err(|error| Error::unavailable(format!("writing the answer: {error}")))?;
    stream
        .write_all(payload)
        .await
        .map_err(|error| Error::unavailable(format!("writing the answer: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| Error::unavailable(format!("writing the answer: {error}")))
}

/// The request shape. `input` defaults to `null`, which is the WIT unit value.
#[derive(serde::Deserialize)]
struct Call {
    interface: String,
    function: String,
    #[serde(default)]
    input: Value,
}

fn json(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| b"{\"ok\":null}".to_vec())
}

fn envelope(error: &Error) -> Vec<u8> {
    json(&serde_json::to_value(error.envelope()).unwrap_or(Value::Null))
}

/// Turns a service answer into a contract `Error` when a test or a caller reads
/// the envelope directly.
pub fn read_error(payload: &[u8]) -> Option<Error> {
    let envelope: ErrorEnvelope = serde_json::from_slice(payload).ok()?;
    let kind = ErrorKind::from_code(&envelope.error.code).unwrap_or(ErrorKind::Internal);
    let error = Error::new(kind, envelope.error.message);
    Some(match envelope.error.detail {
        Some(detail) => error.with_detail(detail),
        None => error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Echo;

    impl Handler for Echo {
        fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
            match function {
                "echo" => Ok(input),
                "fail" => Err(Error::invalid("the fixture refused")),
                _ => Err(Error::not_found(format!(
                    "`{interface}#{function}` is not implemented"
                ))),
            }
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-sdk-service-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    async fn ask(socket: &Path, request: &str) -> (u16, Value) {
        let mut stream = UnixStream::connect(socket).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("headers");
        let status = std::str::from_utf8(&response[..split])
            .expect("utf8")
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|status| status.parse().ok())
            .expect("status");
        (
            status,
            serde_json::from_slice(&response[split + 4..]).expect("json"),
        )
    }

    async fn start(name: &str) -> (PathBuf, PathBuf) {
        let dir = scratch(name);
        let socket = dir.join("service.sock");
        let path = socket.clone();
        tokio::spawn(async move {
            let _ = serve(&path, Arc::new(Echo)).await;
        });
        for _ in 0..50 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        (dir, socket)
    }

    #[tokio::test]
    async fn health_and_a_call_answer() {
        let (dir, socket) = start("health").await;
        let (status, value) =
            ask(&socket, "GET /health HTTP/1.1\r\nContent-Length: 0\r\n\r\n").await;
        assert_eq!(status, 200);
        assert_eq!(value, serde_json::json!({"ok": true}));

        let body = r#"{"interface":"anthracite:fs/read@1.0.0","function":"echo","input":{"a":1}}"#;
        let request = format!(
            "POST /__anth/call HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (status, value) = ask(&socket, &request).await;
        assert_eq!(status, 200);
        assert_eq!(value, serde_json::json!({"ok": {"a": 1}}));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_handler_error_keeps_its_status_and_envelope() {
        let (dir, socket) = start("error").await;
        let body = r#"{"interface":"x:y/z@1.0.0","function":"fail","input":{}}"#;
        let request = format!(
            "POST /__anth/call HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let (status, value) = ask(&socket, &request).await;
        assert_eq!(status, 400);
        assert_eq!(value["error"]["code"], "invalid");
        assert_eq!(value["error"]["message"], "the fixture refused");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_unknown_endpoint_is_not_found() {
        let (dir, socket) = start("not-found").await;
        let (status, value) = ask(&socket, "GET /nope HTTP/1.1\r\nContent-Length: 0\r\n\r\n").await;
        assert_eq!(status, 404);
        assert_eq!(value["error"]["code"], "not-found");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_oversized_call_is_refused_before_the_body() {
        let (dir, socket) = start("large").await;
        let request = format!(
            "POST /__anth/call HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            CALL_CAP + 1
        );
        let (status, value) = ask(&socket, &request).await;
        assert_eq!(status, 413);
        assert_eq!(value["error"]["code"], "too-large");
        let _ = std::fs::remove_dir_all(dir);
    }
}
