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
use std::pin::Pin;
use std::sync::Arc;

use anth_contract::{Error, ErrorEnvelope, ErrorKind};
use futures_core::Stream;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

/// The per-call cap, matching the engine.
pub const CALL_CAP: usize = 4 * 1024 * 1024;
/// Header blocks larger than this are refused before the body is read.
const HEADER_CAP: usize = 64 * 1024;

/// One chunk of a streamed answer, in the private service protocol (§20).
pub type ChunkStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, Error>> + Send>>;

/// One function of one exported interface.
pub trait Handler: Send + Sync + 'static {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error>;

    /// Starts a streamed function. The default keeps services that only buffer
    /// honest: a stream route answers `not-found` instead of inventing chunks.
    ///
    /// The returned stream must not block in `poll_next`; a source that waits
    /// for output (a PTY) wakes the task through its own channel or notifier.
    fn stream(
        &self,
        _interface: &str,
        _function: &str,
        _input: Value,
    ) -> Result<ChunkStream, Error> {
        Err(Error::not_found(
            "this service does not implement streamed functions",
        ))
    }
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
    let (method, path, body) = match request(&mut stream).await {
        Ok(parts) => parts,
        Err(error) => {
            let payload = envelope(&error);
            return respond(&mut stream, error.status(), &payload).await;
        }
    };
    if path == "/__anth/stream" {
        return stream_route(&mut stream, &method, &body, handler).await;
    }
    // Native handlers may block (a PTY read waits for output); keep them
    // on the blocking pool so one call cannot stall the runtime.
    let handler = Arc::clone(&handler);
    let result =
        tokio::task::spawn_blocking(move || route(&method, &path, &body, handler.as_ref())).await;
    let (status, payload) = match result {
        Ok(pair) => pair,
        Err(error) => (
            500,
            envelope(&Error::internal(format!(
                "the service handler did not complete: {error}"
            ))),
        ),
    };
    respond(&mut stream, status, &payload).await
}

/// `POST /__anth/stream`: the same call shape as `/__anth/call`, answered with a
/// chunked body. An error before the first byte is the ordinary JSON envelope;
/// an error after it closes the connection without the terminator, which the
/// engine reports as a truncated stream (§20).
async fn stream_route(
    stream: &mut UnixStream,
    method: &str,
    body: &[u8],
    handler: Arc<dyn Handler>,
) -> Result<(), Error> {
    if method != "POST" {
        return respond(
            stream,
            405,
            &envelope(&Error::method_not_allowed(
                "streams are `POST /__anth/stream`",
            )),
        )
        .await;
    }
    let call: Call = match serde_json::from_slice(body) {
        Ok(call) => call,
        Err(error) => {
            return respond(
                stream,
                400,
                &envelope(&Error::invalid(format!(
                    "the call body is not valid: {error}"
                ))),
            )
            .await;
        }
    };
    if call.interface.is_empty() || call.function.is_empty() {
        return respond(
            stream,
            400,
            &envelope(&Error::invalid("a call needs `interface` and `function`")),
        )
        .await;
    }
    let interface = call.interface;
    let function = call.function;
    let input = call.input;
    let started = tokio::task::spawn_blocking(move || handler.stream(&interface, &function, input))
        .await
        .map_err(|error| {
            Error::internal(format!("the service handler did not complete: {error}"))
        })?;
    let mut chunks = match started {
        Ok(chunks) => chunks,
        Err(error) => return respond(stream, error.status(), &envelope(&error)).await,
    };

    stream
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
              Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
        )
        .await
        .map_err(|error| Error::unavailable(format!("writing the stream: {error}")))?;
    loop {
        let next = std::future::poll_fn(|context| chunks.as_mut().poll_next(context)).await;
        match next {
            None => {
                stream
                    .write_all(b"0\r\n\r\n")
                    .await
                    .map_err(|error| Error::unavailable(format!("closing the stream: {error}")))?;
                return stream
                    .flush()
                    .await
                    .map_err(|error| Error::unavailable(format!("flushing the stream: {error}")));
            }
            Some(Ok(chunk)) if chunk.is_empty() => continue,
            Some(Ok(chunk)) => {
                // Writing before polling again is the backpressure: the producer
                // never runs ahead of the engine's reader.
                let head = format!("{:x}\r\n", chunk.len());
                stream
                    .write_all(head.as_bytes())
                    .await
                    .map_err(|error| Error::unavailable(format!("writing the stream: {error}")))?;
                stream
                    .write_all(&chunk)
                    .await
                    .map_err(|error| Error::unavailable(format!("writing the stream: {error}")))?;
                stream
                    .write_all(b"\r\n")
                    .await
                    .map_err(|error| Error::unavailable(format!("writing the stream: {error}")))?;
            }
            Some(Err(_)) => {
                // No terminator: the peer must see a truncated body, not a
                // clean EOF that would hide the failed producer.
                let _ = stream.shutdown().await;
                return Ok(());
            }
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

        fn stream(
            &self,
            interface: &str,
            function: &str,
            _input: Value,
        ) -> Result<ChunkStream, Error> {
            match function {
                "chunks" => Ok(Box::pin(futures_util::stream::iter(vec![
                    Ok(b"one".to_vec()),
                    Ok(b"two".to_vec()),
                    Ok(b"three".to_vec()),
                ]))),
                "break" => Ok(Box::pin(futures_util::stream::iter(vec![
                    Ok(b"head".to_vec()),
                    Err(Error::internal("the fixture stream broke")),
                ]))),
                "refuse" => Err(Error::invalid("the fixture refused to stream")),
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
        let (status, _, body) = ask_raw(socket, request).await;
        (status, serde_json::from_slice(&body).expect("json"))
    }

    async fn ask_raw(socket: &Path, request: &str) -> (u16, String, Vec<u8>) {
        let mut stream = UnixStream::connect(socket).await.expect("connect");
        stream.write_all(request.as_bytes()).await.expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        let split = response
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .expect("headers");
        let header = std::str::from_utf8(&response[..split])
            .expect("utf8")
            .to_string();
        let status = header
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|status| status.parse().ok())
            .expect("status");
        (status, header, response[split + 4..].to_vec())
    }

    async fn start(name: &str) -> (PathBuf, PathBuf) {
        start_with(name, Arc::new(Echo)).await
    }

    async fn start_with(name: &str, handler: Arc<dyn Handler>) -> (PathBuf, PathBuf) {
        let dir = scratch(name);
        let socket = dir.join("service.sock");
        let path = socket.clone();
        tokio::spawn(async move {
            let _ = serve(&path, handler).await;
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

    fn stream_request(function: &str) -> String {
        let body = format!(r#"{{"interface":"x:y/z@1.0.0","function":"{function}","input":{{}}}}"#);
        format!(
            "POST /__anth/stream HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn a_stream_answers_with_chunked_frames() {
        let (dir, socket) = start("stream").await;
        let (status, header, payload) = ask_raw(&socket, &stream_request("chunks")).await;
        assert_eq!(status, 200);
        assert!(
            header
                .to_ascii_lowercase()
                .contains("transfer-encoding: chunked"),
            "{header}"
        );
        assert!(
            header
                .to_ascii_lowercase()
                .contains("content-type: application/octet-stream"),
            "{header}"
        );
        assert_eq!(
            payload,
            b"3\r\none\r\n3\r\ntwo\r\n5\r\nthree\r\n0\r\n\r\n",
            "{}",
            String::from_utf8_lossy(&payload)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_stream_error_before_the_first_byte_is_an_envelope() {
        let (dir, socket) = start("stream-refuse").await;
        let (status, value) = ask(&socket, &stream_request("refuse")).await;
        assert_eq!(status, 400);
        assert_eq!(value["error"]["code"], "invalid");
        assert_eq!(value["error"]["message"], "the fixture refused to stream");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_stream_error_after_a_chunk_truncates_without_the_terminator() {
        let (dir, socket) = start("stream-break").await;
        let (status, _, payload) = ask_raw(&socket, &stream_request("break")).await;
        assert_eq!(status, 200);
        assert_eq!(
            payload,
            b"4\r\nhead\r\n",
            "{}",
            String::from_utf8_lossy(&payload)
        );
        assert!(
            !payload.ends_with(b"0\r\n\r\n"),
            "the failed stream is not a clean EOF"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_handler_without_streams_answers_not_found() {
        struct Buffered;
        impl Handler for Buffered {
            fn call(&self, _: &str, _: &str, _: Value) -> Result<Value, Error> {
                Ok(Value::Null)
            }
        }
        let (dir, socket) = start_with("stream-default", Arc::new(Buffered)).await;
        let (status, value) = ask(&socket, &stream_request("chunks")).await;
        assert_eq!(status, 404);
        assert_eq!(value["error"]["code"], "not-found");
        let _ = std::fs::remove_dir_all(dir);
    }
}
