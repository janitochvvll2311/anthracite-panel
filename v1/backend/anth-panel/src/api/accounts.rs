use super::guards::*;
use super::prelude::*;

/// `GET /api/volumes` — one storage snapshot.
pub async fn get_accounts(
    State(state): State<AppState>,
) -> Result<Json<AccountSnapshot>, ApiError> {
    get_snapshot(&state, Request::AccountList, account_snapshot_json).await
}

pub(crate) fn account_snapshot_json(
    response: CommandResponse,
) -> Result<AccountSnapshot, ApiError> {
    match response {
        CommandResponse::AccountSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::AccountError(error) => Err(account_api_error(error)),
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
