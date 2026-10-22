use super::prelude::*;

/// `GET /api/host-info` — one snapshot.
pub async fn get_host_info(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    get_snapshot(&state, Request::HostInfo, host_info_json).await
}

/// `GET /api/host-info/watch?interval=2s` — a snapshot per interval as SSE.
pub async fn watch_host_info(
    State(state): State<AppState>,
    query: Result<Query<WatchQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    watch_snapshots(state, query, "host-info", Request::HostInfo, host_info_json).await
}

pub(crate) fn host_info_json(response: CommandResponse) -> Result<Value, ApiError> {
    match response {
        CommandResponse::HostInfo(info) => Ok(Value::from(info.as_ref())),
        CommandResponse::Error { detail } => Err(gateway(detail)),
        other => Err(unexpected_response(other)),
    }
}
