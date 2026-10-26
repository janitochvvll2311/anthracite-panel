//! One operator account, two tokens and a first-run bootstrap.
//!
//! Credentials live in `secrets.toml`; refresh-token records live in the state
//! directory. The username remains operator policy in `[auth] username`.

pub mod jwt;
pub mod password;
pub mod sessions;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use anth_wire::{ErrorKind, WireError};
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, value};

use crate::config::{AuthConfig, BootstrapMode, Paths};
use crate::state;

pub use sessions::SessionRecord;

/// The short-lived access cookie.
pub const ACCESS_COOKIE: &str = "anth_access";
/// The long-lived refresh cookie, scoped to the auth routes.
pub const REFRESH_COOKIE: &str = "anth_refresh";
const REFRESH_PATH: &str = "/api/auth";

/// The refresh-token state shown to the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Bootstrap {
    Web,
    Closed,
}

/// What the browser asks before choosing between registration and login.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthState {
    pub required: bool,
    pub registered: bool,
    pub authenticated: bool,
    pub bootstrap: Bootstrap,
    pub username: String,
}

/// A login pair handed to the browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: String,
}

/// Why an authentication step refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    NotRegistered,
    AlreadyRegistered,
    Unauthorized(String),
    Invalid(String),
    WeakPassword(String),
    Forbidden(String),
    Internal(String),
}

impl AuthError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            AuthError::NotRegistered | AuthError::AlreadyRegistered => ErrorKind::Conflict,
            AuthError::Unauthorized(_) => ErrorKind::Unauthorized,
            AuthError::Invalid(_) | AuthError::WeakPassword(_) => ErrorKind::Invalid,
            AuthError::Forbidden(_) => ErrorKind::Forbidden,
            AuthError::Internal(_) => ErrorKind::Internal,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            AuthError::NotRegistered => "not_registered",
            AuthError::AlreadyRegistered => "bootstrap_closed",
            AuthError::Unauthorized(_) => "invalid_credentials",
            AuthError::Invalid(_) => "invalid_request",
            AuthError::WeakPassword(_) => "password_too_short",
            AuthError::Forbidden(_) => "forbidden",
            AuthError::Internal(_) => "internal",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            AuthError::NotRegistered => "no account is registered yet",
            AuthError::AlreadyRegistered => "an account is already registered",
            AuthError::Unauthorized(why)
            | AuthError::Invalid(why)
            | AuthError::WeakPassword(why)
            | AuthError::Forbidden(why)
            | AuthError::Internal(why) => why,
        }
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for AuthError {}

impl From<AuthError> for WireError {
    fn from(error: AuthError) -> Self {
        WireError::new(error.kind(), error.code(), error.message())
    }
}

#[derive(Debug, Clone)]
struct Account {
    password_hash: String,
    signing_key: Vec<u8>,
}

/// Shared authentication state.
pub struct AuthService {
    config: AuthConfig,
    secrets_path: PathBuf,
    sessions_path: PathBuf,
    account: RwLock<Option<Account>>,
    sessions: Mutex<Vec<SessionRecord>>,
    bootstrap: RwLock<Bootstrap>,
}

impl AuthService {
    /// Loads credentials and runs the first-run bootstrap.
    ///
    /// Returns a generated password for the caller to print once, never for the
    /// logger. A generated password is only produced when no hash exists.
    pub fn load(config: &AuthConfig, paths: &Paths) -> Result<(Arc<Self>, Option<String>), String> {
        let mut secrets = read_secrets(&paths.secrets)?;
        let mut generated = None;
        let mut account = None;
        let mut bootstrap = Bootstrap::Closed;

        let key_was_missing = secrets.auth.signing_key.is_empty();
        if key_was_missing {
            secrets.auth.signing_key = sessions::hex(&random_bytes(32)?);
        }

        if !secrets.auth.password_hash.is_empty() {
            if key_was_missing {
                write_secrets(&paths.secrets, &secrets.auth)?;
            }
            let key = sessions::unhex(&secrets.auth.signing_key)
                .ok_or_else(|| "`auth.signing_key` is not hexadecimal".to_string())?;
            account = Some(Account {
                password_hash: secrets.auth.password_hash.clone(),
                signing_key: key,
            });
        } else if config.enabled {
            match config.bootstrap.resolve() {
                BootstrapMode::Generated => {
                    let password = generate_password()?;
                    let hash = password::hash(&password)?;
                    secrets.auth.password_hash = hash;
                    write_secrets(&paths.secrets, &secrets.auth)?;
                    let key = sessions::unhex(&secrets.auth.signing_key)
                        .ok_or_else(|| "`auth.signing_key` is not hexadecimal".to_string())?;
                    account = Some(Account {
                        password_hash: secrets.auth.password_hash.clone(),
                        signing_key: key,
                    });
                    generated = Some(password);
                }
                BootstrapMode::Web | BootstrapMode::Auto => {
                    // `Auto` was resolved above; the arm is unreachable in
                    // practice but keeps the match total.
                    write_secrets(&paths.secrets, &secrets.auth)?;
                    bootstrap = Bootstrap::Web;
                }
            }
        } else {
            // Auth is off. Keep an existing credential, but do not create one.
            if !secrets.auth.password_hash.is_empty() {
                let key = sessions::unhex(&secrets.auth.signing_key)
                    .ok_or_else(|| "`auth.signing_key` is not hexadecimal".to_string())?;
                account = Some(Account {
                    password_hash: secrets.auth.password_hash.clone(),
                    signing_key: key,
                });
            }
        }

        let mut records = sessions::load(&paths.sessions)?;
        sessions::trim(&mut records, jwt::now_seconds());

        Ok((
            Arc::new(Self {
                config: config.clone(),
                secrets_path: paths.secrets.clone(),
                sessions_path: paths.sessions.clone(),
                account: RwLock::new(account),
                sessions: Mutex::new(records),
                bootstrap: RwLock::new(bootstrap),
            }),
            generated,
        ))
    }

    pub fn state(&self) -> AuthState {
        AuthState {
            required: self.config.enabled,
            registered: self.account.read().expect("account lock").is_some(),
            authenticated: false,
            bootstrap: *self.bootstrap.read().expect("bootstrap lock"),
            username: self.config.username.clone(),
        }
    }

    pub fn required(&self) -> bool {
        self.config.enabled
    }

    /// Registers the first password while the web bootstrap is open.
    pub fn register(&self, password: &str) -> Result<Grant, AuthError> {
        if !self.config.enabled {
            return Err(AuthError::Forbidden(
                "authentication is disabled".to_string(),
            ));
        }
        if self.account.read().expect("account lock").is_some()
            || *self.bootstrap.read().expect("bootstrap lock") != Bootstrap::Web
        {
            return Err(AuthError::AlreadyRegistered);
        }

        let hash = hash_password(password)?;
        let mut secrets = read_secrets(&self.secrets_path).map_err(AuthError::Internal)?;
        secrets.auth.password_hash = hash.clone();
        write_secrets(&self.secrets_path, &secrets.auth).map_err(AuthError::Internal)?;
        let key = sessions::unhex(&secrets.auth.signing_key)
            .ok_or_else(|| AuthError::Internal("the signing key is not hexadecimal".to_string()))?;
        {
            let mut account = self.account.write().expect("account lock");
            *account = Some(Account {
                password_hash: hash,
                signing_key: key,
            });
        }
        *self.bootstrap.write().expect("bootstrap lock") = Bootstrap::Closed;
        self.start_session()
    }

    /// Checks a password and opens a session.
    pub fn login(&self, username: &str, password: &str) -> Result<Grant, AuthError> {
        let account = self
            .account
            .read()
            .expect("account lock")
            .clone()
            .ok_or(AuthError::NotRegistered)?;
        let name_matches = constant_time_eq(username.as_bytes(), self.config.username.as_bytes());
        let password_matches = password::verify(password, &account.password_hash);
        if !name_matches || !password_matches {
            return Err(AuthError::Unauthorized(
                "incorrect username or password".to_string(),
            ));
        }
        self.start_session()
    }

    /// Rotates a refresh token. A reused token drops its whole chain.
    pub fn refresh(&self, presented: &str) -> Result<Grant, AuthError> {
        let account = self
            .account
            .read()
            .expect("account lock")
            .clone()
            .ok_or(AuthError::NotRegistered)?;
        let now = jwt::now_seconds();
        let digest = sessions::digest(presented);
        let mut records = self.sessions.lock().expect("session lock");

        let Some(index) = records.iter().position(|record| record.digest == digest) else {
            return Err(AuthError::Unauthorized(
                "the refresh token is not recognised".to_string(),
            ));
        };
        if records[index].used {
            let chain = records[index].chain.clone();
            records.retain(|record| record.chain != chain);
            sessions::save(&self.sessions_path, &records).map_err(AuthError::Internal)?;
            return Err(AuthError::Unauthorized(
                "this refresh token was already used".to_string(),
            ));
        }
        if records[index].expires <= now {
            records.remove(index);
            sessions::save(&self.sessions_path, &records).map_err(AuthError::Internal)?;
            return Err(AuthError::Unauthorized(
                "the session expired; log in again".to_string(),
            ));
        }

        let sid = records[index].sid.clone();
        let chain = records[index].chain.clone();
        records[index].used = true;
        let token = random_token()?;
        records.push(SessionRecord {
            sid: sid.clone(),
            digest: sessions::digest(&token),
            chain,
            expires: now + self.config.refresh_token_secs,
            used: false,
        });
        sessions::trim(&mut records, now);
        sessions::save(&self.sessions_path, &records).map_err(AuthError::Internal)?;
        drop(records);

        Ok(Grant {
            access_token: self.issue_access(&account, &sid, now)?,
            token_type: "Bearer".to_string(),
            expires_in: self.config.access_token_secs,
            refresh_token: token,
        })
    }

    /// Drops every session.
    pub fn logout(&self) -> Result<usize, AuthError> {
        let mut records = self.sessions.lock().expect("session lock");
        let count = records.len();
        records.clear();
        sessions::save(&self.sessions_path, &records).map_err(AuthError::Internal)?;
        Ok(count)
    }

    /// Replaces the password and ends every session.
    pub fn change_password(&self, current: &str, next: &str) -> Result<(), AuthError> {
        let account = self
            .account
            .read()
            .expect("account lock")
            .clone()
            .ok_or(AuthError::NotRegistered)?;
        if !password::verify(current, &account.password_hash) {
            return Err(AuthError::Unauthorized(
                "the current password is incorrect".to_string(),
            ));
        }
        let hash = hash_password(next)?;
        let mut secrets = read_secrets(&self.secrets_path).map_err(AuthError::Internal)?;
        secrets.auth.password_hash = hash.clone();
        write_secrets(&self.secrets_path, &secrets.auth).map_err(AuthError::Internal)?;
        {
            let mut account = self.account.write().expect("account lock");
            if let Some(account) = account.as_mut() {
                account.password_hash = hash;
            }
        }
        let mut records = self.sessions.lock().expect("session lock");
        records.clear();
        sessions::save(&self.sessions_path, &records).map_err(AuthError::Internal)?;
        Ok(())
    }

    /// Verifies an access token and returns its session id.
    pub fn verify_access(&self, token: &str) -> Result<String, AuthError> {
        let account =
            self.account
                .read()
                .expect("account lock")
                .clone()
                .ok_or(AuthError::Unauthorized(
                    "no account is registered".to_string(),
                ))?;
        let claims = jwt::verify(token, &account.signing_key, jwt::now_seconds())
            .map_err(|_| AuthError::Unauthorized("the access token is not valid".to_string()))?;
        if claims.kind != "access" || claims.sub != self.config.username {
            return Err(AuthError::Unauthorized(
                "the access token is not valid".to_string(),
            ));
        }
        let now = jwt::now_seconds();
        let records = self.sessions.lock().expect("session lock");
        if !records
            .iter()
            .any(|record| record.sid == claims.sid && record.expires > now)
        {
            return Err(AuthError::Unauthorized(
                "the session is no longer live".to_string(),
            ));
        }
        Ok(claims.sid)
    }

    /// The cookies that hand a grant to the browser.
    pub fn set_cookies(&self, grant: &Grant) -> Vec<String> {
        let secure = if self.config.cookie_secure {
            "; Secure"
        } else {
            ""
        };
        vec![
            format!(
                "{ACCESS_COOKIE}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}{secure}",
                grant.access_token, grant.expires_in
            ),
            format!(
                "{REFRESH_COOKIE}={}; Path={REFRESH_PATH}; HttpOnly; SameSite=Strict; Max-Age={}{secure}",
                grant.refresh_token, self.config.refresh_token_secs
            ),
        ]
    }

    /// Cookies that clear both tokens.
    pub fn clear_cookies(&self) -> Vec<String> {
        let secure = if self.config.cookie_secure {
            "; Secure"
        } else {
            ""
        };
        vec![
            format!("{ACCESS_COOKIE}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}"),
            format!(
                "{REFRESH_COOKIE}=; Path={REFRESH_PATH}; HttpOnly; SameSite=Strict; Max-Age=0{secure}"
            ),
        ]
    }

    fn start_session(&self) -> Result<Grant, AuthError> {
        let account = self
            .account
            .read()
            .expect("account lock")
            .clone()
            .ok_or(AuthError::NotRegistered)?;
        let now = jwt::now_seconds();
        let sid = sessions::hex(&random_bytes(16).map_err(AuthError::Internal)?);
        let token = random_token()?;
        let chain = sessions::hex(&random_bytes(16).map_err(AuthError::Internal)?);

        {
            let mut records = self.sessions.lock().expect("session lock");
            records.push(SessionRecord {
                sid: sid.clone(),
                digest: sessions::digest(&token),
                chain,
                expires: now + self.config.refresh_token_secs,
                used: false,
            });
            sessions::trim(&mut records, now);
            sessions::save(&self.sessions_path, &records).map_err(AuthError::Internal)?;
        }

        Ok(Grant {
            access_token: self.issue_access(&account, &sid, now)?,
            token_type: "Bearer".to_string(),
            expires_in: self.config.access_token_secs,
            refresh_token: token,
        })
    }

    fn issue_access(&self, account: &Account, sid: &str, now: u64) -> Result<String, AuthError> {
        let claims = jwt::Claims {
            sub: self.config.username.clone(),
            iss: "anth-panel".to_string(),
            iat: now,
            exp: now + self.config.access_token_secs,
            sid: sid.to_string(),
            kind: "access".to_string(),
        };
        jwt::sign(&claims, &account.signing_key).map_err(AuthError::Internal)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SecretsFile {
    version: u32,
    auth: SecretsAuth,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SecretsAuth {
    #[serde(default)]
    password_hash: String,
    #[serde(default)]
    signing_key: String,
}

fn read_secrets(path: &Path) -> Result<SecretsFile, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SecretsFile {
                version: 1,
                auth: SecretsAuth::default(),
            });
        }
        Err(error) => return Err(format!("reading {}: {error}", path.display())),
    };
    let file: SecretsFile = toml::from_str(&text)
        .map_err(|error| format!("{} is not valid secret state: {error}", path.display()))?;
    if file.version != 1 {
        return Err(format!(
            "{} has secret version {}, expected 1",
            path.display(),
            file.version
        ));
    }
    Ok(file)
}

fn write_secrets(path: &Path, auth: &SecretsAuth) -> Result<(), String> {
    let current = state::read_or_empty(path)?;
    let fresh = current.trim().is_empty();
    let mut document = if fresh {
        DocumentMut::new()
    } else {
        current
            .parse::<DocumentMut>()
            .map_err(|error| format!("{} is not valid TOML: {error}", path.display()))?
    };
    document["version"] = value(1);
    let table = document
        .entry("auth")
        .or_insert(Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .ok_or_else(|| "the `auth` key is not a table".to_string())?;
    table["password_hash"] = value(&auth.password_hash);
    table["signing_key"] = value(&auth.signing_key);
    let mut text = String::new();
    if fresh {
        text.push_str("# Anthracite Panel credentials. Machine-owned; do not edit by hand.\n");
    }
    text.push_str(&document.to_string());
    state::write_atomic(path, text.as_bytes(), 0o600)
}

fn hash_password(password: &str) -> Result<String, AuthError> {
    password::hash(password).map_err(|message| {
        if message.contains("at least") {
            AuthError::WeakPassword(message)
        } else {
            AuthError::Internal(message)
        }
    })
}

fn random_bytes(length: usize) -> Result<Vec<u8>, String> {
    let mut bytes = vec![0u8; length];
    getrandom::fill(&mut bytes).map_err(|error| format!("reading entropy: {error}"))?;
    Ok(bytes)
}

fn random_token() -> Result<String, AuthError> {
    use base64::Engine;
    let bytes = random_bytes(32).map_err(AuthError::Internal)?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn generate_password() -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let bytes = random_bytes(20)?;
    let mut out = String::with_capacity(bytes.len());
    for byte in bytes {
        out.push(ALPHABET[byte as usize % ALPHABET.len()] as char);
    }
    Ok(out)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn scratch(name: &str) -> Paths {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::path::PathBuf::from("/tmp/anthracite-panel/tests")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&root).expect("create scratch");
        let config = Config {
            socket_path: "./anth.sock".to_string(),
            state_dir: "./state".to_string(),
            ..Config::default()
        };
        config.paths(root.join("config.toml")).expect("paths")
    }

    #[test]
    fn generated_mode_creates_a_password_once() {
        let paths = scratch("auth-generated");
        let config = AuthConfig {
            bootstrap: BootstrapMode::Generated,
            ..AuthConfig::default()
        };
        let (auth, password) = AuthService::load(&config, &paths).expect("load");
        let password = password.expect("generated");
        assert!(password.chars().count() >= 12);
        assert!(auth.state().registered);
        assert_eq!(auth.state().bootstrap, Bootstrap::Closed);
        assert!(auth.login("admin", &password).is_ok());
        std::fs::remove_dir_all(paths.config_dir).unwrap();
    }

    #[test]
    fn web_mode_registers_once_and_closes() {
        let paths = scratch("auth-web");
        let config = AuthConfig {
            bootstrap: BootstrapMode::Web,
            ..AuthConfig::default()
        };
        let (auth, password) = AuthService::load(&config, &paths).expect("load");
        assert!(password.is_none());
        assert!(!auth.state().registered);
        assert_eq!(auth.state().bootstrap, Bootstrap::Web);
        assert_eq!(
            auth.register("short").expect_err("short").code(),
            "password_too_short"
        );
        auth.register("a sufficiently long password")
            .expect("register");
        assert!(auth.state().registered);
        assert_eq!(auth.state().bootstrap, Bootstrap::Closed);
        assert!(auth.register("another long password").is_err());
        std::fs::remove_dir_all(paths.config_dir).unwrap();
    }

    #[test]
    fn refresh_rotates_and_reuse_is_refused() {
        let paths = scratch("auth-refresh");
        let config = AuthConfig {
            bootstrap: BootstrapMode::Generated,
            ..AuthConfig::default()
        };
        let (auth, password) = AuthService::load(&config, &paths).expect("load");
        let password = password.expect("password");
        let grant = auth.login("admin", &password).expect("login");
        let next = auth.refresh(&grant.refresh_token).expect("refresh");
        assert_ne!(grant.refresh_token, next.refresh_token);
        assert!(auth.refresh(&grant.refresh_token).is_err());
        std::fs::remove_dir_all(paths.config_dir).unwrap();
    }

    #[test]
    fn password_change_ends_sessions() {
        let paths = scratch("auth-password");
        let config = AuthConfig {
            bootstrap: BootstrapMode::Generated,
            ..AuthConfig::default()
        };
        let (auth, password) = AuthService::load(&config, &paths).expect("load");
        let password = password.expect("password");
        let grant = auth.login("admin", &password).expect("login");
        auth.change_password(&password, "a different long password")
            .expect("change");
        assert!(auth.verify_access(&grant.access_token).is_err());
        assert!(auth.login("admin", "a different long password").is_ok());
        std::fs::remove_dir_all(paths.config_dir).unwrap();
    }
}
