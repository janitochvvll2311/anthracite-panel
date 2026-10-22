use super::error::error_detail;
use super::prelude::*;

#[derive(Debug, Deserialize)]
pub struct WatchQuery {
    interval: Option<String>,
}

/// Reads one snapshot and applies the endpoint's response mapper.
pub async fn get_snapshot<T, F>(
    state: &AppState,
    request: Request,
    map: F,
) -> Result<Json<T>, ApiError>
where
    T: serde::Serialize,
    F: FnOnce(CommandResponse) -> Result<T, ApiError>,
{
    map(exchange(state, &request).await?).map(Json)
}

/// Builds the common SSE response used by every snapshot endpoint.
pub async fn watch_snapshots<T, F>(
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
pub(crate) async fn exchange(
    state: &AppState,
    request: &Request,
) -> Result<CommandResponse, ApiError> {
    let mut client = state
        .create_client()
        .await
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "daemon unavailable"))?;
    query_command(&mut client, request).await
}

pub(crate) async fn query_command(
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
