//! HTTP surface over the same `Daemon` and auth service.
//!
//! Every handler builds a request or calls the service; no business logic lives
//! in the transport. Errors use the one `WireError` shape and the kind-derived
//! status.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anth_wire::{ErrorKind, FileChunk, FileEntry, Page, Request, Response, WireError};
use axum::body::Body;
use axum::extract::rejection::QueryRejection;
use axum::extract::{
    ConnectInfo, FromRequest, FromRequestParts, Path as AxumPath, Query, Request as AxumRequest,
    State,
};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::auth::{ACCESS_COOKIE, AuthError, AuthService, Grant, REFRESH_COOKIE};
use crate::config::Config;
use crate::dispatch::Daemon;
use crate::log::{Level, Logger};
use crate::www;

const MAX_BODY_BYTES: usize = 8 * 1024;
const LOGIN_WINDOW: Duration = Duration::from_secs(60);
const LOGIN_ATTEMPTS: usize = 5;

/// Shared state for the HTTP routes.
#[derive(Clone)]
pub struct AppState {
    pub daemon: Arc<Daemon>,
    pub auth: Arc<AuthService>,
    pub logger: Arc<Logger>,
    pub limiter: Arc<LoginLimiter>,
}

impl AppState {
    pub fn new(daemon: Arc<Daemon>, auth: Arc<AuthService>, logger: Arc<Logger>) -> Self {
        Self {
            daemon,
            auth,
            logger,
            limiter: Arc::new(LoginLimiter::default()),
        }
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/auth/state", get(auth_state))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/refresh", post(refresh))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(password))
        .route("/api/auth/sessions", get(list_sessions))
        .route("/api/auth/sessions/revoke", post(revoke_session))
        .route("/api/host-info", get(host_info))
        .route("/api/host-info/watch", get(watch_host_info))
        .route("/api/files", get(list_files).delete(delete_file))
        .route("/api/files/read", get(read_file))
        .route("/api/restart", post(restart))
        .route("/api/reboot", post(reboot))
        .route("/api/settings", get(get_settings).patch(patch_settings))
        .route("/", get(index))
        .route("/{*path}", get(static_path))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            log_request,
        ))
        .with_state(state)
        .layer(axum::middleware::from_fn(protocol_header))
}

async fn log_request(
    State(state): State<AppState>,
    request: AxumRequest,
    next: axum::middleware::Next,
) -> HttpResponse {
    let method = request.method().as_str().to_string();
    let path = request.uri().path().to_string();
    let started = Instant::now();
    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();
    let ms = started.elapsed().as_millis().to_string();
    if path == "/healthz" {
        state
            .logger
            .event(Level::Debug, "http", "answered", &[("status", &status)]);
    } else {
        state.logger.event(
            Level::Info,
            "http",
            "answered",
            &[
                ("method", &method),
                ("path", &path),
                ("status", &status),
                ("ms", &ms),
            ],
        );
    }
    response
}

async fn protocol_header(request: AxumRequest, next: axum::middleware::Next) -> HttpResponse {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-anth-protocol", HeaderValue::from_static("2"));
    response
}

async fn healthz() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

async fn auth_state(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let mut status = state.auth.state();
    status.authenticated = !status.required
        || bearer_or_cookie(&headers, ACCESS_COOKIE)
            .is_some_and(|token| state.auth.verify_access(&token).is_ok());
    Json(status)
}

/// An `Authed` extractor is the visible protection on a route.
pub struct Authed {
    pub sid: String,
}

impl FromRequestParts<AppState> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if !state.auth.required() {
            return Ok(Authed {
                sid: "anonymous".to_string(),
            });
        }
        let token = bearer_or_cookie(&parts.headers, ACCESS_COOKIE).ok_or_else(|| {
            ApiError::from(WireError::new(
                ErrorKind::Unauthorized,
                "auth_required",
                "authentication required",
            ))
        })?;
        let sid = state.auth.verify_access(&token).map_err(ApiError::from)?;
        Ok(Authed { sid })
    }
}

/// A JSON body extractor that always answers the panel's error shape.
pub struct ApiJson<T>(pub T);

impl<T> FromRequest<AppState> for ApiJson<T>
where
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(
        request: AxumRequest,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let bytes = axum::body::to_bytes(request.into_body(), MAX_BODY_BYTES)
            .await
            .map_err(|error| ApiError::invalid(format!("the body could not be read: {error}")))?;
        let value = if bytes.is_empty() {
            serde_json::from_str("{}").expect("an empty object always parses")
        } else {
            serde_json::from_slice(&bytes)
                .map_err(|error| ApiError::invalid(format!("the body is not JSON: {error}")))?
        };
        Ok(ApiJson(value))
    }
}

#[derive(Debug, Deserialize)]
struct RegisterBody {
    password: String,
}

#[derive(Debug, Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

#[derive(Debug, Default, Deserialize)]
struct RefreshBody {
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PasswordBody {
    current_password: String,
    new_password: String,
}

#[derive(Debug, Deserialize)]
struct WatchQuery {
    #[serde(default)]
    interval: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct FilesQuery {
    #[serde(default)]
    path: String,
    #[serde(default)]
    limit: Option<u16>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FileReadQuery {
    path: String,
    #[serde(default)]
    offset: u64,
    #[serde(default)]
    max_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct FileDeleteQuery {
    path: String,
}

#[derive(Debug, Default, Deserialize)]
struct RebootBody {
    #[serde(default)]
    delay_s: u32,
}

#[derive(Debug, Deserialize)]
struct SettingBody {
    key: String,
    value: serde_json::Value,
}

#[derive(Debug, Default, Deserialize)]
struct RevokeBody {
    #[serde(default)]
    sid: Option<String>,
    /// Revoke every session except the caller's.
    #[serde(default)]
    others: bool,
}

async fn restart(
    State(state): State<AppState>,
    _authed: Authed,
    headers: HeaderMap,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    match state.daemon.execute(Request::Restart).await {
        Response::Restarting { pid } => Ok((
            StatusCode::ACCEPTED,
            Json(json!({ "restarting": true, "pid": pid })),
        )
            .into_response()),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a restart answered unexpectedly: {other:?}"
        ))),
    }
}

async fn reboot(
    State(state): State<AppState>,
    _authed: Authed,
    headers: HeaderMap,
    ApiJson(body): ApiJson<RebootBody>,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    match state
        .daemon
        .execute(Request::SystemReboot {
            delay_s: body.delay_s,
        })
        .await
    {
        Response::Rebooting { delay_s } => Ok((
            StatusCode::ACCEPTED,
            Json(json!({ "rebooting": true, "delay_s": delay_s })),
        )
            .into_response()),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a reboot answered unexpectedly: {other:?}"
        ))),
    }
}

async fn get_settings(State(state): State<AppState>, _authed: Authed) -> Json<Config> {
    Json(state.daemon.config().clone())
}

async fn patch_settings(
    State(state): State<AppState>,
    _authed: Authed,
    headers: HeaderMap,
    ApiJson(body): ApiJson<SettingBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    same_origin(&headers)?;
    let literal = toml_literal(&body.value)?;
    Config::set_key(&state.daemon.paths().config, &body.key, &literal)
        .map_err(ApiError::invalid)?;
    Ok(Json(json!({
        "ok": true,
        "key": body.key,
        "restart_required": true,
    })))
}

/// Converts a JSON value into a TOML literal.
///
/// Arrays of scalars and tables are valid in both formats; a JSON object is
/// not, so it is refused rather than silently rewritten into something else.
fn toml_literal(value: &serde_json::Value) -> Result<String, ApiError> {
    match value {
        serde_json::Value::String(text) => {
            serde_json::to_string(text).map_err(|error| ApiError::invalid(error.to_string()))
        }
        serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Array(_) => {
            serde_json::to_string(value).map_err(|error| ApiError::invalid(error.to_string()))
        }
        _ => Err(ApiError::invalid(
            "the value must be a string, number, boolean or array",
        )),
    }
}

async fn list_sessions(State(state): State<AppState>, authed: Authed) -> Json<serde_json::Value> {
    Json(json!({
        "sessions": state.auth.list_sessions(&authed.sid),
        "max": crate::auth::sessions::MAX_SESSIONS,
    }))
}

async fn revoke_session(
    State(state): State<AppState>,
    authed: Authed,
    headers: HeaderMap,
    ApiJson(body): ApiJson<RevokeBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    same_origin(&headers)?;
    let auth = Arc::clone(&state.auth);
    let current = authed.sid.clone();
    let sid = body.sid.clone();
    let others = body.others || sid.is_none();
    let result = tokio::task::spawn_blocking(move || {
        if others {
            auth.revoke_others(&current).map(|count| (count, "others"))
        } else {
            auth.revoke_session(sid.as_deref().unwrap_or_default())
                .map(|found| (usize::from(found), "session"))
        }
    })
    .await
    .map_err(|error| ApiError::internal(format!("the revoke task failed: {error}")))?;
    let (revoked, scope) = result.map_err(ApiError::from)?;
    Ok(Json(
        json!({ "ok": true, "revoked": revoked, "scope": scope }),
    ))
}

async fn register(
    State(state): State<AppState>,
    source: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<RegisterBody>,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    let source = source_name(&source);
    if let Err(retry_after) = state.limiter.check(&source) {
        return Err(ApiError::too_many_after(retry_after));
    }
    let auth = Arc::clone(&state.auth);
    let password = body.password;
    let result = tokio::task::spawn_blocking(move || auth.register(&password))
        .await
        .map_err(|error| ApiError::internal(format!("the register task failed: {error}")))?;
    match result {
        Ok(grant) => {
            state.limiter.succeed(&source);
            Ok(grant_response(&state.auth, StatusCode::CREATED, grant))
        }
        Err(error) => {
            if matches!(error, AuthError::Unauthorized(_)) {
                state.limiter.fail(&source);
            }
            Err(ApiError::from(error))
        }
    }
}

async fn login(
    State(state): State<AppState>,
    source: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<LoginBody>,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    let source = source_name(&source);
    if let Err(retry_after) = state.limiter.check(&source) {
        return Err(ApiError::too_many_after(retry_after));
    }
    let auth = Arc::clone(&state.auth);
    let result = tokio::task::spawn_blocking(move || auth.login(&body.username, &body.password))
        .await
        .map_err(|error| ApiError::internal(format!("the login task failed: {error}")))?;
    match result {
        Ok(grant) => {
            state.limiter.succeed(&source);
            Ok(grant_response(&state.auth, StatusCode::OK, grant))
        }
        Err(error) => {
            if matches!(error, AuthError::Unauthorized(_)) {
                state.limiter.fail(&source);
            }
            Err(ApiError::from(error))
        }
    }
}

async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<RefreshBody>,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    let token = bearer_or_cookie(&headers, REFRESH_COOKIE)
        .or(body.refresh_token)
        .ok_or_else(|| {
            ApiError::from(WireError::new(
                ErrorKind::Unauthorized,
                "auth_required",
                "no refresh token",
            ))
        })?;
    let auth = Arc::clone(&state.auth);
    let grant = tokio::task::spawn_blocking(move || auth.refresh(&token))
        .await
        .map_err(|error| ApiError::internal(format!("the refresh task failed: {error}")))?
        .map_err(ApiError::from)?;
    Ok(grant_response(&state.auth, StatusCode::OK, grant))
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    let auth = Arc::clone(&state.auth);
    tokio::task::spawn_blocking(move || auth.logout())
        .await
        .map_err(|error| ApiError::internal(format!("the logout task failed: {error}")))?
        .map_err(ApiError::from)?;
    let mut response = Json(json!({ "ok": true })).into_response();
    for cookie in state.auth.clear_cookies() {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    Ok(response)
}

async fn password(
    State(state): State<AppState>,
    _authed: Authed,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PasswordBody>,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    let auth = Arc::clone(&state.auth);
    tokio::task::spawn_blocking(move || {
        auth.change_password(&body.current_password, &body.new_password)
    })
    .await
    .map_err(|error| ApiError::internal(format!("the password task failed: {error}")))?
    .map_err(ApiError::from)?;
    let mut response = Json(json!({ "ok": true })).into_response();
    for cookie in state.auth.clear_cookies() {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    Ok(response)
}

async fn host_info(
    State(state): State<AppState>,
    _authed: Authed,
) -> Result<Json<anth_wire::HostInfo>, ApiError> {
    let info = tokio::task::spawn_blocking(anth_host::collect)
        .await
        .map_err(|error| ApiError::internal(format!("the host-info task failed: {error}")))?;
    state.logger.debug("host-info", "snapshot");
    Ok(Json(info))
}

async fn watch_host_info(
    State(state): State<AppState>,
    _authed: Authed,
    query: Result<Query<WatchQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    let interval = match query.interval.as_deref() {
        Some(raw) => anth_wire::parse_interval(raw).map_err(ApiError::invalid)?,
        None => anth_wire::parse_interval(anth_wire::DEFAULT_INTERVAL)
            .expect("the default interval parses"),
    };
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let logger = Arc::clone(&state.logger);
    let stream = stream::unfold(ticker, move |mut ticker| {
        let logger = Arc::clone(&logger);
        async move {
            ticker.tick().await;
            let info = tokio::task::spawn_blocking(anth_host::collect)
                .await
                .unwrap_or_default();
            let event = Event::default()
                .event("host-info")
                .id(info.collected_at_ms.to_string())
                .retry(Duration::from_secs(3))
                .json_data(info)
                .unwrap_or_else(|error| {
                    Event::default().event("error").data(format!(
                        r#"{{"kind":"internal","code":"internal","message":"{error}"}}"#
                    ))
                });
            logger.debug("host-info", "frame");
            Some((Ok::<_, Infallible>(event), ticker))
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

async fn list_files(
    State(state): State<AppState>,
    _authed: Authed,
    query: Result<Query<FilesQuery>, QueryRejection>,
) -> Result<Json<Page<FileEntry>>, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesList {
            path: query.path,
            limit: query.limit,
            cursor: query.cursor,
        })
        .await
    {
        Response::FilesList(page) => Ok(Json(page)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a file list answered unexpectedly: {other:?}"
        ))),
    }
}

async fn read_file(
    State(state): State<AppState>,
    _authed: Authed,
    query: Result<Query<FileReadQuery>, QueryRejection>,
) -> Result<Json<FileChunk>, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesRead {
            path: query.path,
            offset: query.offset,
            max_bytes: query.max_bytes,
        })
        .await
    {
        Response::FilesRead(chunk) => Ok(Json(chunk)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a file read answered unexpectedly: {other:?}"
        ))),
    }
}

async fn delete_file(
    State(state): State<AppState>,
    _authed: Authed,
    query: Result<Query<FileDeleteQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesDelete { path: query.path })
        .await
    {
        Response::FilesDeleted { path } => Ok(Json(json!({ "path": path }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a file delete answered unexpectedly: {other:?}"
        ))),
    }
}

async fn index() -> HttpResponse {
    serve_static("index.html")
}

async fn static_path(AxumPath(path): AxumPath<String>) -> HttpResponse {
    serve_static(&path)
}

fn serve_static(path: &str) -> HttpResponse {
    if path == "api" || path.starts_with("api/") {
        return ApiError::not_found("no such API route").into_response();
    }

    let path = if path.is_empty() { "index.html" } else { path };
    if let Some(asset) = www::get(path) {
        let mut response = HttpResponse::new(Body::from(asset.bytes.into_owned()));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(asset.content_type),
        );
        response.headers_mut().insert(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        return response;
    }

    if www::looks_like_file(path) {
        return ApiError::not_found("no such file").into_response();
    }

    match www::get("index.html") {
        Some(asset) => {
            let mut response = HttpResponse::new(Body::from(asset.bytes.into_owned()));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(asset.content_type),
            );
            response.headers_mut().insert(
                HeaderName::from_static("x-content-type-options"),
                HeaderValue::from_static("nosniff"),
            );
            response
        }
        None => ApiError::not_found("the web UI is not built").into_response(),
    }
}

fn grant_response(auth: &AuthService, status: StatusCode, grant: Grant) -> HttpResponse {
    let mut response = (
        status,
        Json(json!({
            "token_type": grant.token_type,
            "expires_in": grant.expires_in,
        })),
    )
        .into_response();
    for cookie in auth.set_cookies(&grant) {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

fn bearer_or_cookie(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    if let Some(token) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        return Some(token.trim().to_string());
    }
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|cookie| {
        let (name, value) = cookie.trim().split_once('=')?;
        (name == cookie_name).then(|| value.to_string())
    })
}

fn same_origin(headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return Ok(());
    };
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return Err(ApiError::forbidden(
            "the Origin header has no Host to compare",
        ));
    };
    let origin_host = origin.split("://").nth(1).unwrap_or(origin);
    if origin_host.trim_end_matches('/') == host.trim_end_matches('/') {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "the request origin does not match Host",
        ))
    }
}

fn source_name(source: &ConnectInfo<SocketAddr>) -> String {
    source.0.ip().to_string()
}

#[derive(Default)]
pub struct LoginLimiter {
    failures: Mutex<HashMap<String, Vec<Instant>>>,
}

impl LoginLimiter {
    /// Whether `source` may make another attempt, and for how long it may not.
    ///
    /// Only failures are recorded, so a successful login never spends the
    /// budget. The entries are kept in time order, which makes the oldest one
    /// the moment the window frees a slot.
    fn check(&self, source: &str) -> Result<(), u64> {
        let now = Instant::now();
        let mut failures = self.failures.lock().expect("limiter lock");
        let entries = failures.entry(source.to_string()).or_default();
        entries.retain(|at| now.duration_since(*at) < LOGIN_WINDOW);
        if entries.len() < LOGIN_ATTEMPTS {
            return Ok(());
        }
        let oldest = entries.first().copied().unwrap_or(now);
        let remaining = LOGIN_WINDOW.saturating_sub(now.duration_since(oldest));
        Err(remaining.as_secs().max(1))
    }

    /// Records one failed attempt.
    fn fail(&self, source: &str) {
        let now = Instant::now();
        let mut failures = self.failures.lock().expect("limiter lock");
        let entries = failures.entry(source.to_string()).or_default();
        entries.retain(|at| now.duration_since(*at) < LOGIN_WINDOW);
        entries.push(now);
    }

    /// Clears the budget after a successful login.
    fn succeed(&self, source: &str) {
        self.failures.lock().expect("limiter lock").remove(source);
    }
}

/// The error shape every route answers with.
#[derive(Debug)]
pub struct ApiError {
    error: WireError,
    retry_after: Option<u64>,
}

impl ApiError {
    fn new(error: WireError) -> Self {
        Self {
            error,
            retry_after: None,
        }
    }

    pub fn from_auth(error: AuthError) -> Self {
        Self::new(error.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(WireError::not_found(message))
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(WireError::invalid(message))
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(WireError::forbidden(message))
    }

    pub fn too_many_after(seconds: u64) -> Self {
        Self {
            error: WireError::too_many(format!(
                "too many authentication attempts; retry in {seconds}s"
            )),
            retry_after: Some(seconds),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(WireError::internal(message))
    }
}

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        Self::new(error.into())
    }
}

impl From<WireError> for ApiError {
    fn from(error: WireError) -> Self {
        Self::new(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> HttpResponse {
        let status = StatusCode::from_u16(self.error.kind.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = json!({
            "error": {
                "kind": self.error.kind.as_str(),
                "code": self.error.code,
                "message": self.error.message,
            }
        });
        let mut response = (status, Json(body)).into_response();
        if self.error.kind == ErrorKind::TooMany {
            let seconds = self.retry_after.unwrap_or(60).to_string();
            if let Ok(value) = HeaderValue::from_str(&seconds) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limiter_counts_only_failures() {
        let limiter = LoginLimiter::default();
        for _ in 0..LOGIN_ATTEMPTS {
            assert!(limiter.check("source").is_ok());
            limiter.fail("source");
        }
        let retry = limiter.check("source").expect_err("blocked");
        assert!((1..=60).contains(&retry));

        limiter.succeed("source");
        assert!(limiter.check("source").is_ok());

        // Another source keeps its own budget.
        assert!(limiter.check("another").is_ok());
    }

    #[test]
    fn a_successful_login_does_not_spend_the_budget() {
        let limiter = LoginLimiter::default();
        for _ in 0..(LOGIN_ATTEMPTS * 3) {
            assert!(limiter.check("source").is_ok());
            limiter.succeed("source");
        }
    }

    #[test]
    fn setting_values_become_toml_literals() {
        assert_eq!(toml_literal(&json!(true)).unwrap(), "true");
        assert_eq!(toml_literal(&json!(42)).unwrap(), "42");
        assert_eq!(toml_literal(&json!("operator")).unwrap(), "\"operator\"");
        assert_eq!(
            toml_literal(&json!(["/bin/echo"])).unwrap(),
            "[\"/bin/echo\"]"
        );
        assert!(toml_literal(&json!({ "a": 1 })).is_err());
        assert!(toml_literal(&serde_json::Value::Null).is_err());
    }
}
