use super::prelude::*;

/// Body of `POST /api/echo`. The command is implied by the route, so the
/// argument is a top-level member rather than nested under a tag.
#[derive(Debug, Deserialize)]
pub struct EchoArgs {
    pub message: String,
}

/// `GET /api/echo` — one echo.
pub async fn post_echo(
    State(state): State<AppState>,
    payload: Result<Json<EchoArgs>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;

    let request = Request::Echo {
        message: args.message,
    };
    match exchange(&state, &request).await? {
        CommandResponse::Ok { message } => Ok(Json(serde_json::json!({ "message": message }))),
        other => Err(unexpected_response(other)),
    }
}
