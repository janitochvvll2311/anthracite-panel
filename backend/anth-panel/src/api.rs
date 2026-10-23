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
//!
//! A plugin's *own* failure is not an HTTP failure: the route answers the
//! plugin's status and value, because a `404` from an anchor is the plugin
//! saying "I have no such thing", not the panel saying "I have no such route".
//!
//! The shell is served from the same router, and an unknown `/api` path is a
//! JSON `404` rather than the index: answering `200 text/html` for a typo makes
//! a broken client look like a working route until the JSON parse fails.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::get;
use serde_json::json;

use anth_wire::{ErrorKind, PluginPipe, Request, Response, WireError};

use crate::dispatch::Daemon;

/// The router, with the daemon it answers from.
///
/// The daemon is `Arc`ed rather than cloned per request: it owns the compiled
/// wasm modules, and recompiling one per request would put that cost on the
/// request path.
#[derive(Clone)]
pub struct AppState(Arc<Daemon>);

impl AppState {
    pub fn new(daemon: Arc<Daemon>) -> Self {
        Self(daemon)
    }
}

pub fn router(daemon: Arc<Daemon>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/host-info", get(host_info))
        .route("/api/plugins", get(plugins))
        .route("/api/plugins/{id}/call", axum::routing::post(plugin_call))
        .route("/p/{*path}", get(anchor))
        // The browser's form: it names its plugin, so two plugins declaring one
        // path are not a conflict here. `/p/` stays for a path-only caller.
        .route("/api/plugins/{id}/{*path}", get(scoped_anchor))
        // The shell. `/` is separate because a catch-all does not match a bare
        // root, so leaving it out answers the front page with a `404` while
        // every asset beside it loads.
        .route("/", get(index))
        .route("/{*path}", get(asset))
        .with_state(AppState::new(daemon))
}

async fn healthz() -> impl IntoResponse {
    axum::Json(json!({"status": "ok"}))
}

async fn host_info(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    let response = state.0.handle(Request::HostInfo).await;
    match response {
        anth_wire::Response::HostInfo(info) => Ok(axum::Json(info)),
        anth_wire::Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "host-info answered with an unexpected response: {other:?}"
        )))),
    }
}

/// Serves an anchor a plugin declared in its manifest.
///
/// The route is resolved through the registry rather than parsed: a path that
/// no plugin declares is a `404`, and two plugins declaring one path is a
/// conflict the operator has to resolve. The query string becomes the anchor's
/// query, so a plugin that reads `?name=Ana` gets it without the panel knowing
/// what a name is.
async fn anchor(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<HttpResponse, ApiError> {
    let path = format!("/{path}");
    let (entry, _loaded) = state
        .0
        .anchor_owner("GET", &path)
        .map_err(ApiError)?
        .ok_or_else(|| {
            ApiError(WireError::new(
                ErrorKind::NotFound,
                format!("no anchor {path}"),
            ))
        })?;
    let id = entry.id.clone();

    let body = serde_json::json!({
        "method": "GET",
        "path": path,
        "query": parse_query(query.as_deref()),
        "body": serde_json::Value::Null,
    });

    forward(state, id, PluginPipe::Http, body.to_string()).await
}

/// Serves an anchor addressed by plugin id and path.
///
/// This is the route [`@anth/plugin`'s `PluginEndpoints`] builds, and it exists
/// because the unscoped form cannot be used by a widget: two plugins may declare
/// the same path, and a widget knows which plugin it belongs to. `{id}` accepts
/// the plugin's alias too — the registry resolves it, and the response is the
/// canonical plugin's. An id that is not installed and a path that plugin does
/// not declare are both a `404`, so the route cannot be used to enumerate other
/// plugins' anchors.
async fn scoped_anchor(
    State(state): State<AppState>,
    axum::extract::Path((id, path)): axum::extract::Path<(String, String)>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Result<HttpResponse, ApiError> {
    let path = format!("/{path}");
    let (entry, _loaded) = state
        .0
        .anchor_for(&id, "GET", &path)
        .map_err(ApiError)?
        .ok_or_else(|| {
            ApiError(WireError::new(
                ErrorKind::NotFound,
                format!("no anchor {path} in plugin `{id}`"),
            ))
        })?;
    let id = entry.id.clone();

    let body = serde_json::json!({
        "method": "GET",
        "path": path,
        "query": parse_query(query.as_deref()),
        "body": serde_json::Value::Null,
    });

    forward(state, id, PluginPipe::Http, body.to_string()).await
}

/// Turns `?a=1&b=two` into the map the HTTP entry expects.
///
/// `anth_plugin::HttpRequest::query` is a `BTreeMap<String, String>`, so passing
/// the raw query string made every anchor answer `400 not an HTTP request`.
/// Percent-decoding is deliberately not done here: the plugin receives what the
/// client wrote, and a decoder in the panel would be a second place for the same
/// bug to live.
pub(crate) fn parse_query(raw: Option<&str>) -> serde_json::Map<String, serde_json::Value> {
    let mut query = serde_json::Map::new();
    for pair in raw.unwrap_or_default().split('&').filter(|p| !p.is_empty()) {
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        query.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
    query
}

/// Sends one call and turns the plugin's status into the response status.
async fn forward(
    state: AppState,
    id: String,
    pipe: PluginPipe,
    payload: String,
) -> Result<HttpResponse, ApiError> {
    match state
        .0
        .handle(Request::PluginCall { id, pipe, payload })
        .await
    {
        Response::PluginReply { status, value, .. } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            Ok((status, axum::Json(value)).into_response())
        }
        Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "the plugin answered unexpectedly: {other:?}"
        )))),
    }
}

/// Every plugin, including the disabled ones.
async fn plugins(State(state): State<AppState>) -> Result<impl IntoResponse, ApiError> {
    match state.0.handle(Request::PluginList).await {
        Response::PluginList { plugins } => Ok(axum::Json(json!({ "plugins": plugins }))),
        Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "the plugin list answered unexpectedly: {other:?}"
        )))),
    }
}

/// Runs one plugin pipe.
///
/// The body carries the command's arguments at the top level, like every other
/// route here, and `pipe` defaults to `direct` because that is the one that
/// needs no translation: the caller already speaks the plugin's vocabulary.
/// `{id}` is the plugin's id or the alias it declared; the daemon resolves
/// either before forwarding, so the plugin never learns which was used.
async fn plugin_call(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Option<axum::Json<CallBody>>,
) -> Result<HttpResponse, ApiError> {
    let call = body.map(|axum::Json(body)| body).unwrap_or_default();
    // The plugin's status is the answer's status. A non-2xx here is the plugin's
    // outcome, delivered faithfully rather than reclassified: a caller that
    // asked the plugin directly would see the same code.
    forward(state, id, call.pipe, call.payload).await
}

#[derive(serde::Deserialize, Default)]
struct CallBody {
    #[serde(default)]
    pipe: PluginPipe,
    /// The payload in the shape the chosen pipe carries. A `direct` call sends
    /// `{"action":..,"value":..}` here; a `cli` call sends the command line.
    #[serde(default)]
    payload: String,
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

/// Answers the shell's index.
async fn index() -> HttpResponse {
    shell("index.html")
}

/// Serves `path`, falling back to the index for a browser route.
///
/// A path that looks like a file and is not there is a `404`: handing back the
/// index would give a client HTML where it asked for a module, and the parse
/// error would name the module rather than the missing request.
async fn asset(axum::extract::Path(path): axum::extract::Path<String>) -> HttpResponse {
    if path == "api" || path.starts_with("api/") {
        return ApiError(WireError::new(
            ErrorKind::NotFound,
            "no such API route".to_string(),
        ))
        .into_response();
    }
    match crate::www::get(&path) {
        Some(file) => file_into_response(file),
        None if crate::www::looks_like_file(&path) => ApiError(WireError::new(
            ErrorKind::NotFound,
            format!("no such asset `{path}`"),
        ))
        .into_response(),
        None => shell("index.html"),
    }
}

/// One file, or the answer that names what is actually missing.
///
/// A build with no shell and a build whose index is missing are different
/// faults, and both differ from a typo in a path, so the message says which.
fn shell(path: &str) -> HttpResponse {
    match crate::www::get(path) {
        Some(file) => file_into_response(file),
        None => shell_missing(crate::www::available(), path).into_response(),
    }
}

/// The error for a path under `www/` that answered nothing.
///
/// Pure, and so testable: the two causes need different advice, and a test that
/// asked a real build to be in one of those states would pass or fail on the
/// machine it ran on rather than on the code.
pub(crate) fn shell_missing(shell_present: bool, path: &str) -> ApiError {
    if shell_present {
        ApiError(WireError::new(
            ErrorKind::NotFound,
            format!("the shell has no `{path}`"),
        ))
    } else {
        ApiError(WireError::new(
            ErrorKind::NotFound,
            "this build has no shell; run `bun run build` in frontend/".to_string(),
        ))
    }
}

fn file_into_response(file: crate::www::Asset) -> HttpResponse {
    HttpResponse::builder()
        .header("content-type", file.content_type)
        .body(Body::from(file.bytes.into_owned()))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}
