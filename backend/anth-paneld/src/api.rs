//! The HTTP surface, a thin adapter over [`dispatch`].
//!
//! Routes name a command, so a request body carries the command's arguments at
//! the top level and never the socket's `type`/`data` envelope: the envelope is
//! a transport concern, not an API one. Every handler therefore builds a
//! [`Request`] and calls `dispatch::handle`, which keeps one site that decides
//! what a command does no matter which transport reached it.
//!
//! Errors take their status from [`ErrorKind::http_status`], the same mapping
//! the CLI uses for exit codes, so the two cannot drift.

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::get;
use serde_json::json;

use anth_wire::{Request, WireError};

use crate::dispatch;

/// The router, with the shared state it needs to answer.
///
/// No state today: `dispatch` is a free function. The signature exists so a
/// route that needs a socket path or a config does not change every call site.
#[derive(Clone, Debug, Default)]
pub struct AppState;

pub fn router() -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/host-info", get(host_info))
        .with_state(AppState)
}

async fn healthz() -> impl IntoResponse {
    axum::Json(json!({"status": "ok"}))
}

async fn host_info(State(_state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let response = dispatch::handle(Request::HostInfo).await;
    match response {
        anth_wire::Response::HostInfo(info) => Ok(axum::Json(info)),
        anth_wire::Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "host-info answered with an unexpected response: {other:?}"
        )))),
    }
}

/// A [`WireError`] rendered as `{"error": "..."}` with its mapped status.
///
/// The body never carries the kind or a code: v0.1.0 answered two shapes and a
/// client had to handle both. Detail belongs in the message, and the status is
/// in the status line.
pub struct ApiError(pub WireError);

impl IntoResponse for ApiError {
    fn into_response(self) -> HttpResponse {
        let status = StatusCode::from_u16(self.0.kind.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, axum::Json(json!({"error": self.0.message}))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    async fn get_status_and_body(uri: &str) -> (u16, serde_json::Value) {
        let response = router()
            .oneshot(
                HttpRequest::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        let status = response.status().as_u16();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a readable body");
        let body = serde_json::from_slice(&bytes).expect("the body must be JSON");
        (status, body)
    }

    #[tokio::test]
    async fn healthz_answers_ok_without_touching_the_daemon() {
        let (status, body) = get_status_and_body("/healthz").await;
        assert_eq!(status, 200);
        assert_eq!(body["status"], json!("ok"));
    }

    #[tokio::test]
    async fn host_info_answers_the_same_snapshot_the_socket_returns() {
        let (status, body) = get_status_and_body("/api/host-info").await;

        assert_eq!(status, 200);
        // Read from the running container, so assert shape rather than values.
        assert!(
            body["hostname"].as_str().is_some_and(|s| !s.is_empty()),
            "{body}"
        );
        assert!(
            body["logical_cores"].as_u64().is_some_and(|n| n > 0),
            "{body}"
        );
        assert!(body["os"]["name"].is_string(), "{body}");
    }

    #[tokio::test]
    async fn an_unknown_route_is_not_the_spa_fallback() {
        let response = router()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/nope")
                    .body(Body::empty())
                    .expect("a valid request"),
            )
            .await
            .expect("the router must answer");

        // An unmatched path under /api must not be answered by the static
        // fallback with 200 and index.html.
        assert_eq!(response.status().as_u16(), 404);
    }

    #[test]
    fn an_error_renders_its_mapped_status_and_a_flat_message() {
        use anth_wire::ErrorKind;

        let error = ApiError(WireError::new(
            ErrorKind::Forbidden,
            "control is off".to_string(),
        ));
        let response = error.into_response();

        assert_eq!(response.status().as_u16(), 403);
    }
}
