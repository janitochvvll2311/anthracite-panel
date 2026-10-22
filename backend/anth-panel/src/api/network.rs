use super::guards::*;
use super::prelude::*;

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

/// The client address, or `None` when the server has no connect info.
///
/// axum's `ConnectInfo` extractor is mandatory and answers `500` when the
/// router was built without the connect-info layer, which is exactly what the
/// test router does. The peer address only strengthens one guard, so a missing
/// one must degrade to "unknown" instead of failing the request.
pub struct PeerAddr(pub Option<SocketAddr>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for PeerAddr {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(Self(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(address)| *address),
        ))
    }
}

/// Body of `POST /api/network`.
#[derive(serde::Deserialize)]
pub struct NetworkCreateBody {
    pub name: String,
    pub kind: NetworkInterfaceKind,
    #[serde(default)]
    pub peer: Option<String>,
    #[serde(default)]
    pub link: Option<String>,
    #[serde(default)]
    pub vlan_id: Option<u16>,
    #[serde(default)]
    pub mtu: Option<u32>,
    #[serde(default)]
    pub up: bool,
}

/// `POST /api/network` — create one virtual interface.
pub async fn post_network_create(
    State(state): State<AppState>,
    payload: Result<Json<NetworkCreateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_network_manage(&state).await?;

    let request = Request::NetworkCreate(NetworkCreateArgs {
        name: body.name,
        kind: body.kind,
        peer: body.peer,
        link: body.link,
        vlan_id: body.vlan_id,
        mtu: body.mtu,
        up: body.up,
    });
    network_change(&state, request).await
}

/// Body of `PATCH /api/network/{name}`.
#[derive(serde::Deserialize)]
pub struct NetworkEditBody {
    #[serde(default)]
    pub mtu: Option<u32>,
    #[serde(default)]
    pub up: Option<bool>,
}

/// `PATCH /api/network/{name}` — change an MTU or the administrative state.
pub async fn patch_network(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    payload: Result<Json<NetworkEditBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(name) = path.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_network_manage(&state).await?;

    network_change(
        &state,
        Request::NetworkEdit(NetworkEditArgs {
            name,
            mtu: body.mtu,
            up: body.up,
        }),
    )
    .await
}

/// Body of `POST /api/network/{name}/rename`.
#[derive(serde::Deserialize)]
pub struct NetworkRenameBody {
    pub new_name: String,
}

/// `POST /api/network/{name}/rename` — rename an interface that is down.
pub async fn post_network_rename(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    payload: Result<Json<NetworkRenameBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(name) = path.map_err(|e| ApiError::bad_request(e.body_text()))?;
    let Json(body) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_network_manage(&state).await?;

    network_change(
        &state,
        Request::NetworkRename {
            name,
            new_name: body.new_name,
        },
    )
    .await
}

/// `POST /api/network/{name}/delete` — delete an interface.
///
/// The client address is forwarded to the daemon, which refuses to delete the
/// interface the request arrived through. `ConnectInfo` is absent when the
/// server was started without it, so a missing peer degrades to "unknown"
/// rather than silently allowing the delete.
pub async fn post_network_delete(
    State(state): State<AppState>,
    path: Result<Path<String>, PathRejection>,
    peer: PeerAddr,
) -> Result<Json<Value>, ApiError> {
    let Path(name) = path.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_network_manage(&state).await?;

    network_change(
        &state,
        Request::NetworkDelete {
            name,
            peer: peer.0.map(|address| address.ip().to_string()),
        },
    )
    .await
}

async fn network_change(state: &AppState, request: Request) -> Result<Json<Value>, ApiError> {
    match exchange(state, &request).await? {
        CommandResponse::NetworkChanged { message } => {
            Ok(Json(serde_json::json!({ "message": message })))
        }
        CommandResponse::NetworkError(error) => Err(network_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

pub(crate) fn network_snapshot_json(
    response: CommandResponse,
) -> Result<NetworkSnapshot, ApiError> {
    match response {
        CommandResponse::NetworkSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::NetworkError(error) => Err(network_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
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
