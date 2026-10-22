use super::prelude::*;

use super::guards::*;
use futures::StreamExt;

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
pub struct ArchiveCreateArgs {
    pub path: String,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub format: Option<ArchiveFormat>,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveExtractArgs {
    pub path: String,
    #[serde(default)]
    pub into: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ArchivePathQuery {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct FileWriteQuery {
    pub path: String,
    /// The mtime the editor loaded. Omitted means "overwrite regardless".
    #[serde(default)]
    pub expected_modified_ms: Option<u64>,
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

/// `POST /api/files/write?path=/a.txt&expected_modified_ms=...` — save text.
///
/// The body is the raw text, streamed through the same chunk flow as an upload.
/// A JSON body would carry the whole file in one socket line, and the line cap
/// is 1 MiB, so any larger file opened in the editor could not be saved.
pub async fn post_file_write(
    State(state): State<AppState>,
    query: Result<Query<FileWriteQuery>, QueryRejection>,
    body: Body,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    let written = upload_stream(
        &state,
        &query.path,
        true,
        true,
        query.expected_modified_ms,
        body,
    )
    .await?;
    Ok(Json(serde_json::json!({
        "path": written.path,
        "size": written.size,
        "modified_ms": written.modified_ms,
    })))
}

pub async fn post_file_upload(
    State(state): State<AppState>,
    query: Result<Query<FileUploadQuery>, QueryRejection>,
    body: Body,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query.map_err(|e| ApiError::bad_request(e.body_text()))?;
    require_file_write(&state).await?;

    let written = upload_stream(&state, &query.path, query.overwrite, false, None, body).await?;
    Ok(Json(
        serde_json::json!({ "path": written.path, "size": written.size }),
    ))
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
    require_existing: bool,
    expected_modified_ms: Option<u64>,
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
            require_existing,
            expected_modified_ms,
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

pub(crate) fn file_api_error(error: FileError) -> ApiError {
    let status = match error.kind {
        FileErrorKind::NotFound => StatusCode::NOT_FOUND,
        FileErrorKind::PermissionDenied | FileErrorKind::Disabled => StatusCode::FORBIDDEN,
        FileErrorKind::AlreadyExists
        | FileErrorKind::DirectoryNotEmpty
        | FileErrorKind::Conflict => StatusCode::CONFLICT,
        FileErrorKind::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        FileErrorKind::InvalidPath | FileErrorKind::NotADirectory | FileErrorKind::IsADirectory => {
            StatusCode::BAD_REQUEST
        }
        FileErrorKind::Io => StatusCode::INTERNAL_SERVER_ERROR,
    };
    ApiError::new(status, error.detail)
}
