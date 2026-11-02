//! The engine's HTTP surface.
//!
//! The router is a library value on purpose: the official panel mounts it in its
//! own process (§6.5), and a headless deployment serves it directly. Nothing
//! here knows about panels, slots or features.

use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

/// What the router needs from the engine.
#[derive(Debug, Clone)]
pub struct EngineState {
    /// The engine's version, answered by `/api/engine/version`.
    pub version: String,
    /// The API protocol; see §6.4.
    pub protocol: u32,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: 1,
        }
    }
}

/// Builds the engine router.
pub fn router(state: Arc<EngineState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .with_state(state)
}

/// Liveness: 200 once the socket is bound, nothing else.
async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

/// A small helper so callers do not build the state by hand.
pub fn state() -> Arc<EngineState> {
    Arc::new(EngineState::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_answers_ok() {
        let response = router(state())
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn an_unknown_path_is_not_found() {
        let response = router(state())
            .oneshot(Request::builder().uri("/nope").body(Body::empty()).unwrap())
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn the_protocol_is_one() {
        assert_eq!(EngineState::default().protocol, 1);
    }
}
