//! The calling side of the broker: a service reaches a peer through its
//! `{broker}` socket, never through the public router (§22, D9).
//!
//! ```no_run
//! # async fn example() -> Result<(), anth_error::Error> {
//! let broker = anth_service::Broker::new("/var/lib/anth/broker/org.example.consumer.sock");
//! let answer = broker
//!     .call("example:provider/greeting@1.0.0", "greet", serde_json::json!({"name": "Ada"}))
//!     .await?;
//! broker.audit("proxy.apply", serde_json::json!({"host": "a"})).await?;
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};
use std::pin::Pin;

use anth_error::Error;
use futures_core::Stream;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// The engine-served audit bridge.
pub const AUDIT_INTERFACE: &str = "anth:audit/record@1.0.0";
const CALL_CAP: usize = 4 * 1024 * 1024;

/// A provider stream relayed by the broker.
pub type BrokerStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>, Error>> + Send>>;

/// A connection to one service's broker socket.
#[derive(Debug, Clone)]
pub struct Broker {
    socket: PathBuf,
}

impl Broker {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    /// The `{broker}` path the engine handed to this service.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Calls `interface#function` through the grant. The engine resolves the
    /// provider, checks the caller's imports and audits the call.
    pub async fn call(
        &self,
        interface: &str,
        function: &str,
        input: Value,
    ) -> Result<Value, Error> {
        let body = serde_json::to_vec(&json!({
            "interface": interface,
            "function": function,
            "input": input,
        }))
        .map_err(|error| Error::invalid(format!("encoding the broker call: {error}")))?;
        if body.len() > CALL_CAP {
            return Err(Error::too_large(format!(
                "the broker call is {} bytes; the limit is {CALL_CAP}",
                body.len()
            )));
        }
        let (status, payload) = send(&self.socket, "POST", "/__anth/call", &body).await?;
        if status == 200 {
            let value: Value = serde_json::from_slice(&payload).map_err(|error| {
                Error::internal(format!("the broker answered no JSON: {error}"))
            })?;
            value
                .get("ok")
                .cloned()
                .ok_or_else(|| Error::internal("the broker answered without an `ok` value"))
        } else {
            Err(envelope_error(&payload))
        }
    }

    /// Records one entry in the engine's audit log through the broker bridge.
    /// The engine sanitizes the detail; a secret-shaped key is dropped.
    pub async fn audit(&self, action: &str, detail: Value) -> Result<Value, Error> {
        self.call(
            AUDIT_INTERFACE,
            "record",
            json!({"action": action, "detail": detail}),
        )
        .await
    }

    /// `GET /health` on the broker socket.
    pub async fn health(&self) -> Result<(), Error> {
        let (status, _) = send(&self.socket, "GET", "/health", b"").await?;
        if status == 200 {
            Ok(())
        } else {
            Err(Error::unavailable(format!(
                "the broker answered {status} to /health"
            )))
        }
    }

    /// Opens `POST /__anth/stream`. The engine checks the same grant as
    /// [`Broker::call`] and bounds the chunks with the provider's byte budget;
    /// the stream ends with an `exhausted` error when that budget runs out.
    pub async fn stream(
        &self,
        interface: &str,
        function: &str,
        input: Value,
    ) -> Result<BrokerStream, Error> {
        let body = serde_json::to_vec(&json!({
            "interface": interface,
            "function": function,
            "input": input,
        }))
        .map_err(|error| Error::invalid(format!("encoding the broker stream: {error}")))?;
        if body.len() > CALL_CAP {
            return Err(Error::too_large(format!(
                "the broker stream is {} bytes; the limit is {CALL_CAP}",
                body.len()
            )));
        }
        let mut reader =
            BufReader::new(UnixStream::connect(&self.socket).await.map_err(|error| {
                Error::unavailable(format!("connecting to {}: {error}", self.socket.display()))
            })?);
        let head = format!(
            "POST /__anth/stream HTTP/1.1\r\nHost: anthracite\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        {
            let stream = reader.get_mut();
            stream
                .write_all(head.as_bytes())
                .await
                .map_err(|error| Error::unavailable(format!("writing the stream: {error}")))?;
            stream
                .write_all(&body)
                .await
                .map_err(|error| Error::unavailable(format!("writing the stream: {error}")))?;
            stream
                .flush()
                .await
                .map_err(|error| Error::unavailable(format!("writing the stream: {error}")))?;
        }
        let mut status_line = String::new();
        if reader
            .read_line(&mut status_line)
            .await
            .map_err(|error| Error::unavailable(format!("reading the stream: {error}")))?
            == 0
        {
            return Err(Error::unavailable("the broker stream has no status line"));
        }
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or_else(|| Error::unavailable("the broker stream has no status"))?;
        let mut length = None;
        loop {
            let mut header = String::new();
            if reader
                .read_line(&mut header)
                .await
                .map_err(|error| Error::unavailable(format!("reading the stream: {error}")))?
                == 0
                || header == "\r\n"
            {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse::<usize>().ok();
                }
            }
        }
        if status != 200 {
            let payload = match length {
                Some(length) if length <= CALL_CAP => {
                    let mut payload = vec![0u8; length];
                    reader.read_exact(&mut payload).await.map_err(|error| {
                        Error::unavailable(format!("reading the stream error: {error}"))
                    })?;
                    payload
                }
                _ => Vec::new(),
            };
            return Err(envelope_error(&payload));
        }
        Ok(Box::pin(futures_util::stream::try_unfold(
            reader,
            |mut reader| async move {
                match read_frame(&mut reader).await? {
                    Some(chunk) => Ok(Some((chunk, reader))),
                    None => Ok(None),
                }
            },
        )))
    }
}

/// One chunked frame of a broker stream: `{"chunk": <base64>}` or a terminal
/// `{"error": …}`.
async fn read_frame(reader: &mut BufReader<UnixStream>) -> Result<Option<Vec<u8>>, Error> {
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .await
        .map_err(|error| Error::unavailable(format!("reading a stream chunk: {error}")))?
        == 0
    {
        return Err(Error::unavailable(
            "the broker stream ended before its terminator",
        ));
    }
    let size = usize::from_str_radix(line.trim(), 16)
        .map_err(|_| Error::internal("the broker stream has an invalid chunk size"))?;
    if size == 0 {
        loop {
            let mut trailer = String::new();
            if reader
                .read_line(&mut trailer)
                .await
                .map_err(|error| Error::unavailable(format!("reading stream trailers: {error}")))?
                == 0
                || trailer == "\r\n"
            {
                return Ok(None);
            }
        }
    }
    if size > CALL_CAP {
        return Err(Error::too_large("the broker stream chunk is too large"));
    }
    let mut payload = vec![0u8; size];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|error| Error::unavailable(format!("reading a stream chunk: {error}")))?;
    let mut crlf = [0u8; 2];
    reader
        .read_exact(&mut crlf)
        .await
        .map_err(|error| Error::unavailable(format!("reading a stream chunk: {error}")))?;
    let value: Value = serde_json::from_slice(&payload).map_err(|error| {
        Error::internal(format!("the broker stream has no JSON frame: {error}"))
    })?;
    if let Some(chunk) = value.get("chunk").and_then(Value::as_str) {
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(chunk)
            .map_err(|error| Error::internal(format!("the stream chunk is not base64: {error}")))?;
        return Ok(Some(bytes));
    }
    if value.get("error").is_some() {
        return Err(envelope_error(&payload));
    }
    Err(Error::internal(
        "the broker stream frame has neither `chunk` nor `error`",
    ))
}

async fn send(
    socket: &Path,
    method: &str,
    path: &str,
    body: &[u8],
) -> Result<(u16, Vec<u8>), Error> {
    let mut stream = UnixStream::connect(socket).await.map_err(|error| {
        Error::unavailable(format!("connecting to {}: {error}", socket.display()))
    })?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: anthracite\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .map_err(|error| Error::unavailable(format!("writing the broker request: {error}")))?;
    stream
        .write_all(body)
        .await
        .map_err(|error| Error::unavailable(format!("writing the broker request: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| Error::unavailable(format!("writing the broker request: {error}")))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .await
        .map_err(|error| Error::unavailable(format!("reading the broker answer: {error}")))?;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| Error::unavailable("the broker answer has no header terminator"))?;
    let head = String::from_utf8_lossy(&response[..split]);
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| Error::unavailable("the broker answer has no status"))?;
    Ok((status, response[split + 4..].to_vec()))
}

fn envelope_error(payload: &[u8]) -> Error {
    match serde_json::from_slice::<anth_error::ErrorEnvelope>(payload) {
        Ok(envelope) => {
            let body = envelope.error;
            let kind = anth_error::ErrorKind::from_code(&body.code)
                .unwrap_or(anth_error::ErrorKind::Unavailable);
            let mut error = Error::new(kind, body.message);
            if let Some(app) = body.app {
                error = error.with_app(&app);
            }
            if let Some(detail) = body.detail {
                error = error.with_detail(detail);
            }
            error
        }
        Err(_) => Error::unavailable("the broker answered an error without an envelope"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    #[tokio::test]
    async fn a_broker_call_round_trips() {
        let dir = std::env::temp_dir().join("anthracite-panel");
        std::fs::create_dir_all(&dir).expect("scratch");
        let socket = dir.join(format!("broker-test-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).await.expect("read");
            assert!(String::from_utf8_lossy(&buffer[..read]).contains("__anth/call"));
            let body = br#"{"ok":{"greeting":"hi"}}"#;
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).await.expect("head");
            stream.write_all(body).await.expect("body");
        });
        let broker = Broker::new(&socket);
        let value = broker
            .call(
                "example:provider/greeting@1.0.0",
                "greet",
                json!({"name": "Ada"}),
            )
            .await
            .expect("call");
        assert_eq!(value["greeting"], "hi");
        server.await.expect("server");
        let _ = std::fs::remove_file(&socket);
    }

    #[tokio::test]
    async fn a_health_check_uses_get() {
        let dir = std::env::temp_dir().join("anthracite-panel");
        std::fs::create_dir_all(&dir).expect("scratch");
        let socket = dir.join(format!("broker-health-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).await.expect("read");
            assert!(
                String::from_utf8_lossy(&buffer[..read]).starts_with("GET /health"),
                "{}",
                String::from_utf8_lossy(&buffer[..read])
            );
            let body = br#"{"status":"ok"}"#;
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
            stream.write_all(head.as_bytes()).await.expect("head");
            stream.write_all(body).await.expect("body");
        });
        Broker::new(&socket).health().await.expect("health");
        server.await.expect("server");
        let _ = std::fs::remove_file(&socket);
    }

    #[tokio::test]
    async fn a_broker_stream_decodes_chunks_and_an_exhausted_error() {
        use base64::Engine as _;
        use futures_util::StreamExt;

        let dir = std::env::temp_dir().join("anthracite-panel");
        std::fs::create_dir_all(&dir).expect("scratch");
        let socket = dir.join(format!("broker-stream-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).expect("bind");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).await.expect("read");
            assert!(String::from_utf8_lossy(&buffer[..read]).contains("__anth/stream"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("head");
            let chunk = json!({"chunk": base64::engine::general_purpose::STANDARD.encode(b"one")});
            write_test_frame(&mut stream, &serde_json::to_vec(&chunk).expect("chunk")).await;
            let error = serde_json::to_vec(
                &anth_error::Error::exhausted("the stream budget ran out").envelope(),
            )
            .expect("error");
            write_test_frame(&mut stream, &error).await;
            stream.write_all(b"0\r\n\r\n").await.expect("end");
        });
        let mut chunks = Broker::new(&socket)
            .stream("example:provider/greeting@1.0.0", "chunks", json!({}))
            .await
            .expect("stream");
        assert_eq!(chunks.next().await.expect("chunk").expect("ok"), b"one");
        let error = chunks
            .next()
            .await
            .expect("error frame")
            .expect_err("exhausted");
        assert_eq!(error.kind(), anth_error::ErrorKind::Exhausted);
        assert!(chunks.next().await.is_none());
        server.await.expect("server");
        let _ = std::fs::remove_file(&socket);
    }

    async fn write_test_frame(stream: &mut tokio::net::UnixStream, frame: &[u8]) {
        stream
            .write_all(format!("{:x}\r\n", frame.len()).as_bytes())
            .await
            .expect("size");
        stream.write_all(frame).await.expect("frame");
        stream.write_all(b"\r\n").await.expect("crlf");
    }
}
