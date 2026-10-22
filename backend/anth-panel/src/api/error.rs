use super::prelude::*;

/// Every API failure, serialized as `{"error": "..."}` so clients parse one
/// shape regardless of which layer rejected the request.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    detail: String,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }

    pub(crate) fn bad_request(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, detail)
    }

    pub(crate) fn internal(detail: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, detail)
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(serde_json::json!({ "error": self.detail })),
        )
            .into_response()
    }
}

pub(crate) fn gateway(detail: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_GATEWAY, detail)
}

pub(crate) fn unexpected_response(response: CommandResponse) -> ApiError {
    ApiError::new(
        StatusCode::BAD_GATEWAY,
        format!("unexpected daemon response: {response:?}"),
    )
}

/// Text for an SSE error frame, where only a payload is possible.
pub(crate) fn error_detail(error: ApiError) -> String {
    error.detail
}
