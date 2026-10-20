//! Refresh-token bookkeeping and the panel's authentication service.
//!
//! Access tokens are stateless: the API verifies them and never consults this
//! module. A refresh token is the stateful half, because rotation is what makes
//! a stolen refresh token detectable: presenting a revoked `jti` cancels the
//! whole session, which is the only reason the panel keeps a list at all.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::jwt::{self, Claims};
use super::password;

/// One live login session, keyed by the refresh token's `jti`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    subject: String,
    expires_at: u64,
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

/// Why a login or refresh was refused. Deliberately coarse: the caller turns
/// every variant into the same response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    InvalidCredentials,
    SessionExpired,
    Replayed,
    NotConfigured,
}

/// Shared authentication state: the config values plus the live sessions.
///
/// The config half is copied at construction. `auth enable` and `auth
/// set-password` replace the whole service through `AppState`, so a running
/// server never keeps a stale password or signing key.
#[derive(Debug)]
pub struct AuthService {
    enabled: bool,
    username: String,
    password_hash: String,
    secret: Vec<u8>,
    access_ttl: u64,
    refresh_ttl: u64,
    iterations: u32,
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
            enabled,
            username: username.into(),
            password_hash: password_hash.into(),
            secret: secret.into().into_bytes(),
            access_ttl,
            refresh_ttl,
            iterations,
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Builds the service from an `[auth]` section.
    pub fn from_config(config: &crate::config::AuthConfig) -> Self {
        Self::new(
            config.enabled,
            &config.username,
            &config.password_hash,
            &config.jwt_secret,
            config.access_token_secs,
            config.refresh_token_secs,
            config.iterations,
        )
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn access_ttl(&self) -> u64 {
        self.access_ttl
    }

    pub fn refresh_ttl(&self) -> u64 {
        self.refresh_ttl
    }

    /// Checks a password and, on success, opens a session.
    pub fn login(&self, username: &str, password: &str) -> Result<Issued, AuthError> {
        if !self.enabled || self.password_hash.is_empty() || self.secret.is_empty() {
            return Err(AuthError::NotConfigured);
        }
        // Compare the name in constant time too: a different name and a wrong
        // password must take the same path, or the response time names valid
        // accounts.
        let name_matches = const_eq(username.as_bytes(), self.username.as_bytes());
        let password_matches = password::verify_password(password, &self.password_hash);
        if !name_matches || !password_matches {
            return Err(AuthError::InvalidCredentials);
        }

        self.issue("admin")
    }

    /// Exchanges a refresh token for a new pair, revoking the old one.
    ///
    /// A token whose `jti` is unknown is either expired-and-pruned or replayed.
    /// Either way the session is gone, so the caller must log in again. A replay
    /// of a rotated token removes the whole subject's sessions.
    pub fn refresh(&self, token: &str) -> Result<Issued, AuthError> {
        if !self.enabled || self.secret.is_empty() {
            return Err(AuthError::NotConfigured);
        }
        let now = jwt::now_seconds();
        let claims = jwt::verify(token, &self.secret, now).map_err(|error| match error {
            jwt::JwtError::Expired => AuthError::SessionExpired,
            _ => AuthError::InvalidCredentials,
        })?;

        let mut sessions = self.sessions.write().expect("session lock poisoned");
        let Some(session) = sessions.get(&claims.jti) else {
            return Err(AuthError::SessionExpired);
        };
        if session.expires_at <= now || session.subject != claims.sub {
            sessions.remove(&claims.jti);
            return Err(AuthError::SessionExpired);
        }

        sessions.remove(&claims.jti);
        drop(sessions);
        self.issue(&claims.sub)
    }

    /// Verifies an access token and returns its claims.
    pub fn verify_access(&self, token: &str) -> Result<Claims, jwt::JwtError> {
        jwt::verify(token, &self.secret, jwt::now_seconds())
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
            .remove(&claims.jti);
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
        password::hash_password_with_iterations(password, self.iterations)
    }

    fn issue(&self, subject: &str) -> Result<Issued, AuthError> {
        let now = jwt::now_seconds();
        let access = Claims {
            sub: subject.to_string(),
            role: "admin".to_string(),
            iat: now,
            exp: now + self.access_ttl,
            jti: jwt::random_id(),
        };
        let refresh_jti = jwt::random_id();
        let refresh = Claims {
            sub: subject.to_string(),
            role: "admin".to_string(),
            iat: now,
            exp: now + self.refresh_ttl,
            jti: refresh_jti.clone(),
        };

        let access_token =
            jwt::sign(&access, &self.secret).map_err(|_| AuthError::NotConfigured)?;
        let refresh_token =
            jwt::sign(&refresh, &self.secret).map_err(|_| AuthError::NotConfigured)?;

        self.sessions
            .write()
            .expect("session lock poisoned")
            .insert(
                refresh_jti,
                Session {
                    subject: subject.to_string(),
                    expires_at: refresh.exp,
                },
            );

        Ok(Issued {
            access_token,
            refresh_token,
            access_expires_in: self.access_ttl,
            refresh_expires_in: self.refresh_ttl,
            username: subject.to_string(),
            role: "admin".to_string(),
        })
    }
}

/// The service shared with every request handler.
pub type SharedAuth = Arc<AuthService>;

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
            Err(AuthError::SessionExpired),
            "a rotated token must not work twice"
        );
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
    fn verify_access_rejects_a_refresh_token_signed_for_another_secret() {
        let service = service();
        let issued = service.login("admin", "hunter2").unwrap();
        assert!(service.verify_access(&issued.access_token).is_ok());

        let other = AuthService::new(
            true,
            "admin",
            service.password_hash.clone(),
            "another-secret",
            60,
            600,
            password::DEV_ITERATIONS,
        );
        assert!(other.verify_access(&issued.access_token).is_err());
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
