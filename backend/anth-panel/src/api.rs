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
use crate::files;
use crate::plugins::registry::read_ui_module;

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
        .route("/api/files", get(file_read))
        .route("/api/plugins/{id}/call", axum::routing::post(plugin_call))
        // One namespace per job, so neither route has to guess what a path
        // meant: `/p/` serves the bundle's web modules and only those, and
        // `/api/plugins/{id}/` runs anchors and only those. v0.2.0's `/p/`
        // answered both, which made `/p/status` and `/p/<id>/status` mean
        // different things and left a module unreachable when an anchor
        // happened to share its name.
        .route("/p/{*path}", get(web_module_path))
        .route("/api/plugins/{id}/{*path}", anchors())
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

/// `/p/<id-or-alias>/<module>`, split into its parts.
///
/// The route is a catch-all because axum refuses to register `/p/{id}/{*module}`
/// beside it, so the split happens here. `id` is resolved by the registry, which
/// accepts the alias too, and `module` is kept exactly as the URL spelled it:
/// it is compared against the manifest's own `module` string, so anything that
/// rewrote it on the way through would have the URL and the declaration
/// disagree about the same file.
struct PluginFile<'a> {
    id: &'a str,
    module: &'a str,
}

fn split_plugin_file(path: &str) -> Option<PluginFile<'_>> {
    let (id, module) = path.strip_prefix('/')?.split_once('/')?;
    let (id, module) = (id.trim(), module.trim());
    (!id.is_empty() && !module.is_empty()).then_some(PluginFile { id, module })
}

/// Serves the web modules a bundle declared, and nothing else.
///
/// This namespace is the one a browser loads a widget from, so it holds only
/// files: an anchor is an operation and lives under `/api/plugins/{id}/`. The
/// separation is what makes `/p/status` unambiguously "no module called
/// `status`" instead of sometimes an anchor and sometimes a missing file.
async fn web_module_path(
    State(state): State<AppState>,
    axum::extract::Path(path): axum::extract::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let path = format!("/{path}");
    let file = split_plugin_file(&path).ok_or_else(|| {
        // `/p/app.js` names a file but no plugin. Saying "no anchor" here would
        // describe a namespace this route does not serve.
        ApiError(WireError::new(
            ErrorKind::NotFound,
            format!("no plugin in `{path}`: expected `/p/<id-or-alias>/<module>`"),
        ))
    })?;
    web_module(state, file).await
}

/// The anchor route, answering every method a manifest may declare.
///
/// `any()` rather than `get()`: the manifest accepts `GET`, `POST`, `PUT`,
/// `PATCH` and `DELETE`, and a route that registered only `GET` answered a
/// declared `POST` anchor with `405` — the plugin advertised an operation at
/// startup that no URL could reach.
fn anchors() -> axum::routing::MethodRouter<AppState> {
    axum::routing::any(scoped_anchor)
}

/// Serves an anchor addressed by plugin id and path.
///
/// This is the route [`@anth/plugin`'s `PluginEndpoints`] builds, and it exists
/// because the id-scoped form is the one a widget can use: the widget knows
/// which plugin it belongs to, so two plugins declaring one path is not its
/// problem. `{id}` accepts the plugin's alias too — the registry resolves it,
/// and the response is the canonical plugin's. An id that is not installed and
/// a path that plugin does not declare are both a `404`, so the route cannot be
/// used to enumerate other plugins' anchors.
///
/// The method and body are the caller's: the plugin's `from_http` map reads
/// `method`, `path`, `query` and `body`, so anything the panel rewrote here
/// would reach the plugin as a request nobody sent. Only `GET` carries no body,
/// and an absent one is `null` rather than `""` so a plugin can tell "no body"
/// from "an empty body".
async fn scoped_anchor(
    State(state): State<AppState>,
    method: axum::http::Method,
    axum::extract::Path((id, path)): axum::extract::Path<(String, String)>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    body: axum::body::Bytes,
) -> Result<HttpResponse, ApiError> {
    let path = format!("/{path}");
    let (entry, _loaded) = state
        .0
        .anchor_for(&id, method.as_str(), &path)
        .map_err(ApiError)?
        .ok_or_else(|| {
            ApiError(WireError::new(
                ErrorKind::NotFound,
                format!("no anchor {} {path} in plugin `{id}`", method.as_str()),
            ))
        })?;
    let id = entry.id.clone();

    let payload = http_payload(method.as_str(), path, query.as_deref(), &body)?;

    forward(state, id, PluginPipe::Http, payload.to_string()).await
}

/// The `anth_http` envelope for one anchor request.
///
/// One place builds it, so the router cannot answer two transports differently
/// and a test can assert the exact shape a plugin's `from_http` will read.
pub(crate) fn http_payload(
    method: &str,
    path: String,
    query: Option<&str>,
    body: &[u8],
) -> Result<serde_json::Value, ApiError> {
    Ok(serde_json::json!({
        "method": method,
        "path": path,
        "query": parse_query(query),
        "body": body_text(body)?,
    }))
}

/// The anchor's body as the plugin's `from_http` reads it.
///
/// `null` for an absent body and the raw text otherwise, never a re-encoding:
/// the plugin decides whether its body is JSON, so a panel that parsed and
/// re-serialized would reject a payload the plugin could have handled and
/// change the bytes on the way for one it could not. A body that is not UTF-8
/// is refused rather than lossily converted, because `HttpRequest::body` is a
/// `String` and handing the guest replacement characters it never sent would
/// be a silent rewrite of the caller's request.
fn body_text(body: &[u8]) -> Result<serde_json::Value, ApiError> {
    if body.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    match std::str::from_utf8(body) {
        Ok(text) => Ok(serde_json::Value::String(text.to_string())),
        Err(error) => Err(ApiError(WireError::invalid(format!(
            "the anchor body is not UTF-8 at byte {}",
            error.valid_up_to()
        )))),
    }
}

/// Serves one UI module a bundle declared.
///
/// The answer is the file's bytes with a type by extension, not a plugin call:
/// a module is an asset the daemon reads, not something the guest runs, so it
/// needs no sandbox and no transport. It is still gated on the plugin being
/// usable: a disabled bundle never produced a validated manifest, so it has no
/// declaration to serve and no claim worth trusting.
async fn web_module(state: AppState, web: PluginFile<'_>) -> Result<HttpResponse, ApiError> {
    let known = state.0.has_plugin(web.id).map_err(ApiError)?;
    let entry = state
        .0
        .ui_module(web.id, web.module)
        .map_err(ApiError)?
        .ok_or_else(|| {
            let message = if known {
                format!("plugin `{}` declares no ui module `{}`", web.id, web.module)
            } else {
                // `/p/greeting/x` names no plugin at all; answering "declares
                // no module" would blame a plugin that does not exist.
                format!("no plugin `{}`", web.id)
            };
            ApiError(WireError::new(ErrorKind::NotFound, message))
        })?;

    let bytes = read_ui_module(entry, web.module).ok_or_else(|| {
        ApiError(WireError::new(
            ErrorKind::NotFound,
            format!(
                "plugin `{}` declares `{}` but the bundle does not hold it",
                entry.id, web.module
            ),
        ))
    })?;

    Ok(file_into_response(crate::www::Asset {
        bytes: std::borrow::Cow::Owned(bytes),
        content_type: crate::www::content_type_for(std::path::Path::new(web.module)),
    }))
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

/// One slice of a readable file.
///
/// The query names what the CLI names on its own command line, so the two
/// transports cannot disagree about the defaults. `len` above the cap is
/// refused by `files::read` rather than clamped here: a route that quietly
/// shortened a read would hide the limit.
async fn file_read(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    let path = params.get("path").cloned().ok_or_else(|| {
        ApiError(WireError::invalid(
            "`path` is required, as ?path=<file>".to_string(),
        ))
    })?;
    let offset = number_param(&params, "offset", 0)?;
    let len = number_param(&params, "len", u64::from(files::MAX_SLICE))?;

    if len > u64::from(u32::MAX) {
        return Err(ApiError(WireError::invalid(format!(
            "`len` of {len} does not fit a slice"
        ))));
    }

    let response = state
        .0
        .handle(Request::FileRead {
            path,
            offset,
            len: len as u32,
        })
        .await;

    match response {
        Response::FileSlice {
            path,
            offset,
            bytes,
            next,
            eof,
        } => {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            Ok(axum::Json(json!({
                "path": path,
                "offset": offset,
                "bytes": encoded,
                "next": next,
                "eof": eof,
            })))
        }
        Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "a file read answered unexpectedly: {other:?}"
        )))),
    }
}

/// One numeric query parameter, or `fallback` when absent.
///
/// A present but unparseable value is an error rather than the fallback: a
/// caller who wrote `?len=abc` meant something, and answering with the default
/// would answer a different question.
fn number_param(
    params: &std::collections::HashMap<String, String>,
    name: &str,
    fallback: u64,
) -> Result<u64, ApiError> {
    match params.get(name) {
        None => Ok(fallback),
        Some(value) => value.parse::<u64>().map_err(|_| {
            ApiError(WireError::invalid(format!(
                "`{name}` is `{value}`, which is not a whole number"
            )))
        }),
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
