pub mod app;
pub mod cli;

use axum::extract::State;
use axum::http::{Response, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{body::Body, Json, Router};
use rust_embed::RustEmbed;

use crate::app::AppState;

pub fn router(app: AppState) -> Router {
    Router::new()
        .route("/healthz", get(get_healthz))
        .route("/command", post(post_command))
        .fallback(static_handler)
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



#[derive(RustEmbed)]
#[folder = "webui/dist"]
struct Asset;

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Asset::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .header("content-type", mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            // SPA Fallback a index.html para rutas del cliente
            if let Some(content) = Asset::get("index.html") {
                Response::builder()
                    .header("content-type", "text/html")
                    .body(Body::from(content.data))
                    .unwrap()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
    }
}