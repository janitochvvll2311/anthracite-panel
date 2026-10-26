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

use anth_wire::{ErrorKind, WireError};
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
use serde_json::json;

use crate::auth::{ACCESS_COOKIE, AuthError, AuthService, Grant, REFRESH_COOKIE};
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
        .route("/api/host-info", get(host_info))
        .route("/api/host-info/watch", get(watch_host_info))
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
    #[allow(dead_code)]
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

async fn register(
    State(state): State<AppState>,
    source: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<RegisterBody>,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    state.limiter.check(&source_name(&source))?;
    let auth = Arc::clone(&state.auth);
    let password = body.password;
    let grant = tokio::task::spawn_blocking(move || auth.register(&password))
        .await
        .map_err(|error| ApiError::internal(format!("the register task failed: {error}")))?
        .map_err(ApiError::from)?;
    Ok(grant_response(&state.auth, StatusCode::CREATED, grant))
}

async fn login(
    State(state): State<AppState>,
    source: ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ApiJson(body): ApiJson<LoginBody>,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    state.limiter.check(&source_name(&source))?;
    let auth = Arc::clone(&state.auth);
    let grant = tokio::task::spawn_blocking(move || auth.login(&body.username, &body.password))
        .await
        .map_err(|error| ApiError::internal(format!("the login task failed: {error}")))?
        .map_err(ApiError::from)?;
    Ok(grant_response(&state.auth, StatusCode::OK, grant))
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
    hits: Mutex<HashMap<String, Vec<Instant>>>,
}

impl LoginLimiter {
    fn check(&self, source: &str) -> Result<(), ApiError> {
        let now = Instant::now();
        let mut hits = self.hits.lock().expect("limiter lock");
        let entries = hits.entry(source.to_string()).or_default();
        entries.retain(|at| now.duration_since(*at) < LOGIN_WINDOW);
        if entries.len() >= LOGIN_ATTEMPTS {
            return Err(ApiError::too_many(
                "too many authentication attempts; retry in a minute",
            ));
        }
        entries.push(now);
        Ok(())
    }
}

/// The error shape every route answers with.
pub struct ApiError(WireError);

impl ApiError {
    pub fn from_auth(error: AuthError) -> Self {
        Self(error.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self(WireError::not_found(message))
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self(WireError::invalid(message))
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self(WireError::forbidden(message))
    }

    pub fn too_many(message: impl Into<String>) -> Self {
        Self(WireError::too_many(message))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self(WireError::internal(message))
    }
}

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        Self(error.into())
    }
}

impl From<WireError> for ApiError {
    fn from(error: WireError) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> HttpResponse {
        let status = StatusCode::from_u16(self.0.kind.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = json!({
            "error": {
                "kind": self.0.kind.as_str(),
                "code": self.0.code,
                "message": self.0.message,
            }
        });
        let mut response = (status, Json(body)).into_response();
        if self.0.kind == ErrorKind::TooMany {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        }
        response
    }
}
