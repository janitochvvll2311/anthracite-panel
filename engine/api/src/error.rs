use anth_contract::Error;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

/// An error rendered with the one envelope of §7.11.
#[derive(Debug)]
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        Self(error)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status =
            StatusCode::from_u16(self.0.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self.0.envelope())).into_response()
    }
}
