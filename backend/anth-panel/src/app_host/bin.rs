//! Proxy from a bin app's declared anchors to its Unix socket.
//!
//! The panel never starts the service and never lets it listen on TCP: the
//! grant freezes one absolute socket path, and only the anchors the grant names
//! reach it. The proxy strips everything that could carry panel authority
//! (cookies, authorization, hop-by-hop headers) and forwards only the method,
//! path, query, body and content type.

use std::path::Path;
use std::time::Duration;

use anth_wire::{ErrorKind, WireError};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::client::conn::http1::handshake;
use hyper::http::{HeaderValue, Request, header};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tokio::time::timeout;

use crate::app_host::AppReply;

/// Largest answer the panel accepts from a bin app.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Wall-clock bound on one anchor call, including the response body.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Forwards one already-authorized anchor request.
pub async fn call(
    socket: &Path,
    method: &str,
    path: &str,
    query: Option<&str>,
    body: Vec<u8>,
    content_type: Option<&str>,
) -> Result<AppReply, WireError> {
    check_method(method)?;
    let uri = match query.filter(|query| !query.is_empty()) {
        Some(query) => format!("{path}?{query}"),
        None => path.to_string(),
    };
    let stream = UnixStream::connect(socket).await.map_err(|error| {
        WireError::new(
            ErrorKind::Internal,
            "app_unavailable",
            format!("connecting to the app socket {}: {error}", socket.display()),
        )
    })?;
    let (mut sender, connection) = handshake(TokioIo::new(stream)).await.map_err(|error| {
        WireError::new(
            ErrorKind::Internal,
            "app_unavailable",
            format!("the app socket handshake failed: {error}"),
        )
    })?;
    tokio::spawn(async move {
        let _ = connection.await;
    });

    let mut builder = Request::builder()
        .method(method)
        .uri(&uri)
        .header(header::HOST, "localhost");
    if let Some(value) = content_type.and_then(|value| HeaderValue::from_str(value).ok()) {
        builder = builder.header(header::CONTENT_TYPE, value);
    }
    let request = builder
        .body(Full::new(Bytes::from(body)))
        .map_err(|error| WireError::internal(format!("building the app request: {error}")))?;

    timeout(TIMEOUT, async {
        let response = sender.send_request(request).await.map_err(|error| {
            WireError::new(
                ErrorKind::Internal,
                "app_unavailable",
                format!("the app did not answer: {error}"),
            )
        })?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let body = Limited::new(response.into_body(), MAX_RESPONSE_BYTES)
            .collect()
            .await
            .map_err(response_error)?
            .to_bytes()
            .to_vec();
        Ok::<_, WireError>(AppReply {
            status,
            content_type,
            body,
        })
    })
    .await
    .map_err(|_| {
        WireError::new(
            ErrorKind::Internal,
            "app_timeout",
            format!(
                "the app did not answer within {} seconds",
                TIMEOUT.as_secs()
            ),
        )
    })?
}

fn check_method(method: &str) -> Result<(), WireError> {
    if matches!(method, "GET" | "POST" | "PUT" | "PATCH" | "DELETE") {
        Ok(())
    } else {
        Err(WireError::invalid("that method is not an anchor method"))
    }
}

fn response_error(error: Box<dyn std::error::Error + Send + Sync>) -> WireError {
    let detail = error.to_string();
    if detail.contains("length limit") {
        WireError::new(
            ErrorKind::TooLarge,
            "app_response_too_large",
            format!("the app answer exceeds {MAX_RESPONSE_BYTES} bytes"),
        )
    } else {
        WireError::internal(format!("reading the app answer: {detail}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_anchor_methods_are_forwarded() {
        assert_eq!(check_method("TRACE").expect_err("TRACE").code, "invalid");
        assert!(check_method("CONNECT").is_err());
        assert!(check_method("GET").is_ok());
        assert!(check_method("DELETE").is_ok());
    }
}
