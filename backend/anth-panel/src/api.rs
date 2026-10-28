//! HTTP surface over the same `Daemon` and auth service.
//!
//! Every handler builds a request or calls the service; no business logic lives
//! in the transport. Errors use the one `WireError` shape and the kind-derived
//! status.

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::plugins::{PluginReply, PluginsView};
use anth_wire::{
    AccountCreate, AccountDelete, AccountGroupCreate, AccountGroupDelete, AccountSetPassword,
    AccountSnapshot, ArchiveFormat, ArchiveListing, DEFAULT_TERMINAL_SIZE, ErrorKind, FileChunk,
    FileEntry, FileWritten, FirewallAction, FirewallChain, FirewallProtocol, FirewallSnapshot,
    NetworkCreate, NetworkEdit, NetworkSnapshot, Page, PageRequest, PartitionKind, ProcessSignal,
    ProcessSnapshot, Request, Response, SecretString, TerminalSize, TrashListing,
    VolumeFormatRequest, VolumeMountRequest, VolumePartitionCreate, VolumePartitionDelete,
    VolumePartitionResize, VolumeSnapshot, VolumeUnmountRequest, WireError,
};
use axum::body::{Body, Bytes};
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{
    ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Path as AxumPath, Query,
    Request as AxumRequest, State, ws::WebSocketUpgrade,
};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response as HttpResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
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
const DEMO_COOKIE: &str = "anth_demo";

/// The owner assigned by the demo cookie middleware.
#[derive(Clone)]
struct DemoOwner(String);

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
    // The write routes accept JSON bodies; keep the limits tied to the config
    // so a larger upload cap is not silently capped by the extractor default.
    let max_upload =
        usize::try_from(state.daemon.config().files.max_upload_bytes).unwrap_or(usize::MAX);
    let write_limit = max_upload.saturating_add(1024 * 1024);
    let upload_limit = max_upload
        .saturating_mul(4)
        .saturating_div(3)
        .saturating_add(1024 * 1024);
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/demo", get(demo_status))
        .route("/api/demo/reset", post(demo_reset))
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
        .route("/api/network", get(network))
        .route("/api/network/watch", get(watch_network))
        .route("/api/network/state", post(network_state))
        .route("/api/network/create", post(network_create))
        .route("/api/network/edit", post(network_edit))
        .route("/api/network/rename", post(network_rename))
        .route("/api/network/delete", post(network_delete))
        .route("/api/firewall", get(firewall_status))
        .route("/api/firewall/rule", post(firewall_add))
        .route("/api/firewall/update", post(firewall_update))
        .route("/api/firewall/delete", post(firewall_delete))
        .route("/api/firewall/flush", post(firewall_flush))
        .route("/api/volumes", get(volumes))
        .route("/api/volumes/mount", post(volumes_mount))
        .route("/api/volumes/unmount", post(volumes_unmount))
        .route(
            "/api/volumes/partition/create",
            post(volume_partition_create),
        )
        .route(
            "/api/volumes/partition/resize",
            post(volume_partition_resize),
        )
        .route(
            "/api/volumes/partition/delete",
            post(volume_partition_delete),
        )
        .route("/api/volumes/format", post(volume_format))
        .route("/api/accounts", get(accounts))
        .route("/api/accounts/users", post(account_create))
        .route("/api/accounts/users/delete", post(account_delete))
        .route("/api/accounts/users/password", post(account_password))
        .route("/api/accounts/groups", post(account_group_create))
        .route("/api/accounts/groups/delete", post(account_group_delete))
        .route("/api/plugins", get(plugins))
        .route(
            "/api/plugins/install",
            post(plugin_install).layer(DefaultBodyLimit::max(48 * 1024 * 1024)),
        )
        .route("/api/plugins/{id}/enable", post(plugin_enable))
        .route("/api/plugins/{id}/disable", post(plugin_disable))
        .route("/api/plugins/{id}/remove", post(plugin_remove))
        .route("/api/plugins/{id}/approve", post(plugin_approve))
        .route("/api/plugins/{id}/web/{*path}", get(plugin_asset))
        .route("/p/{id}/{*module}", get(plugin_frame))
        .route(
            "/api/plugins/{id}/{*path}",
            get(plugin_anchor)
                .post(plugin_anchor)
                .put(plugin_anchor)
                .patch(plugin_anchor)
                .delete(plugin_anchor),
        )
        .route("/api/firewall/policy", post(firewall_policy))
        .route("/api/files", get(list_files).delete(delete_file))
        .route("/api/files/read", get(read_file))
        .route(
            "/api/files/write",
            post(write_file).layer(DefaultBodyLimit::max(write_limit)),
        )
        .route(
            "/api/files/upload",
            post(upload_file).layer(DefaultBodyLimit::max(upload_limit)),
        )
        .route("/api/files/image", get(image_file))
        .route("/api/files/download", get(download_file))
        .route("/api/files/trash", post(trash_file))
        .route("/api/files/trash/list", get(list_trash))
        .route("/api/files/trash/restore", post(restore_trash))
        .route("/api/files/trash/empty", post(empty_trash))
        .route("/api/files/archive", post(create_archive))
        .route("/api/files/archive/list", get(list_archive))
        .route("/api/files/archive/extract", post(extract_archive))
        .route("/api/terminal", get(ws_terminal))
        .route("/api/processes", get(processes))
        .route("/api/processes/watch", get(watch_processes))
        .route("/api/processes/signal", post(process_signal))
        .route("/api/processes/spawn", post(process_spawn))
        .route("/api/restart", post(restart))
        .route("/api/reboot", post(reboot))
        .route("/api/settings", get(get_settings).patch(patch_settings))
        .route("/", get(index))
        .route("/{*path}", get(static_path))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            demo_session,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            log_request,
        ))
        .with_state(state)
        .layer(axum::middleware::from_fn(protocol_header))
}

/// Gives every demo browser its own anonymous owner.
///
/// It runs before the auth extractor, which uses the extracted owner when real
/// auth is off. A cookie is generated server-side, so a visitor cannot choose
/// another session's id.
async fn demo_session(
    State(state): State<AppState>,
    mut request: AxumRequest,
    next: axum::middleware::Next,
) -> HttpResponse {
    if !state.daemon.demo_mode() {
        return next.run(request).await;
    }
    let existing = request
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value.split(';').map(str::trim).find_map(|part| {
                part.strip_prefix(&format!("{DEMO_COOKIE}=")).filter(|sid| {
                    sid.len() == 32 && sid.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            })
        })
        .map(str::to_string);
    let (sid, fresh) = match existing {
        Some(sid) => (sid, false),
        None => {
            let mut bytes = [0u8; 16];
            if getrandom::fill(&mut bytes).is_err() {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "the demo session could not be created",
                )
                    .into_response();
            }
            (hex(&bytes), true)
        }
    };
    request.extensions_mut().insert(DemoOwner(sid.clone()));
    let mut response = crate::dispatch::with_demo_owner(sid.clone(), next.run(request)).await;
    if fresh {
        let max_age = state
            .daemon
            .demo_status(&sid)
            .map_or(86_400, |status| status.ttl_seconds.max(3600));
        let secure = if state.daemon.config().auth.cookie_secure {
            "; Secure"
        } else {
            ""
        };
        let cookie = format!(
            "{DEMO_COOKIE}={sid}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
        );
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().insert(header::SET_COOKIE, value);
        }
    }
    response
}

fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from_digit(u32::from(byte >> 4), 16).expect("nibble"));
        text.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("nibble"));
    }
    text
}

async fn demo_status(State(state): State<AppState>, authed: Authed) -> Json<Value> {
    match state.daemon.demo_status(&authed.sid) {
        Some(status) => Json(json!({ "mode": "demo", "status": status })),
        None => Json(json!({ "mode": "real" })),
    }
}

async fn demo_reset(
    State(state): State<AppState>,
    authed: Authed,
) -> Result<Json<Value>, ApiError> {
    let reset_ms = state.daemon.demo_reset(&authed.sid).ok_or_else(|| {
        ApiError::from(WireError::new(
            ErrorKind::NotFound,
            "demo_unavailable",
            "the demo backend is not active",
        ))
    })?;
    Ok(Json(json!({ "status": "reset", "reset_ms": reset_ms })))
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
            if let Some(owner) = parts.extensions.get::<DemoOwner>() {
                return Ok(Authed {
                    sid: owner.0.clone(),
                });
            }
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

#[derive(Debug, Default, Deserialize)]
struct ProcessesQuery {
    #[serde(default)]
    limit: Option<u16>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProcessWatchQuery {
    #[serde(default)]
    interval: Option<String>,
    #[serde(default)]
    limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct ProcessSignalBody {
    pid: u32,
    start_ticks: u64,
    signal: String,
    #[serde(default)]
    group: bool,
}

#[derive(Debug, Deserialize)]
struct ProcessSpawnBody {
    program: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    cwd: Option<String>,
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
struct FileWriteBody {
    path: String,
    content: String,
    #[serde(default)]
    create: bool,
}

#[derive(Debug, Deserialize)]
struct FileUploadBody {
    path: String,
    content_base64: String,
    #[serde(default)]
    create: bool,
}

#[derive(Debug, Deserialize)]
struct FilePathQuery {
    path: String,
}

#[derive(Debug, Deserialize)]
struct TerminalQuery {
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct TrashPathBody {
    path: String,
}

#[derive(Debug, Deserialize)]
struct TrashRestoreBody {
    id: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Debug, Deserialize)]
struct ArchiveCreateBody {
    path: String,
    #[serde(default)]
    destination: Option<String>,
    #[serde(default)]
    format: Option<ArchiveFormat>,
}

#[derive(Debug, Deserialize)]
struct ArchiveExtractBody {
    path: String,
    #[serde(default)]
    into: Option<String>,
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
    let targets: Vec<String> = if others {
        state
            .auth
            .list_sessions(&current)
            .into_iter()
            .filter(|session| !session.current)
            .map(|session| session.sid)
            .collect()
    } else {
        sid.clone().into_iter().collect()
    };
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
    for sid in targets {
        if !sid.is_empty() {
            state.daemon.close_terminals(&sid);
        }
    }
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
    authed: Authed,
    headers: HeaderMap,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    state.daemon.close_terminals(&authed.sid);
    let auth = Arc::clone(&state.auth);
    tokio::task::spawn_blocking(move || auth.logout())
        .await
        .map_err(|error| ApiError::internal(format!("the logout task failed: {error}")))?
        .map_err(ApiError::from)?;
    let mut response = Json(json!({ "ok": true })).into_response();
    append_cookies(&mut response, state.auth.clear_cookies());
    Ok(response)
}

async fn password(
    State(state): State<AppState>,
    authed: Authed,
    headers: HeaderMap,
    ApiJson(body): ApiJson<PasswordBody>,
) -> Result<HttpResponse, ApiError> {
    same_origin(&headers)?;
    state.daemon.close_terminals(&authed.sid);
    let auth = Arc::clone(&state.auth);
    tokio::task::spawn_blocking(move || {
        auth.change_password(&body.current_password, &body.new_password)
    })
    .await
    .map_err(|error| ApiError::internal(format!("the password task failed: {error}")))?
    .map_err(ApiError::from)?;
    let mut response = Json(json!({ "ok": true })).into_response();
    append_cookies(&mut response, state.auth.clear_cookies());
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
    let interval = watch_interval(query.interval.as_deref())?;
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

async fn network(
    State(state): State<AppState>,
    _authed: Authed,
) -> Result<Json<NetworkSnapshot>, ApiError> {
    match state.daemon.execute(Request::Network).await {
        Response::Network(snapshot) => Ok(Json(snapshot)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a network snapshot answered unexpectedly: {other:?}"
        ))),
    }
}

async fn watch_network(
    State(state): State<AppState>,
    authed: Authed,
    query: Result<Query<WatchQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    let interval = watch_interval(query.interval.as_deref())?;
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let daemon = Arc::clone(&state.daemon);
    let logger = Arc::clone(&state.logger);
    let owner = authed.sid;
    let stream = stream::unfold(ticker, move |mut ticker| {
        let daemon = Arc::clone(&daemon);
        let logger = Arc::clone(&logger);
        let owner = owner.clone();
        async move {
            ticker.tick().await;
            let event = match daemon.execute_as(&owner, Request::Network).await {
                Response::Network(snapshot) => Event::default()
                    .event("network")
                    .id(snapshot.timestamp_ms.to_string())
                    .retry(Duration::from_secs(3))
                    .json_data(snapshot)
                    .unwrap_or_else(|error| {
                        sse_error(&WireError::internal(format!(
                            "the network snapshot could not be encoded: {error}"
                        )))
                    }),
                Response::Error(error) => sse_error(&error),
                other => sse_error(&WireError::internal(format!(
                    "a network snapshot answered unexpectedly: {other:?}"
                ))),
            };
            logger.debug("network", "frame");
            Some((Ok::<_, Infallible>(event), ticker))
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

#[derive(Debug, Deserialize)]
struct NetworkStateBody {
    name: String,
    up: bool,
}

#[derive(Debug, Deserialize)]
struct NetworkCreateBody {
    name: String,
    kind: String,
    #[serde(default)]
    peer: Option<String>,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    vlan_id: Option<u16>,
    #[serde(default)]
    mtu: Option<u32>,
    #[serde(default)]
    up: bool,
}

#[derive(Debug, Deserialize)]
struct NetworkEditBody {
    name: String,
    #[serde(default)]
    mtu: Option<u32>,
    #[serde(default)]
    up: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct NetworkRenameBody {
    name: String,
    new_name: String,
}

#[derive(Debug, Deserialize)]
struct NetworkDeleteBody {
    name: String,
}

/// The caller's address, or `None` when the router was built without it.
///
/// Only the delete guard uses it, and a missing peer degrades to "unknown"
/// instead of failing the request.
struct PeerAddress(Option<String>);

impl<S: Send + Sync> FromRequestParts<S> for PeerAddress {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(address)| address.ip().to_string()),
        ))
    }
}

async fn network_state(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<NetworkStateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    network_change(
        &state.daemon,
        Request::NetworkSetState {
            name: body.name,
            up: body.up,
        },
    )
    .await
}

async fn network_create(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<NetworkCreateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    network_change(
        &state.daemon,
        Request::NetworkCreate(NetworkCreate {
            name: body.name,
            kind: body.kind,
            peer: body.peer,
            link: body.link,
            vlan_id: body.vlan_id,
            mtu: body.mtu,
            up: body.up,
        }),
    )
    .await
}

async fn network_edit(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<NetworkEditBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    network_change(
        &state.daemon,
        Request::NetworkEdit(NetworkEdit {
            name: body.name,
            mtu: body.mtu,
            up: body.up,
        }),
    )
    .await
}

async fn network_rename(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<NetworkRenameBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    network_change(
        &state.daemon,
        Request::NetworkRename {
            name: body.name,
            new_name: body.new_name,
        },
    )
    .await
}

async fn network_delete(
    State(state): State<AppState>,
    _authed: Authed,
    peer: PeerAddress,
    body: Result<Json<NetworkDeleteBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    network_change(
        &state.daemon,
        Request::NetworkDelete {
            name: body.name,
            peer: peer.0,
        },
    )
    .await
}

async fn network_change(daemon: &Arc<Daemon>, request: Request) -> Result<Json<Value>, ApiError> {
    match daemon.execute(request).await {
        Response::NetworkChanged { message } => Ok(Json(json!({ "message": message }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a network change answered unexpectedly: {other:?}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
struct FirewallAddBody {
    #[serde(default)]
    chain: FirewallChain,
    action: String,
    protocol: String,
    port: u16,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FirewallUpdateBody {
    handle: u64,
    #[serde(default)]
    chain: FirewallChain,
    action: String,
    protocol: String,
    port: u16,
    #[serde(default)]
    comment: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FirewallDeleteBody {
    handle: u64,
}

#[derive(Debug, Deserialize)]
struct FirewallFlushBody {
    #[serde(default)]
    chain: FirewallChain,
}

#[derive(Debug, Deserialize)]
struct FirewallPolicyBody {
    chain: FirewallChain,
    policy: String,
}

async fn firewall_status(
    State(state): State<AppState>,
    _authed: Authed,
) -> Result<Json<FirewallSnapshot>, ApiError> {
    match state.daemon.execute(Request::FirewallStatus).await {
        Response::FirewallSnapshot(snapshot) => Ok(Json(snapshot)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a firewall snapshot answered unexpectedly: {other:?}"
        ))),
    }
}

async fn firewall_add(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<FirewallAddBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    let action = body
        .action
        .parse::<FirewallAction>()
        .map_err(ApiError::invalid)?;
    let protocol = body
        .protocol
        .parse::<FirewallProtocol>()
        .map_err(ApiError::invalid)?;
    firewall_change(
        &state.daemon,
        Request::FirewallAdd {
            chain: body.chain,
            action,
            protocol,
            port: body.port,
            comment: body.comment,
        },
    )
    .await
}

async fn firewall_update(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<FirewallUpdateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    let action = body
        .action
        .parse::<FirewallAction>()
        .map_err(ApiError::invalid)?;
    let protocol = body
        .protocol
        .parse::<FirewallProtocol>()
        .map_err(ApiError::invalid)?;
    firewall_change(
        &state.daemon,
        Request::FirewallUpdate {
            handle: body.handle,
            chain: body.chain,
            action,
            protocol,
            port: body.port,
            comment: body.comment,
        },
    )
    .await
}

async fn firewall_delete(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<FirewallDeleteBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    firewall_change(
        &state.daemon,
        Request::FirewallDelete {
            handle: body.handle,
        },
    )
    .await
}

async fn firewall_flush(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<FirewallFlushBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    firewall_change(&state.daemon, Request::FirewallFlush { chain: body.chain }).await
}

async fn firewall_policy(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<FirewallPolicyBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    let policy = body
        .policy
        .parse::<FirewallAction>()
        .map_err(ApiError::invalid)?;
    firewall_change(
        &state.daemon,
        Request::FirewallSetPolicy {
            chain: body.chain,
            policy,
        },
    )
    .await
}

async fn firewall_change(daemon: &Arc<Daemon>, request: Request) -> Result<Json<Value>, ApiError> {
    match daemon.execute(request).await {
        Response::FirewallChanged { message } => Ok(Json(json!({ "message": message }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a firewall change answered unexpectedly: {other:?}"
        ))),
    }
}

async fn volumes(
    State(state): State<AppState>,
    _authed: Authed,
) -> Result<Json<VolumeSnapshot>, ApiError> {
    match state.daemon.execute(Request::Volumes).await {
        Response::Volumes(snapshot) => Ok(Json(snapshot)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a storage snapshot answered unexpectedly: {other:?}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
struct VolumeMountBody {
    device: String,
    target: String,
    #[serde(default)]
    fs_type: Option<String>,
    #[serde(default)]
    read_only: bool,
    #[serde(default)]
    allow_removable: bool,
}

#[derive(Debug, Deserialize)]
struct VolumeUnmountBody {
    target: String,
}

async fn volumes_mount(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<VolumeMountBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    volume_change(
        &state.daemon,
        Request::VolumesMount(VolumeMountRequest {
            device: body.device,
            target: body.target,
            fs_type: body.fs_type,
            read_only: body.read_only,
            allow_removable: body.allow_removable,
        }),
    )
    .await
}

async fn volumes_unmount(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<VolumeUnmountBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    volume_change(
        &state.daemon,
        Request::VolumesUnmount(VolumeUnmountRequest {
            target: body.target,
        }),
    )
    .await
}

async fn volume_change(daemon: &Arc<Daemon>, request: Request) -> Result<Json<Value>, ApiError> {
    match daemon.execute(request).await {
        Response::VolumeChanged { message } => Ok(Json(json!({ "message": message }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a storage change answered unexpectedly: {other:?}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
struct VolumePartitionCreateBody {
    disk: String,
    kind: String,
    #[serde(default)]
    start_bytes: u64,
    size_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct VolumePartitionResizeBody {
    disk: String,
    number: u32,
    size_bytes: u64,
}

#[derive(Debug, Deserialize)]
struct VolumePartitionDeleteBody {
    disk: String,
    number: u32,
}

#[derive(Debug, Deserialize)]
struct VolumeFormatBody {
    partition: String,
    fs_type: String,
    #[serde(default)]
    label: Option<String>,
}

async fn volume_partition_create(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<VolumePartitionCreateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    let kind = body
        .kind
        .parse::<PartitionKind>()
        .map_err(ApiError::invalid)?;
    volume_change(
        &state.daemon,
        Request::VolumePartitionCreate(VolumePartitionCreate {
            disk: body.disk,
            kind,
            start_bytes: body.start_bytes,
            size_bytes: body.size_bytes,
        }),
    )
    .await
}

async fn volume_partition_resize(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<VolumePartitionResizeBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    volume_change(
        &state.daemon,
        Request::VolumePartitionResize(VolumePartitionResize {
            disk: body.disk,
            number: body.number,
            size_bytes: body.size_bytes,
        }),
    )
    .await
}

async fn volume_partition_delete(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<VolumePartitionDeleteBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    volume_change(
        &state.daemon,
        Request::VolumePartitionDelete(VolumePartitionDelete {
            disk: body.disk,
            number: body.number,
        }),
    )
    .await
}

async fn volume_format(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<VolumeFormatBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    volume_change(
        &state.daemon,
        Request::VolumeFormat(VolumeFormatRequest {
            partition: body.partition,
            fs_type: body.fs_type,
            label: body.label,
        }),
    )
    .await
}

async fn accounts(
    State(state): State<AppState>,
    _authed: Authed,
) -> Result<Json<AccountSnapshot>, ApiError> {
    match state.daemon.execute(Request::Accounts).await {
        Response::Accounts(snapshot) => Ok(Json(snapshot)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "an account snapshot answered unexpectedly: {other:?}"
        ))),
    }
}

#[derive(Debug, Deserialize)]
struct AccountCreateBody {
    name: String,
    #[serde(default)]
    uid: Option<u32>,
    #[serde(default)]
    gid: Option<u32>,
    #[serde(default)]
    gecos: Option<String>,
    #[serde(default)]
    home: Option<String>,
    #[serde(default)]
    shell: Option<String>,
    #[serde(default)]
    system: bool,
}

async fn account_create(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<AccountCreateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    account_change(
        &state.daemon,
        Request::AccountCreate(AccountCreate {
            name: body.name,
            uid: body.uid,
            gid: body.gid,
            gecos: body.gecos,
            home: body.home,
            shell: body.shell,
            system: body.system,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
struct AccountDeleteBody {
    name: String,
    #[serde(default)]
    remove_home: bool,
}

async fn account_delete(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<AccountDeleteBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    account_change(
        &state.daemon,
        Request::AccountDelete(AccountDelete {
            name: body.name,
            remove_home: body.remove_home,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
struct AccountPasswordBody {
    name: String,
    password: String,
}

async fn account_password(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<AccountPasswordBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    account_change(
        &state.daemon,
        Request::AccountSetPassword(AccountSetPassword {
            name: body.name,
            password: SecretString::new(body.password),
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
struct AccountGroupCreateBody {
    name: String,
    #[serde(default)]
    gid: Option<u32>,
    #[serde(default)]
    system: bool,
}

async fn account_group_create(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<AccountGroupCreateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    account_change(
        &state.daemon,
        Request::AccountGroupCreate(AccountGroupCreate {
            name: body.name,
            gid: body.gid,
            system: body.system,
        }),
    )
    .await
}

#[derive(Debug, Deserialize)]
struct AccountGroupDeleteBody {
    name: String,
}

async fn account_group_delete(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<AccountGroupDeleteBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    account_change(
        &state.daemon,
        Request::AccountGroupDelete(AccountGroupDelete { name: body.name }),
    )
    .await
}

async fn account_change(daemon: &Arc<Daemon>, request: Request) -> Result<Json<Value>, ApiError> {
    match daemon.execute(request).await {
        Response::AccountChanged { message } => Ok(Json(json!({ "message": message }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "an account change answered unexpectedly: {other:?}"
        ))),
    }
}

async fn plugins(
    State(state): State<AppState>,
    _authed: Authed,
) -> Result<Json<PluginsView>, ApiError> {
    let service = state.daemon.plugins().clone();
    let view = plugin_blocking(move || service.snapshot()).await?;
    Ok(Json(view))
}

#[derive(Debug, Deserialize)]
struct PluginInstallBody {
    bundle_base64: String,
    #[serde(default)]
    accept_capabilities: bool,
}

async fn plugin_install(
    State(state): State<AppState>,
    _authed: Authed,
    headers: HeaderMap,
    body: Result<Json<PluginInstallBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    same_origin(&headers)?;
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    if !state.daemon.config().plugins.install_from_web {
        return Err(ApiError::from(WireError::forbidden(
            "browser plugin installs are disabled; set `[plugins] install_from_web = true`",
        )));
    }
    let bytes = STANDARD
        .decode(body.bundle_base64.as_bytes())
        .map_err(|_| ApiError::invalid("the bundle is not valid base64"))?;
    if bytes.len() as u64 > crate::plugins::bundle::MAX_BUNDLE_BYTES {
        return Err(ApiError::from(WireError::too_large(
            "the bundle is larger than 32 MiB",
        )));
    }
    let service = state.daemon.plugins().clone();
    let accepted = body.accept_capabilities;
    let installed = plugin_blocking(move || service.install_bytes(&bytes, accepted)).await?;
    Ok(Json(json!({
        "id": installed.grant.id,
        "name": installed.manifest.plugin.name,
        "version": installed.grant.plugin_version,
        "capabilities": installed.grant.allowed_ids(),
        "warning": installed.warning,
    })))
}

async fn plugin_enable(
    State(state): State<AppState>,
    _authed: Authed,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    same_origin(&headers)?;
    let service = state.daemon.plugins().clone();
    plugin_blocking(move || service.set_enabled(&id, true)).await?;
    Ok(Json(json!({ "message": "plugin enabled" })))
}

async fn plugin_disable(
    State(state): State<AppState>,
    _authed: Authed,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    same_origin(&headers)?;
    let service = state.daemon.plugins().clone();
    plugin_blocking(move || service.set_enabled(&id, false)).await?;
    Ok(Json(json!({ "message": "plugin disabled" })))
}

async fn plugin_remove(
    State(state): State<AppState>,
    _authed: Authed,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    same_origin(&headers)?;
    let service = state.daemon.plugins().clone();
    plugin_blocking(move || service.remove(&id)).await?;
    Ok(Json(json!({ "message": "plugin removed" })))
}

#[derive(Debug, Deserialize)]
struct PluginApproveBody {
    #[serde(default)]
    accept_capabilities: bool,
}

async fn plugin_approve(
    State(state): State<AppState>,
    _authed: Authed,
    AxumPath(id): AxumPath<String>,
    headers: HeaderMap,
    body: Result<Json<PluginApproveBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    same_origin(&headers)?;
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    let service = state.daemon.plugins().clone();
    let accepted = body.accept_capabilities;
    let grant = plugin_blocking(move || service.approve(&id, accepted)).await?;
    Ok(Json(json!({
        "id": grant.id,
        "capabilities": grant.allowed_ids(),
    })))
}

async fn plugin_blocking<T, F>(task: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, WireError> + Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .map_err(|error| ApiError::internal(format!("the plugin worker failed: {error}")))?
        .map_err(ApiError::from)
}

/// Largest body forwarded to an anchor. An anchor is a control operation, not
/// an upload channel.
const MAX_ANCHOR_BODY: usize = 1024 * 1024;

async fn plugin_frame(
    State(state): State<AppState>,
    _authed: Authed,
    AxumPath((id, module)): AxumPath<(String, String)>,
) -> Result<HttpResponse, ApiError> {
    if module.is_empty()
        || module
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ApiError::invalid("the frame module path is not plain"));
    }
    let declared = if module.starts_with("web/") {
        module.clone()
    } else {
        format!("web/{module}")
    };
    let service = state.daemon.plugins().clone();
    let read = declared.clone();
    let asset_id = id.clone();
    let bytes = plugin_blocking(move || service.asset(&asset_id, &read)).await?;
    let source = String::from_utf8(bytes)
        .map_err(|_| ApiError::invalid("a UI module must be UTF-8 JavaScript"))?;
    let source = source.replace('<', "\\u003c");
    let mut nonce_bytes = [0u8; 16];
    getrandom::fill(&mut nonce_bytes)
        .map_err(|error| ApiError::internal(format!("reading entropy: {error}")))?;
    let nonce = crate::plugins::signature::encode_hex(&nonce_bytes);
    let code = serde_json::to_string(&source)
        .map_err(|error| ApiError::internal(format!("encoding the module: {error}")))?;
    let id_json = serde_json::to_string(&id)
        .map_err(|error| ApiError::internal(format!("encoding the plugin id: {error}")))?;
    let html = format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8">
<meta name="referrer" content="no-referrer">
<style>
html,body{{margin:0;background:var(--panel,#0b121b);color:var(--text,#dbe6f2);
color-scheme:dark;font:13px/1.45 var(--sans,system-ui,sans-serif)}}
#root{{display:block;padding:0.4rem}}
#error{{margin:0.5rem;padding:0.5rem;border:1px solid var(--err,#f87171);
border-radius:7px;color:var(--err,#f87171);font:12px/1.4 var(--mono,monospace)}}
</style></head><body><div id="root"></div>
<script type="module" nonce="{nonce}">
const CODE = {code};
const PLUGIN_ID = {id_json};
const root = document.getElementById('root');
let snapshot = null, locale = 'en', theme = {{}};
let initialized;
const readyForInit = new Promise((resolve) => {{ initialized = resolve }});
const pending = new Map();
let nextId = 1;
const send = (type, extra = {{}}) => parent.postMessage({{anthPlugin: true, type, ...extra}}, '*');
const call = (path, body = null, method = 'POST') => new Promise((resolve, reject) => {{
  const requestId = nextId++;
  pending.set(requestId, {{ resolve, reject }});
  send('call', {{ requestId, method, path, body }});
}});
const api = {{
  call,
  snapshot: () => snapshot,
  locale: () => locale,
  theme: () => theme,
  resize: (height) => send('resize', {{ height }}),
}};
window.addEventListener('message', (event) => {{
  const data = event.data;
  if (!data || data.anthPluginHost !== true) return;
  if (data.type === 'init') {{
    snapshot = data.snapshot;
    locale = data.locale || 'en';
    theme = data.theme || {{}};
    for (const [name, value] of Object.entries(theme)) {{
      if (typeof value === 'string') document.documentElement.style.setProperty('--' + name, value);
    }}
    initialized();
  }}
  if (data.type === 'result') {{
    const wait = pending.get(data.requestId);
    if (!wait) return;
    pending.delete(data.requestId);
    if (data.ok) wait.resolve({{ status: data.status, body: data.body }});
    else wait.reject(new Error(data.message || 'the host refused the call'));
  }}
}});
send('ready');
try {{
  await readyForInit;
  const url = URL.createObjectURL(new Blob([CODE], {{ type: 'text/javascript' }}));
  const module = await import(url);
  URL.revokeObjectURL(url);
  const mount = module.mount || module.default;
  if (typeof mount !== 'function') throw new Error('the module exports no mount()');
  await mount(root, api);
  send('mounted');
}} catch (error) {{
  const message = error && error.message ? error.message : String(error);
  const box = document.createElement('p');
  box.id = 'error';
  box.textContent = message;
  root.replaceChildren(box);
  send('error', {{ message }});
}}
</script></body></html>"#
    );
    let csp = format!(
        "default-src 'none'; script-src 'nonce-{nonce}' blob:; style-src 'unsafe-inline'; img-src data: blob:; connect-src 'none'; frame-ancestors 'self'"
    );
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8".to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CACHE_CONTROL, "no-store".to_string()),
            (header::REFERRER_POLICY, "no-referrer".to_string()),
            (header::CONTENT_SECURITY_POLICY, csp.clone()),
        ],
        html,
    )
        .into_response())
}

async fn plugin_asset(
    State(state): State<AppState>,
    _authed: Authed,
    AxumPath((id, path)): AxumPath<(String, String)>,
) -> Result<HttpResponse, ApiError> {
    if path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || part == ".." || part == ".")
    {
        return Err(ApiError::invalid(
            "the asset path is not a plain module path",
        ));
    }
    let module = format!("web/{path}");
    let content_type = crate::plugins::web::content_type(&module);
    let service = state.daemon.plugins().clone();
    let read_module = module.clone();
    let bytes = plugin_blocking(move || service.asset(&id, &read_module)).await?;
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            (header::CACHE_CONTROL, "no-store".to_string()),
            (header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".to_string()),
        ],
        Body::from(bytes),
    )
        .into_response())
}

async fn plugin_anchor(
    State(state): State<AppState>,
    _authed: Authed,
    AxumPath((id, path)): AxumPath<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<HttpResponse, ApiError> {
    let service = state.daemon.plugins().clone();
    if !service.config().enabled {
        return Err(ApiError::from(WireError::forbidden(
            "plugins are disabled; set `[plugins] enabled = true`",
        )));
    }
    crate::plugins::validate_id(&id).map_err(|error| ApiError::invalid(error.message))?;
    if !matches!(
        method,
        Method::GET | Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    ) {
        return Err(ApiError::from(WireError::invalid(
            "that method is not an anchor method",
        )));
    }
    if method != Method::GET {
        same_origin(&headers)?;
    }
    if body.len() > MAX_ANCHOR_BODY {
        return Err(ApiError::from(WireError::too_large(format!(
            "an anchor body is at most {MAX_ANCHOR_BODY} bytes"
        ))));
    }
    let path = canonical_anchor_path(&path);
    let query = anchor_query(uri.query());
    let body = if body.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&body).into_owned())
    };
    let method = method.as_str().to_string();
    let logger = Arc::clone(state.daemon.logger());
    let reply: PluginReply =
        plugin_blocking(move || service.call_anchor(&id, &method, &path, query, body, logger))
            .await?;
    Ok(plugin_reply_response(reply))
}

fn plugin_reply_response(reply: PluginReply) -> HttpResponse {
    let status = StatusCode::from_u16(reply.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let (body, content_type) = match reply.value {
        serde_json::Value::Null => (String::new(), "text/plain; charset=utf-8"),
        serde_json::Value::String(text) => (text, "text/plain; charset=utf-8"),
        other => (other.to_string(), "application/json"),
    };
    (
        status,
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        body,
    )
        .into_response()
}

/// axum strips the leading slash from a `{*path}` capture; the guest contract
/// always sees one.
fn canonical_anchor_path(capture: &str) -> String {
    let trimmed = capture.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn anchor_query(raw: Option<&str>) -> BTreeMap<String, String> {
    raw.unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (percent_decode(key), percent_decode(value))
        })
        .collect()
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &raw[index + 1..index + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Validates the `interval` query shared by the SSE routes.
fn watch_interval(value: Option<&str>) -> Result<Duration, ApiError> {
    match value {
        Some(raw) => anth_wire::parse_interval(raw).map_err(ApiError::invalid),
        None => Ok(anth_wire::parse_interval(anth_wire::DEFAULT_INTERVAL)
            .expect("the default interval parses")),
    }
}

/// One SSE `error` frame for a failed tick.
fn sse_error(error: &WireError) -> Event {
    Event::default().event("error").data(
        json!({
            "kind": error.kind.as_str(),
            "code": error.code,
            "message": error.message,
        })
        .to_string(),
    )
}

async fn processes(
    State(state): State<AppState>,
    _authed: Authed,
    query: Result<Query<ProcessesQuery>, QueryRejection>,
) -> Result<Json<ProcessSnapshot>, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::Processes {
            limit: query.limit,
            cursor: query.cursor,
        })
        .await
    {
        Response::Processes(snapshot) => Ok(Json(snapshot)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a process listing answered unexpectedly: {other:?}"
        ))),
    }
}

async fn watch_processes(
    State(state): State<AppState>,
    authed: Authed,
    query: Result<Query<ProcessWatchQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    let interval = watch_interval(query.interval.as_deref())?;
    let request = PageRequest {
        limit: Some(query.limit.unwrap_or(500)),
        cursor: None,
    };
    if !request.valid() {
        return Err(ApiError::invalid("the page limit is out of bounds"));
    }

    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let daemon = Arc::clone(&state.daemon);
    let logger = Arc::clone(&state.logger);
    let owner = authed.sid;
    let stream = stream::unfold(ticker, move |mut ticker| {
        let daemon = Arc::clone(&daemon);
        let logger = Arc::clone(&logger);
        let owner = owner.clone();
        let request = request.clone();
        async move {
            ticker.tick().await;
            let event = match daemon
                .execute_as(
                    &owner,
                    Request::Processes {
                        limit: request.limit,
                        cursor: None,
                    },
                )
                .await
            {
                Response::Processes(snapshot) => Event::default()
                    .event("processes")
                    .id(snapshot.timestamp_ms.to_string())
                    .retry(Duration::from_secs(3))
                    .json_data(snapshot)
                    .unwrap_or_else(|error| {
                        sse_error(&WireError::internal(format!(
                            "the process snapshot could not be encoded: {error}"
                        )))
                    }),
                Response::Error(error) => sse_error(&error),
                other => sse_error(&WireError::internal(format!(
                    "a process snapshot answered unexpectedly: {other:?}"
                ))),
            };
            logger.debug("processes", "frame");
            Some((Ok::<_, Infallible>(event), ticker))
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

async fn process_signal(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<ProcessSignalBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    let signal = ProcessSignal::parse(&body.signal)
        .ok_or_else(|| ApiError::invalid(format!("`{}` is not an allowed signal", body.signal)))?;
    match state
        .daemon
        .execute(Request::ProcessSignal {
            pid: body.pid,
            start_ticks: body.start_ticks,
            signal,
            group: body.group,
        })
        .await
    {
        Response::ProcessSignalled { pid, signal } => {
            Ok(Json(json!({ "pid": pid, "signal": signal.as_str() })))
        }
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a process signal answered unexpectedly: {other:?}"
        ))),
    }
}

async fn process_spawn(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<ProcessSpawnBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::ProcessSpawn {
            program: body.program,
            args: body.args,
            cwd: body.cwd,
        })
        .await
    {
        Response::ProcessStarted(started) => Ok(Json(json!({
            "pid": started.pid,
            "program": started.program,
            "args": started.args,
        }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a process spawn answered unexpectedly: {other:?}"
        ))),
    }
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
    query: Result<Query<FilePathQuery>, QueryRejection>,
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

async fn write_file(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<FileWriteBody>, JsonRejection>,
) -> Result<Json<FileWritten>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesWrite {
            path: body.path,
            content: body.content,
            create: body.create,
        })
        .await
    {
        Response::FilesWritten(written) => Ok(Json(written)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a file write answered unexpectedly: {other:?}"
        ))),
    }
}

async fn upload_file(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<FileUploadBody>, JsonRejection>,
) -> Result<Json<FileWritten>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesUpload {
            path: body.path,
            content_base64: body.content_base64,
            create: body.create,
        })
        .await
    {
        Response::FilesWritten(written) => Ok(Json(written)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a file upload answered unexpectedly: {other:?}"
        ))),
    }
}

async fn image_file(
    State(state): State<AppState>,
    authed: Authed,
    query: Result<Query<FilePathQuery>, QueryRejection>,
) -> Result<HttpResponse, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    let (bytes, content_type) = state
        .daemon
        .image(&authed.sid, &query.path)
        .map_err(ApiError::from)?;
    let mut response = binary_response(bytes, content_type);
    response.headers_mut().insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; sandbox"),
    );
    Ok(response)
}

async fn download_file(
    State(state): State<AppState>,
    authed: Authed,
    query: Result<Query<FilePathQuery>, QueryRejection>,
) -> Result<HttpResponse, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    let bytes = state
        .daemon
        .download(&authed.sid, &query.path)
        .map_err(ApiError::from)?;
    let mut response = binary_response(bytes, "application/octet-stream");
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment"),
    );
    Ok(response)
}

async fn trash_file(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<TrashPathBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesTrash { path: body.path })
        .await
    {
        Response::FilesTrashed { path, id } => Ok(Json(json!({ "path": path, "id": id }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a file trash answered unexpectedly: {other:?}"
        ))),
    }
}

async fn list_trash(
    State(state): State<AppState>,
    _authed: Authed,
) -> Result<Json<TrashListing>, ApiError> {
    match state.daemon.execute(Request::FilesTrashList).await {
        Response::FilesTrashList(listing) => Ok(Json(listing)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a trash listing answered unexpectedly: {other:?}"
        ))),
    }
}

async fn restore_trash(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<TrashRestoreBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesTrashRestore {
            id: body.id,
            overwrite: body.overwrite,
        })
        .await
    {
        Response::FilesRestored { id, path } => Ok(Json(json!({ "id": id, "path": path }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "a trash restore answered unexpectedly: {other:?}"
        ))),
    }
}

async fn empty_trash(
    State(state): State<AppState>,
    _authed: Authed,
) -> Result<Json<Value>, ApiError> {
    match state.daemon.execute(Request::FilesTrashEmpty).await {
        Response::FilesTrashEmptied { removed } => Ok(Json(json!({ "removed": removed }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "emptying the trash answered unexpectedly: {other:?}"
        ))),
    }
}

async fn create_archive(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<ArchiveCreateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesArchive {
            path: body.path,
            destination: body.destination,
            format: body.format,
        })
        .await
    {
        Response::FilesArchiveCreated { path } => Ok(Json(json!({ "path": path }))),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "creating an archive answered unexpectedly: {other:?}"
        ))),
    }
}

async fn list_archive(
    State(state): State<AppState>,
    _authed: Authed,
    query: Result<Query<FilePathQuery>, QueryRejection>,
) -> Result<Json<ArchiveListing>, ApiError> {
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesArchiveList { path: query.path })
        .await
    {
        Response::FilesArchiveList(listing) => Ok(Json(listing)),
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "an archive listing answered unexpectedly: {other:?}"
        ))),
    }
}

async fn extract_archive(
    State(state): State<AppState>,
    _authed: Authed,
    body: Result<Json<ArchiveExtractBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body.map_err(|error| ApiError::invalid(error.to_string()))?;
    match state
        .daemon
        .execute(Request::FilesArchiveExtract {
            path: body.path,
            into: body.into,
        })
        .await
    {
        Response::FilesArchiveExtracted { path, entries } => {
            Ok(Json(json!({ "path": path, "entries": entries })))
        }
        Response::Error(error) => Err(ApiError::from(error)),
        other => Err(ApiError::internal(format!(
            "extracting an archive answered unexpectedly: {other:?}"
        ))),
    }
}

async fn ws_terminal(
    authed: Authed,
    headers: HeaderMap,
    query: Result<Query<TerminalQuery>, QueryRejection>,
    websocket: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    same_origin(&headers)?;
    let Query(query) = query.map_err(|error| ApiError::invalid(error.to_string()))?;
    let size = match (query.cols, query.rows) {
        (None, None) => DEFAULT_TERMINAL_SIZE,
        (Some(cols), Some(rows)) => TerminalSize { cols, rows },
        _ => return Err(ApiError::invalid("cols and rows must be provided together")),
    };
    size.validate().map_err(ApiError::invalid)?;

    let daemon = Arc::clone(&state.daemon);
    let owner = authed.sid.clone();
    Ok(websocket
        .max_message_size(crate::terminal_web::MAX_WEB_MESSAGE)
        .max_frame_size(crate::terminal_web::MAX_WEB_MESSAGE)
        .on_upgrade(move |socket| crate::terminal_web::serve_socket(socket, daemon, owner, size)))
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
        return asset_response(asset);
    }

    if www::looks_like_file(path) {
        return ApiError::not_found("no such file").into_response();
    }

    match www::get("index.html") {
        Some(asset) => asset_response(asset),
        None => ApiError::not_found("the web UI is not built").into_response(),
    }
}

/// Serves one shell asset. The shell is tiny and changes between builds, so
/// every asset revalidates instead of relying on a stale cache.
fn asset_response(asset: www::Asset) -> HttpResponse {
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
    response
}

/// A binary file response with the headers every file route shares.
fn binary_response(bytes: Vec<u8>, content_type: &'static str) -> HttpResponse {
    let mut response = HttpResponse::new(Body::from(bytes));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response
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
    append_cookies(&mut response, auth.set_cookies(&grant));
    response
}

/// Appends each cookie as its own `Set-Cookie` header.
fn append_cookies(response: &mut HttpResponse, cookies: Vec<String>) {
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
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
