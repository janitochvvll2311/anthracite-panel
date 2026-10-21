use axum::body::{Body, Bytes};
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{Response, StatusCode, Uri};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::StreamExt;
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::Value;

use std::str::FromStr;
use std::sync::Arc;

use crate::app::AppState;
use crate::poll;
use crate::sse::SseStream;
use crate::stream;
use anth_proto::{
    AccountError, AccountErrorKind, AccountSnapshot, AnthClient, DEFAULT_FILE_CHUNK,
    DEFAULT_TERMINAL_SIZE, FileChunk, FileError, FileErrorKind, FileListing, FileWritten,
    FirewallAction, FirewallError, FirewallErrorKind, FirewallProtocol, FirewallSnapshot,
    NetworkError, NetworkErrorKind, NetworkSnapshot, ProcessSignal, ProcessSnapshot,
    ProcessSpawnErrorKind, Request, Response as CommandResponse, TerminalSize, VolumeError,
    VolumeErrorKind, VolumeSnapshot,
};

pub fn router(app: AppState) -> Router {
    let upload_limit = usize::try_from(app.max_upload_bytes())
        .unwrap_or(usize::MAX)
        .max(1);

    Router::new()
        .route("/healthz", get(get_healthz))
        .route("/api/auth/login", post(crate::auth::routes::post_login))
        .route("/api/auth/refresh", post(crate::auth::routes::post_refresh))
        .route("/api/auth/logout", post(crate::auth::routes::post_logout))
        .route("/api/auth/session", get(crate::auth::routes::get_session))
        .route(
            "/api/auth/password",
            post(crate::auth::routes::post_password),
        )
        .route(
            "/api/config",
            get(crate::config_api::get_config).patch(crate::config_api::patch_config),
        )
        .route("/api/host-info", get(get_host_info))
        .route("/api/host-info/watch", get(watch_host_info))
        .route("/api/echo", post(post_echo))
        .route("/api/terminal", get(ws_terminal))
        .route("/api/processes", get(get_processes))
        .route("/api/processes/watch", get(watch_processes))
        .route("/api/processes/{pid}/signal", post(post_process_signal))
        .route("/api/processes/spawn", post(post_process_spawn))
        .route("/api/network", get(get_network))
        .route("/api/network/watch", get(watch_network))
        .route("/api/network/{name}/state", post(post_network_state))
        .route("/api/firewall", get(get_firewall))
        .route("/api/firewall/rules", post(post_firewall_rule))
        .route(
            "/api/firewall/rules/{handle}/delete",
            post(post_firewall_delete),
        )
        .route("/api/firewall/flush", post(post_firewall_flush))
        .route("/api/accounts", get(get_accounts))
        .route("/api/accounts/users", post(post_account_create))
        .route("/api/accounts/users/{name}", post(post_account_modify))
        .route(
            "/api/accounts/users/{name}/delete",
            post(post_account_delete),
        )
        .route(
            "/api/accounts/users/{name}/password",
            post(post_account_password),
        )
        .route("/api/accounts/groups", post(post_account_group_create))
        .route(
            "/api/accounts/groups/{name}",
            post(post_account_group_modify),
        )
        .route(
            "/api/accounts/groups/{name}/delete",
            post(post_account_group_delete),
        )
        .route("/api/volumes", get(get_volumes))
        .route("/api/volumes/mount", post(post_volume_mount))
        .route("/api/volumes/unmount", post(post_volume_unmount))
        .route("/api/files", get(get_files))
        .route("/api/files/content", get(get_file_content))
        .route("/api/files/download", get(download_file))
        .route("/api/files/mkdir", post(post_file_mkdir))
        .route("/api/files/rename", post(post_file_rename))
        .route("/api/files/delete", post(post_file_delete))
        .route("/api/files/upload", post(post_file_upload))
        .layer(DefaultBodyLimit::max(upload_limit))
        .fallback(static_handler)
        .layer(axum::middleware::from_fn_with_state(
            app.clone(),
            crate::auth::middleware::require_auth,
        ))
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
    pub(crate) fn new(status: StatusCode, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }

    pub(crate) fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, detail)
    }

    pub(crate) fn internal(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, detail)
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

/// Query for `GET /api/terminal`. Both members are optional together; the
/// browser normally sends both from the fitted xterm size.
#[derive(Debug, Deserialize)]
struct TerminalQuery {
    cols: Option<u16>,
    rows: Option<u16>,
}

fn requested_terminal_size(query: TerminalQuery) -> Result<TerminalSize, String> {
    let size = match (query.cols, query.rows) {
        (None, None) => DEFAULT_TERMINAL_SIZE,
        (Some(cols), Some(rows)) => TerminalSize { cols, rows },
        _ => return Err("cols and rows must be provided together".to_string()),
    };

    if !size.is_sane() {
        return Err(format!(
            "terminal size {}x{} is outside {}x{}..{}x{}",
            size.cols,
            size.rows,
            TerminalSize::MIN.cols,
            TerminalSize::MIN.rows,
            TerminalSize::MAX.cols,
            TerminalSize::MAX.rows
        ));
    }

    Ok(size)
}

/// `GET /api/terminal` — one interactive PTY per WebSocket.
async fn ws_terminal(
    query: Result<Query<TerminalQuery>, QueryRejection>,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let size = requested_terminal_size(query).map_err(ApiError::bad_request)?;

    Ok(ws
        .max_message_size(crate::terminal::web::MAX_WEB_MESSAGE)
        .max_frame_size(crate::terminal::web::MAX_WEB_MESSAGE)
        .on_upgrade(move |socket| crate::terminal::web::serve_socket(socket, state, size)))
}

/// `GET /api/processes` — one process-table snapshot.
pub async fn get_processes(
    State(state): State<AppState>,
) -> Result<Json<ProcessSnapshot>, ApiError> {
    get_snapshot(&state, Request::ProcessList, process_snapshot_json).await
}

/// `GET /api/processes/watch?interval=2s` — a process snapshot per interval as SSE.
pub async fn watch_processes(
    State(state): State<AppState>,
    query: Result<Query<WatchQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    watch_snapshots(
        state,
        query,
        "processes",
        Request::ProcessList,
        process_snapshot_json,
    )
    .await
}

/// Body of `POST /api/processes/{pid}/signal`.
#[derive(Debug, Deserialize)]
pub struct SignalArgs {
    pub signal: String,
}

/// `POST /api/processes/{pid}/signal` — deliver an allowlisted signal.
pub async fn post_process_signal(
    State(state): State<AppState>,
    path: Result<Path<u32>, PathRejection>,
    payload: Result<Json<SignalArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(pid) = path.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let signal = ProcessSignal::from_str(&args.signal).map_err(ApiError::bad_request)?;

    if !state.config().await.process.control {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "process control is disabled",
        ));
    }

    match exchange(&state, &Request::ProcessSignal { pid, signal }).await? {
        CommandResponse::ProcessSignalled { pid, signal } => {
            Ok(Json(serde_json::json!({ "pid": pid, "signal": signal })))
        }
        CommandResponse::Error { detail } => Err(ApiError::bad_request(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// Body of `POST /api/processes/spawn`.
#[derive(Debug, Deserialize)]
pub struct SpawnArgs {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
}

/// `POST /api/processes/spawn` — start a program from the allowlist.
///
/// The daemon re-checks the switch and the allowlist, so this handler does not
/// duplicate the policy: asking twice is how the two copies drift apart. It only
/// maps the refusal kind to a status code a client can act on.
pub async fn post_process_spawn(
    State(state): State<AppState>,
    payload: Result<Json<SpawnArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;

    let response = exchange(
        &state,
        &Request::ProcessSpawn {
            program: args.program,
            args: args.args,
            cwd: args.cwd,
        },
    )
    .await?;

    match response {
        CommandResponse::ProcessStarted(started) => Ok(Json(serde_json::json!({
            "pid": started.pid,
            "program": started.program,
            "args": started.args,
        }))),
        CommandResponse::ProcessSpawnError(error) => Err(match error.kind {
            // One refusal class: the operator is not allowed to do this, and a
            // retry with the same input will fail the same way.
            ProcessSpawnErrorKind::Disabled
            | ProcessSpawnErrorKind::NotAllowed
            | ProcessSpawnErrorKind::NotAbsolute
            | ProcessSpawnErrorKind::NotExecutable
            | ProcessSpawnErrorKind::PermissionDenied => {
                ApiError::new(StatusCode::FORBIDDEN, error.detail)
            }
            ProcessSpawnErrorKind::NotFound => ApiError::new(StatusCode::NOT_FOUND, error.detail),
            ProcessSpawnErrorKind::Failed => ApiError::bad_request(error.detail),
        }),
        other => Err(unexpected_response(other)),
    }
}

/// `GET /api/network` — one network-interface snapshot.
pub async fn get_network(State(state): State<AppState>) -> Result<Json<NetworkSnapshot>, ApiError> {
    get_snapshot(&state, Request::NetworkList, network_snapshot_json).await
}

/// `GET /api/network/watch?interval=2s` — interface snapshots as SSE.
pub async fn watch_network(
    State(state): State<AppState>,
    query: Result<Query<WatchQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    watch_snapshots(
        state,
        query,
        "network",
        Request::NetworkList,
        network_snapshot_json,
    )
    .await
}

/// Body of `POST /api/network/{name}/state`.
#[derive(Debug, Deserialize)]
pub struct NetworkStateArgs {
    pub up: bool,
}

/// `GET /api/volumes` — one storage snapshot.
pub async fn get_accounts(
    State(state): State<AppState>,
) -> Result<Json<AccountSnapshot>, ApiError> {
    get_snapshot(&state, Request::AccountList, account_snapshot_json).await
}

fn account_snapshot_json(response: CommandResponse) -> Result<AccountSnapshot, ApiError> {
    match response {
        CommandResponse::AccountSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::AccountError(error) => Err(account_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

pub async fn get_volumes(State(state): State<AppState>) -> Result<Json<VolumeSnapshot>, ApiError> {
    get_snapshot(&state, Request::VolumeList, volume_snapshot_json).await
}

/// Body of `POST /api/volumes/mount`.
#[derive(Debug, Deserialize)]
pub struct VolumeMountBody {
    pub device: String,
    pub target: String,
    #[serde(default)]
    pub fs_type: Option<String>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub allow_removable: bool,
}

/// `POST /api/volumes/mount` — mount one device on a directory.
pub async fn post_volume_mount(
    State(state): State<AppState>,
    payload: Result<Json<VolumeMountBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_volume_control(&state).await?;

    match exchange(
        &state,
        &Request::VolumeMount {
            device: body.device,
            target: body.target,
            fs_type: body.fs_type,
            read_only: body.read_only,
            allow_removable: body.allow_removable,
        },
    )
    .await?
    {
        CommandResponse::VolumeChanged { message } => {
            Ok(Json(serde_json::json!({ "message": message })))
        }
        CommandResponse::VolumeError(error) => Err(volume_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// Body of `POST /api/volumes/unmount`.
#[derive(Debug, Deserialize)]
pub struct VolumeUnmountBody {
    pub target: String,
}

/// `POST /api/volumes/unmount` — unmount one directory.
pub async fn post_volume_unmount(
    State(state): State<AppState>,
    payload: Result<Json<VolumeUnmountBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_volume_control(&state).await?;

    match exchange(
        &state,
        &Request::VolumeUnmount {
            target: body.target,
        },
    )
    .await?
    {
        CommandResponse::VolumeChanged { message } => {
            Ok(Json(serde_json::json!({ "message": message })))
        }
        CommandResponse::VolumeError(error) => Err(volume_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// Body of `POST /api/accounts/users`.
#[derive(Debug, Deserialize)]
pub struct AccountCreateBody {
    pub name: String,
    #[serde(default)]
    pub uid: Option<u32>,
    #[serde(default)]
    pub gid: Option<u32>,
    #[serde(default)]
    pub gecos: Option<String>,
    #[serde(default)]
    pub home: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub system: bool,
}

/// `POST /api/accounts/users` — create a locked local user.
pub async fn post_account_create(
    State(state): State<AppState>,
    payload: Result<Json<AccountCreateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_account_control(&state).await?;
    account_change(
        &state,
        Request::AccountCreate {
            name: body.name,
            uid: body.uid,
            gid: body.gid,
            gecos: body.gecos,
            home: body.home,
            shell: body.shell,
            system: body.system,
        },
    )
    .await
}

/// Body of `POST /api/accounts/users/{name}`. Only the fields present change.
#[derive(Debug, Deserialize)]
pub struct AccountModifyBody {
    #[serde(default)]
    pub gecos: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub home: Option<String>,
    #[serde(default)]
    pub primary_gid: Option<u32>,
    #[serde(default)]
    pub locked: Option<bool>,
}

/// `POST /api/accounts/users/{name}` — change a local user.
pub async fn post_account_modify(
    State(state): State<AppState>,
    Path(name): Path<String>,
    payload: Result<Json<AccountModifyBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_account_control(&state).await?;
    account_change(
        &state,
        Request::AccountModify {
            name,
            gecos: body.gecos,
            shell: body.shell,
            home: body.home,
            primary_gid: body.primary_gid,
            locked: body.locked,
        },
    )
    .await
}

/// Body of `POST /api/accounts/users/{name}/delete`.
#[derive(Debug, Deserialize, Default)]
pub struct AccountDeleteBody {
    #[serde(default)]
    pub remove_home: bool,
}

/// `POST /api/accounts/users/{name}/delete` — delete a local user.
pub async fn post_account_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
    payload: Result<Json<AccountDeleteBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    // An empty body is the common case, so a rejection is only fatal when the
    // body was non-empty and unparseable.
    let remove_home = match payload {
        Ok(Json(body)) => body.remove_home,
        Err(_) => false,
    };
    require_account_control(&state).await?;
    account_change(&state, Request::AccountDelete { name, remove_home }).await
}

/// Body of `POST /api/accounts/users/{name}/password`.
#[derive(Debug, Deserialize)]
pub struct AccountPasswordBody {
    pub password: String,
}

/// `POST /api/accounts/users/{name}/password` — set a password.
pub async fn post_account_password(
    State(state): State<AppState>,
    Path(name): Path<String>,
    payload: Result<Json<AccountPasswordBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_account_password_control(&state).await?;
    account_change(
        &state,
        Request::AccountSetPassword {
            name,
            password: anth_proto::SecretString::new(body.password),
        },
    )
    .await
}

/// Body of `POST /api/accounts/groups`.
#[derive(Debug, Deserialize)]
pub struct AccountGroupCreateBody {
    pub name: String,
    #[serde(default)]
    pub gid: Option<u32>,
    #[serde(default)]
    pub system: bool,
}

/// `POST /api/accounts/groups` — create a local group.
pub async fn post_account_group_create(
    State(state): State<AppState>,
    payload: Result<Json<AccountGroupCreateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_account_control(&state).await?;
    account_change(
        &state,
        Request::AccountGroupCreate {
            name: body.name,
            gid: body.gid,
            system: body.system,
        },
    )
    .await
}

/// Body of `POST /api/accounts/groups/{name}`.
#[derive(Debug, Deserialize)]
pub struct AccountGroupModifyBody {
    #[serde(default)]
    pub gid: Option<u32>,
    #[serde(default)]
    pub members: Option<Vec<String>>,
}

/// `POST /api/accounts/groups/{name}` — change a group.
pub async fn post_account_group_modify(
    State(state): State<AppState>,
    Path(name): Path<String>,
    payload: Result<Json<AccountGroupModifyBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_account_control(&state).await?;
    account_change(
        &state,
        Request::AccountGroupModify {
            name,
            gid: body.gid,
            members: body.members,
        },
    )
    .await
}

/// `POST /api/accounts/groups/{name}/delete` — delete a local group.
pub async fn post_account_group_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_account_control(&state).await?;
    account_change(&state, Request::AccountGroupDelete { name }).await
}

async fn account_change(state: &AppState, request: Request) -> Result<Json<Value>, ApiError> {
    match exchange(state, &request).await? {
        CommandResponse::AccountChanged { message } => {
            Ok(Json(serde_json::json!({ "message": message })))
        }
        CommandResponse::AccountError(error) => Err(account_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

async fn require_account_control(state: &AppState) -> Result<(), ApiError> {
    if !state.config().await.accounts.control {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "account control is disabled",
        ));
    }
    Ok(())
}

async fn require_account_password_control(state: &AppState) -> Result<(), ApiError> {
    if !state.config().await.accounts.manage_passwords {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "password changes are disabled",
        ));
    }
    Ok(())
}

async fn require_volume_control(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.volumes.control {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "volume control is disabled",
        ))
    }
}

fn account_api_error(error: AccountError) -> ApiError {
    let status = match error.kind {
        AccountErrorKind::Disabled
        | AccountErrorKind::Protected
        | AccountErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        AccountErrorKind::NotFound => StatusCode::NOT_FOUND,
        AccountErrorKind::Conflict => StatusCode::CONFLICT,
        AccountErrorKind::Unsupported => StatusCode::NOT_IMPLEMENTED,
        AccountErrorKind::Invalid => StatusCode::BAD_REQUEST,
        AccountErrorKind::Failed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.detail)
}

fn volume_api_error(error: VolumeError) -> ApiError {
    let status = match error.kind {
        VolumeErrorKind::Disabled
        | VolumeErrorKind::Protected
        | VolumeErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        VolumeErrorKind::NotFound => StatusCode::NOT_FOUND,
        VolumeErrorKind::Busy => StatusCode::CONFLICT,
        VolumeErrorKind::Invalid => StatusCode::BAD_REQUEST,
        VolumeErrorKind::Failed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.detail)
}

fn volume_snapshot_json(response: CommandResponse) -> Result<VolumeSnapshot, ApiError> {
    match response {
        CommandResponse::VolumeSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::VolumeError(error) => Err(volume_api_error(error)),
        other => Err(unexpected_response(other)),
    }
}

/// `POST /api/network/{name}/state` — bring one interface up or down.
pub async fn post_network_state(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    payload: Result<Json<NetworkStateArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(name) = path.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_network_control(&state).await?;

    match exchange(
        &state,
        &Request::NetworkSetState {
            name: name.clone(),
            up: args.up,
        },
    )
    .await?
    {
        CommandResponse::NetworkStateChanged { name, up } => {
            Ok(Json(serde_json::json!({ "name": name, "up": up })))
        }
        CommandResponse::NetworkError(error) => Err(network_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

async fn require_network_control(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.network.control {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "network control is disabled",
        ))
    }
}

fn network_api_error(error: NetworkError) -> ApiError {
    let status = match error.kind {
        NetworkErrorKind::Disabled | NetworkErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        NetworkErrorKind::Protected => StatusCode::FORBIDDEN,
        NetworkErrorKind::NotFound => StatusCode::NOT_FOUND,
        NetworkErrorKind::Invalid => StatusCode::BAD_REQUEST,
        NetworkErrorKind::Failed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.detail)
}

fn gateway(detail: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, detail)
}

fn unexpected_response(response: CommandResponse) -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        format!("unexpected daemon response: {response:?}"),
    )
}

fn network_snapshot_json(response: CommandResponse) -> Result<NetworkSnapshot, ApiError> {
    match response {
        CommandResponse::NetworkSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::NetworkError(error) => Err(network_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `GET /api/firewall` — one snapshot of the managed nftables table.
pub async fn get_firewall(
    State(state): State<AppState>,
) -> Result<Json<FirewallSnapshot>, ApiError> {
    let response = exchange(&state, &Request::FirewallStatus).await?;
    firewall_snapshot_json(response).map(Json)
}

/// Body of `POST /api/firewall/rules`.
#[derive(Debug, Deserialize)]
pub struct FirewallRuleArgs {
    pub action: FirewallAction,
    pub port: u16,
    pub protocol: Option<FirewallProtocol>,
    pub comment: Option<String>,
}

/// `POST /api/firewall/rules` — add an allow/deny rule.
pub async fn post_firewall_rule(
    State(state): State<AppState>,
    payload: Result<Json<FirewallRuleArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_firewall_control(&state).await?;

    match exchange(
        &state,
        &Request::FirewallAddRule {
            action: args.action,
            port: args.port,
            protocol: args.protocol.unwrap_or(FirewallProtocol::Tcp),
            comment: args.comment,
        },
    )
    .await?
    {
        CommandResponse::FirewallChanged { message } => {
            Ok(Json(serde_json::json!({ "message": message })))
        }
        CommandResponse::FirewallError(error) => Err(firewall_api_error(error)),
        CommandResponse::Error { detail } => Err(ApiError::new(StatusCode::BAD_GATEWAY, detail)),
        other => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected daemon response: {other:?}"),
        )),
    }
}

/// `POST /api/firewall/rules/{handle}/delete` — delete one rule by handle.
pub async fn post_firewall_delete(
    State(state): State<AppState>,
    path: Result<Path<u64>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(handle) = path.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_firewall_control(&state).await?;

    match exchange(&state, &Request::FirewallDeleteRule { handle }).await? {
        CommandResponse::FirewallChanged { message } => {
            Ok(Json(serde_json::json!({ "message": message })))
        }
        CommandResponse::FirewallError(error) => Err(firewall_api_error(error)),
        CommandResponse::Error { detail } => Err(ApiError::new(StatusCode::BAD_GATEWAY, detail)),
        other => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected daemon response: {other:?}"),
        )),
    }
}

/// `POST /api/firewall/flush` — remove every managed rule.
pub async fn post_firewall_flush(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    require_firewall_control(&state).await?;

    match exchange(&state, &Request::FirewallFlush).await? {
        CommandResponse::FirewallChanged { message } => {
            Ok(Json(serde_json::json!({ "message": message })))
        }
        CommandResponse::FirewallError(error) => Err(firewall_api_error(error)),
        CommandResponse::Error { detail } => Err(ApiError::new(StatusCode::BAD_GATEWAY, detail)),
        other => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected daemon response: {other:?}"),
        )),
    }
}

async fn require_firewall_control(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.firewall.control {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "firewall control is disabled",
        ))
    }
}

fn firewall_api_error(error: FirewallError) -> ApiError {
    let status = match error.kind {
        FirewallErrorKind::Disabled | FirewallErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        FirewallErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        FirewallErrorKind::Invalid => StatusCode::BAD_REQUEST,
        FirewallErrorKind::NotFound => StatusCode::NOT_FOUND,
        FirewallErrorKind::Failed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.detail)
}

fn firewall_snapshot_json(response: CommandResponse) -> Result<FirewallSnapshot, ApiError> {
    match response {
        CommandResponse::FirewallSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::FirewallError(error) => Err(firewall_api_error(error)),
        CommandResponse::Error { detail } => Err(ApiError::new(StatusCode::BAD_GATEWAY, detail)),
        other => Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!("unexpected daemon response: {other:?}"),
        )),
    }
}

/// Query for filesystem routes. A missing `path` means the virtual root.
#[derive(Debug, Deserialize)]
pub struct FileQuery {
    pub path: Option<String>,
}

/// Query for `POST /api/files/upload`.
#[derive(Debug, Deserialize)]
pub struct FileUploadQuery {
    pub path: String,
    #[serde(default)]
    pub overwrite: bool,
}

#[derive(Debug, Deserialize)]
pub struct FileMkdirArgs {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
}

#[derive(Debug, Deserialize)]
pub struct FileRenameArgs {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
pub struct FileDeleteArgs {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
}

/// `GET /api/files?path=/` — one directory listing.
pub async fn get_files(
    State(state): State<AppState>,
    query: Result<Query<FileQuery>, QueryRejection>,
) -> Result<Json<FileListing>, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let path = query.path.unwrap_or_else(|| "/".to_string());

    match exchange(&state, &Request::FileList { path }).await? {
        CommandResponse::FileListing(listing) => Ok(Json(listing)),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `GET /api/files/content?path=/...` — inline preview, capped by
/// `[files] max_preview_bytes`.
pub async fn get_file_content(
    State(state): State<AppState>,
    query: Result<Query<FileQuery>, QueryRejection>,
) -> Result<Response<Body>, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let path = query.path.unwrap_or_else(|| "/".to_string());
    let (bytes, size, truncated) = read_preview(&state, &path).await?;

    file_body(
        &path,
        Body::from(bytes),
        size,
        truncated,
        "inline",
        Some("no-store"),
    )
}

/// `GET /api/files/download?path=/...` — stream the whole file.
pub async fn download_file(
    State(state): State<AppState>,
    query: Result<Query<FileQuery>, QueryRejection>,
) -> Result<Response<Body>, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let path = query.path.unwrap_or_else(|| "/".to_string());

    let mut client = state
        .create_client()
        .await
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "daemon unavailable"))?;
    let first = query_file_chunk(&mut client, &path, 0).await?;
    let size = first.size;
    let offset = first.offset + first.bytes.len() as u64;
    let eof = first.eof;
    let first_bytes = first.bytes;

    let tail_path = path.clone();
    let tail = futures::stream::try_unfold(
        (client, tail_path, offset, eof),
        |(mut client, path, offset, eof)| async move {
            if eof {
                return Ok(None);
            }
            let chunk = query_file_chunk(&mut client, &path, offset)
                .await
                .map_err(|error| std::io::Error::other(error.detail()))?;
            let next_offset = chunk.offset + chunk.bytes.len() as u64;
            let next_eof = chunk.eof;
            Ok::<_, std::io::Error>(Some((
                Bytes::from(chunk.bytes),
                (client, path, next_offset, next_eof),
            )))
        },
    );
    let stream =
        futures::stream::once(async move { Ok::<Bytes, std::io::Error>(Bytes::from(first_bytes)) })
            .chain(tail);

    file_body(
        &path,
        Body::from_stream(stream),
        size,
        false,
        "attachment",
        Some("no-store"),
    )
}

/// `POST /api/files/mkdir` — create one directory.
pub async fn post_file_mkdir(
    State(state): State<AppState>,
    payload: Result<Json<FileMkdirArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    match exchange(
        &state,
        &Request::FileMkdir {
            path: args.path,
            recursive: args.recursive,
        },
    )
    .await?
    {
        CommandResponse::FileCreated { path } => Ok(Json(serde_json::json!({ "path": path }))),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `POST /api/files/rename` — rename one entry without overwriting.
pub async fn post_file_rename(
    State(state): State<AppState>,
    payload: Result<Json<FileRenameArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    match exchange(
        &state,
        &Request::FileRename {
            from: args.from,
            to: args.to,
        },
    )
    .await?
    {
        CommandResponse::FileRenamed { from, to } => {
            Ok(Json(serde_json::json!({ "from": from, "to": to })))
        }
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `POST /api/files/delete` — delete one entry.
pub async fn post_file_delete(
    State(state): State<AppState>,
    payload: Result<Json<FileDeleteArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    match exchange(
        &state,
        &Request::FileDelete {
            path: args.path,
            recursive: args.recursive,
        },
    )
    .await?
    {
        CommandResponse::FileDeleted { path } => Ok(Json(serde_json::json!({ "path": path }))),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `POST /api/files/upload?path=/...` — raw request body, streamed to the daemon.
pub async fn post_file_upload(
    State(state): State<AppState>,
    query: Result<Query<FileUploadQuery>, QueryRejection>,
    body: Body,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    let written = upload_stream(&state, &query.path, query.overwrite, body).await?;
    Ok(Json(
        serde_json::json!({ "path": written.path, "size": written.size }),
    ))
}

async fn require_file_write(state: &AppState) -> Result<(), ApiError> {
    if state.config().await.files.write {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "file writes are disabled",
        ))
    }
}

async fn read_preview(state: &AppState, path: &str) -> Result<(Vec<u8>, u64, bool), ApiError> {
    let max = state.config().await.files.max_preview_bytes;
    let mut client = state
        .create_client()
        .await
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "daemon unavailable"))?;

    let mut bytes = Vec::new();
    let mut offset = 0u64;
    let mut size;
    let mut truncated = false;

    loop {
        let chunk = query_file_chunk(&mut client, path, offset).await?;
        size = chunk.size;

        let remaining = max.saturating_sub(bytes.len() as u64) as usize;
        if remaining == 0 {
            truncated = !chunk.eof;
            break;
        }

        let take = chunk.bytes.len().min(remaining);
        bytes.extend_from_slice(&chunk.bytes[..take]);
        if take < chunk.bytes.len() {
            truncated = true;
            break;
        }

        offset = chunk.offset + chunk.bytes.len() as u64;
        if chunk.eof {
            break;
        }
        if bytes.len() as u64 >= max {
            truncated = true;
            break;
        }
    }

    Ok((bytes, size, truncated))
}

async fn query_file_chunk(
    client: &mut AnthClient,
    path: &str,
    offset: u64,
) -> Result<FileChunk, ApiError> {
    match query_command(
        client,
        &Request::FileRead {
            path: path.to_string(),
            offset,
            len: DEFAULT_FILE_CHUNK,
        },
    )
    .await?
    {
        CommandResponse::FileChunk(chunk) => Ok(chunk),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

async fn upload_stream(
    state: &AppState,
    path: &str,
    overwrite: bool,
    body: Body,
) -> Result<FileWritten, ApiError> {
    let mut client = state
        .create_client()
        .await
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "daemon unavailable"))?;

    let id = match query_command(
        &mut client,
        &Request::FileWriteStart {
            path: path.to_string(),
            overwrite,
        },
    )
    .await?
    {
        CommandResponse::FileWriteStarted { id } => id,
        CommandResponse::FileError(error) => return Err(file_api_error(error)),
        CommandResponse::Error { detail } => {
            return Err(gateway(detail));
        }
        other => {
            return Err(unexpected_response(other));
        }
    };

    let max_upload = state.config().await.files.max_upload_bytes;
    let result = forward_upload(&mut client, id, body, max_upload).await;
    if result.is_err() {
        let _ = query_command(&mut client, &Request::FileWriteAbort { id }).await;
    }
    result
}

async fn forward_upload(
    client: &mut AnthClient,
    id: u32,
    body: Body,
    max_bytes: u64,
) -> Result<FileWritten, ApiError> {
    let mut stream = body.into_data_stream();
    let mut received = 0u64;

    while let Some(frame) = stream.next().await {
        let bytes = frame.map_err(|error| {
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("upload body rejected: {error}"),
            )
        })?;
        received = received
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "upload size overflow"))?;
        if received > max_bytes {
            return Err(ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("upload exceeds files.max_upload_bytes ({max_bytes})"),
            ));
        }

        for chunk in bytes.chunks(DEFAULT_FILE_CHUNK as usize) {
            send_upload_chunk(client, id, chunk).await?;
        }
    }

    match query_command(client, &Request::FileWriteFinish { id }).await? {
        CommandResponse::FileWritten(written) => Ok(written),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

async fn send_upload_chunk(client: &mut AnthClient, id: u32, bytes: &[u8]) -> Result<(), ApiError> {
    match query_command(
        client,
        &Request::FileWriteChunk {
            id,
            bytes: bytes.to_vec(),
        },
    )
    .await?
    {
        CommandResponse::FileWriteAccepted { .. } => Ok(()),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

fn file_body(
    path: &str,
    body: Body,
    size: u64,
    truncated: bool,
    disposition: &str,
    cache: Option<&str>,
) -> Result<Response<Body>, ApiError> {
    let guessed = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = if disposition == "inline" && !is_safe_preview_image(guessed.as_ref()) {
        "text/plain; charset=utf-8"
    } else {
        guessed.as_ref()
    };

    let mut builder = Response::builder()
        .header("content-type", content_type)
        .header("x-content-type-options", "nosniff")
        .header(
            "content-disposition",
            content_disposition(disposition, path),
        )
        .header("x-anth-size", size.to_string())
        .header("x-anth-truncated", if truncated { "true" } else { "false" });
    if let Some(cache) = cache {
        builder = builder.header("cache-control", cache);
    }

    builder
        .body(body)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Images that browsers can render safely in an `<img>` without executing
/// active content. SVG is deliberately excluded.
fn is_safe_preview_image(mime: &str) -> bool {
    matches!(
        mime,
        "image/png"
            | "image/apng"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "image/avif"
            | "image/bmp"
            | "image/x-icon"
            | "image/vnd.microsoft.icon"
    )
}

fn content_disposition(kind: &str, path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or("file");
    let safe: String = name
        .chars()
        .map(|c| match c {
            '"' | '\\' | '\r' | '\n' => '_',
            other if !other.is_ascii() => '_',
            other => other,
        })
        .collect();
    format!("{kind}; filename=\"{safe}\"")
}

fn file_api_error(error: FileError) -> ApiError {
    let status = match error.kind {
        FileErrorKind::NotFound => StatusCode::NOT_FOUND,
        FileErrorKind::PermissionDenied | FileErrorKind::Disabled => StatusCode::FORBIDDEN,
        FileErrorKind::AlreadyExists | FileErrorKind::DirectoryNotEmpty => StatusCode::CONFLICT,
        FileErrorKind::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        FileErrorKind::InvalidPath | FileErrorKind::NotADirectory | FileErrorKind::IsADirectory => {
            StatusCode::BAD_REQUEST
        }
        FileErrorKind::Io => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.detail)
}

/// `GET /api/host-info` — one snapshot.
pub async fn get_host_info(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    get_snapshot(&state, Request::HostInfo, host_info_json).await
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
        other => Err(unexpected_response(other)),
    }
}

/// `GET /api/host-info/watch?interval=2s` — a snapshot per interval as SSE.
pub async fn watch_host_info(
    State(state): State<AppState>,
    query: Result<Query<WatchQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    watch_snapshots(state, query, "host-info", Request::HostInfo, host_info_json).await
}

/// Reads one snapshot and applies the endpoint's response mapper.
async fn get_snapshot<T, F>(state: &AppState, request: Request, map: F) -> Result<Json<T>, ApiError>
where
    T: serde::Serialize,
    F: FnOnce(CommandResponse) -> Result<T, ApiError>,
{
    map(exchange(state, &request).await?).map(Json)
}

/// Builds the common SSE response used by every snapshot endpoint.
async fn watch_snapshots<T, F>(
    state: AppState,
    query: Result<Query<WatchQuery>, QueryRejection>,
    event: &'static str,
    request: Request,
    map: F,
) -> Result<impl IntoResponse, ApiError>
where
    T: serde::Serialize + Send + 'static,
    F: Fn(CommandResponse) -> Result<T, ApiError> + Clone + Send + 'static,
{
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
    let request = Arc::new(request);
    let items = poll::stream(period, move || {
        let client = Arc::clone(&client);
        let request = Arc::clone(&request);
        let map = map.clone();
        async move {
            let mut client = client.lock().await;
            query_command(&mut client, &request)
                .await
                .and_then(map)
                .map_err(error_detail)
        }
    });

    Ok(SseStream::new(event).stream(items))
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
        .map_err(|e| gateway(format!("send failed: {e}")))?;

    match client.receive_response().await {
        Some(Ok(response)) => Ok(response),
        Some(Err(e)) => Err(ApiError::bad_request(format!("malformed response: {e}"))),
        None => Err(gateway("daemon closed the connection")),
    }
}

fn host_info_json(response: CommandResponse) -> Result<Value, ApiError> {
    match response {
        CommandResponse::HostInfo(info) => Ok(Value::from(info.as_ref())),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

fn process_snapshot_json(response: CommandResponse) -> Result<ProcessSnapshot, ApiError> {
    match response {
        CommandResponse::ProcessSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// Text for an SSE error frame, where only a payload is possible.
fn error_detail(error: ApiError) -> String {
    error.detail
}

#[derive(RustEmbed)]
#[folder = "www"]
struct Asset;

/// Serves the embedded SPA, and answers an unknown `/api` path with JSON.
///
/// Falling through to `index.html` would answer `200 text/html` for a typo in a
/// client, which is indistinguishable from a working route until the JSON parse
/// fails. The SPA fallback stays for browser routes only.
async fn static_handler(uri: Uri) -> impl IntoResponse {
    if uri.path() == "/api" || uri.path().starts_with("/api/") {
        return ApiError::new(StatusCode::NOT_FOUND, "no such API route").into_response();
    }

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
