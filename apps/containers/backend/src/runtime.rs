//! The Docker-compatible runtime adapter: HTTP over its Unix socket.
//!
//! The service never shells out to `docker`. Every call maps one WIT function
//! to one Engine API request; the socket path is the argv root the operator
//! approved. Errors keep the runtime status when it matters.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anth_contract::Error;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const HEAD_CAP: usize = 64 * 1024;
const BODY_CAP: usize = 16 * 1024 * 1024;

pub struct Runtime {
    socket: PathBuf,
    version: String,
}

pub struct Head {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub reader: BufReader<UnixStream>,
}

impl Head {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

impl Runtime {
    pub fn new(socket: impl Into<PathBuf>, version: impl Into<String>) -> Self {
        Self {
            socket: socket.into(),
            version: version.into(),
        }
    }

    fn path(&self, path: &str) -> String {
        format!("/{}{}", self.version, path)
    }

    async fn open(&self, method: &str, path: &str, body: Option<&[u8]>) -> Result<Head, Error> {
        let mut stream = UnixStream::connect(&self.socket).await.map_err(|error| {
            Error::unavailable(format!(
                "the container runtime socket {} is unavailable: {error}",
                self.socket.display()
            ))
        })?;
        let length = body.map(|body| body.len()).unwrap_or(0);
        let mut request = format!(
            "{method} {path} HTTP/1.1\r\nHost: docker\r\nAccept: application/json\r\n\
             Content-Type: application/json\r\nContent-Length: {length}\r\n\
             Connection: close\r\n\r\n"
        )
        .into_bytes();
        if let Some(body) = body {
            request.extend_from_slice(body);
        }
        stream
            .write_all(&request)
            .await
            .map_err(|error| Error::unavailable(format!("writing to the runtime: {error}")))?;
        stream
            .flush()
            .await
            .map_err(|error| Error::unavailable(format!("writing to the runtime: {error}")))?;
        let mut reader = BufReader::new(stream);
        let mut head = Vec::new();
        loop {
            let mut line = String::new();
            let read = reader
                .read_line(&mut line)
                .await
                .map_err(|error| Error::unavailable(format!("reading the runtime: {error}")))?;
            if read == 0 {
                return Err(Error::unavailable(
                    "the runtime closed the connection before its headers",
                ));
            }
            head.extend_from_slice(line.as_bytes());
            if head.len() > HEAD_CAP {
                return Err(Error::too_large("the runtime headers are too large"));
            }
            if line == "\r\n" {
                break;
            }
        }
        let text = String::from_utf8_lossy(&head);
        let mut lines = text.lines();
        let status_line = lines.next().unwrap_or_default();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or_else(|| Error::unavailable("the runtime answered no status"))?;
        let mut headers = HashMap::new();
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        Ok(Head {
            status,
            headers,
            reader,
        })
    }

    async fn json(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<(u16, Value), Error> {
        let mut head = self.open(method, path, body).await?;
        let payload = read_body(&mut head).await?;
        let value = if payload.is_empty() {
            Value::Null
        } else {
            match serde_json::from_slice(&payload) {
                Ok(value) => value,
                Err(error) => {
                    if !head.ok() {
                        Value::String(String::from_utf8_lossy(&payload).to_string())
                    } else {
                        return Err(Error::unavailable(format!(
                            "the runtime answered no JSON: {error}"
                        )));
                    }
                }
            }
        };
        if !head.ok() {
            return Err(runtime_error(head.status, &value));
        }
        Ok((head.status, value))
    }

    pub async fn containers(&self) -> Result<Value, Error> {
        let (_, value) = self
            .json("GET", &self.path("/containers/json?all=1"), None)
            .await?;
        Ok(value)
    }

    pub async fn inspect(&self, id: &str) -> Result<Option<Value>, Error> {
        match self
            .json(
                "GET",
                &self.path(&format!("/containers/{}/json", encode(id))),
                None,
            )
            .await
        {
            Ok((_, value)) => Ok(Some(value)),
            Err(error) if error.kind() == anth_contract::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub async fn create(&self, name: Option<&str>, body: &Value) -> Result<String, Error> {
        let query = name
            .map(|name| format!("?name={}", encode(name)))
            .unwrap_or_default();
        let (_, value) = self
            .json(
                "POST",
                &self.path(&format!("/containers/create{query}")),
                Some(body.to_string().as_bytes()),
            )
            .await?;
        value
            .get("Id")
            .and_then(Value::as_str)
            .map(|id| id.to_string())
            .ok_or_else(|| Error::unavailable("the runtime created a container without an id"))
    }

    pub async fn lifecycle(
        &self,
        id: &str,
        action: &str,
        timeout: Option<u32>,
    ) -> Result<(), Error> {
        let path = match action {
            "start" => format!("/containers/{}/start", encode(id)),
            "stop" => format!(
                "/containers/{}/stop?t={}",
                encode(id),
                timeout.unwrap_or(10)
            ),
            "restart" => {
                format!(
                    "/containers/{}/restart?t={}",
                    encode(id),
                    timeout.unwrap_or(10)
                )
            }
            "pause" => format!("/containers/{}/pause", encode(id)),
            "unpause" => format!("/containers/{}/unpause", encode(id)),
            other => return Err(Error::internal(format!("unknown lifecycle `{other}`"))),
        };
        self.json("POST", &self.path(&path), Some(b"{}")).await?;
        Ok(())
    }

    pub async fn kill(&self, id: &str, signal: &str) -> Result<(), Error> {
        self.json(
            "POST",
            &self.path(&format!(
                "/containers/{}/kill?signal={}",
                encode(id),
                encode(signal)
            )),
            Some(b"{}"),
        )
        .await?;
        Ok(())
    }

    pub async fn remove(&self, id: &str, force: bool) -> Result<(), Error> {
        let path = format!(
            "/containers/{}?force={}",
            encode(id),
            if force { "1" } else { "0" }
        );
        self.json("DELETE", &self.path(&path), None).await?;
        Ok(())
    }

    pub async fn images(&self) -> Result<Value, Error> {
        let (_, value) = self.json("GET", &self.path("/images/json"), None).await?;
        Ok(value)
    }

    pub async fn remove_image(&self, id: &str, force: bool) -> Result<(), Error> {
        let path = format!(
            "/images/{}?force={}",
            encode(id),
            if force { "1" } else { "0" }
        );
        self.json("DELETE", &self.path(&path), None).await?;
        Ok(())
    }

    pub async fn volumes(&self) -> Result<Value, Error> {
        let (_, value) = self.json("GET", &self.path("/volumes"), None).await?;
        Ok(value)
    }

    pub async fn create_volume(&self, name: &str) -> Result<Value, Error> {
        let (_, value) = self
            .json(
                "POST",
                &self.path("/volumes/create"),
                Some(json!({"Name": name}).to_string().as_bytes()),
            )
            .await?;
        Ok(value)
    }

    pub async fn remove_volume(&self, name: &str, force: bool) -> Result<(), Error> {
        let path = format!(
            "/volumes/{}?force={}",
            encode(name),
            if force { "1" } else { "0" }
        );
        self.json("DELETE", &self.path(&path), None).await?;
        Ok(())
    }

    pub async fn networks(&self) -> Result<Value, Error> {
        let (_, value) = self.json("GET", &self.path("/networks"), None).await?;
        Ok(value)
    }

    /// `follow = false` reads the whole body and decodes it; `follow = true`
    /// returns a live byte stream. `tty` selects raw vs Docker's 8-byte frame
    /// multiplex.
    pub async fn logs(
        &self,
        id: &str,
        tail: u32,
        follow: bool,
        tty: bool,
    ) -> Result<futures_util::stream::BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        let path = self.path(&format!(
            "/containers/{}/logs?stdout=1&stderr=1&timestamps=0&tail={}&follow={}",
            encode(id),
            tail,
            if follow { "1" } else { "0" }
        ));
        let mut head = self.open("GET", &path, None).await?;
        if !head.ok() {
            let body = read_body(&mut head).await.unwrap_or_default();
            return Err(http_error(head.status, &body));
        }
        let chunked = head
            .headers
            .get("transfer-encoding")
            .map(|value| value.eq_ignore_ascii_case("chunked"))
            .unwrap_or(false);
        if follow {
            Ok(docker_stream(head.reader, tty, chunked))
        } else {
            let body = read_body(&mut head).await?;
            let mut decoder = Decoder::new(tty);
            let chunks = decoder.push(&body);
            Ok(Box::pin(futures_util::stream::iter(
                chunks.into_iter().map(Ok),
            )))
        }
    }

    /// One-shot stats every two seconds, as the WIT `sample` records.
    pub async fn stats(
        &self,
        id: &str,
    ) -> Result<futures_util::stream::BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        let runtime = Runtime::new(self.socket.clone(), self.version.clone());
        let id = id.to_string();
        // Probe once so a missing container fails before the stream opens.
        runtime.stat(&id).await?;
        let stream =
            futures_util::stream::unfold((runtime, id, true), |(runtime, id, first)| async move {
                if !first {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                match runtime.stat(&id).await {
                    Ok(sample) => {
                        Some((Ok(serde_json::to_vec(&sample).ok()?), (runtime, id, false)))
                    }
                    Err(error) => Some((Err(error), (runtime, id, false))),
                }
            });
        Ok(Box::pin(stream))
    }

    async fn stat(&self, id: &str) -> Result<Value, Error> {
        let (_, value) = self
            .json(
                "GET",
                &self.path(&format!("/containers/{}/stats?stream=false", encode(id))),
                None,
            )
            .await?;
        Ok(sample(&value))
    }

    pub async fn pull(
        &self,
        reference: &str,
    ) -> Result<futures_util::stream::BoxStream<'static, Result<Vec<u8>, Error>>, Error> {
        let (image, tag) = split_reference(reference)?;
        let path = self.path(&format!(
            "/images/create?fromImage={}&tag={}",
            encode(&image),
            encode(&tag)
        ));
        let mut head = self.open("POST", &path, Some(b"{}")).await?;
        if !head.ok() {
            let body = read_body(&mut head).await.unwrap_or_default();
            return Err(http_error(head.status, &body));
        }
        let chunked = head
            .headers
            .get("transfer-encoding")
            .map(|value| value.eq_ignore_ascii_case("chunked"))
            .unwrap_or(false);
        Ok(docker_stream(head.reader, true, chunked))
    }

    pub async fn exec_create(
        &self,
        id: &str,
        command: &[String],
        tty: bool,
    ) -> Result<String, Error> {
        let body = json!({
            "AttachStdin": true,
            "AttachStdout": true,
            "AttachStderr": true,
            "Tty": tty,
            "Cmd": command,
        });
        let (_, value) = self
            .json(
                "POST",
                &self.path(&format!("/containers/{}/exec", encode(id))),
                Some(body.to_string().as_bytes()),
            )
            .await?;
        value
            .get("Id")
            .and_then(Value::as_str)
            .map(|id| id.to_string())
            .ok_or_else(|| Error::unavailable("the runtime created an exec without an id"))
    }

    /// Starts an exec and returns the upgraded stream.
    pub async fn exec_start(
        &self,
        exec: &str,
        tty: bool,
    ) -> Result<
        (
            BufReader<tokio::net::unix::OwnedReadHalf>,
            tokio::net::unix::OwnedWriteHalf,
        ),
        Error,
    > {
        let mut stream = UnixStream::connect(&self.socket).await.map_err(|error| {
            Error::unavailable(format!(
                "the container runtime socket {} is unavailable: {error}",
                self.socket.display()
            ))
        })?;
        let body = json!({"Detach": false, "Tty": tty}).to_string();
        let path = self.path(&format!("/exec/{}/start", encode(exec)));
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: docker\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(|error| Error::unavailable(format!("starting the exec: {error}")))?;
        stream
            .flush()
            .await
            .map_err(|error| Error::unavailable(format!("starting the exec: {error}")))?;
        // Read the small header byte by byte: a buffered reader would swallow
        // the first output frames and into_inner would lose them.
        let mut head = Vec::new();
        loop {
            let mut byte = [0u8; 1];
            stream
                .read_exact(&mut byte)
                .await
                .map_err(|error| Error::unavailable(format!("reading the exec: {error}")))?;
            head.push(byte[0]);
            if head.len() > HEAD_CAP {
                return Err(Error::too_large("the exec headers are too large"));
            }
            if head.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&head);
        let status = text
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(0);
        if !matches!(status, 101 | 200) {
            return Err(Error::unavailable(format!(
                "the runtime refused the exec with {status}"
            )));
        }
        let (read, write) = stream.into_split();
        Ok((BufReader::new(read), write))
    }

    pub async fn exec_resize(&self, exec: &str, cols: u32, rows: u32) -> Result<(), Error> {
        self.json(
            "POST",
            &self.path(&format!(
                "/exec/{}/resize?h={}&w={}",
                encode(exec),
                rows,
                cols
            )),
            Some(b"{}"),
        )
        .await?;
        Ok(())
    }
}

/// Turns a live Docker body into decoded byte chunks. The stream ends when the
/// runtime closes it; an error keeps the CA-style envelope.
fn docker_stream(
    reader: BufReader<UnixStream>,
    tty: bool,
    chunked: bool,
) -> futures_util::stream::BoxStream<'static, Result<Vec<u8>, Error>> {
    let stream = futures_util::stream::unfold(
        (reader, Decoder::new(tty), false, chunked),
        |(mut reader, mut decoder, done, chunked)| async move {
            if done {
                return None;
            }
            let chunk = if chunked {
                match read_stream_chunk(&mut reader).await {
                    Ok(Some(chunk)) => chunk,
                    Ok(None) => return None,
                    Err(error) => return Some((Err(error), (reader, decoder, true, chunked))),
                }
            } else {
                let mut buffer = [0u8; 4096];
                match reader.read(&mut buffer).await {
                    Ok(0) => return None,
                    Ok(read) => buffer[..read].to_vec(),
                    Err(error) => {
                        return Some((
                            Err(Error::unavailable(format!(
                                "reading the runtime stream: {error}"
                            ))),
                            (reader, decoder, true, chunked),
                        ))
                    }
                }
            };
            let decoded = decoder.push(&chunk);
            if decoded.is_empty() {
                // Nothing complete yet; keep polling. Returning an empty chunk
                // would be a wasted frame, so recurse through the stream.
                Some((Ok(Vec::new()), (reader, decoder, false, chunked)))
            } else {
                Some((Ok(decoded.concat()), (reader, decoder, false, chunked)))
            }
        },
    );
    Box::pin(stream)
}

/// A Docker log/exec body is a sequence of frames:
/// `[stream, 0, 0, 0, len-be]` + `len` bytes. A TTY stream is raw bytes.
pub struct Decoder {
    tty: bool,
    buffer: Vec<u8>,
}

impl Decoder {
    pub fn new(tty: bool) -> Self {
        Self {
            tty,
            buffer: Vec::new(),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        if self.tty {
            return vec![chunk.to_vec()];
        }
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        while self.buffer.len() >= 8 {
            let size = u32::from_be_bytes([
                self.buffer[4],
                self.buffer[5],
                self.buffer[6],
                self.buffer[7],
            ]) as usize;
            if self.buffer.len() < 8 + size {
                break;
            }
            out.push(self.buffer[8..8 + size].to_vec());
            self.buffer.drain(..8 + size);
        }
        out
    }
}

async fn read_stream_chunk(reader: &mut BufReader<UnixStream>) -> Result<Option<Vec<u8>>, Error> {
    let mut size_line = String::new();
    let read = reader
        .read_line(&mut size_line)
        .await
        .map_err(|error| Error::unavailable(format!("reading the runtime stream: {error}")))?;
    if read == 0 {
        return Ok(None);
    }
    let size_text = size_line.trim();
    if size_text.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let size = usize::from_str_radix(size_text, 16)
        .map_err(|_| Error::unavailable("the runtime stream has a broken chunk size"))?;
    if size == 0 {
        return Ok(None);
    }
    if size > BODY_CAP {
        return Err(Error::too_large("a runtime stream chunk is too large"));
    }
    let mut chunk = vec![0u8; size];
    reader
        .read_exact(&mut chunk)
        .await
        .map_err(|error| Error::unavailable(format!("reading the runtime stream: {error}")))?;
    let mut crlf = [0u8; 2];
    reader
        .read_exact(&mut crlf)
        .await
        .map_err(|error| Error::unavailable(format!("reading the runtime stream: {error}")))?;
    Ok(Some(chunk))
}

async fn read_body(head: &mut Head) -> Result<Vec<u8>, Error> {
    let chunked = head
        .headers
        .get("transfer-encoding")
        .map(|value| value.eq_ignore_ascii_case("chunked"))
        .unwrap_or(false);
    if chunked {
        let mut body = Vec::new();
        while let Some(chunk) = read_stream_chunk(&mut head.reader).await? {
            body.extend_from_slice(&chunk);
            if body.len() > BODY_CAP {
                return Err(Error::too_large("the runtime body is too large"));
            }
        }
        return Ok(body);
    }
    let length = head
        .headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok());
    match length {
        Some(length) if length <= BODY_CAP => {
            let mut body = vec![0u8; length];
            head.reader.read_exact(&mut body).await.map_err(|error| {
                Error::unavailable(format!("reading the runtime body: {error}"))
            })?;
            Ok(body)
        }
        Some(_) => Err(Error::too_large("the runtime body is too large")),
        None => {
            let mut body = Vec::new();
            head.reader.read_to_end(&mut body).await.map_err(|error| {
                Error::unavailable(format!("reading the runtime body: {error}"))
            })?;
            if body.len() > BODY_CAP {
                return Err(Error::too_large("the runtime body is too large"));
            }
            Ok(body)
        }
    }
}

fn runtime_error(status: u16, body: &Value) -> Error {
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("the container runtime refused the request");
    match status {
        404 => Error::not_found(message.to_string()),
        409 => Error::conflict(message.to_string()),
        400 | 422 => Error::invalid(message.to_string()),
        401 | 403 => Error::denied(message.to_string()),
        429 => Error::rate_limited(message.to_string()),
        _ => Error::unavailable(format!("the runtime answered {status}: {message}")),
    }
}

fn http_error(status: u16, body: &[u8]) -> Error {
    let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    runtime_error(status, &value)
}

fn encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn split_reference(reference: &str) -> Result<(String, String), Error> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(Error::invalid("an image reference is required"));
    }
    let (image, tag) = match reference.rsplit_once(':') {
        Some((image, tag)) if !tag.contains('/') && !image.ends_with('/') => (image, tag),
        _ => (reference, "latest"),
    };
    Ok((image.to_string(), tag.to_string()))
}

/// Maps one one-shot Docker stats document to the WIT `sample`.
pub fn sample(stats: &Value) -> Value {
    let cpu = stats.get("cpu_stats").cloned().unwrap_or(Value::Null);
    let previous = stats.get("precpu_stats").cloned().unwrap_or(Value::Null);
    let cpu_total = cpu
        .get("cpu_usage")
        .and_then(|usage| usage.get("total_usage"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let previous_total = previous
        .get("cpu_usage")
        .and_then(|usage| usage.get("total_usage"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let system = cpu
        .get("system_cpu_usage")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let previous_system = previous
        .get("system_cpu_usage")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cpus = cpu
        .get("online_cpus")
        .and_then(Value::as_u64)
        .or_else(|| {
            cpu.get("cpu_usage")
                .and_then(|usage| usage.get("percpu_usage"))
                .and_then(Value::as_array)
                .map(|entries| entries.len() as u64)
        })
        .unwrap_or(1);
    let cpu_delta = cpu_total.saturating_sub(previous_total);
    let system_delta = system.saturating_sub(previous_system);
    let percent = if system_delta > 0 {
        (cpu_delta as f64 / system_delta as f64) * cpus as f64 * 100.0
    } else {
        0.0
    };
    let memory = stats.get("memory_stats").cloned().unwrap_or(Value::Null);
    let usage = memory.get("usage").and_then(Value::as_u64).unwrap_or(0);
    let cache = memory
        .get("stats")
        .and_then(|stats| stats.get("cache"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let limit = memory.get("limit").and_then(Value::as_u64).unwrap_or(0);
    let mut rx = 0;
    let mut tx = 0;
    if let Some(networks) = stats.get("networks").and_then(Value::as_object) {
        for network in networks.values() {
            rx += network.get("rx_bytes").and_then(Value::as_u64).unwrap_or(0);
            tx += network.get("tx_bytes").and_then(Value::as_u64).unwrap_or(0);
        }
    }
    let pids = stats
        .get("pids_stats")
        .and_then(|stats| stats.get("current"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "cpu-percent": percent,
        "memory-bytes": usage.saturating_sub(cache),
        "memory-limit": limit,
        "net-rx": rx,
        "net-tx": tx,
        "pids": pids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_converts_cpu_and_memory() {
        let value = json!({
            "cpu_stats": {
                "cpu_usage": {"total_usage": 200},
                "system_cpu_usage": 1000,
                "online_cpus": 2
            },
            "precpu_stats": {
                "cpu_usage": {"total_usage": 100},
                "system_cpu_usage": 500
            },
            "memory_stats": {"usage": 1000, "limit": 4096, "stats": {"cache": 200}},
            "networks": {"eth0": {"rx_bytes": 10, "tx_bytes": 20}},
            "pids_stats": {"current": 3}
        });
        let sample = sample(&value);
        assert_eq!(sample["cpu-percent"], 40.0);
        assert_eq!(sample["memory-bytes"], 800);
        assert_eq!(sample["memory-limit"], 4096);
        assert_eq!(sample["net-rx"], 10);
        assert_eq!(sample["net-tx"], 20);
        assert_eq!(sample["pids"], 3);
    }

    #[test]
    fn references_split_tags_and_default_latest() {
        assert_eq!(
            split_reference("nginx:1.27").expect("image"),
            ("nginx".to_string(), "1.27".to_string())
        );
        assert_eq!(
            split_reference("ghcr.io/acme/app").expect("image"),
            ("ghcr.io/acme/app".to_string(), "latest".to_string())
        );
    }

    #[test]
    fn the_decoder_handles_split_frames() {
        let mut frame = vec![1, 0, 0, 0, 0, 0, 0, 3];
        frame.extend_from_slice(b"abc");
        let mut decoder = Decoder::new(false);
        assert!(decoder.push(&frame[..5]).is_empty());
        let out = decoder.push(&frame[5..]);
        assert_eq!(out, vec![b"abc".to_vec()]);
    }
}
