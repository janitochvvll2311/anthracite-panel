use axum::extract::{Query, State};
use axum::http::{Response, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router, body::Body};
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::Value;

use std::sync::Arc;

use crate::app::AppState;
use crate::poll;
use crate::sse::SseStream;
use crate::stream;
use anth_proto::{AnthClient, Request, Response as CommandResponse};

pub fn router(app: AppState) -> Router {
    Router::new()
        .route("/healthz", get(get_healthz))
        .route("/api/host-info", get(get_host_info))
        .route("/api/host-info/watch", get(watch_host_info))
        .route("/api/echo", post(post_echo))
        .fallback(static_handler)
        .with_state(app)
}

pub async fn get_healthz() -> &'static str {
    "ok"
}

/// Body of `POST /api/echo`. The command is implied by the route, so the
/// argument is a top-level member rather than nested under a tag.
#[derive(Debug, Deserialize)]
pub struct EchoArgs {
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct WatchQuery {
    interval: Option<String>,
}

/// `GET /api/host-info` — one snapshot.
pub async fn get_host_info(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let response = exchange(&state, &Request::HostInfo).await?;
    Ok(Json(host_info_json(response)?))
}

/// `GET /api/echo` — one echo.
pub async fn post_echo(
    State(state): State<AppState>,
    Json(args): Json<EchoArgs>,
) -> Result<Json<Value>, StatusCode> {
    let request = Request::Echo {
        message: args.message,
    };
    let response = exchange(&state, &request).await?;
    match response {
        CommandResponse::Ok { message } => Ok(Json(serde_json::json!({ "message": message }))),
        _ => Err(StatusCode::BAD_GATEWAY),
    }
}

/// `GET /api/host-info/watch?interval=2s` — a snapshot per interval as SSE.
pub async fn watch_host_info(
    State(state): State<AppState>,
    Query(query): Query<WatchQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let period = match query.interval.as_deref() {
        None => stream::DEFAULT_INTERVAL,
        Some(raw) => stream::parse_interval(raw).map_err(|e| (StatusCode::BAD_REQUEST, e))?,
    };

    let client = state.create_client().await.map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon unavailable".to_string(),
        )
    })?;

    // One socket for the whole stream, shared by successive reads.
    let client = Arc::new(tokio::sync::Mutex::new(client));
    let items = poll::stream(period, move || {
        let client = Arc::clone(&client);
        async move {
            let mut client = client.lock().await;
            query_command(&mut client, &Request::HostInfo)
                .await
                .and_then(host_info_json)
                .map_err(error_detail)
        }
    });

    Ok(SseStream::new("host-info").stream(items))
}

/// Sends one request and returns the daemon response, mapping failures to
/// status codes that a one-shot endpoint can still express.
async fn exchange(state: &AppState, request: &Request) -> Result<CommandResponse, StatusCode> {
    let mut client = state
        .create_client()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    query_command(&mut client, request).await
}

async fn query_command(
    client: &mut AnthClient,
    request: &Request,
) -> Result<CommandResponse, StatusCode> {
    client
        .send_request(request)
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    match client.receive_response().await {
        Some(Ok(response)) => Ok(response),
        Some(Err(_)) => Err(StatusCode::BAD_REQUEST),
        None => Err(StatusCode::BAD_GATEWAY),
    }
}

fn host_info_json(response: CommandResponse) -> Result<Value, StatusCode> {
    match response {
        CommandResponse::HostInfo(info) => Ok(Value::from(info.as_ref())),
        CommandResponse::Error { .. } => Err(StatusCode::BAD_GATEWAY),
        CommandResponse::Ok { .. } => Err(StatusCode::BAD_GATEWAY),
    }
}

/// Human-readable text for a status, used in SSE error events where only a
/// payload is possible.
fn error_detail(status: StatusCode) -> String {
    match status {
        StatusCode::SERVICE_UNAVAILABLE => "daemon unavailable".to_string(),
        StatusCode::BAD_GATEWAY => "daemon did not answer".to_string(),
        StatusCode::BAD_REQUEST => "daemon rejected the request".to_string(),
        other => other.to_string(),
    }
}

#[derive(RustEmbed)]
#[folder = "www"]
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
