//! Refresh-token bookkeeping and the panel's authentication service.
//!
//! Access tokens are stateless in the signature sense, but not in the
//! revocation sense: an access token names the session it belongs to, and the
//! store below is asked whether that session is still live. Without that check
//! a token issued before a password change keeps working for its whole lifetime,
//! which defeats the reason the operator changed the password.
//!
//! Refresh tokens rotate. Presenting a token that is not the current one for a
//! live session cancels that session: a valid signature naming an old
//! identifier is what a copied cookie looks like from here.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::jwt::{self, Claims};
use super::password;

/// One live login session.
///
/// The map is keyed by a session id that both tokens carry:
///
/// - the refresh half is the *current* `jti`, so anything else presented for a
///   live session is a replay;
/// - the access half has that session id plus a distinct `jti`, so an access
///   token can be traced back here for revocation without being confused with
///   the refresh token.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    subject: String,
    expires_at: u64,
    /// Fingerprint of the current refresh identifier. The identifier is already
    /// unguessable; this only keeps a copy out of a memory dump in a form that
    /// could be replayed.
    refresh_hash: u64,
}

/// What a login attempt produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issued {
    pub access_token: String,
    pub refresh_token: String,
    /// Remaining access-token lifetime, for the client's refresh scheduling.
    pub access_expires_in: u64,
    /// Remaining refresh-token lifetime, which is also the cookie's `Max-Age`.
    pub refresh_expires_in: u64,
    pub username: String,
    pub role: String,
}

/// Why a login or refresh was refused.
///
/// [`AuthError::Replayed`] is separate from [`AuthError::SessionExpired`] only
/// so the caller can say why the login screen came back; neither variant names
/// a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    InvalidCredentials,
    SessionExpired,
    Replayed,
    NotConfigured,
}

/// The `[auth]` fields that are policy rather than credentials.
///
/// Kept apart from the password digest and the signing key so the settings page
/// can change a value without having to rebuild the service, which would end
/// every session.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Policy {
    enabled: bool,
    username: String,
    access_ttl: u64,
    refresh_ttl: u64,
    iterations: u32,
    cookie_secure: bool,
}

/// Shared authentication state: the config values plus the live sessions.
///
/// The policy half is mutable because the settings page changes it while the
/// panel runs, and that keeps the sessions. Changing a *credential* replaces
/// the whole service through `AppState::replace_auth`, so a stale password or
/// signing key never survives a change.
#[derive(Debug)]
pub struct AuthService {
    policy: RwLock<Policy>,
    password_hash: String,
    secret: Vec<u8>,
    sessions: RwLock<HashMap<String, Session>>,
}

impl AuthService {
    pub fn new(
        enabled: bool,
        username: impl Into<String>,
        password_hash: impl Into<String>,
        secret: impl Into<String>,
        access_ttl: u64,
        refresh_ttl: u64,
        iterations: u32,
    ) -> Self {
        Self {
            policy: RwLock::new(Policy {
                enabled,
                username: username.into(),
                access_ttl,
                refresh_ttl,
                iterations,
                cookie_secure: false,
            }),
            password_hash: password_hash.into(),
            secret: secret.into().into_bytes(),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Builds the service from an `[auth]` section.
    pub fn from_config(config: &crate::config::AuthConfig) -> Self {
        let service = Self::new(
            config.enabled,
            &config.username,
            &config.password_hash,
            &config.jwt_secret,
            config.access_token_secs,
            config.refresh_token_secs,
            config.iterations,
        );
        service.apply_config(config);
        service
    }

    /// Applies a new `[auth]` section without ending any session.
    ///
    /// Only the policy fields move: the password hash, the signing key and the
    /// session map stay. The caller is responsible for routing a credential
    /// change through [`crate::app::AppState::replace_auth`], which does end
    /// them.
    pub fn apply_config(&self, config: &crate::config::AuthConfig) {
        let mut policy = self.policy.write().expect("policy lock poisoned");
        policy.enabled = config.enabled;
        policy.username.clone_from(&config.username);
        policy.access_ttl = config.access_token_secs;
        policy.refresh_ttl = config.refresh_token_secs;
        policy.iterations = config.iterations;
        policy.cookie_secure = config.cookie_secure;
    }

    fn policy(&self) -> std::sync::RwLockReadGuard<'_, Policy> {
        self.policy.read().expect("policy lock poisoned")
    }

    pub fn enabled(&self) -> bool {
        self.policy().enabled
    }

    pub fn username(&self) -> String {
        self.policy().username.clone()
    }

    pub fn access_ttl(&self) -> u64 {
        self.policy().access_ttl
    }

    /// Seconds left on an access token, for the client's refresh scheduling.
    pub fn access_remaining(&self, token: &str) -> u64 {
        jwt::verify(token, &self.secret, 0)
            .map(|claims| claims.exp.saturating_sub(jwt::now_seconds()))
            .unwrap_or(0)
    }

    /// Seconds left on a refresh token.
    pub fn refresh_remaining(&self, token: &str) -> u64 {
        jwt::verify(token, &self.secret, 0)
            .map(|claims| claims.exp.saturating_sub(jwt::now_seconds()))
            .unwrap_or(0)
    }

    pub fn refresh_ttl(&self) -> u64 {
        self.policy().refresh_ttl
    }

    pub fn cookie_secure(&self) -> bool {
        self.policy().cookie_secure
    }

    /// Checks a password and, on success, opens a session.
    pub fn login(&self, username: &str, password: &str) -> Result<Issued, AuthError> {
        let policy = self.policy();
        if !policy.enabled || self.password_hash.is_empty() || self.secret.is_empty() {
            return Err(AuthError::NotConfigured);
        }
        // Compare the name in constant time too: a different name and a wrong
        // password must take the same path, or the response time names valid
        // accounts.
        let name_matches = const_eq(username.as_bytes(), policy.username.as_bytes());
        let password_matches = password::verify_password(password, &self.password_hash);
        if !name_matches || !password_matches {
            return Err(AuthError::InvalidCredentials);
        }

        let subject = policy.username.clone();
        drop(policy);
        self.issue(&subject)
    }

    /// Exchanges a refresh token for a new pair, spending the presented one.
    ///
    /// A token whose session is unknown is expired-and-pruned or came from a
    /// service that restarted: the caller logs in again. A token whose session
    /// is live but whose `jti` is not the current one is a replay, and it costs
    /// the subject that session.
    pub fn refresh(&self, token: &str) -> Result<Issued, AuthError> {
        if !self.policy().enabled || self.secret.is_empty() {
            return Err(AuthError::NotConfigured);
        }
        // An expired refresh token must still be *recognised*, so that a token
        // from a session that was cancelled is not reported as a replay.
        let claims =
            jwt::verify(token, &self.secret, 0).map_err(|_| AuthError::InvalidCredentials)?;
        if claims.kind != KIND_REFRESH {
            return Err(AuthError::InvalidCredentials);
        }

        let session_id = claims.sid.clone();
        let presented_hash = fingerprint(&claims.jti);

        // The check, the spend and the re-issue happen under one write lock.
        // Splitting them leaves a window where two callers holding the same
        // token both pass the check and both receive a pair, which is exactly
        // the replay this rotation exists to detect.
        let mut sessions = self.sessions.write().expect("session lock poisoned");
        let Some(session) = sessions.get(&session_id) else {
            return Err(AuthError::SessionExpired);
        };

        let now = jwt::now_seconds();
        if session.expires_at <= now || session.subject != claims.sub {
            sessions.remove(&session_id);
            return Err(AuthError::SessionExpired);
        }
        if session.refresh_hash != presented_hash {
            // A valid signature for a live session that is not its current
            // refresh token: a replay, or a token from another half offered
            // here. Both mean a token is where it should not be.
            sessions.remove(&session_id);
            return Err(AuthError::Replayed);
        }

        let (issued, rotated) = {
            let snapshot = session.clone();
            self.mint(
                &snapshot.subject,
                &session_id,
                now,
                &snapshot,
                Some(&claims),
            )?
        };
        if let Some(stored) = sessions.get_mut(&session_id) {
            stored.refresh_hash = rotated.refresh_hash;
            stored.expires_at = rotated.expires_at;
        }
        Ok(issued)
    }

    /// Verifies an access token and checks the session it belongs to.
    ///
    /// The signature check alone is not enough: a token signed before a
    /// password change would otherwise keep working until it expired, and the
    /// password change is exactly what the operator did to stop that.
    pub fn verify_access(&self, token: &str) -> Result<Claims, jwt::JwtError> {
        let claims = jwt::verify(token, &self.secret, jwt::now_seconds())?;
        if claims.sid.is_empty() || claims.kind != KIND_ACCESS {
            return Err(jwt::JwtError::Malformed);
        }

        let now = jwt::now_seconds();
        let sessions = self.sessions.read().expect("session lock poisoned");
        match sessions.get(&claims.sid) {
            Some(session) if session.expires_at > now => Ok(claims),
            _ => Err(jwt::JwtError::Revoked),
        }
    }

    /// Whether a refresh token is the *current* one for its live session.
    ///
    /// `GET /api/auth/session` asks this rather than merely checking that the
    /// session exists, so a cookie that was rotated elsewhere does not claim the
    /// login.
    pub fn holds_current_refresh(&self, token: &str) -> bool {
        if !self.policy().enabled || self.secret.is_empty() {
            return false;
        }
        let Ok(claims) = jwt::verify(token, &self.secret, 0) else {
            return false;
        };
        if claims.kind != KIND_REFRESH {
            return false;
        }
        let now = jwt::now_seconds();
        let sessions = self.sessions.read().expect("session lock poisoned");
        sessions.get(&claims.sid).is_some_and(|session| {
            session.expires_at > now
                && session.subject == claims.sub
                && session.refresh_hash == fingerprint(&claims.jti)
        })
    }

    /// Ends one session. Unknown identifiers are ignored: logging out twice is
    /// not an error.
    pub fn revoke(&self, token: &str) {
        let Ok(claims) = jwt::verify(
            token,
            &self.secret,
            // An expired refresh token still has to be revocable.
            0,
        ) else {
            return;
        };
        self.sessions
            .write()
            .expect("session lock poisoned")
            .remove(&claims.sid);
    }

    /// Ends every session, which is what a password change must do.
    pub fn revoke_all(&self) {
        self.sessions
            .write()
            .expect("session lock poisoned")
            .clear();
    }

    /// Number of live sessions, for the settings view.
    pub fn session_count(&self) -> usize {
        self.sessions.read().expect("session lock poisoned").len()
    }

    pub fn hashes_password(&self, password: &str) -> bool {
        password::verify_password(password, &self.password_hash)
    }

    /// Hashes a new password with this service's cost.
    pub fn hash_new_password(&self, password: &str) -> Result<String, String> {
        password::hash_password_with_iterations(password, self.policy().iterations)
    }

    /// Starts a session and returns its first token pair.
    fn issue(&self, subject: &str) -> Result<Issued, AuthError> {
        let session_id = jwt::random_id();
        let now = jwt::now_seconds();
        let session = Session {
            subject: subject.to_string(),
            expires_at: now + self.refresh_ttl(),
            refresh_hash: 0,
        };

        let (issued, started) = self.mint(subject, &session_id, now, &session, None)?;
        self.sessions
            .write()
            .expect("session lock poisoned")
            .insert(session_id, started);
        Ok(issued)
    }

    /// Signs a pair for `session_id` and returns it with the updated record.
    ///
    /// The refresh token keeps the `exp` of the session it belongs to: rotating
    /// must not extend a login forever, only replace the identifier.
    fn mint(
        &self,
        subject: &str,
        session_id: &str,
        now: u64,
        session: &Session,
        previous: Option<&Claims>,
    ) -> Result<(Issued, Session), AuthError> {
        let refresh_jti = jwt::random_id();
        let refresh_exp = previous.map_or(session.expires_at, |claims| claims.exp);

        let access = Claims {
            sub: subject.to_string(),
            role: "admin".to_string(),
            iat: now,
            exp: now + self.access_ttl(),
            jti: jwt::random_id(),
            kind: KIND_ACCESS.to_string(),
            sid: session_id.to_string(),
        };
        let refresh = Claims {
            sub: subject.to_string(),
            role: "admin".to_string(),
            iat: previous.map_or(now, |claims| claims.iat),
            exp: refresh_exp,
            jti: refresh_jti.clone(),
            kind: KIND_REFRESH.to_string(),
            sid: session_id.to_string(),
        };

        let access_token =
            jwt::sign(&access, &self.secret).map_err(|_| AuthError::NotConfigured)?;
        let refresh_token =
            jwt::sign(&refresh, &self.secret).map_err(|_| AuthError::NotConfigured)?;

        let mut updated = session.clone();
        updated.refresh_hash = fingerprint(&refresh_jti);
        updated.expires_at = refresh_exp;

        Ok((
            Issued {
                access_token,
                refresh_token,
                access_expires_in: self.access_ttl(),
                refresh_expires_in: refresh_exp.saturating_sub(now),
                username: subject.to_string(),
                role: "admin".to_string(),
            },
            updated,
        ))
    }
}

/// The service shared with every request handler.
pub type SharedAuth = Arc<AuthService>;

/// Token halves. A refresh token is not an API credential.
const KIND_ACCESS: &str = "access";
const KIND_REFRESH: &str = "refresh";

/// A stable fingerprint of a token identifier.
///
/// The identifiers are 128 random bits, so this is not a security boundary; it
/// keeps the stored form of a refresh identifier from being replayable on its
/// own.
fn fingerprint(value: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn const_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> AuthService {
        AuthService::new(
            true,
            "admin",
            password::hash_password_with_iterations("hunter2", password::DEV_ITERATIONS).unwrap(),
            "test-secret",
            60,
            600,
            password::DEV_ITERATIONS,
        )
    }

    #[test]
    fn a_valid_login_issues_both_tokens() {
        let issued = service().login("admin", "hunter2").unwrap();
        assert_eq!(issued.username, "admin");
        assert_eq!(issued.access_expires_in, 60);
        assert!(issued.access_token.split('.').count() == 3);
        assert!(issued.refresh_token.split('.').count() == 3);
    }

    #[test]
    fn a_wrong_password_is_refused() {
        let service = service();
        assert_eq!(
            service.login("admin", "hunter3"),
            Err(AuthError::InvalidCredentials)
        );
        assert_eq!(
            service.login("root", "hunter2"),
            Err(AuthError::InvalidCredentials)
        );
    }

    #[test]
    fn a_disabled_service_refuses_every_login() {
        let service = AuthService::new(
            false,
            "admin",
            password::hash_password_with_iterations("hunter2", password::DEV_ITERATIONS).unwrap(),
            "test-secret",
            60,
            600,
            password::DEV_ITERATIONS,
        );
        assert_eq!(
            service.login("admin", "hunter2"),
            Err(AuthError::NotConfigured)
        );
    }

    #[test]
    fn refresh_rotates_and_invalidates_the_presented_token() {
        let service = service();
        let first = service.login("admin", "hunter2").unwrap();
        let second = service.refresh(&first.refresh_token).unwrap();
        assert_ne!(first.refresh_token, second.refresh_token);
        assert_eq!(
            service.refresh(&first.refresh_token),
            Err(AuthError::Replayed),
            "a rotated token must not work twice"
        );
    }

    #[test]
    fn replaying_a_rotated_token_cancels_the_session() {
        let service = service();
        let first = service.login("admin", "hunter2").unwrap();
        let second = service.refresh(&first.refresh_token).unwrap();

        // The intruder replays the copied token.
        assert_eq!(
            service.refresh(&first.refresh_token),
            Err(AuthError::Replayed)
        );

        // The legitimate client's fresh token is gone too, which is the point:
        // one of the two holders is an intruder and neither can be trusted.
        assert_eq!(service.session_count(), 0);
        assert_eq!(
            service.refresh(&second.refresh_token),
            Err(AuthError::SessionExpired)
        );
    }

    #[test]
    fn an_older_access_token_stops_at_once_after_a_revoke() {
        let service = service();
        let issued = service.login("admin", "hunter2").unwrap();
        assert!(service.verify_access(&issued.access_token).is_ok());

        service.revoke_all();
        assert_eq!(
            service.verify_access(&issued.access_token),
            Err(jwt::JwtError::Revoked),
            "revocation must not wait for the access token to expire"
        );
    }

    #[test]
    fn rotation_does_not_extend_the_session() {
        let service = service();
        let first = service.login("admin", "hunter2").unwrap();
        let second = service.refresh(&first.refresh_token).unwrap();
        assert!(
            second.refresh_expires_in <= first.refresh_expires_in,
            "a rotated token must inherit the session's expiry"
        );
    }

    #[test]
    fn an_access_token_from_a_replaced_service_is_refused() {
        let service = service();
        let issued = service.login("admin", "hunter2").unwrap();

        // What `POST /api/auth/password` does: a new service with a new secret.
        let replacement = AuthService::new(
            true,
            "admin",
            service.password_hash.clone(),
            "another-secret",
            60,
            600,
            password::DEV_ITERATIONS,
        );
        assert!(replacement.verify_access(&issued.access_token).is_err());
    }

    #[test]
    fn revoke_ends_a_session() {
        let service = service();
        let issued = service.login("admin", "hunter2").unwrap();
        assert_eq!(service.session_count(), 1);
        service.revoke(&issued.refresh_token);
        assert_eq!(service.session_count(), 0);
        assert_eq!(
            service.refresh(&issued.refresh_token),
            Err(AuthError::SessionExpired)
        );
    }

    #[test]
    fn a_live_session_is_reported_and_a_revoked_one_is_not() {
        let service = service();
        let issued = service.login("admin", "hunter2").unwrap();
        assert!(service.holds_current_refresh(&issued.refresh_token));

        service.revoke(&issued.refresh_token);
        assert!(!service.holds_current_refresh(&issued.refresh_token));
    }

    #[test]
    fn a_forged_cookie_is_not_a_live_session() {
        let service = service();
        assert!(!service.holds_current_refresh("whatever"));
        assert!(!service.holds_current_refresh("a.b.c"));
    }

    #[test]
    fn a_rotated_away_cookie_does_not_claim_the_login() {
        let service = service();
        let first = service.login("admin", "hunter2").unwrap();
        service.refresh(&first.refresh_token).unwrap();
        assert!(
            !service.holds_current_refresh(&first.refresh_token),
            "a cookie that was rotated elsewhere must not report a session"
        );
    }

    #[test]
    fn an_access_token_cannot_be_used_as_a_refresh_token() {
        let service = service();
        let issued = service.login("admin", "hunter2").unwrap();
        // Rejected on the token kind, before the session is even consulted, so
        // a client that mixes the two up does not lose its session over it.
        assert_eq!(
            service.refresh(&issued.access_token),
            Err(AuthError::InvalidCredentials)
        );
        assert_eq!(service.session_count(), 1);
    }

    #[test]
    fn a_refresh_token_is_not_an_api_credential() {
        let service = service();
        let issued = service.login("admin", "hunter2").unwrap();
        assert!(service.verify_access(&issued.access_token).is_ok());
        assert_eq!(
            service.verify_access(&issued.refresh_token),
            Err(jwt::JwtError::Malformed),
            "a refresh token must not reach a protected route"
        );

        // Spending it must not turn the *old* one into a credential either.
        let rotated = service.refresh(&issued.refresh_token).unwrap();
        assert_eq!(
            service.verify_access(&issued.refresh_token),
            Err(jwt::JwtError::Malformed)
        );
        assert_eq!(
            service.verify_access(&rotated.refresh_token),
            Err(jwt::JwtError::Malformed)
        );
    }

    #[test]
    fn a_concurrent_refresh_of_one_token_yields_one_pair() {
        let service = std::sync::Arc::new(service());
        let issued = service.login("admin", "hunter2").unwrap();

        let mut handles = Vec::new();
        for _ in 0..8 {
            let service = std::sync::Arc::clone(&service);
            let token = issued.refresh_token.clone();
            handles.push(std::thread::spawn(move || service.refresh(&token)));
        }

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let successes = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(
            successes, 1,
            "two callers holding one token must not both get a pair"
        );
    }

    #[test]
    fn the_remaining_lifetime_shrinks() {
        let service = AuthService::new(
            true,
            "admin",
            password::hash_password_with_iterations("hunter2", password::DEV_ITERATIONS).unwrap(),
            "test-secret",
            60,
            600,
            password::DEV_ITERATIONS,
        );
        let issued = service.login("admin", "hunter2").unwrap();
        let access = service.access_remaining(&issued.access_token);
        let refresh = service.refresh_remaining(&issued.refresh_token);
        assert!(access > 0 && access <= 60);
        assert!(refresh > 0 && refresh <= 600);
        assert!(
            refresh > access,
            "the refresh token outlives the access one"
        );
    }

    #[test]
    fn revoke_all_clears_every_session() {
        let service = service();
        service.login("admin", "hunter2").unwrap();
        service.login("admin", "hunter2").unwrap();
        assert_eq!(service.session_count(), 2);
        service.revoke_all();
        assert_eq!(service.session_count(), 0);
    }
}
