use std::net::SocketAddr;
use std::sync::Arc;

use anth_auth::Auth;
use anth_contract::Error;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{CONTENT_SECURITY_POLICY, HOST, ORIGIN};
use axum::http::{HeaderName, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::Response;

use crate::{
    deny, session_token, Actor, ApiError, AuthContext, AuthKind, EngineState, PeerUid, Trusted,
};

/// Every `/api` route needs a peer credential from the service user or root.
pub(crate) async fn require_peer(
    State(state): State<Arc<EngineState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    if request.extensions().get::<Trusted>().is_some() {
        // A panel authenticates its browser requests and marks them. It is not
        // the socket: the first-boot gate still applies and a password change
        // must prove the old password.
        let gated = state
            .auth
            .as_deref()
            .is_some_and(Auth::must_change_password);
        request.extensions_mut().insert(Actor("system".to_string()));
        request.extensions_mut().insert(AuthContext {
            actor: "system".to_string(),
            kind: AuthKind::Trusted,
            must_change_password: gated,
            session: None,
            token: None,
        });
        return Ok(next.run(request).await);
    }
    let peer = request
        .extensions()
        .get::<ConnectInfo<PeerUid>>()
        .map(|ConnectInfo(uid)| uid.0);
    match peer {
        None => {
            deny(&state, "anonymous".to_string(), "no peer credential");
            Err(ApiError(Error::unauthenticated(
                "this surface needs a local peer credential",
            )))
        }
        Some(uid) if uid == 0 || uid == state.allowed_uid => {
            request.extensions_mut().insert(Actor(format!("uid:{uid}")));
            request
                .extensions_mut()
                .insert(AuthContext::peer(format!("uid:{uid}"), AuthKind::Peer));
            Ok(next.run(request).await)
        }
        Some(uid) => {
            deny(
                &state,
                format!("uid:{uid}"),
                "the peer is not the service user",
            );
            Err(ApiError(Error::unauthenticated(
                "the peer is not the service user or root",
            )))
        }
    }
}

/// Every `/api` route on a TCP listener needs a session, except the login that
/// mints one.
pub(crate) async fn require_session(
    State(state): State<Arc<EngineState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let path = request.uri().path().to_string();
    if request.method() == Method::POST && path == "/engine/session" {
        return Ok(next.run(request).await);
    }
    let token = session_token(request.headers());
    let session = token.as_deref().and_then(|token| {
        state
            .auth
            .as_deref()
            .and_then(|auth| auth.authenticate(token))
    });
    match session {
        Some(session) => {
            request
                .extensions_mut()
                .insert(Actor(session.operator.clone()));
            request.extensions_mut().insert(AuthContext {
                actor: session.operator.clone(),
                kind: AuthKind::Session,
                must_change_password: session.must_change_password,
                session: Some(session),
                token,
            });
            Ok(next.run(request).await)
        }
        None => {
            deny(&state, "anonymous".to_string(), "no session");
            Err(ApiError(Error::unauthenticated(
                "this surface needs a session; log in first",
            )))
        }
    }
}

/// Cookies are `SameSite=Strict`, but a browser can still be tricked by a
/// same-site form post; the Origin header is the second lock. A request with no
/// Origin (the CLI, curl) is not a browser and is allowed to authenticate itself.
pub(crate) async fn require_origin(
    State(state): State<Arc<EngineState>>,
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let unsafe_method = matches!(
        *request.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if unsafe_method {
        if let Some(origin) = request.headers().get(ORIGIN) {
            let origin = origin
                .to_str()
                .map_err(|_| ApiError(Error::denied("the Origin header is not text")))?;
            let authority = origin.split_once("://").map(|(_, rest)| rest).unwrap_or("");
            let forwarded = request
                .extensions()
                .get::<ConnectInfo<SocketAddr>>()
                .is_some_and(|ConnectInfo(address)| address.ip().is_loopback());
            let host = (if forwarded {
                request.headers().get("x-forwarded-host")
            } else {
                None
            })
            .or_else(|| request.headers().get(HOST))
            .and_then(|host| host.to_str().ok())
            .map(|host| host.split(',').next().unwrap_or("").trim())
            .unwrap_or("");
            if authority != host {
                deny(&state, "anonymous".to_string(), "cross-origin request");
                return Err(ApiError(Error::denied(
                    "cross-origin requests are not allowed",
                )));
            }
        }
    }
    Ok(next.run(request).await)
}

/// The engine answers JSON and app modules, never a document, so the policy can
/// be the strictest one. It is applied before any route-specific header.
pub(crate) async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers
        .entry(CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static(
            "default-src 'none'; frame-ancestors 'none'; base-uri 'none'",
        ));
    headers
        .entry(HeaderName::from_static("x-content-type-options"))
        .or_insert(HeaderValue::from_static("nosniff"));
    headers
        .entry(HeaderName::from_static("referrer-policy"))
        .or_insert(HeaderValue::from_static("no-referrer"));
    headers
        .entry(HeaderName::from_static("x-frame-options"))
        .or_insert(HeaderValue::from_static("DENY"));
    response
}

/// The first-boot password is a gate, not a warning: until it changes, only the
/// operator resource and the session itself are reachable.
pub(crate) async fn password_change_gate(
    request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let gated = request
        .extensions()
        .get::<AuthContext>()
        .is_some_and(|context| context.must_change_password);
    if gated {
        let path = request.uri().path();
        let allowed = (request.method() == Method::PATCH && path == "/engine/operator")
            || (path == "/engine/session"
                && matches!(
                    *request.method(),
                    Method::GET | Method::POST | Method::DELETE
                ));
        if !allowed {
            return Err(ApiError(Error::password_change_required(
                "change the first-boot password before using the engine",
            )));
        }
    }
    Ok(next.run(request).await)
}
