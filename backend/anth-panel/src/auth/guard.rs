//! The HTTP side of authentication: who is calling, and what they may reach.
//!
//! # Why a cookie, and not only a header
//!
//! A bearer header is the obvious way to carry a JWT, and it does not work for
//! three of this panel's consumers:
//!
//! | Consumer | Can set a header? |
//! | --- | --- |
//! | `fetch` | yes |
//! | `EventSource` (the watch stream) | **no** — the browser owns the request |
//! | `import()` (a plugin's web module) | **no** — there is no hook |
//! | a third-party plugin widget | **must not have to** |
//!
//! Each would need its own workaround: a ticket in the query string for the
//! stream, a token baked into every plugin's transport, and a module loader that
//! can inject headers it cannot inject. One `HttpOnly` cookie answers all four,
//! because the browser attaches it to every same-origin request whatever
//! mechanism made it.
//!
//! It is also the safer default: `HttpOnly` means script cannot read the token,
//! so a cross-site scripting bug cannot exfiltrate the session, and no token is
//! ever held in `localStorage` for a later bug to find.
//!
//! The `Authorization` header is still accepted, because `curl`, a script and a
//! test are real callers and a cookie is inconvenient for all three. The cookie
//! is what the browser uses; the header is what everything else uses.
//!
//! # Two cookies, two scopes
//!
//! | Cookie | Path | Lifetime | Why that path |
//! | --- | --- | --- | --- |
//! | `anth_access` | `/` | minutes | every API call needs it |
//! | `anth_refresh` | `/api/auth` | days | only `refresh` and `logout` read it |
//!
//! The narrower path is what keeps a long-lived credential off every request:
//! a plugin anchor, a static asset and a health check never see it.
//!
//! # CSRF, given `SameSite=Strict`
//!
//! `SameSite=Strict` already stops another site from making a request that
//! carries these cookies. It does **not** separate two ports on the same host —
//! `localhost:1234` and `localhost:8080` are the same site — so a state-changing
//! request also has to prove where it came from: [`same_origin`] compares the
//! `Origin` header against `Host`, and every mutating route calls it.
//!
//! # What is not protected
//!
//! `/healthz` answers without a session: a health check that needs a credential
//! reports "down" whenever a token expires. The shell and its assets are also
//! public — they are the login page. Everything under `/api/` except
//! `/api/auth/` is behind [`Authed`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;

use super::{Account, AuthError, session, store};

/// The cookie carrying the short-lived access token.
pub const ACCESS_COOKIE: &str = "anth_access";

/// The cookie carrying the long-lived refresh token.
pub const REFRESH_COOKIE: &str = "anth_refresh";

/// Where the refresh cookie is scoped, so only the auth routes receive it.
const REFRESH_PATH: &str = "/api/auth";

/// What the browser asks before it decides which form to show.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct State {
    /// Whether an account exists. `false` means the bootstrap is open.
    pub registered: bool,
    /// Whether a session is required at all.
    ///
    /// Read by the shell before it renders the gate: a daemon with auth off
    /// answers this `false` and the browser goes straight to the panel. Without
    /// this field the gate would show a login form whose `login` route refuses
    /// every password.
    pub required: bool,
}

/// The authentication state of one daemon, shared by every request.
///
/// The account is cached rather than re-read per request: reading the file would
/// put a filesystem call and a full TOML parse in front of every request and
/// every SSE frame. It is replaced whenever a request changes it, so the cache
/// cannot drift from the file for longer than one call.
pub struct Auth {
    config: PathBuf,
    account: RwLock<Option<Arc<Account>>>,
    /// Whether a session is required. `false` only when the settings file says
    /// so; see [`crate::config::Paths::auth`].
    required: bool,
}

impl Auth {
    /// Loads the account once, at startup, and records whether a session is
    /// required.
    ///
    /// An unreadable account is not fatal: the daemon still serves, the state
    /// route reports what is wrong, and `register` refuses to overwrite a
    /// damaged credential. Better than a panel that will not start because a
    /// preference file is corrupt.
    ///
    /// Turning `required` off does not delete the account or its key: the file
    /// is untouched, so turning it back on restores the same operator with the
    /// same password and an empty session table.
    pub fn load(config: impl Into<PathBuf>, required: bool) -> Self {
        let config = config.into();
        let account = store::read(&config).ok().flatten().map(Arc::new);
        Self {
            config,
            account: RwLock::new(account),
            required,
        }
    }

    /// Whether a request needs a session.
    pub fn required(&self) -> bool {
        self.required
    }

    pub fn path(&self) -> &Path {
        &self.config
    }

    /// The cached account, if there is one.
    pub fn account(&self) -> Option<Arc<Account>> {
        self.account.read().ok().and_then(|slot| slot.clone())
    }

    pub fn registered(&self) -> bool {
        self.account().is_some()
    }

    /// The state the browser reads before showing either form.
    pub fn state(&self) -> State {
        State {
            registered: self.registered(),
            required: self.required,
        }
    }

    /// Registers the first account and logs it in.
    pub fn register(&self, username: &str, password: &str) -> Result<session::Grant, AuthError> {
        let account = super::register(&self.config, username, password)?;
        let grant = session::start(&self.config, &account)?;
        self.store_account(account);
        Ok(grant)
    }

    /// Checks a password and opens a session.
    pub fn login(&self, username: &str, password: &str) -> Result<session::Grant, AuthError> {
        let account = super::login(&self.config, username, password)?;
        let grant = session::start(&self.config, &account)?;
        self.store_account(account);
        Ok(grant)
    }

    /// Rotates a refresh token.
    pub fn refresh(&self, presented: &str) -> Result<session::Grant, AuthError> {
        session::refresh(&self.config, presented)
    }

    /// Ends every session, and returns how many.
    pub fn logout(&self) -> Result<usize, AuthError> {
        session::revoke_all(&self.config)
    }

    /// Changes the password, which also ends every session.
    pub fn change_password(
        &self,
        username: &str,
        current: &str,
        next: &str,
    ) -> Result<Account, AuthError> {
        let account = super::change_password(&self.config, username, current, next)?;
        self.store_account(account.clone());
        Ok(account)
    }

    fn store_account(&self, account: Account) {
        if let Ok(mut slot) = self.account.write() {
            *slot = Some(Arc::new(account));
        }
    }

    /// Checks an access token, returning the account it authorises.
    pub fn verify(&self, token: &str) -> Result<Arc<Account>, AuthError> {
        let account = self
            .account()
            .ok_or_else(|| AuthError::Unauthorized("no account is registered".to_string()))?;
        session::verify_access(&account, token)?;
        Ok(account)
    }
}

/// The `Set-Cookie` headers that hand a grant to the browser.
///
/// `HttpOnly` so script cannot read either token, and `SameSite=Strict` so
/// another site cannot make a request that carries them.
///
/// `Secure` is deliberately absent: the panel serves plain HTTP by default and a
/// `Secure` cookie would never be stored, which would look like a broken login.
/// An operator behind TLS has no knob for it yet, and that is a real gap worth
/// naming rather than a setting that pretends to exist.
pub fn set_cookies(grant: &session::Grant) -> Vec<String> {
    vec![
        format!(
            "{ACCESS_COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
            grant.access_token, grant.expires_in
        ),
        format!(
            "{REFRESH_COOKIE}={}; Path={REFRESH_PATH}; HttpOnly; SameSite=Strict; Max-Age={}",
            grant.refresh_token,
            super::REFRESH_TTL_SECONDS
        ),
    ]
}

/// The headers that clear both cookies, for `logout` and a failed refresh.
pub fn clear_cookies() -> Vec<String> {
    vec![
        format!("{ACCESS_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"),
        format!("{REFRESH_COOKIE}=; Path={REFRESH_PATH}; HttpOnly; SameSite=Strict; Max-Age=0"),
    ]
}

/// One cookie's value from a `Cookie` header.
///
/// Hand-rolled rather than a crate: the grammar a browser sends is
/// `name=value; name=value`, and a parser crate would bring quoting and escaping
/// rules that no browser here exercises and no test would cover.
pub fn cookie(parts: &Parts, name: &str) -> Option<String> {
    let header = parts.headers.get(header::COOKIE)?.to_str().ok()?;
    header.split(';').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key.trim() == name && !value.trim().is_empty()).then(|| value.trim().to_string())
    })
}

/// The token from an `Authorization: Bearer …` header, if there is one.
///
/// Case-insensitive on the scheme, because a client that sends `bearer` is
/// correct and refusing it would be a bug nobody would guess at.
pub fn bearer(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

/// The access token a request presented, from the header or the cookie.
///
/// The header wins when both are present: a caller that sets one is saying which
/// identity it means, and a stale cookie in the same browser must not override
/// it.
pub fn access_token(parts: &Parts) -> Option<String> {
    bearer(parts).or_else(|| cookie(parts, ACCESS_COOKIE))
}

/// The refresh token a request presented, from the body or the cookie.
pub fn refresh_token(parts: &Parts, body: Option<String>) -> Option<String> {
    body.filter(|token| !token.is_empty())
        .or_else(|| cookie(parts, REFRESH_COOKIE))
}

/// Whether a mutating request came from this panel's own pages.
///
/// `Origin` is compared against `Host`, which is the check the browser's own
/// same-origin policy cannot make for a cookie: a cookie travels on a cross-site
/// request, so the server has to ask where the request started.
///
/// A request with no `Origin` at all is allowed, and that is not a hole: every
/// browser sends `Origin` on a `POST`, so its absence means the caller is not a
/// browser — `curl`, a script, a test — and such a caller cannot be made to send
/// a cookie it does not already hold.
pub fn same_origin(parts: &Parts) -> Result<(), AuthError> {
    let origin = parts
        .headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let host = parts
        .headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());

    match (origin, host) {
        (None, _) => Ok(()),
        (Some(_), None) => Err(AuthError::Unauthorized(
            "the request has no `Host` to compare its origin with".to_string(),
        )),
        (Some(origin), Some(host)) => {
            // The scheme is not compared: the panel cannot know whether the
            // operator put TLS in front of it, and only the host and port decide
            // whether a request is cross-site.
            let bare = origin
                .strip_prefix("http://")
                .or_else(|| origin.strip_prefix("https://"));
            if bare == Some(host) {
                Ok(())
            } else {
                Err(AuthError::Unauthorized(
                    "this request came from another origin".to_string(),
                ))
            }
        }
    }
}

/// A caller who presented a valid access token.
///
/// An extractor rather than a helper each handler calls, so a route either has
/// one in its signature — and is therefore protected — or it does not, which is
/// visible in one line. A handler that called a helper could lose the call in a
/// refactor and still compile.
pub struct Authed {
    /// The account the token authorises. One account exists, but carrying it
    /// keeps a second from being added later without this type changing.
    ///
    /// `None` when the daemon runs with auth off: there is no session to name an
    /// operator, and no token to authorise one. Callers that only need "this
    /// request may proceed" ignore it; the two that name the caller in a message
    /// fall back to an anonymous label.
    pub account: Option<Arc<Account>>,
}

impl Authed {
    /// The operator this request speaks for, named for a log or an error.
    pub fn username(&self) -> &str {
        match &self.account {
            Some(account) => account.username(),
            None => "anonymous",
        }
    }

    /// The account, for a route that has no meaning without one.
    ///
    /// With auth off there is no session to name, so a route that rotates the
    /// password of "whoever is calling" has nothing to act on. Refusing is
    /// honest: the alternative is to pick the only account and change a
    /// credential nobody proved they own.
    pub fn account(&self) -> Result<&Arc<Account>, AuthError> {
        self.account.as_ref().ok_or_else(|| {
            AuthError::Unauthorized(
                "this route needs a signed-in operator: enable [auth] to use it".to_string(),
            )
        })
    }
}

impl FromRequestParts<super::super::api::AppState> for Authed {
    type Rejection = super::super::api::ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &super::super::api::AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = state.auth();
        if !auth.required() {
            // Auth off: the route is open by configuration, not by a request
            // that failed to prove something. A token that *is* presented is
            // still verified, so a stale cookie cannot make an open daemon
            // answer as somebody it is not.
            let account = match access_token(parts) {
                Some(token) => auth.verify(&token).ok(),
                None => None,
            };
            return Ok(Self { account });
        }
        let token = access_token(parts).ok_or_else(|| {
            super::super::api::ApiError::auth(AuthError::Unauthorized(
                "this request needs a session: log in, or send `Authorization: Bearer`".to_string(),
            ))
        })?;
        let account = auth.verify(&token)?;
        Ok(Self {
            account: Some(account),
        })
    }
}
