use super::files::file_api_error;
use super::files::{ArchiveCreateArgs, ArchiveExtractArgs, ArchivePathQuery};
use super::guards::*;
use super::prelude::*;

/// `POST /api/files/archive` — compress one entry.
pub async fn post_archive_create(
    State(state): State<AppState>,
    payload: Result<Json<ArchiveCreateArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    match exchange(
        &state,
        &Request::FileArchiveCreate {
            path: args.path,
            destination: args.destination,
            format: args.format,
        },
    )
    .await?
    {
        CommandResponse::FileArchiveCreated { path } => {
            Ok(Json(serde_json::json!({ "path": path })))
        }
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `GET /api/files/archive/list?path=/a.zip` — members of one archive.
pub async fn get_archive_listing(
    State(state): State<AppState>,
    query: Result<Query<ArchivePathQuery>, QueryRejection>,
) -> Result<Json<ArchiveListing>, ApiError> {
    let Query(args) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;

    match exchange(&state, &Request::FileArchiveList { path: args.path }).await? {
        CommandResponse::ArchiveListing(listing) => Ok(Json(listing)),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}

/// `POST /api/files/archive/extract` — unpack one archive.
pub async fn post_archive_extract(
    State(state): State<AppState>,
    payload: Result<Json<ArchiveExtractArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    match exchange(
        &state,
        &Request::FileArchiveExtract {
            path: args.path,
            into: args.into,
        },
    )
    .await?
    {
        CommandResponse::FileArchiveExtracted { path, entries } => Ok(Json(
            serde_json::json!({ "path": path, "entries": entries }),
        )),
        CommandResponse::FileError(error) => Err(file_api_error(error)),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}
