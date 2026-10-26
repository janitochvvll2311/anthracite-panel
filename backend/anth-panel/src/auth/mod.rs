//! Who may use the panel: one operator account, two tokens, one bootstrap.
//!
//! # The shapes, and why these ones
//!
//! **One account.** A host panel has one operator, not a user table. A second
//! account would need roles, an owner of each role, and a policy for deleting
//! one — none of which has a consumer yet. The first run creates the account and
//! every later run may rotate its password; there is no path that creates a
//! second one.
//!
//! **Two tokens.** The access token is a short-lived JWT (minutes) that travels
//! on every request. The refresh token is a long-lived opaque string (days) that
//! exists for one job: to mint a new access token without asking for the
//! password again. A JWT cannot be revoked before it expires — that is what
//! "stateless" buys — so the token that *is* revocable is kept opaque, stored
//! hashed, and rotated on every use. Kiling a session is therefore one write,
//! and the window a stolen access token is useful for is its remaining minutes
//! rather than its remaining days.
//!
//! **A refresh token is single-use.** Each refresh deletes the presented token
//! and records its replacement. A token presented twice is a token that leaked —
//! the browser consumed it, so a second presenter is someone else — and the
//! answer is to drop every session that chain belongs to rather than to hand out
//! another access token.
//!
//! # What is written down, and in which form
//!
//! Three secrets, and each is stored in the form its use requires:
//!
//! | Secret | Stored as | Why that form |
//! | --- | --- | --- |
//! | password | Argon2id PHC string | never needed back, only compared |
//! | refresh tokens | SHA-256 digest | never needed back, only compared |
//! | JWT signing key | the key itself, in hex | HMAC needs the value to sign |
//!
//! The refresh tokens are hashed with SHA-256 rather than Argon2 deliberately:
//! they are 256 bits of machine-generated entropy, not something a person chose,
//! so there is no dictionary to run and the slow hash would be paid on every
//! request. The signing key cannot be hashed at all — a hashed key signs
//! nothing — so it is the one secret at rest in the clear, and the file that
//! holds it is created `0600`. That is the honest trade: a `config.toml` readable
//! by another user is a compromised panel, which is why [`store`] refuses to
//! loosen the mode and says so when the file is already too readable.
//!
//! # Why `config.toml`
//!
//! The operator already has that file and a resolvable path to it, so a
//! credential beside it needs no second location to discover, back up or
//! permission. The repository root git-ignores it for exactly this reason.

pub mod guard;
pub mod session;
pub mod store;

use std::fmt;
use std::path::Path;

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use sha2::{Digest, Sha256};

/// How long an access token is good for.
///
/// Minutes, not hours: the token cannot be revoked, so its lifetime is the whole
/// cost of a leaked copy. A caller that needs longer does not get it — it
/// refreshes, which is one round trip and the reason the refresh token exists.
pub const ACCESS_TTL_SECONDS: u64 = 15 * 60;

/// How long a session survives without being used.
///
/// Days, because this is "stay logged in", and the sliding part lives in
/// [`session::refresh`]: every rotation moves the deadline out again.
pub const REFRESH_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

/// The most sessions one operator may hold at once.
///
/// Each login and each rotation adds one, so without a bound a script could grow
/// the file forever. Ten is more than the number of browsers an operator has and
/// few enough that the oldest is plainly the one to drop.
pub const MAX_SESSIONS: usize = 10;

/// The shortest password accepted.
///
/// Length is the only requirement, which is deliberate: composition rules
/// ("one digit, one symbol") measure a password's shape rather than its secret,
/// and this one is stretched by Argon2id anyway. Twelve characters is the
/// shortest length a phrase people actually use clears.
pub const MIN_PASSWORD_CHARS: usize = 12;

/// The longest password accepted, so a hash cannot become a denial of service.
const MAX_PASSWORD_BYTES: usize = 1024;

/// The longest username accepted.
const MAX_USER_CHARS: usize = 64;

/// Argon2id with the parameters OWASP recommends for interactive logins.
///
/// 19 MiB and two passes: a guess costs tens of milliseconds and a login costs
/// one, which is the trade this hash exists to make. `m` is in KiB.
fn argon2() -> Result<Argon2<'static>, AuthError> {
    let params = Params::new(19_456, 2, 1, None)
        .map_err(|e| AuthError::internal(format!("argon2 parameters: {e}")))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Why an authentication step refused.
///
/// Carries the status the route answers with and the exit code the CLI derives,
/// so one value serves both transports without either spelling a number in a
/// handler. The distinction that matters: `Unauthorized` is "prove who you are",
/// `Forbidden` is "you may not", and a caller that treats them alike locks the
/// operator out of the recovery path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// No account exists, so the bootstrap is open.
    NotRegistered,
    /// The bootstrap is closed and a request tried to register.
    AlreadyRegistered,
    /// A credential was wrong, or a token did not verify.
    Unauthorized(String),
    /// A request asked for something this daemon will not do.
    Invalid(String),
    /// The stored configuration could not be used.
    Internal(String),
}

impl AuthError {
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    /// The message a client sees. Never carries a hash, a key or a token.
    pub fn message(&self) -> &str {
        match self {
            Self::NotRegistered => "no account is registered yet; register to continue",
            Self::AlreadyRegistered => "an account is already registered",
            Self::Unauthorized(why) | Self::Invalid(why) | Self::Internal(why) => why,
        }
    }

    /// The HTTP status, derived here and nowhere else.
    ///
    /// `NotRegistered` is a `409` rather than a `401`: such a request is not
    /// unauthorised, it is out of order — nothing can authorise it until someone
    /// registers. A client that saw `401` would show a login form that cannot
    /// succeed, which is the bug this status exists to prevent.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::NotRegistered | Self::AlreadyRegistered => 409,
            Self::Unauthorized(_) => 401,
            Self::Invalid(_) => 400,
            Self::Internal(_) => 500,
        }
    }

    /// The CLI exit code, matching `anth-wire`'s rule: a bad request is `2`,
    /// everything else `1`.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Invalid(_) => 2,
            _ => 1,
        }
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for AuthError {}

/// The account, and the key that signs its tokens.
///
/// Held by the daemon rather than built per request: one process serves one
/// operator, and a second copy of the signing key would be a second way to mint
/// a token that the first would accept.
#[derive(Debug, Clone)]
pub struct Account {
    pub username: String,
    /// The Argon2id hash in PHC form. The password itself is kept nowhere.
    password_hash: String,
    /// The HS256 key. `0600` on disk is its protection; see the module comment.
    signing_key: Vec<u8>,
}

impl Account {
    /// Checks a password against the stored hash.
    ///
    /// The comparison is `argon2`'s, which is constant-time in the digest;
    /// comparing here would leak how many leading bytes matched. A malformed
    /// stored hash is an internal error rather than a wrong password, because
    /// "the file is corrupt" and "you typed it wrong" want different answers.
    pub fn verify_password(&self, password: &str) -> Result<(), AuthError> {
        let parsed = PasswordHash::new(&self.password_hash)
            .map_err(|e| AuthError::internal(format!("the stored password hash: {e}")))?;
        argon2()?
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| AuthError::Unauthorized("incorrect username or password".to_string()))
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn signing_key(&self) -> &[u8] {
        &self.signing_key
    }

    /// Rebuilds an account read back from the settings file.
    pub(crate) fn from_parts(
        username: String,
        password_hash: String,
        signing_key: Vec<u8>,
    ) -> Self {
        Self {
            username,
            password_hash,
            signing_key,
        }
    }

    pub(crate) fn password_hash(&self) -> &str {
        &self.password_hash
    }
}

/// Registers the one account, which is only possible while none exists.
///
/// The check and the write are one step: [`store::write_account`] refuses a file
/// that already holds an account, so two concurrent registrations cannot both
/// succeed and the loser is told why rather than silently replacing a password.
pub fn register(config: &Path, username: &str, password: &str) -> Result<Account, AuthError> {
    let username = validate_username(username)?;
    validate_password(password)?;

    let account = Account {
        username,
        password_hash: hash_password(password)?,
        signing_key: random_bytes(32)?,
    };

    store::write_account(config, &account)?;
    Ok(account)
}

/// Checks a password against the account on disk, for a login.
pub fn login(config: &Path, username: &str, password: &str) -> Result<Account, AuthError> {
    let account = load(config)?;
    if account.username != username.trim() {
        // The password is hashed even when the name is wrong: answering "no such
        // user" in a millisecond would tell an attacker which half to keep
        // guessing, and one message for both is the only answer that does not.
        let _ = hash_password(password);
        return Err(AuthError::Unauthorized(
            "incorrect username or password".to_string(),
        ));
    }
    account.verify_password(password)?;
    Ok(account)
}

/// Reads the account, or says that there is none to read.
///
/// A registered-but-unreadable account is `Internal` rather than
/// `NotRegistered`: the difference is "open the bootstrap" versus "fix the
/// file", and answering the first when the second is true would let anyone
/// re-register over a damaged credential.
pub fn load(config: &Path) -> Result<Account, AuthError> {
    match store::read(config)? {
        Some(account) => Ok(account),
        None => Err(AuthError::NotRegistered),
    }
}

/// Replaces the password of the existing account.
///
/// Requires the current password. A refresh token is not enough: it is a
/// long-lived credential sitting in a browser, and a stolen one must not be able
/// to lock the operator out of their own panel.
pub fn change_password(
    config: &Path,
    username: &str,
    current: &str,
    next: &str,
) -> Result<Account, AuthError> {
    validate_password(next)?;
    if current == next {
        return Err(AuthError::invalid("the new password is the current one"));
    }
    let account = login(config, username, current)?;

    // A new key, not the old one. The key signs access tokens, so keeping it
    // would leave every token minted before the change verifiable until it
    // expired — and the usual reason to change a password is that someone else
    // may have had it.
    let rotated = Account {
        username: account.username,
        password_hash: hash_password(next)?,
        signing_key: random_bytes(32)?,
    };
    store::write_replace(config, &rotated)?;
    // Every session was granted under the old password, and the reason to change
    // one is often that it leaked. Dropping them is the whole point.
    session::clear(config)?;
    Ok(rotated)
}

/// Rejects a username the panel will not store, returning the trimmed value.
///
/// Trimmed first, so a name that is only spaces is empty rather than valid. The
/// character rule is "no control characters": an account name reaches a log line
/// and, later, a UI, and a newline there is a forged log entry.
fn validate_username(username: &str) -> Result<String, AuthError> {
    let name = username.trim();
    if name.is_empty() {
        return Err(AuthError::invalid("the username cannot be empty"));
    }
    if name.chars().count() > MAX_USER_CHARS {
        return Err(AuthError::invalid(format!(
            "the username is longer than {MAX_USER_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(AuthError::invalid(
            "the username cannot contain control characters",
        ));
    }
    Ok(name.to_string())
}

/// Rejects a password the panel will not hash.
///
/// Counted in characters for the minimum (what a person perceives) and in bytes
/// for the maximum (what Argon2 must allocate), and the two are documented
/// together so neither bound looks arbitrary.
fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(AuthError::invalid(format!(
            "the password must be at least {MIN_PASSWORD_CHARS} characters"
        )));
    }
    if password.len() > MAX_PASSWORD_BYTES {
        return Err(AuthError::invalid(format!(
            "the password is longer than {MAX_PASSWORD_BYTES} bytes"
        )));
    }
    Ok(())
}

/// An Argon2id hash of `password`, in PHC string form.
fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    argon2()?
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AuthError::internal(format!("hashing the password: {e}")))
}

/// `bytes` of entropy from the operating system.
///
/// A failure here must stop the operation: a signing key of zeros, or a refresh
/// token two clients could guess, is worse than a refused login.
pub(crate) fn random_bytes(count: usize) -> Result<Vec<u8>, AuthError> {
    let mut buffer = vec![0_u8; count];
    getrandom::fill(&mut buffer)
        .map_err(|e| AuthError::internal(format!("the system entropy source: {e}")))?;
    Ok(buffer)
}

/// A token that is stored but never served: 32 bytes, base64url, no padding.
pub(crate) fn random_token() -> Result<String, AuthError> {
    Ok(base64_url(&random_bytes(32)?))
}

/// The digest stored in place of a refresh token.
///
/// SHA-256, and the reasoning is in the module comment: the input is 256 bits of
/// machine entropy, so there is no dictionary to run and no reason to pay
/// Argon2's cost on a path every request takes.
pub(crate) fn token_digest(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex(&hasher.finalize())
}

/// Lowercase hex, for the digests and ids in the settings file.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// The inverse of [`hex`], for reading a stored key back.
pub(crate) fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok())
        .collect()
}

/// base64url without padding, which is what a JWT segment and a URL token use.
pub(crate) fn base64_url(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests;
