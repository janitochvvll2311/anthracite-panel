//! Contract for native (bin) services.
//!
//! A service implements [`Service`] and serves HTTP over a Unix socket with
//! [`serve_unix`]. The panel proxies only the routes declared in the app
//! manifest, so a service never listens on a public TCP port.

use std::path::Path;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::any;
use axum::{Router, serve};

/// One request forwarded by the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub body: Vec<u8>,
}

/// What the service answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: String,
}

impl Response {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self::json(200, body)
    }

    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "application/json".to_string(),
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into().into_bytes(),
            content_type: "text/plain; charset=utf-8".to_string(),
        }
    }

    pub fn error(status: u16, message: &str) -> Self {
        Self::json(status, serde_json::json!({ "error": message }).to_string())
    }
}

/// A native service.
pub trait Service: Send + Sync + 'static {
    fn handle(&self, request: Request) -> Response;
}

/// Serves `service` on a Unix socket.
///
/// The caller owns the socket path and must have removed a stale socket before
/// binding. The panel connects to this path only after the app is installed and
/// enabled.
pub async fn serve_unix<S: Service>(path: impl AsRef<Path>, service: S) -> std::io::Result<()> {
    let listener = tokio::net::UnixListener::bind(path)?;
    let router = Router::new()
        .fallback(any(dispatch::<S>))
        .with_state(Arc::new(service));
    serve(listener, router)
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))
}

async fn dispatch<S: Service>(
    State(service): State<Arc<S>>,
    request: axum::extract::Request,
) -> HttpResponse {
    let method = request.method().as_str().to_string();
    let path = request.uri().path().to_string();
    let query = request.uri().query().map(str::to_string);
    let body = match axum::body::to_bytes(request.into_body(), usize::MAX).await {
        Ok(bytes) => bytes.to_vec(),
        Err(error) => {
            return Response::text(400, format!("reading the request body: {error}")).into_http();
        }
    };
    let answer = service.handle(Request {
        method,
        path,
        query,
        body,
    });
    answer.into_http()
}

impl Response {
    fn into_http(self) -> HttpResponse {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = (status, Bytes::from(self.body)).into_response();
        if let Ok(value) = HeaderValue::from_str(&self.content_type) {
            response.headers_mut().insert(header::CONTENT_TYPE, value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    struct Echo;

    impl Service for Echo {
        fn handle(&self, request: Request) -> Response {
            Response::text(
                200,
                format!("{} {} {}", request.method, request.path, request.body.len()),
            )
        }
    }

    #[tokio::test]
    async fn a_service_receives_the_forwarded_request() {
        let router = Router::new()
            .fallback(any(dispatch::<Echo>))
            .with_state(Arc::new(Echo));
        let response = router
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/status?x=1")
                    .body(Body::from("hello"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        assert_eq!(&body[..], b"POST /status 5");
    }

    #[test]
    fn responses_carry_their_content_type() {
        let response = Response::json(200, "{}").into_http();
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }
}
