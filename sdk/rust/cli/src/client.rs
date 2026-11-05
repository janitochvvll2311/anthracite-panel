//! The engine client: HTTP/1.1 over the Unix socket.
//!
//! The CLI is a thin client: it connects to the engine's socket, and the kernel
//! peer credentials are the authentication. There is no token and no session
//! for a local operator.
//!
//! The dialect is deliberately small: one request per connection, `Connection:
//! close`, a `Content-Length` or chunked body. That is all the engine's router
//! answers, and implementing it here avoids pulling a full HTTP stack into the
//! client.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anth_contract::{Error, ErrorKind};

/// A local engine answers in milliseconds. The bound exists so a wedged
/// process cannot hang the terminal forever.
const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Client {
    socket: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    /// Lowercase header names, as the engine sent them.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Client {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Sends one request and returns the response. An error envelope becomes an
    /// `Error` with the code the engine chose.
    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        headers: &[(&str, &str)],
    ) -> Result<Response, Error> {
        let mut stream = UnixStream::connect(&self.socket).map_err(|error| {
            Error::unavailable(format!(
                "cannot reach {}: {error}; is the engine running?",
                self.socket.display()
            ))
        })?;
        stream
            .set_read_timeout(Some(TIMEOUT))
            .and_then(|()| stream.set_write_timeout(Some(TIMEOUT)))
            .map_err(|error| Error::internal(format!("configuring the socket: {error}")))?;
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Length: {}\r\n",
            body.len()
        );
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .and_then(|()| stream.write_all(body))
            .map_err(|error| io("writing to the engine", error))?;
        let mut raw = Vec::new();
        stream
            .read_to_end(&mut raw)
            .map_err(|error| io("reading from the engine", error))?;
        let response = parse(&raw)?;
        if response.status >= 400 {
            return Err(envelope(response.status, &response.body));
        }
        Ok(response)
    }

    pub fn json(&self, method: &str, path: &str, body: &[u8]) -> Result<serde_json::Value, Error> {
        let response = self.request(method, path, body, &[])?;
        serde_json::from_slice(&response.body).map_err(|error| {
            Error::internal(format!("the engine answered with invalid JSON: {error}"))
        })
    }

    pub fn get_json(&self, path: &str) -> Result<serde_json::Value, Error> {
        self.json("GET", path, &[])
    }
}

fn parse(raw: &[u8]) -> Result<Response, Error> {
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| Error::internal("the engine answer has no header terminator"))?;
    let head = std::str::from_utf8(&raw[..split])
        .map_err(|error| Error::internal(format!("the engine answer is not text: {error}")))?;
    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| Error::internal("the engine answer has no status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| Error::internal(format!("`{status_line}` is not an HTTP status line")))?;
    let mut headers: HashMap<String, String> = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let body = &raw[split + 4..];
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        decode_chunked(body)?
    } else {
        body.to_vec()
    };
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    loop {
        let end = data
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| Error::internal("a chunk header is not terminated"))?;
        let size = std::str::from_utf8(&data[..end])
            .map_err(|error| Error::internal(format!("a chunk size is not text: {error}")))?;
        let size = size
            .split(';')
            .next()
            .and_then(|size| usize::from_str_radix(size.trim(), 16).ok())
            .ok_or_else(|| Error::internal(format!("`{size}` is not a chunk size")))?;
        data = &data[end + 2..];
        if size == 0 {
            break;
        }
        if data.len() < size + 2 {
            return Err(Error::internal("a chunk is shorter than its size"));
        }
        body.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
    Ok(body)
}

/// The error envelope of §7.11, or a kind derived from the status when the body
/// is not one (readiness `503` answers `{"status":"starting"}`).
fn envelope(status: u16, body: &[u8]) -> Error {
    let value: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
    let error = &value["error"];
    let kind = error["code"]
        .as_str()
        .and_then(ErrorKind::from_code)
        .unwrap_or(match status {
            400 => ErrorKind::Invalid,
            401 => ErrorKind::Unauthenticated,
            403 => ErrorKind::Denied,
            404 => ErrorKind::NotFound,
            409 => ErrorKind::Conflict,
            413 => ErrorKind::TooLarge,
            429 => ErrorKind::Exhausted,
            503 => ErrorKind::Unavailable,
            _ => ErrorKind::Internal,
        });
    let message = match (error["message"].as_str(), value["status"].as_str()) {
        (Some(message), _) => message.to_string(),
        (None, Some(state)) => format!("the engine is not ready: {state}"),
        (None, None) => "the engine refused the request".to_string(),
    };
    let mut failure = Error::new(kind, message);
    if let Some(app) = error["app"].as_str() {
        failure = failure.with_app(app);
    }
    if let Some(detail) = error.get("detail") {
        failure = failure.with_detail(detail.clone());
    }
    failure
}

/// A timed-out socket is unavailable, not an engine bug.
fn io(action: &str, error: std::io::Error) -> Error {
    if matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ) {
        return Error::unavailable(format!("the engine did not answer while {action}"));
    }
    Error::internal(format!("{action}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::thread;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-cli-client-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir.join("engine.sock")
    }

    /// Serves one request and answers `response`, recording the request bytes.
    fn serve(path: &Path, response: &'static [u8]) -> mpsc::Receiver<Vec<u8>> {
        let listener = UnixListener::bind(path).expect("bind");
        let (send, receive) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            // Read until the headers are complete, then the declared body.
            loop {
                let read = stream.read(&mut buffer).expect("read");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if let Some(split) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&request[..split]).to_lowercase();
                    let length = head
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if request.len() >= split + 4 + length {
                        break;
                    }
                }
            }
            let _ = send.send(request);
            stream.write_all(response).expect("write");
        });
        receive
    }

    #[test]
    fn a_get_parses_the_status_and_body() {
        let path = scratch("get");
        let receive = serve(&path, b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        let response = Client::new(&path)
            .request("GET", "/api/health", &[], &[])
            .expect("get");
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"{}");
        let request = receive.recv().expect("request");
        let text = String::from_utf8_lossy(&request);
        assert!(text.starts_with("GET /api/health HTTP/1.1\r\n"), "{text}");
    }

    #[test]
    fn an_error_envelope_becomes_a_typed_error() {
        let path = scratch("error");
        let body = br#"{"error":{"code":"not-found","message":"`x` is not installed"}}"#;
        let mut response = b"HTTP/1.1 404 Not Found\r\nContent-Length: ".to_vec();
        response.extend_from_slice(body.len().to_string().as_bytes());
        response.extend_from_slice(b"\r\n\r\n");
        response.extend_from_slice(body);
        let receive = serve(&path, Box::leak(response.into_boxed_slice()));
        let error = Client::new(&path)
            .request("GET", "/api/engine/apps/x", &[], &[])
            .expect_err("404");
        assert_eq!(error.kind(), ErrorKind::NotFound);
        assert!(error.message().contains("not installed"), "{error}");
        let _ = receive;
    }

    #[test]
    fn a_readiness_503_without_an_envelope_is_unavailable() {
        let path = scratch("readiness");
        let receive = serve(
            &path,
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 21\r\n\r\n{\"status\":\"starting\"}",
        );
        let error = Client::new(&path)
            .request("GET", "/api/health", &[], &[])
            .expect_err("503");
        assert_eq!(error.kind(), ErrorKind::Unavailable);
        assert!(error.message().contains("starting"), "{error}");
        let _ = receive;
    }

    #[test]
    fn a_chunked_body_is_decoded() {
        let path = scratch("chunked");
        let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n";
        let _receive = serve(&path, response);
        let response = Client::new(&path)
            .request("GET", "/", &[], &[])
            .expect("chunked");
        assert_eq!(response.body, b"{}");
    }

    #[test]
    fn a_post_sends_the_body_and_headers() {
        let path = scratch("post");
        let receive = serve(&path, b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
        Client::new(&path)
            .request(
                "POST",
                "/api/engine/apps",
                b"zip",
                &[("x-anth-acknowledge", "dependents")],
            )
            .expect("post");
        let request = String::from_utf8_lossy(&receive.recv().expect("request")).to_string();
        assert!(
            request.starts_with("POST /api/engine/apps HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(
            request.to_lowercase().contains("content-length: 3"),
            "{request}"
        );
        assert!(
            request
                .to_lowercase()
                .contains("x-anth-acknowledge: dependents"),
            "{request}"
        );
        assert!(request.ends_with("zip"), "{request}");
    }

    #[test]
    fn a_missing_socket_is_unavailable() {
        let error = Client::new("/nonexistent/engine.sock")
            .request("GET", "/", &[], &[])
            .expect_err("missing");
        assert_eq!(error.kind(), ErrorKind::Unavailable);
        assert!(
            error.message().contains("is the engine running?"),
            "{error}"
        );
    }
}
