//! Where the client connects and how one HTTP request travels.
//!
//! Two transports, one dialect. A Unix socket is the local path and needs no
//! token; a TCP address is for a remote client and may carry a bearer token.
//! Every request opens its own connection: the CLI issues a handful per run,
//! and a fresh connection keeps the code free of a pool and of stale-socket
//! handling.

use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll};

use anth_wire::{ErrorKind, WireError};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::client::conn::http1::{SendRequest, handshake};
use hyper::http::{HeaderMap, Method, StatusCode, header};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpStream, UnixStream};

/// Where the daemon listens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    /// The local socket. The daemon authenticates the peer by uid.
    Unix(PathBuf),
    /// A plain HTTP base such as `http://127.0.0.1:8080`.
    Http(String),
}

impl Endpoint {
    /// The path of the local socket.
    pub fn unix(path: impl Into<PathBuf>) -> Self {
        Self::Unix(path.into())
    }

    /// A remote base address. TLS is terminated by a proxy in front of the
    /// daemon, so the client speaks plain HTTP.
    pub fn http(base: impl Into<String>) -> Self {
        Self::Http(base.into())
    }

    fn address(&self) -> Result<String, WireError> {
        match self {
            Self::Unix(path) => Ok(path.display().to_string()),
            Self::Http(base) => {
                let rest = base.strip_prefix("http://").ok_or_else(|| {
                    WireError::new(
                        ErrorKind::Invalid,
                        "invalid_endpoint",
                        format!("`{base}` is not an http:// address"),
                    )
                })?;
                Ok(rest.split('/').next().unwrap_or(rest).to_string())
            }
        }
    }

    fn host(&self) -> Result<String, WireError> {
        match self {
            Self::Unix(_) => Ok("localhost".to_string()),
            Self::Http(_) => self.address(),
        }
    }
}

/// One HTTP exchange as the daemon answered it.
pub struct RawResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Download,
}

impl RawResponse {
    /// Whether the daemon answered with a success status.
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Decodes the body as one DTO.
    pub async fn json<T: DeserializeOwned>(self) -> Result<T, WireError> {
        let bytes = self.body.into_bytes().await?;
        serde_json::from_slice(&bytes)
            .map_err(|error| WireError::internal(format!("the daemon sent invalid JSON: {error}")))
    }

    /// Turns a failed response into the daemon's error DTO.
    pub async fn error(self) -> WireError {
        let status = self.status;
        let bytes = match self.body.into_bytes().await {
            Ok(bytes) => bytes,
            Err(error) => return error,
        };
        #[derive(serde::Deserialize)]
        struct Envelope {
            error: WireError,
        }
        match serde_json::from_slice::<Envelope>(&bytes) {
            Ok(Envelope { error }) => error,
            Err(_) => WireError::new(
                ErrorKind::Internal,
                "unexpected_response",
                format!("the daemon answered HTTP {status} without an error body"),
            ),
        }
    }
}

/// A streaming response body.
pub struct Download {
    body: Incoming,
    finished: bool,
}

impl Download {
    /// Reads the next piece of the body, or `None` at the end.
    pub async fn read(&mut self) -> Result<Option<Bytes>, WireError> {
        if self.finished {
            return Ok(None);
        }
        match self.body.frame().await {
            None => {
                self.finished = true;
                Ok(None)
            }
            Some(Ok(frame)) => match frame.into_data() {
                Ok(bytes) => Ok(Some(bytes)),
                // Trailers carry nothing the API uses.
                Err(_) => Ok(Some(Bytes::new())),
            },
            Some(Err(error)) => Err(WireError::internal(format!(
                "reading the response body: {error}"
            ))),
        }
    }

    /// Reads the whole body into memory.
    pub async fn into_bytes(mut self) -> Result<Vec<u8>, WireError> {
        let mut out = Vec::new();
        while let Some(chunk) = self.read().await? {
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }
}

/// A transport-ready HTTP response, before the caller decides how to read it.
pub(crate) async fn send(
    endpoint: &Endpoint,
    token: Option<&str>,
    method: Method,
    path: &str,
    body: Option<Vec<u8>>,
    content_type: Option<&str>,
) -> Result<RawResponse, WireError> {
    let mut sender = connect(endpoint).await?;
    let mut builder = hyper::Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, endpoint.host()?);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let request = builder
        .body(Full::new(Bytes::from(body.unwrap_or_default())))
        .map_err(|error| WireError::internal(format!("building the request: {error}")))?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|error| transport_error(endpoint, &error))?;
    let (parts, body) = response.into_parts();
    Ok(RawResponse {
        status: parts.status,
        headers: parts.headers,
        body: Download {
            body,
            finished: false,
        },
    })
}

pub(crate) async fn open_stream(endpoint: &Endpoint) -> Result<Stream, WireError> {
    match endpoint {
        Endpoint::Unix(path) => Ok(Stream::Unix(
            UnixStream::connect(path)
                .await
                .map_err(|error| transport_error(endpoint, &error))?,
        )),
        Endpoint::Http(_) => Ok(Stream::Tcp(
            TcpStream::connect(endpoint.address()?)
                .await
                .map_err(|error| transport_error(endpoint, &error))?,
        )),
    }
}

async fn connect(endpoint: &Endpoint) -> Result<SendRequest<Full<Bytes>>, WireError> {
    let stream = open_stream(endpoint).await?;
    let (sender, connection) = handshake(TokioIo::new(stream))
        .await
        .map_err(|error| transport_error(endpoint, &error))?;
    tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
    });
    Ok(sender)
}

fn transport_error(endpoint: &Endpoint, error: &dyn std::fmt::Display) -> WireError {
    WireError::new(
        ErrorKind::Internal,
        "transport",
        format!(
            "talking to {}: {error}",
            endpoint.address().unwrap_or_default()
        ),
    )
}

/// One request body, already encoded as JSON.
pub(crate) fn json_body<T: Serialize>(value: &T) -> Result<Vec<u8>, WireError> {
    serde_json::to_vec(value)
        .map_err(|error| WireError::internal(format!("encoding the request: {error}")))
}

/// Encodes one query value, keeping `/` and the unreserved set readable.
pub fn encode(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push(HEX[(byte >> 4) as usize] as char);
                out.push(HEX[(byte & 0x0f) as usize] as char);
            }
        }
    }
    out
}

/// The raw byte stream, so the Unix and TCP branches can share one handshake.
pub(crate) enum Stream {
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Unix(stream) => Pin::new(stream).poll_read(cx, buffer),
            Stream::Tcp(stream) => Pin::new(stream).poll_read(cx, buffer),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Unix(stream) => Pin::new(stream).poll_write(cx, buffer),
            Stream::Tcp(stream) => Pin::new(stream).poll_write(cx, buffer),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Unix(stream) => Pin::new(stream).poll_flush(cx),
            Stream::Tcp(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Unix(stream) => Pin::new(stream).poll_shutdown(cx),
            Stream::Tcp(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}
