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

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
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
pub struct AppState {
    daemon: Arc<Daemon>,
    auth: Arc<crate::auth::guard::Auth>,
}

impl AppState {
    pub fn new(daemon: Arc<Daemon>, auth: Arc<crate::auth::guard::Auth>) -> Self {
        Self { daemon, auth }
    }

    pub fn daemon(&self) -> &Arc<Daemon> {
        &self.daemon
    }

    /// The one authentication state every protected route asks.
    pub fn auth(&self) -> &Arc<crate::auth::guard::Auth> {
        &self.auth
    }
}

pub fn router(daemon: Arc<Daemon>, auth: Arc<crate::auth::guard::Auth>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        // The auth routes are the only `/api/` paths a caller may reach without
        // a token: they are how a token is obtained. Every other route below
        // names an [`Authed`] extractor in its signature, and that is what
        // protects it — a route added without one is visibly unprotected in the
        // one line that declares it.
        .route("/api/auth/state", get(auth_state))
        .route("/api/auth/register", axum::routing::post(auth_register))
        .route("/api/auth/login", axum::routing::post(auth_login))
        .route("/api/auth/refresh", axum::routing::post(auth_refresh))
        .route("/api/auth/logout", axum::routing::post(auth_logout))
        .route("/api/auth/password", axum::routing::post(auth_password))
        .route("/api/restart", axum::routing::post(restart))
        .route("/api/reboot", axum::routing::post(reboot))
        .route("/api/host-info", get(host_info))
        .route("/api/host-info/watch", get(watch_host_info))
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
        .with_state(AppState::new(daemon, auth))
}

/// Splits a request into its parts and a decoded body.
///
/// The credential routes need both: a body for the fields, and the parts for the
/// `Origin` check and the cookies. Extracting `Json` directly would consume the
/// request before the handler could look at its headers.
async fn decode<T: serde::de::DeserializeOwned>(
    request: axum::extract::Request,
) -> Result<(axum::http::request::Parts, T), ApiError> {
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(body, MAX_BODY_BYTES)
        .await
        .map_err(|error| {
            ApiError(WireError::invalid(format!(
                "the request body could not be read: {error}"
            )))
        })?;
    let value = if bytes.is_empty() {
        serde_json::from_str("{}").expect("an empty object always parses")
    } else {
        serde_json::from_slice(&bytes).map_err(|error| {
            ApiError(WireError::invalid(format!(
                "the request body is not JSON: {error}"
            )))
        })?
    };
    Ok((parts, value))
}

/// How large a credential body may be. A username and a password are bytes; a
/// megabyte of them is an attack, not a login.
const MAX_BODY_BYTES: usize = 8 * 1024;

async fn healthz() -> impl IntoResponse {
    axum::Json(json!({"status": "ok"}))
}

/// The body the credential routes accept.
///
/// One struct with optional fields rather than one per route: `register` and
/// `login` take a username and a password, `refresh` may take a token, and a
/// route that finds a field it needs missing says which. Three structs would let
/// a route decode a body meant for another without noticing.
#[derive(serde::Deserialize)]
struct Credentials {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    /// Accepted from the body so a script need not juggle cookies; the browser
    /// leaves it out and the cookie is read instead.
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    current_password: Option<String>,
    #[serde(default)]
    new_password: Option<String>,
}

impl Credentials {
    fn required(value: &Option<String>, name: &str) -> Result<String, ApiError> {
        value
            .clone()
            .filter(|text| !text.is_empty())
            .ok_or_else(|| {
                ApiError::auth(crate::auth::AuthError::invalid(format!(
                    "`{name}` is required"
                )))
            })
    }
}

/// A grant's JSON body, plus the two cookies that carry the same thing.
///
/// Both: a browser has no use for the token in the body and a script has no use
/// for the cookie, and answering one shape only would force the other caller to
/// reimplement a header.
fn grant_response(status: StatusCode, grant: crate::auth::session::Grant) -> HttpResponse {
    let mut response = (status, axum::Json(&grant)).into_response();
    for cookie in crate::auth::guard::set_cookies(&grant) {
        if let Ok(value) = axum::http::HeaderValue::from_str(&cookie) {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }
    response
}

/// The response that ends a session: the answer, and cookies that are gone.
fn ended(status: StatusCode, body: serde_json::Value) -> HttpResponse {
    let mut response = (status, axum::Json(body)).into_response();
    for cookie in crate::auth::guard::clear_cookies() {
        if let Ok(value) = axum::http::HeaderValue::from_str(&cookie) {
            response
                .headers_mut()
                .append(axum::http::header::SET_COOKIE, value);
        }
    }
    response
}

/// `/api/auth/state` — whether the bootstrap is open.
///
/// Unauthenticated by necessity: it is the request that tells a browser which
/// form to show. It discloses one bit — an account exists — which the login form
/// already reveals by failing.
async fn auth_state(State(state): State<AppState>) -> impl IntoResponse {
    axum::Json(state.auth().state())
}

/// `/api/auth/register` — the first-run bootstrap, open once.
async fn auth_register(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<HttpResponse, ApiError> {
    let (_parts, body) = decode::<Credentials>(request).await?;
    crate::auth::guard::same_origin(&_parts)?;

    let username = Credentials::required(&body.username, "username")?;
    let password = Credentials::required(&body.password, "password")?;

    let auth = Arc::clone(state.auth());
    // Argon2 costs tens of milliseconds, and a runtime worker must not pay it:
    // `/healthz` has to stay answerable while a login is hashing.
    let grant = tokio::task::spawn_blocking(move || auth.register(&username, &password))
        .await
        .map_err(|e| ApiError::auth(crate::auth::AuthError::internal(e.to_string())))??;
    Ok(grant_response(StatusCode::CREATED, grant))
}

/// `/api/auth/login` — password in, a session out.
async fn auth_login(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<HttpResponse, ApiError> {
    let (parts, body) = decode::<Credentials>(request).await?;
    crate::auth::guard::same_origin(&parts)?;

    let username = Credentials::required(&body.username, "username")?;
    let password = Credentials::required(&body.password, "password")?;

    let auth = Arc::clone(state.auth());
    let grant = tokio::task::spawn_blocking(move || auth.login(&username, &password))
        .await
        .map_err(|e| ApiError::auth(crate::auth::AuthError::internal(e.to_string())))??;
    Ok(grant_response(StatusCode::OK, grant))
}

/// `/api/auth/refresh` — rotate the refresh token, mint a new access token.
///
/// Authorised by the refresh token rather than by an access token: requiring
/// one that has just expired is the situation this route exists to recover from.
async fn auth_refresh(
    State(state): State<AppState>,
    request: axum::extract::Request,
) -> Result<HttpResponse, ApiError> {
    let (parts, body) = decode::<Credentials>(request).await?;
    crate::auth::guard::same_origin(&parts)?;

    let presented = crate::auth::guard::refresh_token(&parts, body.refresh_token.clone())
        .ok_or_else(|| {
            ApiError::auth(crate::auth::AuthError::Unauthorized(
                "no refresh token: log in again".to_string(),
            ))
        })?;

    match state.auth().refresh(&presented) {
        Ok(grant) => Ok(grant_response(StatusCode::OK, grant)),
        // A refused refresh is the end of a session, so the cookies go with the
        // error: leaving them in the browser would retry the same dead token on
        // every page load.
        Err(error) => Ok(ended(
            StatusCode::UNAUTHORIZED,
            json!({"error": error.message()}),
        )),
    }
}

/// `/api/auth/logout` — end every session, and say how many.
async fn auth_logout(
    State(state): State<AppState>,
    authed: crate::auth::guard::Authed,
    request: axum::extract::Request,
) -> Result<HttpResponse, ApiError> {
    let (parts, _) = decode::<serde::de::IgnoredAny>(request).await?;
    crate::auth::guard::same_origin(&parts)?;
    // The extractor proved a live access token; say who, so the log names a
    // session that is ending rather than an anonymous request.
    let username = authed.username().to_string();

    let auth = Arc::clone(state.auth());
    let ended_count = tokio::task::spawn_blocking(move || auth.logout())
        .await
        .map_err(|e| ApiError::auth(crate::auth::AuthError::internal(e.to_string())))??;
    let _ = username;
    Ok(ended(StatusCode::OK, json!({ "ended": ended_count })))
}

/// `/api/auth/password` — rotate the password, ending every session.
async fn auth_password(
    State(state): State<AppState>,
    authed: crate::auth::guard::Authed,
    request: axum::extract::Request,
) -> Result<HttpResponse, ApiError> {
    let (parts, body) = decode::<Credentials>(request).await?;
    crate::auth::guard::same_origin(&parts)?;

    let current = Credentials::required(&body.current_password, "current_password")?;
    let next = Credentials::required(&body.new_password, "new_password")?;
    let username = authed.account()?.username().to_string();

    let auth = Arc::clone(state.auth());
    tokio::task::spawn_blocking(move || auth.change_password(&username, &current, &next))
        .await
        .map_err(|e| ApiError::auth(crate::auth::AuthError::internal(e.to_string())))??;
    // Every session ended with the password, this one included: the browser must
    // see the cookies go and show the login form rather than a stale token.
    Ok(ended(
        StatusCode::OK,
        json!({ "changed": true, "sessions_ended": true }),
    ))
}

/// `/api/restart` — replace the daemon process with a fresh one.
///
/// The panel only: the host and every other process on it keep running. Answers
/// `202` because the replacement happens after this response is written — a
/// `200` would claim the new process is already serving.
///
/// A `POST` with an origin check, like every other mutating route: another site
/// driving this with a borrowed cookie would be restarting a daemon.
async fn restart(
    State(state): State<AppState>,
    _auth: crate::auth::guard::Authed,
    parts: axum::http::request::Parts,
) -> Result<impl IntoResponse, ApiError> {
    crate::auth::guard::same_origin(&parts)?;
    match state.daemon().handle(Request::Restart).await {
        Response::Restarting { pid } => Ok((
            StatusCode::ACCEPTED,
            axum::Json(json!({
                "restarting": true,
                "pid": pid,
                // Said here because a caller cannot infer it: a restart applies
                // the settings and drops every terminal, and this response is
                // the last thing the old process will ever say.
                "applies_settings": true,
                "drop_terminals": true,
            })),
        )),
        Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "a restart answered unexpectedly: {other:?}"
        )))),
    }
}

/// `/api/reboot` — take the host down.
///
/// Not a variant of `/api/restart`: this replaces every process on the machine,
/// applies no settings and needs `CAP_SYS_BOOT`. The daemon refuses it unless
/// started with `--allow-reboot`, and that refusal is a `403` here rather than a
/// silent no-op.
async fn reboot(
    State(state): State<AppState>,
    _auth: crate::auth::guard::Authed,
    parts: axum::http::request::Parts,
    body: Option<axum::Json<RebootBody>>,
) -> Result<impl IntoResponse, ApiError> {
    crate::auth::guard::same_origin(&parts)?;
    let delay_s = body.map(|axum::Json(body)| body.delay_s).unwrap_or(0);
    match state
        .daemon()
        .handle(Request::SystemReboot { delay_s })
        .await
    {
        Response::Rebooting { delay_s } => Ok((
            StatusCode::ACCEPTED,
            axum::Json(json!({ "rebooting": true, "delay_s": delay_s })),
        )),
        Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "a reboot answered unexpectedly: {other:?}"
        )))),
    }
}

#[derive(serde::Deserialize, Default)]
struct RebootBody {
    #[serde(default)]
    delay_s: u32,
}

/// `/api/host-info` — one snapshot.
async fn host_info(
    State(state): State<AppState>,
    _auth: crate::auth::guard::Authed,
) -> Result<impl IntoResponse, ApiError> {
    let response = state.daemon().handle(Request::HostInfo).await;
    match response {
        anth_wire::Response::HostInfo(info) => Ok(axum::Json(info)),
        anth_wire::Response::Error(error) => Err(ApiError(error)),
        other => Err(ApiError(WireError::internal(format!(
            "host-info answered with an unexpected response: {other:?}"
        )))),
    }
}

/// How often an idle stream sends a comment so a proxy does not close it.
///
/// The browser reconnects on its own after a drop, but a connection closed by a
/// proxy in the middle of a quiet period is a reconnect the operator sees as a
/// flicker. `EventSource` ignores a comment line, so keeping the socket warm
/// costs a few bytes and nothing else.
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// Delay the browser should wait before reconnecting after a drop.
const RETRY: Duration = Duration::from_millis(3_000);

/// `/api/host-info/watch?interval=500ms` — one snapshot per interval, as SSE.
///
/// The route adds a transport, not a message. Every frame is a `HostInfo`
/// answered by the same `dispatch` the one-shot route calls, so the daemon keeps
/// one code path and a bug in the snapshot is the same bug either way. What
/// changes is who repeats it: the CLI repeats on its own clock
/// (`anth-panel host-info --watch`), and a browser is told to repeat here
/// because it cannot hold a timer across a page reload.
///
/// `interval` is read by `anth_wire::parse_interval`, the same function the CLI
/// flag uses, so the two cannot accept different spellings or disagree about the
/// bounds. A bad interval is a `400` **before** the stream starts, which is the
/// last moment the status line can still say so.
async fn watch_host_info(
    State(state): State<AppState>,
    _auth: crate::auth::guard::Authed,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Sse<impl futures_util::stream::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let raw = params
        .get("interval")
        .map_or(anth_wire::DEFAULT_INTERVAL, String::as_str);
    let period = anth_wire::parse_interval(raw)
        .map_err(WireError::invalid)
        .map_err(ApiError)?;

    let daemon = Arc::clone(state.daemon());
    let produce = move || {
        let daemon = Arc::clone(&daemon);
        async move { daemon.handle(Request::HostInfo).await }
    };

    Ok(
        Sse::new(snapshot_stream(tokio::time::interval(period), produce))
            .keep_alive(KeepAlive::new().interval(KEEP_ALIVE)),
    )
}

/// Turns a ticker and the request to repeat into the frames of a watch.
///
/// Split from the handler so the two behaviours that matter can be tested
/// without a daemon that fails on demand: the first frame is immediate, and a
/// failure **ends** the stream instead of being retried forever. `produce` is
/// the seam — production passes a call into `Daemon::handle`, a test passes
/// something that answers `Error` on purpose.
pub(crate) fn snapshot_stream<F, Fut>(
    mut ticks: tokio::time::Interval,
    produce: F,
) -> impl futures_util::stream::Stream<Item = Result<Event, Infallible>>
where
    F: FnMut() -> Fut + Send,
    Fut: std::future::Future<Output = Response> + Send,
{
    // `Delay` rather than the default: a snapshot that took longer than the
    // interval must not be followed by a burst of catch-up frames, each asking
    // the daemon the same question.
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // `None` is the end of the stream: the closure returns it after a failure,
    // and `unfold` stops asking. A sentinel count instead would keep polling a
    // daemon that has already refused.
    futures_util::stream::unfold(Some((ticks, produce, 0_u64)), |current| async move {
        let (mut ticks, mut produce, frames) = current?;
        // The first tick fires immediately, so a client sees a snapshot on
        // connect rather than one silent period later.
        ticks.tick().await;

        let (event, failed) = match produce().await {
            Response::HostInfo(info) => match serde_json::to_string(&info) {
                Ok(data) => (snapshot_event(&data, frames), false),
                Err(error) => (error_event(&error.to_string()), true),
            },
            Response::Error(error) => (
                error_event(&format!("{}: {}", error.kind.as_str(), error.message)),
                true,
            ),
            other => (
                error_event(&format!("unexpected answer to a host snapshot: {other:?}")),
                true,
            ),
        };

        // An error ends the stream. Once the first byte is out the status
        // can no longer change, so the failure travels as an `error` event
        // and the response closes; `EventSource` reconnects, which is the
        // recovery path rather than a stream that keeps asking a daemon that
        // keeps refusing.
        Some((Ok(event), (!failed).then_some((ticks, produce, frames + 1))))
    })
}

/// One snapshot frame, of the event name the browser client listens for.
///
/// The first frame carries the retry hint, because it is the only frame that is
/// guaranteed to have been written before a drop.
fn snapshot_event(data: &str, frames: u64) -> Event {
    let event = Event::default()
        .event("host-info")
        .id((frames + 1).to_string())
        .data(data);
    if frames == 0 {
        event.retry(RETRY)
    } else {
        event
    }
}

/// An SSE event that reports a failure rather than a snapshot.
fn error_event(message: &str) -> Event {
    Event::default().event("error").id("error").data(message)
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
    _auth: crate::auth::guard::Authed,
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
    _auth: crate::auth::guard::Authed,
    method: axum::http::Method,
    parts: axum::http::request::Parts,
    axum::extract::Path((id, path)): axum::extract::Path<(String, String)>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
    body: axum::body::Bytes,
) -> Result<HttpResponse, ApiError> {
    // An anchor may be `GET` or a declared `POST`. Only the methods that can
    // change something need the check: a `GET` anchor is a read, and refusing a
    // cross-origin read is the browser's job, not a policy this route adds.
    if method != axum::http::Method::GET {
        crate::auth::guard::same_origin(&parts)?;
    }
    let path = format!("/{path}");
    let (entry, _loaded) = state
        .daemon()
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
    let known = state.daemon().has_plugin(web.id).map_err(ApiError)?;
    let entry = state
        .daemon()
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
        .daemon()
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
async fn plugins(
    State(state): State<AppState>,
    _auth: crate::auth::guard::Authed,
) -> Result<impl IntoResponse, ApiError> {
    match state.daemon().handle(Request::PluginList).await {
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
    _auth: crate::auth::guard::Authed,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<impl IntoResponse, ApiError> {
    let path = params.get("path").cloned().ok_or_else(|| {
        ApiError(WireError::invalid(
            "`path` is required, as ?path=<file>".to_string(),
        ))
    })?;
    let offset = number_param(&params, "offset", 0)?;
    let len = number_param(&params, "len", u64::from(files::MAX_SLICE))?;
    // A follower asks for a wait; the daemon owns the ceiling. Asking for more
    // is refused rather than trimmed, for the same reason a long `len` is:
    // shortening it silently would make the client believe a deadline it does
    // not have.
    let wait_ms = number_param(&params, "wait_ms", 0)?;
    if wait_ms > u64::from(files::MAX_WAIT_MS) {
        return Err(ApiError(WireError::invalid(format!(
            "`wait_ms` of {wait_ms} exceeds the {} ms limit",
            files::MAX_WAIT_MS
        ))));
    }

    if len > u64::from(u32::MAX) {
        return Err(ApiError(WireError::invalid(format!(
            "`len` of {len} does not fit a slice"
        ))));
    }

    let response = state
        .daemon()
        .handle(Request::FileRead {
            path,
            offset,
            len: len as u32,
            wait_ms: wait_ms as u32,
        })
        .await;

    match response {
        Response::FileSlice {
            path,
            offset,
            bytes,
            next,
            eof,
            waited,
        } => {
            use base64::Engine;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            Ok(axum::Json(json!({
                "path": path,
                "offset": offset,
                "bytes": encoded,
                "next": next,
                "eof": eof,
                "waited": waited,
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
    _auth: crate::auth::guard::Authed,
    parts: axum::http::request::Parts,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Option<axum::Json<CallBody>>,
) -> Result<HttpResponse, ApiError> {
    // A `POST` that runs a plugin's code: the origin check is what keeps another
    // site from driving it with this browser's cookie.
    crate::auth::guard::same_origin(&parts)?;
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

impl ApiError {
    /// A refusal from the authentication layer.
    ///
    /// The kind is derived from the status rather than chosen here, so a route
    /// and a CLI command cannot disagree about what `Unauthorized` means: `401`
    /// maps back to `Forbidden` in `anth-wire`, which is that crate's name for
    /// "your credential did not stand".
    pub fn auth(error: crate::auth::AuthError) -> Self {
        let status = error.http_status();
        let kind = ErrorKind::from_http_status(status);
        Self(WireError::new(kind, error.message().to_string()))
    }
}

/// So a handler can `?` an [`crate::auth::AuthError`] straight out.
impl From<crate::auth::AuthError> for ApiError {
    fn from(error: crate::auth::AuthError) -> Self {
        Self::auth(error)
    }
}

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
