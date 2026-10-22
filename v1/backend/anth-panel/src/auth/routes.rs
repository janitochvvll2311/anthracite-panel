//! The authentication endpoints.
//!
//! `login` is the only place a password is accepted, `refresh` the only place a
//! refresh token is spent, and both set the cookies so the browser never has to
//! hold a token in JavaScript. The UI therefore cannot leak a token through
//! `localStorage`, a log line or a rendered string.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

use super::cookies;
use crate::api::ApiError;
use crate::app::AppState;
use crate::auth::session::{AuthError, Issued};

#[derive(Debug, Deserialize)]
pub struct LoginArgs {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub enabled: bool,
    pub authenticated: bool,
    pub username: String,
    /// Access-token lifetime, so the client can schedule a silent refresh
    /// slightly before it expires.
    pub access_expires_in: u64,
    /// Remaining lifetime of the current refresh token, in seconds.
    pub refresh_expires_in: u64,
}

/// `POST /api/auth/login`.
pub async fn post_login(
    State(state): State<AppState>,
    payload: Result<Json<LoginArgs>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;

    let auth = state.auth().await;
    let issued = auth
        .login(&args.username, &args.password)
        .map_err(login_error)?;
    let config = state.config().await;
    Ok(issued_response(issued, config.auth.cookie_secure))
}

/// `POST /api/auth/refresh` — rotates a refresh token for a new pair.
///
/// The refresh token comes from the cookie, never from the body: an attacker
/// who can read a body can read a header, and the cookie is the copy the browser
/// sends automatically.
pub async fn post_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let token = cookies::read(&headers, cookies::REFRESH_COOKIE)
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "no refresh token"))?;

    let auth = state.auth().await;
    let issued = auth.refresh(&token).map_err(refresh_error)?;
    let config = state.config().await;
    Ok(issued_response(issued, config.auth.cookie_secure))
}

/// `POST /api/auth/logout` — revokes the session and clears both cookies.
pub async fn post_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Some(token) = cookies::read(&headers, cookies::REFRESH_COOKIE) {
        state.auth().await.revoke(&token);
    }
    let (access, refresh) = {
        let mut cleared = cookies::clearing().into_iter();
        (
            cleared.next().expect("two cookies"),
            cleared.next().expect("two cookies"),
        )
    };
    let mut headers = HeaderMap::new();
    cookies::append(&mut headers, access);
    cookies::append(&mut headers, refresh);
    (headers, Json(serde_json::json!({ "ok": true }))).into_response()
}

/// `GET /api/auth/session` — what the UI asks before rendering the shell.
///
/// Public on purpose: the login screen has to know whether auth is even on and
/// whether the current cookies still work. It never reveals the password hash or
/// the signing key.
pub async fn get_session(State(state): State<AppState>, headers: HeaderMap) -> Json<SessionInfo> {
    let auth = state.auth().await;

    if !auth.enabled() {
        return Json(SessionInfo {
            enabled: false,
            authenticated: true,
            username: auth.username(),
            access_expires_in: 0,
            refresh_expires_in: 0,
        });
    }

    // Each cookie reports the time it has left, not the configured lifetime.
    // A tab loaded halfway through a token would otherwise schedule its silent
    // refresh a full lifetime too late and sit on 401s until the timer fired.
    let access = cookies::read(&headers, cookies::ACCESS_COOKIE)
        .filter(|token| auth.verify_access(token).is_ok())
        .map(|token| auth.access_remaining(&token));
    // A cookie is not a session: it has to verify *and* name a live session.
    // Reporting `authenticated: true` from an unverified cookie renders the
    // panels while every `/api` call answers 401.
    let refresh = cookies::read(&headers, cookies::REFRESH_COOKIE)
        .filter(|token| auth.holds_current_refresh(token))
        .map(|token| auth.refresh_remaining(&token));

    Json(SessionInfo {
        enabled: true,
        authenticated: access.is_some() || refresh.is_some(),
        username: auth.username(),
        access_expires_in: access.unwrap_or(0),
        refresh_expires_in: refresh.unwrap_or(0),
    })
}

#[derive(Debug, Deserialize)]
pub struct ChangePasswordArgs {
    pub current_password: String,
    pub new_password: String,
}

/// `POST /api/auth/password` — changes the panel password.
///
/// Every session is dropped, including the caller's: a password change is what
/// an operator does when a credential may be compromised, so it must not leave
/// a working token behind.
pub async fn post_password(
    State(state): State<AppState>,
    payload: Result<Json<ChangePasswordArgs>, axum::extract::rejection::JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(args) = payload.map_err(|e| ApiError::bad_request(e.body_text()))?;

    if args.new_password.chars().count() < 8 {
        return Err(ApiError::bad_request(
            "the new password must be at least 8 characters",
        ));
    }

    let auth = state.auth().await;
    if !auth.enabled() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "authentication is disabled",
        ));
    }
    if !auth.hashes_password(&args.current_password) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "the current password is wrong",
        ));
    }

    let hash = auth
        .hash_new_password(&args.new_password)
        .map_err(ApiError::bad_request)?;

    let mut config = state.config().await;
    config.auth.password_hash = hash;
    // A new signing key *and* an empty session map: rebuilding the service is
    // what makes the access tokens already issued stop working immediately,
    // instead of at the end of their 15-minute life.
    config.auth.jwt_secret = crate::auth::jwt::random_id()
        + &crate::auth::jwt::random_id()
        + &crate::auth::jwt::random_id();
    state.replace_auth(config).await;
    state.persist().await.map_err(ApiError::internal)?;

    let (access, refresh) = {
        let mut cleared = cookies::clearing().into_iter();
        (
            cleared.next().expect("two cookies"),
            cleared.next().expect("two cookies"),
        )
    };
    let mut headers = HeaderMap::new();
    cookies::append(&mut headers, access);
    cookies::append(&mut headers, refresh);
    Ok((
        headers,
        Json(serde_json::json!({ "ok": true, "reauth": true })),
    )
        .into_response())
}

fn issued_response(issued: Issued, secure: bool) -> Response {
    let mut headers = HeaderMap::new();
    cookies::append(
        &mut headers,
        cookies::access_cookie(&issued.access_token, issued.access_expires_in, secure),
    );
    cookies::append(
        &mut headers,
        cookies::refresh_cookie(&issued.refresh_token, issued.refresh_expires_in, secure),
    );
    (
        headers,
        Json(serde_json::json!({
            "username": issued.username,
            "role": issued.role,
            "access_expires_in": issued.access_expires_in,
        })),
    )
        .into_response()
}

fn login_error(error: AuthError) -> ApiError {
    match error {
        AuthError::NotConfigured => {
            ApiError::new(StatusCode::CONFLICT, "authentication is disabled")
        }
        // One message for a wrong name and a wrong password: telling them apart
        // would confirm which account exists.
        _ => ApiError::new(StatusCode::UNAUTHORIZED, "invalid credentials"),
    }
}

fn refresh_error(error: AuthError) -> ApiError {
    match error {
        AuthError::NotConfigured => {
            ApiError::new(StatusCode::CONFLICT, "authentication is disabled")
        }
        _ => ApiError::new(StatusCode::UNAUTHORIZED, "session expired"),
    }
}
