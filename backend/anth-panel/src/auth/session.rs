//! Sessions: the refresh tokens that a login hands out, and the access tokens
//! they mint.
//!
//! # Why a session is a record and not just a token
//!
//! A refresh token that is only a signature cannot be revoked, and revoking is
//! the one thing "log out everywhere" needs. So each token is an entry in the
//! settings file, stored as its digest, and revoking is deleting the entry.
//!
//! # Rotation, and what a reused token means
//!
//! Every refresh consumes the presented token and issues a new one in the same
//! *chain*. A token presented a second time is therefore not a retry — the
//! browser already replaced it — it is a copy that somebody else holds. The
//! answer is to drop the whole chain: whoever holds the stale copy and whoever
//! holds the fresh one are two parties, and continuing to serve either would
//! keep serving the thief half the time.
//!
//! The consumed entries are kept until they expire so that the reuse can be
//! recognised at all. That is the one place this module stores something it does
//! not need to serve a request, and the prune in [`Sessions::refresh`] is what
//! keeps it bounded.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{Account, AuthError, REFRESH_TTL_SECONDS, store, token_digest};

/// What a login or a refresh hands back.
///
/// `expires_in` rather than an absolute time because a browser clock and the
/// daemon's clock are different clocks, and a client that stored an absolute
/// instant would refresh at the wrong moment whenever the two disagree.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: String,
}

/// The claims an access token carries.
///
/// Deliberately thin: `sub` is the username and nothing else is needed, because
/// there is one account. A role claim would be a promise that a second account
/// could hold a different one, and nothing reads it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iss: String,
    pub iat: u64,
    pub exp: u64,
}

/// Who issued the token, checked on every verify.
pub const ISSUER: &str = "anth-panel";

/// Clock skew allowed when verifying, in seconds.
///
/// Small and positive only: a token is accepted a few seconds before `nbf` and
/// after `exp`, which is what two machines with slightly different clocks need.
/// A large value here would silently extend every token's life.
const LEEWAY_SECONDS: u64 = 5;

/// A change to the session list, and the answer it produces.
///
/// The change says **whether the file must be written**, because one case must
/// not be: a detected reuse drops the chain *and* fails, and returning through
/// `?` before the write would drop the chain in memory only. The next request
/// would re-read the file, find the session still alive, and serve the thief.
enum Edit<T> {
    /// The change succeeded; write it back.
    Write(T),
    /// The change failed, but the list is now something that must be written:
    /// a reuse is the case, and a session that is still on disk after one is a
    /// session the thief keeps.
    WriteThen(AuthError),
    /// The change failed and the list is untouched.
    Fail(AuthError),
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One live or recently consumed refresh token.
#[derive(Debug, Clone)]
struct Record {
    /// SHA-256 of the token, hex. The token itself is never stored.
    digest: String,
    /// Which login this token descends from, so a reuse can drop its siblings.
    chain: String,
    /// Unix seconds after which the token is dead whether used or not.
    expires: u64,
    /// Whether a refresh already consumed it.
    used: bool,
}

/// A login, once the password checked out.
pub fn start(config: &Path, account: &Account) -> Result<Grant, AuthError> {
    start_at(config, account, now())
}

/// [`start`] with the clock passed in, so a test can place a session in the past.
pub fn start_at(config: &Path, account: &Account, at: u64) -> Result<Grant, AuthError> {
    let token = super::random_token()?;
    let chain = super::random_token()?;
    let record = Record {
        digest: token_digest(&token),
        chain,
        expires: at + REFRESH_TTL_SECONDS,
        used: false,
    };

    edit_sessions(config, |sessions| {
        sessions.push(record);
        trim(sessions, at);
        Edit::Write(())
    })?;

    Ok(Grant {
        access_token: issue_access(account, at)?,
        token_type: "Bearer".to_string(),
        expires_in: super::ACCESS_TTL_SECONDS,
        refresh_token: token,
    })
}

/// Exchanges a refresh token for a new pair, rotating the token.
pub fn refresh(config: &Path, presented: &str) -> Result<Grant, AuthError> {
    refresh_at(config, presented, now())
}

/// [`refresh`] with the clock passed in.
///
/// The three answers are distinct on purpose. An unknown token is a `401` with
/// one message; a reused one is a `401` *and* costs every session in its chain;
/// an expired one is a `401` and costs only itself. Collapsing them would make
/// the reuse detection invisible to the operator, who is the one who needs to
/// know that a token escaped.
pub fn refresh_at(config: &Path, presented: &str, at: u64) -> Result<Grant, AuthError> {
    let digest = token_digest(presented);
    let account = super::load(config)?;

    let edit = edit_sessions_returning(config, |sessions| {
        let found = match sessions.iter().position(|record| record.digest == digest) {
            Some(index) => index,
            None => {
                return Edit::Fail(AuthError::Unauthorized(
                    "the refresh token is not recognised".to_string(),
                ));
            }
        };

        if sessions[found].used {
            let chain = sessions[found].chain.clone();
            sessions.retain(|record| record.chain != chain);
            return Edit::WriteThen(AuthError::Unauthorized(
                "this refresh token was already used, so every session in its chain was dropped"
                    .to_string(),
            ));
        }

        if sessions[found].expires <= at {
            sessions.remove(found);
            return Edit::WriteThen(AuthError::Unauthorized(
                "the session expired; log in again".to_string(),
            ));
        }

        let chain = sessions[found].chain.clone();
        sessions[found].used = true;
        let token = match super::random_token() {
            Ok(token) => token,
            Err(error) => return Edit::Fail(error),
        };
        sessions.push(Record {
            digest: token_digest(&token),
            chain,
            expires: at + REFRESH_TTL_SECONDS,
            used: false,
        });
        trim(sessions, at);
        Edit::Write(token)
    })?;

    match edit {
        Edit::Write(token) => Ok(Grant {
            access_token: issue_access(&account, at)?,
            token_type: "Bearer".to_string(),
            expires_in: super::ACCESS_TTL_SECONDS,
            refresh_token: token,
        }),
        Edit::WriteThen(error) | Edit::Fail(error) => Err(error),
    }
}

/// Drops every session, for `logout`.
///
/// Returns how many were dropped, so a caller can say "you were logged in
/// elsewhere too" instead of silently ending more sessions than were asked for.
pub fn revoke_all(config: &Path) -> Result<usize, AuthError> {
    edit_sessions_returning(config, |sessions| {
        let count = sessions.len();
        sessions.clear();
        Edit::Write(count)
    })
    .map(|edit| match edit {
        Edit::Write(count) => count,
        // `revoke_all` never reports a reuse, so these cannot arise; `0` is the
        // honest answer for "nothing was written".
        Edit::WriteThen(_) | Edit::Fail(_) => 0,
    })
}

/// Mints an access token for `account`, valid from `at`.
pub fn issue_access(account: &Account, at: u64) -> Result<String, AuthError> {
    let claims = Claims {
        sub: account.username().to_string(),
        iss: ISSUER.to_string(),
        iat: at,
        exp: at + super::ACCESS_TTL_SECONDS,
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(account.signing_key()),
    )
    .map_err(|e| AuthError::internal(format!("signing the access token: {e}")))
}

/// Checks a bearer token and returns what it claims.
///
/// The algorithm is pinned to HS256 and the issuer is required: `jsonwebtoken`
/// otherwise trusts the `alg` in the header, and `alg: none` is the classic way
/// to get a token accepted that nobody signed.
pub fn verify_access(account: &Account, token: &str) -> Result<Claims, AuthError> {
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::HS256);
    validation.leeway = LEEWAY_SECONDS;
    validation.set_issuer(&[ISSUER]);
    validation.set_required_spec_claims(&["exp", "sub", "iss"]);

    let decoded = jsonwebtoken::decode::<Claims>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(account.signing_key()),
        &validation,
    )
    .map_err(|error| match error.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
            AuthError::Unauthorized("the access token expired; refresh it".to_string())
        }
        _ => AuthError::Unauthorized("the access token is not valid".to_string()),
    })?;

    if decoded.claims.sub != account.username() {
        return Err(AuthError::Unauthorized(
            "the access token names another account".to_string(),
        ));
    }
    Ok(decoded.claims)
}

/// Reads the sessions out of the settings file.
///
/// A malformed entry is skipped rather than fatal, and skipped *individually*:
/// one corrupt record must not log the operator out of the sessions that are
/// still sound, and it must not stop the daemon from starting.
fn read_sessions(config: &Path) -> Result<Vec<Record>, AuthError> {
    let doc = store::document(config)?;
    let Some(array) = doc
        .get("auth")
        .and_then(|item| item.as_table_like())
        .and_then(|auth| auth.get("sessions"))
        .and_then(|item| item.as_array_of_tables())
    else {
        return Ok(Vec::new());
    };

    Ok(array
        .iter()
        .filter_map(|table| {
            Some(Record {
                digest: table.get("id")?.as_str()?.to_string(),
                chain: table.get("chain")?.as_str()?.to_string(),
                expires: table.get("expires")?.as_integer()? as u64,
                used: table.get("used").and_then(|v| v.as_bool()).unwrap_or(false),
            })
        })
        .collect())
}

/// Applies a change that produces nothing, and writes the list back.
fn edit_sessions<F>(config: &Path, change: F) -> Result<(), AuthError>
where
    F: FnOnce(&mut Vec<Record>) -> Edit<()>,
{
    edit_sessions_returning(config, change).and_then(|edit| match edit {
        Edit::Write(()) => Ok(()),
        Edit::WriteThen(error) | Edit::Fail(error) => Err(error),
    })
}

/// Applies `change`, and writes the list **if the change asked for it**.
///
/// The write happens after the change, not on success only: a change that
/// mutates and then fails — dropping a chain after a reuse — has to reach the
/// disk, or the daemon and the file disagree about whether that session is alive.
fn edit_sessions_returning<T, F>(config: &Path, change: F) -> Result<Edit<T>, AuthError>
where
    F: FnOnce(&mut Vec<Record>) -> Edit<T>,
{
    let mut sessions = read_sessions(config)?;
    let edit = change(&mut sessions);

    let value = match edit {
        Edit::Fail(error) => return Ok(Edit::Fail(error)),
        Edit::Write(value) => value,
        Edit::WriteThen(error) => {
            persist(config, &sessions)?;
            return Ok(Edit::WriteThen(error));
        }
    };

    persist(config, &sessions)?;
    Ok(Edit::Write(value))
}

/// Writes the session list into the settings file, leaving the rest alone.
fn persist(config: &Path, sessions: &[Record]) -> Result<(), AuthError> {
    store::edit(config, |doc| {
        let auth = doc
            .entry("auth")
            .or_insert(toml_edit::table())
            .as_table_mut()
            .expect("the `auth` key is created as a table");

        let mut array = toml_edit::ArrayOfTables::new();
        for record in sessions {
            let mut table = toml_edit::Table::new();
            table["id"] = toml_edit::value(record.digest.as_str());
            table["chain"] = toml_edit::value(record.chain.as_str());
            table["expires"] = toml_edit::value(record.expires as i64);
            if record.used {
                table["used"] = toml_edit::value(true);
            }
            array.push(table);
        }
        auth.insert("sessions", toml_edit::Item::ArrayOfTables(array));
        Ok(())
    })
}

/// Drops what no longer serves a purpose, and caps the list.
///
/// Two rules, in this order: a consumed record whose expiry has passed can no
/// longer prove a reuse, and a live one keeps the newest [`super::MAX_SESSIONS`].
/// Dropping the oldest live session is what "you are logged in on ten devices"
/// turns into: the eleventh login evicts the first instead of growing the file.
fn trim(sessions: &mut Vec<Record>, at: u64) {
    sessions.retain(|record| !record.used || record.expires > at);

    let live = sessions.iter().filter(|record| !record.used).count();
    if live > super::MAX_SESSIONS {
        let mut to_evict = live - super::MAX_SESSIONS;
        let mut at_index = 0;
        while to_evict > 0 && at_index < sessions.len() {
            if !sessions[at_index].used {
                sessions.remove(at_index);
                to_evict -= 1;
            } else {
                at_index += 1;
            }
        }
    }
}

/// Drops every session, called when the password changes.
pub fn clear(config: &Path) -> Result<(), AuthError> {
    revoke_all(config).map(|_| ())
}
