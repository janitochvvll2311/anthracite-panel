//! The engine's HTTP surface.
//!
//! The router is a library value on purpose: the official panel mounts it in its
//! own process (§6.5), and a headless deployment serves it directly.
//!
//! Two routers: the full one, meant for the Unix socket where a peer credential
//! exists, and the public one, which is all a TCP listener serves until sessions
//! arrive. `/health` is public on both; everything under `/api` needs a peer.

use std::sync::Arc;

use anth_contract::Error;
use axum::extract::{connect_info::Connected, ConnectInfo, Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio::net::UnixListener;

/// What the router needs from the engine.
#[derive(Debug, Clone)]
pub struct EngineState {
    pub version: String,
    pub protocol: u32,
    /// The uid that owns the daemon; root is always accepted as well.
    pub allowed_uid: u32,
}

impl EngineState {
    pub fn new(allowed_uid: u32) -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: 1,
            allowed_uid,
        }
    }
}

/// A marker the embedding host sets once it has authenticated a request.
///
/// A panel that mounts this router receives browser requests that carry no peer
/// credential. It authenticates them and marks the request; the router still owns
/// the decision of what an authenticated request may reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trusted;

/// The uid the kernel reports for a socket connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerUid(pub u32);

impl Connected<axum::serve::IncomingStream<'_, UnixListener>> for PeerUid {
    fn connect_info(stream: axum::serve::IncomingStream<'_, UnixListener>) -> Self {
        let uid = stream
            .io()
            .peer_cred()
            .map(|credential| credential.uid())
            .unwrap_or(u32::MAX);
        Self(uid)
    }
}

/// The full surface, for the Unix socket.
pub fn router(state: Arc<EngineState>) -> Router {
    let api = Router::new()
        .route("/health", get(readiness))
        .route("/engine/version", get(version))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_peer));

    Router::new()
        .route("/health", get(health))
        .nest("/api", api)
        .with_state(state)
}

/// Only what a TCP listener may serve before sessions exist.
pub fn public_router() -> Router {
    Router::new().route("/health", get(health))
}

/// Liveness: 200 once the socket is bound, nothing else.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// Readiness: 200 once the surface can serve. There is nothing to start yet, so
/// bound means ready; the moment apps load this answers `503 starting` first.
async fn readiness() -> Json<Value> {
    Json(json!({ "status": "ready" }))
}

/// Readiness and the protocol version.
async fn version(State(state): State<Arc<EngineState>>) -> Json<Value> {
    Json(json!({
        "engine": state.version,
        "protocol": state.protocol,
    }))
}

/// Every `/api` route needs a peer credential from the service user or root.
async fn require_peer(
    State(state): State<Arc<EngineState>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if request.extensions().get::<Trusted>().is_some() {
        return Ok(next.run(request).await);
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<PeerUid>>()
        .map(|ConnectInfo(uid)| uid.0);
    match peer {
        None => Err(ApiError(Error::unauthenticated(
            "this surface needs a local peer credential",
        ))),
        Some(uid) if uid == 0 || uid == state.allowed_uid => Ok(next.run(request).await),
        Some(_) => Err(ApiError(Error::unauthenticated(
            "the peer is not the service user or root",
        ))),
    }
}

/// An error rendered with the one envelope of §7.11.
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self.0.envelope())).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn state_with(uid: u32) -> Arc<EngineState> {
        Arc::new(EngineState::new(uid))
    }

    fn state() -> Arc<EngineState> {
        state_with(1000)
    }

    /// The uid this test process runs as. CI does not run as root, so a fixed
    /// number here would pass locally and fail there.
    fn current_euid() -> u32 {
        // SAFETY: `geteuid` has no preconditions.
        unsafe { libc::geteuid() }
    }

    fn request(uri: &str, peer: Option<u32>) -> Request<Body> {
        let mut request = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("request");
        if let Some(uid) = peer {
            request.extensions_mut().insert(ConnectInfo(PeerUid(uid)));
        }
        request
    }

    async fn status(uri: &str, peer: Option<u32>) -> StatusCode {
        router(state())
            .oneshot(request(uri, peer))
            .await
            .expect("response")
            .status()
    }

    #[tokio::test]
    async fn health_is_public() {
        assert_eq!(status("/health", None).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn the_api_needs_a_peer_credential() {
        assert_eq!(status("/api/health", None).await, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn the_service_user_and_root_are_accepted() {
        assert_eq!(status("/api/health", Some(1000)).await, StatusCode::OK);
        assert_eq!(status("/api/health", Some(0)).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn readiness_is_a_different_word_than_liveness() {
        let response = router(state())
            .oneshot(request("/api/health", Some(1000)))
            .await
            .expect("response");
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["status"], "ready");
    }

    #[tokio::test]
    async fn a_trusted_in_process_request_is_accepted() {
        let mut request = request("/api/health", None);
        request.extensions_mut().insert(Trusted);
        let response = router(state()).oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn a_real_socket_connection_carries_the_peer_uid() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let dir = std::env::temp_dir().join(format!("anth-api-uds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("engine.sock");

        let listener = UnixListener::bind(&path).expect("bind");
        let app =
            router(state_with(current_euid())).into_make_service_with_connect_info::<PeerUid>();
        let server = tokio::spawn(async move { axum::serve(listener, app).await });

        let mut stream = tokio::net::UnixStream::connect(&path)
            .await
            .expect("connect");
        stream
            .write_all(b"GET /api/health HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .await
            .expect("write");
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.expect("read");
        let text = String::from_utf8_lossy(&response);
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(text.contains("ready"), "{text}");

        server.abort();
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[tokio::test]
    async fn any_other_peer_is_refused() {
        assert_eq!(
            status("/api/health", Some(4242)).await,
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn version_reports_the_protocol() {
        let response = router(state())
            .oneshot(request("/api/engine/version", Some(1000)))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(value["protocol"], 1);
    }

    #[tokio::test]
    async fn the_public_router_serves_only_health() {
        let public = public_router();
        let ok = public
            .clone()
            .oneshot(request("/health", None))
            .await
            .expect("response");
        assert_eq!(ok.status(), StatusCode::OK);
        let missing = public
            .oneshot(request("/api/health", None))
            .await
            .expect("response");
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    }
}
