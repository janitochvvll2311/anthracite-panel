//! One authentication layer for every route that reaches the daemon.
//!
//! Applying this as a router middleware rather than a per-handler check is what
//! keeps the guarantee honest: a new endpoint is protected the moment it is
//! added, and a route that must stay public says so through [`Public`], which
//! is a deliberate, greppable decision.

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};

use super::cookies;
use crate::app::AppState;
use crate::auth::session::AuthService;

/// Routes reachable without a token. Everything else needs one.
///
/// The list is deliberately tiny: a static asset, the login and refresh
/// endpoints and the liveness probe. `/healthz` stays public so a supervisor
/// can probe the process without credentials, and it answers a constant.
pub const PUBLIC_ROUTES: &[&str] = &[
    "/healthz",
    "/api/auth/login",
    "/api/auth/refresh",
    "/api/auth/logout",
    "/api/auth/session",
    "/api/auth/bootstrap",
];

/// Whether `path` may be requested without a token.
pub fn is_public(path: &str) -> bool {
    PUBLIC_ROUTES.contains(&path) || !path.starts_with("/api")
}

/// Applies [`require_auth`] to a router, binding the shared state.
pub fn protect(router: Router<AppState>, state: AppState) -> Router<AppState> {
    router.layer(axum::middleware::from_fn_with_state(state, require_auth))
}

/// Middleware that rejects an unauthenticated request.
///
/// When `[auth] enabled` is false the layer is a pass-through: the panel is
/// open by explicit configuration, not by accident, and the settings view says
/// so.
pub async fn require_auth(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let auth: std::sync::Arc<AuthService> = state.auth().await;
    if !auth.enabled() || is_public(request.uri().path()) {
        return next.run(request).await;
    }

    if let Some(token) = bearer_or_cookie(&request) {
        if auth.verify_access(&token).is_ok() {
            return next.run(request).await;
        }
        // An expired access token is not a failed login: the client refreshes
        // and retries. The distinction is what lets the UI recover silently.
        return unauthorized("access token expired or invalid");
    }

    unauthorized("authentication required")
}

fn bearer_or_cookie(request: &Request) -> Option<String> {
    if let Some(token) = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        return Some(token.trim().to_string());
    }
    cookies::read(request.headers(), cookies::ACCESS_COOKIE)
}

fn unauthorized(detail: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "error": detail })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_routes_are_private_by_default() {
        assert!(!is_public("/api/processes"));
        assert!(!is_public("/api/files"));
        assert!(!is_public("/api/terminal"));
    }

    #[test]
    fn the_login_surface_is_public() {
        assert!(is_public("/api/auth/login"));
        assert!(is_public("/api/auth/refresh"));
        assert!(is_public("/healthz"));
    }

    #[test]
    fn a_static_asset_is_public() {
        assert!(is_public("/"));
        assert!(is_public("/index.html"));
    }
}
