//! The calling side of the broker: a service reaches a peer through its
//! `{broker}` socket, never through the public router (§22, D9).
//!
//! ```no_run
//! # async fn example() -> Result<(), anth_contract::Error> {
//! let broker = anth_service::Broker::new("/var/lib/anth/broker/org.example.consumer.sock");
//! let answer = broker
//!     .call("example:provider/greeting@1.0.0", "greet", serde_json::json!({"name": "Ada"}))
//!     .await?;
//! broker.audit("proxy.apply", serde_json::json!({"host": "a"})).await?;
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};

use anth_contract::Error;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// The engine-served audit bridge.
pub const AUDIT_INTERFACE: &str = "anth:audit/record@1.0.0";
const CALL_CAP: usize = 4 * 1024 * 1024;

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
        let (status, payload) = post(&self.socket, "/__anth/call", &body).await?;
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
        let (status, _) = post(&self.socket, "/health", b"").await?;
        if status == 200 {
            Ok(())
        } else {
            Err(Error::unavailable(format!(
                "the broker answered {status} to /health"
            )))
        }
    }
}

async fn post(socket: &Path, path: &str, body: &[u8]) -> Result<(u16, Vec<u8>), Error> {
    let mut stream = UnixStream::connect(socket).await.map_err(|error| {
        Error::unavailable(format!("connecting to {}: {error}", socket.display()))
    })?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: anthracite\r\nContent-Type: application/json\r\n\
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
    match serde_json::from_slice::<anth_contract::ErrorEnvelope>(payload) {
        Ok(envelope) => {
            let body = envelope.error;
            let kind = anth_contract::ErrorKind::from_code(&body.code)
                .unwrap_or(anth_contract::ErrorKind::Unavailable);
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
}
