use axum::extract::rejection::{JsonRejection, QueryRejection};
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

/// Every API failure, serialized as `{"error": "..."}` so clients parse one
/// shape regardless of which layer rejected the request.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    detail: String,
}

impl ApiError {
    fn new(status: StatusCode, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }

    fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, detail)
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.detail })),
        )
            .into_response()
    }
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
pub async fn get_host_info(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let response = exchange(&state, &Request::HostInfo).await?;
    host_info_json(response).map(Json)
}

/// `GET /api/echo` — one echo.
pub async fn post_echo(
    State(state): State<AppState>,
    payload: Result<Json<EchoArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;

    let request = Request::Echo {
        message: args.message,
    };
    match exchange(&state, &request).await? {
        CommandResponse::Ok { message } => Ok(Json(serde_json::json!({ "message": message }))),
        other => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected daemon response: {other:?}"),
        )),
    }
}

/// `GET /api/host-info/watch?interval=2s` — a snapshot per interval as SSE.
pub async fn watch_host_info(
    State(state): State<AppState>,
    query: Result<Query<WatchQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;

    let period = match query.interval.as_deref() {
        None => stream::DEFAULT_INTERVAL,
        Some(raw) => stream::parse_interval(raw).map_err(ApiError::bad_request)?,
    };

    let client = state
        .create_client()
        .await
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "daemon unavailable"))?;

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
async fn exchange(state: &AppState, request: &Request) -> Result<CommandResponse, ApiError> {
    let mut client = state
        .create_client()
        .await
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "daemon unavailable"))?;
    query_command(&mut client, request).await
}

async fn query_command(
    client: &mut AnthClient,
    request: &Request,
) -> Result<CommandResponse, ApiError> {
    client
        .send_request(request)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("send failed: {e}")))?;

    match client.receive_response().await {
        Some(Ok(response)) => Ok(response),
        Some(Err(e)) => Err(ApiError::bad_request(format!("malformed response: {e}"))),
        None => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "daemon closed the connection",
        )),
    }
}

fn host_info_json(response: CommandResponse) -> Result<Value, ApiError> {
    match response {
        CommandResponse::HostInfo(info) => Ok(Value::from(info.as_ref())),
        CommandResponse::Error { detail } => Err(ApiError::new(StatusCode::BAD_GATEWAY, detail)),
        CommandResponse::Ok { message } => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected daemon response: {message}"),
        )),
        // No REST route opens a terminal yet; a one-shot route could not carry
        // the stream anyway.
        CommandResponse::TerminalOpened(opened) => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!(
                "unexpected terminal response on a one-shot route: {}",
                opened.id
            ),
        )),
        CommandResponse::TerminalOutput { id, .. } => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected terminal output for session {id} on a one-shot route"),
        )),
        CommandResponse::TerminalExited { id, status } => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected terminal exit for session {id} (status {status})"),
        )),
    }
}

/// Text for an SSE error frame, where only a payload is possible.
fn error_detail(error: ApiError) -> String {
    error.detail
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
