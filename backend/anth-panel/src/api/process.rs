use super::prelude::*;

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

pub(crate) fn process_snapshot_json(
    response: CommandResponse,
) -> Result<ProcessSnapshot, ApiError> {
    match response {
        CommandResponse::ProcessSnapshot(snapshot) => Ok(snapshot),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}
