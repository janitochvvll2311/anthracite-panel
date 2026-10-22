use super::guards::*;
use super::prelude::*;

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

pub(crate) fn firewall_snapshot_json(
    response: CommandResponse,
) -> Result<FirewallSnapshot, ApiError> {
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
