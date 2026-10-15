pub mod app;
pub mod cli;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::app::AppState;

pub fn router(app: AppState) -> Router {
    Router::new()
        .route("/healthz", get(get_healthz))
        .route("/command", post(post_command))
        .with_state(app)
}

pub async fn get_healthz() -> &'static str {
    "ok"
}

pub async fn post_command(
    State(state): State<AppState>,
    Json(request): Json<anth_proto::Request>,
) -> Result<Json<anth_proto::Response>, StatusCode> {
    let mut client = state
        .create_client()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    client
        .send_request(&request)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    match client.receive_response().await {
        Some(Ok(message)) => Ok(Json(message)),
        Some(Err(_)) => Err(StatusCode::BAD_REQUEST),
        None => Err(StatusCode::BAD_GATEWAY),
    }
}
