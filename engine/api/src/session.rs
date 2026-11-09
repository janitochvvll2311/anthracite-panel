use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_auth::Auth;
use anth_compose::audit::timestamp;
use anth_contract::{AuditAction, AuditEntry, Error, Outcome};
use axum::extract::{Extension, State};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{deny, ApiError, AuthContext, AuthKind, EngineState, SourceAddress};

/// The cookie name. It is deliberately not `__Host-`: v0.5.0 serves plain HTTP
/// and the prefix would make browsers reject the cookie.
pub const SESSION_COOKIE: &str = "anth_session";
pub(crate) const SESSION_BODY_LIMIT: u64 = 16 * 1024;

#[derive(Deserialize)]
pub(crate) struct LoginRequest {
    operator: String,
    password: String,
}

#[derive(Deserialize)]
pub(crate) struct PasswordRequest {
    #[serde(default)]
    current_password: Option<String>,
    new_password: String,
}

fn auth(state: &EngineState) -> Result<&Arc<Auth>, ApiError> {
    state.auth.as_ref().ok_or_else(|| {
        ApiError(Error::unavailable(
            "this router has no credentials; start the engine with a state tree",
        ))
    })
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    let header = headers.get(COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        (name == SESSION_COOKIE).then(|| value.to_string())
    })
}

fn set_session_cookie(response: &mut Response, token: &str, max_age: u64, secure: bool) {
    let secure = if secure { "; Secure" } else { "" };
    let value = format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}"
    );
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&value).expect("a session cookie is a valid header"),
    );
}

fn clear_session_cookie(response: &mut Response) {
    let value = format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0");
    response.headers_mut().insert(
        SET_COOKIE,
        HeaderValue::from_str(&value).expect("a session cookie is a valid header"),
    );
}

/// The engine speaks plain HTTP; a TLS terminator in front of it can mark the
/// request with `X-Forwarded-Proto: https`, which is the only time `Secure` is
/// safe to add. Without TLS the flag would make the browser drop the cookie.
pub(crate) fn cookie_secure(headers: &HeaderMap) -> bool {
    headers
        .get("x-forwarded-proto")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("https"))
        })
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

/// `POST /api/engine/session`: check the operator and mint a bearer token.
pub(crate) async fn login_session(
    State(state): State<Arc<EngineState>>,
    Extension(source): Extension<SourceAddress>,
    headers: HeaderMap,
    Json(request): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let auth = auth(&state)?;
    let source = source.0;
    let login = match auth.login(&source, &request.operator, &request.password) {
        Ok(login) => login,
        Err(error) => {
            deny(&state, request.operator, "invalid credentials");
            return Err(ApiError(error));
        }
    };
    if let Some(apps) = state.apps.as_deref() {
        let _ = apps.audit.append(&AuditEntry {
            at: timestamp(),
            actor: login.session.operator.clone(),
            action: AuditAction::SessionLogin,
            app: None,
            result: Outcome::Ok,
            detail: None,
        });
    }
    let mut response = Json(json!({
        "operator": login.session.operator,
        "must_change_password": login.session.must_change_password,
        "expires_at": login.session.expires_at,
    }))
    .into_response();
    set_session_cookie(
        &mut response,
        &login.token,
        auth.ttl_seconds()
            .max(login.session.expires_at.saturating_sub(now_seconds())),
        cookie_secure(&headers),
    );
    Ok(response)
}

/// `GET /api/engine/session`: who this request is, without touching the token.
pub(crate) async fn read_session(Extension(context): Extension<AuthContext>) -> Json<Value> {
    Json(match context.session {
        Some(session) => json!({
            "operator": session.operator,
            "transport": "session",
            "must_change_password": session.must_change_password,
            "expires_at": session.expires_at,
        }),
        None => json!({
            "operator": context.actor,
            "transport": match context.kind {
                AuthKind::Peer => "socket",
                AuthKind::Trusted => "panel",
                AuthKind::Session => "session",
            },
            "must_change_password": context.must_change_password,
            "expires_at": Value::Null,
        }),
    })
}

/// `DELETE /api/engine/session`: drop the token and clear the cookie.
pub(crate) async fn logout_session(
    State(state): State<Arc<EngineState>>,
    Extension(context): Extension<AuthContext>,
) -> Response {
    if let (Some(token), Some(auth)) = (context.token.as_deref(), state.auth.as_deref()) {
        auth.logout(token);
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    clear_session_cookie(&mut response);
    response
}

/// `GET /api/engine/operator`: the one identity v0.5.0 has.
pub(crate) async fn read_operator(Extension(context): Extension<AuthContext>) -> Json<Value> {
    Json(json!({ "name": context.actor }))
}

/// `PATCH /api/engine/operator`: change the password. A session proves the old
/// one; the socket's peer credential is proof enough. A password change revokes
/// every token, so a session client gets a fresh cookie in the same response.
pub(crate) async fn change_operator_password(
    State(state): State<Arc<EngineState>>,
    headers: HeaderMap,
    Extension(context): Extension<AuthContext>,
    Json(request): Json<PasswordRequest>,
) -> Result<Response, ApiError> {
    let auth = auth(&state)?;
    auth.change_password(
        request.current_password.as_deref(),
        &request.new_password,
        context.kind == AuthKind::Peer,
    )
    .map_err(|error| {
        deny(&state, context.actor.clone(), "password change refused");
        ApiError(error)
    })?;
    let mut response = Json(json!({
        "name": context.actor,
        "must_change_password": false,
    }))
    .into_response();
    if context.kind == AuthKind::Session {
        let login = auth.issue().map_err(ApiError)?;
        set_session_cookie(
            &mut response,
            &login.token,
            auth.ttl_seconds(),
            cookie_secure(&headers),
        );
    }
    Ok(response)
}
