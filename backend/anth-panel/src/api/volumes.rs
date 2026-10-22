use super::guards::*;
use super::prelude::*;

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

pub(crate) fn volume_snapshot_json(response: CommandResponse) -> Result<VolumeSnapshot, ApiError> {
    match response {
        CommandResponse::VolumeSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::VolumeError(error) => Err(volume_api_error(error)),
        other => Err(unexpected_response(other)),
    }
}
