use super::files::file_api_error;
use super::guards::*;
use super::prelude::*;

#[derive(Debug, Deserialize)]
pub struct TrashPathArgs {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct TrashRestoreArgs {
    pub id: String,
    #[serde(default)]
    pub overwrite: bool,
}

/// `POST /api/files/trash` — move one entry to the trash.
pub async fn post_file_trash(
    State(state): State<AppState>,
    payload: Result<Json<TrashPathArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    match exchange(&state, &Request::FileTrash { path: args.path }).await? {
        CommandResponse::FileTrashed { path, id } => {
            Ok(Json(serde_json::json!({ "path": path, "id": id })))
        }
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `GET /api/files/trash/list` — what the trash holds.
pub async fn get_trash(State(state): State<AppState>) -> Result<Json<TrashListing>, ApiError> {
    match exchange(&state, &Request::FileTrashList).await? {
        CommandResponse::TrashListing(listing) => Ok(Json(listing)),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `POST /api/files/trash/restore` — put one entry back.
pub async fn post_trash_restore(
    State(state): State<AppState>,
    payload: Result<Json<TrashRestoreArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    match exchange(
        &state,
        &Request::FileTrashRestore {
            id: args.id,
            overwrite: args.overwrite,
        },
    )
    .await?
    {
        CommandResponse::FileRestored { id, path } => {
            Ok(Json(serde_json::json!({ "id": id, "path": path })))
        }
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `POST /api/files/trash/empty` — delete everything in the trash.
pub async fn post_trash_empty(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    require_file_write(&state).await?;

    match exchange(&state, &Request::FileTrashEmpty).await? {
        CommandResponse::FileTrashEmptied { removed } => {
            Ok(Json(serde_json::json!({ "removed": removed })))
        }
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}
