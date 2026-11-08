//! The operator's credentials and the sessions they open.
//!
//! The socket authenticates by peer credential; TCP has no peer, so it carries
//! a bearer token minted by [`Auth::login`]. One operator exists in v0.5.0, and
//! the password is generated on first boot into `state/credentials.txt` with
//! mode `0600` (§6.5).

use std::collections::{HashMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anth_contract::Error;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// The file the state tree owns, relative to its root.
pub const CREDENTIALS: &str = "credentials.txt";

/// The name the first-boot operator gets. A stable name keeps the panel's Basic
/// stopgap, the audit actor and the CLI aligned.
pub const OPERATOR: &str = "operator";

const FILE_MODE: u32 = 0o600;
const MIN_PASSWORD: usize = 12;
const LOGIN_LIMIT: usize = 5;
const LOGIN_WINDOW: Duration = Duration::from_secs(60);
const LOGIN_BLOCK: Duration = Duration::from_secs(60);
/// The limiter is an attacker-controlled map; bound it so a scan cannot grow it
/// without limit while still keeping the recent window for every source.
const MAX_SOURCES: usize = 4096;

/// The on-disk document. It is the operator's recovery copy, so the password is
/// stored in clear text on purpose (§6.5); the file mode is the boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    operator: String,
    password: String,
    must_change_password: bool,
}

/// What a login or an authenticated request learns about the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionInfo {
    pub operator: String,
    pub must_change_password: bool,
    /// Unix seconds. The engine is the clock that matters.
    pub expires_at: u64,
}

/// A successful login: the bearer token and what it represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Login {
    pub token: String,
    pub session: SessionInfo,
}

struct Session {
    info: SessionInfo,
    expires_at: SystemTime,
}

#[derive(Default)]
struct Attempts {
    failures: VecDeque<Instant>,
    blocked_until: Option<Instant>,
}

struct Inner {
    document: Document,
    sessions: HashMap<String, Session>,
    attempts: HashMap<String, Attempts>,
}

/// Credentials plus the live sessions. Both the daemon and a linked panel need
/// the same value, so it lives behind an `Arc` in the router state.
pub struct Auth {
    root: PathBuf,
    ttl: Duration,
    inner: Mutex<Inner>,
}

impl Auth {
    /// Reads the credentials, generating them on first boot. The file is created
    /// atomically with `0600`, so a concurrent boot sees either nothing or the
    /// complete document.
    pub fn load_or_create(root: &Path, ttl: Duration) -> Result<Self, Error> {
        let path = root.join(CREDENTIALS);
        let document = match read_document(&path)? {
            Some(document) => document,
            None => {
                let generated = Document {
                    operator: OPERATOR.to_string(),
                    password: random_hex(16)?,
                    must_change_password: true,
                };
                if create(&path, &generated)? {
                    generated
                } else {
                    read_document(&path)?.ok_or_else(|| {
                        Error::internal("the credentials vanished during the first boot")
                    })?
                }
            }
        };
        Ok(Self {
            root: root.to_path_buf(),
            ttl,
            inner: Mutex::new(Inner {
                document,
                sessions: HashMap::new(),
                attempts: HashMap::new(),
            }),
        })
    }

    /// Where the operator reads the generated password.
    pub fn credentials_path(&self) -> PathBuf {
        self.root.join(CREDENTIALS)
    }

    pub fn operator(&self) -> String {
        self.inner
            .lock()
            .expect("credentials")
            .document
            .operator
            .clone()
    }

    pub fn must_change_password(&self) -> bool {
        self.inner
            .lock()
            .expect("credentials")
            .document
            .must_change_password
    }

    pub fn ttl_seconds(&self) -> u64 {
        self.ttl.as_secs()
    }

    /// Checks the operator and password and mints a token. The error is the same
    /// for both failures so a caller cannot enumerate operators. Too many
    /// failures from one source are refused before the password is checked.
    pub fn login(&self, source: &str, operator: &str, password: &str) -> Result<Login, Error> {
        let mut inner = self.inner.lock().expect("credentials");
        if let Some(retry_after) = inner.blocked(source) {
            return Err(Error::rate_limited("too many login attempts; retry later")
                .with_detail(json!({ "retry_after": retry_after })));
        }
        if !constant_time_eq(&inner.document.operator, operator)
            || !constant_time_eq(&inner.document.password, password)
        {
            inner.failed(source);
            return Err(Error::unauthenticated("invalid operator or password"));
        }
        inner.succeeded(source);
        issue(&mut inner, self.ttl)
    }

    /// Mints a session without re-checking the password. Used right after a
    /// successful password change revoked every token.
    pub fn issue(&self) -> Result<Login, Error> {
        let mut inner = self.inner.lock().expect("credentials");
        issue(&mut inner, self.ttl)
    }

    /// Resolves a bearer token, dropping it if it has expired.
    pub fn authenticate(&self, token: &str) -> Option<SessionInfo> {
        let mut inner = self.inner.lock().expect("credentials");
        let now = SystemTime::now();
        inner.sessions.retain(|_, session| session.expires_at > now);
        inner
            .sessions
            .get(token)
            .map(|session| session.info.clone())
    }

    pub fn logout(&self, token: &str) -> bool {
        self.inner
            .lock()
            .expect("credentials")
            .sessions
            .remove(token)
            .is_some()
    }

    /// Changes the password. A session must prove the old password; the socket's
    /// peer credential is proof enough, so `trusted` skips that check.
    ///
    /// Every live session is revoked: a password change is the moment to cut
    /// existing tokens. The caller mints a fresh session when it needs one.
    pub fn change_password(
        &self,
        current: Option<&str>,
        new: &str,
        trusted: bool,
    ) -> Result<(), Error> {
        if new.chars().count() < MIN_PASSWORD {
            return Err(Error::invalid(format!(
                "the new password must be at least {MIN_PASSWORD} characters"
            )));
        }
        let mut inner = self.inner.lock().expect("credentials");
        if !trusted {
            let current = current
                .ok_or_else(|| Error::unauthenticated("the current password is required"))?;
            if !constant_time_eq(&inner.document.password, current) {
                return Err(Error::unauthenticated("the current password is wrong"));
            }
        }
        if constant_time_eq(&inner.document.password, new) {
            return Err(Error::invalid(
                "the new password must differ from the current one",
            ));
        }
        let mut next = inner.document.clone();
        next.password = new.to_string();
        next.must_change_password = false;
        write(&self.credentials_path(), &next)?;
        inner.document = next;
        inner.sessions.clear();
        Ok(())
    }
}

impl Inner {
    /// Drops expired sources and evicts one arbitrary entry when the map is at
    /// its cap. The cap is a memory bound, not a security boundary: the window
    /// still refuses repeated failures from any source that stays in the map.
    fn touch(&mut self, source: &str) -> &mut Attempts {
        let now = Instant::now();
        self.attempts.retain(|_, attempts| {
            let recent = attempts
                .failures
                .iter()
                .any(|at| now.duration_since(*at) < LOGIN_WINDOW);
            let blocked = attempts.blocked_until.is_some_and(|until| until > now);
            recent || blocked
        });
        if self.attempts.len() >= MAX_SOURCES && !self.attempts.contains_key(source) {
            if let Some(key) = self.attempts.keys().next().cloned() {
                self.attempts.remove(&key);
            }
        }
        self.attempts.entry(source.to_string()).or_default()
    }

    fn blocked(&mut self, source: &str) -> Option<u64> {
        let now = Instant::now();
        let attempts = self.touch(source);
        if let Some(until) = attempts.blocked_until {
            if until > now {
                return Some((until - now).as_secs().max(1));
            }
            attempts.blocked_until = None;
        }
        None
    }

    fn failed(&mut self, source: &str) {
        let now = Instant::now();
        let attempts = self.touch(source);
        attempts.failures.push_back(now);
        if attempts.failures.len() >= LOGIN_LIMIT {
            attempts.failures.clear();
            attempts.blocked_until = Some(now + LOGIN_BLOCK);
        }
    }

    fn succeeded(&mut self, source: &str) {
        self.attempts.remove(source);
    }
}

fn issue(inner: &mut Inner, ttl: Duration) -> Result<Login, Error> {
    let now = SystemTime::now();
    inner.sessions.retain(|_, session| session.expires_at > now);
    let expires_at = now + ttl;
    let token = loop {
        let candidate = random_hex(32)?;
        if !inner.sessions.contains_key(&candidate) {
            break candidate;
        }
    };
    let info = SessionInfo {
        operator: inner.document.operator.clone(),
        must_change_password: inner.document.must_change_password,
        expires_at: unix(&expires_at),
    };
    inner.sessions.insert(
        token.clone(),
        Session {
            info: info.clone(),
            expires_at,
        },
    );
    Ok(Login {
        token,
        session: info,
    })
}

/// Reads and validates the recovery file. A symlink or a mode looser than `0600`
/// is refused: the file holds the password in clear text.
fn read_document(path: &Path) -> Result<Option<Document>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::internal(format!(
                "inspecting {}: {error}",
                path.display()
            )));
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Error::invalid(format!(
            "{} is not a real credentials file",
            path.display()
        )));
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != FILE_MODE {
        return Err(Error::invalid(format!(
            "{} is {mode:04o}; it should be {FILE_MODE:04o}",
            path.display()
        )));
    }
    let bytes = fs::read(path)
        .map_err(|error| Error::internal(format!("reading {}: {error}", path.display())))?;
    parse(path, &bytes).map(Some)
}

fn parse(path: &Path, bytes: &[u8]) -> Result<Document, Error> {
    let document: Document = serde_json::from_slice(bytes)
        .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
    if document.operator.is_empty() || document.password.is_empty() {
        return Err(Error::invalid(format!(
            "{} has an empty operator or password",
            path.display()
        )));
    }
    Ok(document)
}

/// Creates the file atomically, returning `false` if another process won the
/// race. A hard link is the no-replace primitive: readers never see a half-written
/// document and the loser cannot overwrite the winner.
fn create(path: &Path, document: &Document) -> Result<bool, Error> {
    let bytes = serde_json::to_vec(document)
        .map_err(|error| Error::internal(format!("encoding credentials: {error}")))?;
    let temporary = path.with_extension(format!("new-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);
    write_file(&temporary, &bytes)?;
    let linked = match fs::hard_link(&temporary, path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(Error::internal(format!(
            "creating {}: {error}",
            path.display()
        ))),
    };
    let _ = fs::remove_file(&temporary);
    linked
}

/// Replaces an existing document atomically: a reader never sees half a file.
fn write(path: &Path, document: &Document) -> Result<(), Error> {
    let bytes = serde_json::to_vec(document)
        .map_err(|error| Error::internal(format!("encoding credentials: {error}")))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let _ = fs::remove_file(&temporary);
    write_file(&temporary, &bytes)?;
    fs::rename(&temporary, path).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        Error::internal(format!("replacing {}: {error}", path.display()))
    })
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(path)
        .map_err(|error| Error::internal(format!("creating {}: {error}", path.display())))?;
    file.write_all(bytes)
        .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))
}

fn unix(time: &SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn random_hex(bytes: usize) -> Result<String, Error> {
    let mut buffer = vec![0u8; bytes];
    fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut buffer))
        .map_err(|error| Error::internal(format!("reading /dev/urandom: {error}")))?;
    let mut value = String::with_capacity(bytes * 2);
    for byte in buffer {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    Ok(value)
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = u8::from(left.len() != right.len());
    for index in 0..left.len().max(right.len()) {
        difference |=
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0);
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("anth-auth-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn document(auth: &Auth) -> Document {
        serde_json::from_slice(&fs::read(auth.credentials_path()).expect("read")).expect("parse")
    }

    #[test]
    fn first_boot_generates_a_private_recovery_file() {
        let root = scratch("first");
        let auth = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        let path = auth.credentials_path();
        let mode = fs::metadata(&path).expect("metadata").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let document = document(&auth);
        assert_eq!(document.operator, OPERATOR);
        assert!(document.must_change_password);
        assert!(document.password.len() >= 32);
        assert!(auth.must_change_password());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_second_boot_reuses_the_same_credentials() {
        let root = scratch("reuse");
        let first = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        let second = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        assert_eq!(document(&first).password, document(&second).password);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_login_mints_and_resolves_a_token() {
        let root = scratch("login");
        let auth = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        let document = document(&auth);
        let login = auth
            .login("test", &document.operator, &document.password)
            .expect("login");
        assert!(login.session.must_change_password);
        assert_eq!(auth.authenticate(&login.token), Some(login.session.clone()));
        assert!(auth.logout(&login.token));
        assert_eq!(auth.authenticate(&login.token), None);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_wrong_password_is_indistinguishable_from_a_wrong_operator() {
        let root = scratch("wrong");
        let auth = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        assert_eq!(
            auth.login("a", "nobody", "whatever")
                .expect_err("operator")
                .kind(),
            anth_contract::ErrorKind::Unauthenticated
        );
        assert_eq!(
            auth.login("b", OPERATOR, "whatever")
                .expect_err("password")
                .kind(),
            anth_contract::ErrorKind::Unauthenticated
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn repeated_failures_block_the_source_even_for_the_right_password() {
        let root = scratch("limit");
        let auth = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        let password = document(&auth).password;
        for _ in 0..5 {
            assert_eq!(
                auth.login("same-source", OPERATOR, "wrong")
                    .expect_err("wrong")
                    .kind(),
                anth_contract::ErrorKind::Unauthenticated
            );
        }
        assert_eq!(
            auth.login("same-source", OPERATOR, &password)
                .expect_err("limited")
                .kind(),
            anth_contract::ErrorKind::RateLimited
        );
        assert!(
            auth.login("other-source", OPERATOR, &password).is_ok(),
            "a different source is not punished"
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_session_expires() {
        let root = scratch("expiry");
        let auth = Auth::load_or_create(&root, Duration::from_millis(1)).expect("auth");
        let document = document(&auth);
        let login = auth
            .login("test", &document.operator, &document.password)
            .expect("login");
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(auth.authenticate(&login.token), None);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn changing_the_password_lifts_the_gate_revokes_sessions_and_persists() {
        let root = scratch("change");
        let auth = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        let current = document(&auth);
        let login = auth
            .login("test", &current.operator, &current.password)
            .expect("login");
        auth.change_password(Some(&current.password), "a-new-password", false)
            .expect("change");
        let reloaded = Auth::load_or_create(&root, Duration::from_secs(60)).expect("reload");
        let changed = document(&reloaded);
        assert_eq!(changed.password, "a-new-password");
        assert!(!changed.must_change_password);
        assert_eq!(auth.authenticate(&login.token), None);
        assert_eq!(
            reloaded
                .login("test", OPERATOR, &current.password)
                .expect_err("old")
                .kind(),
            anth_contract::ErrorKind::Unauthenticated
        );
        assert!(reloaded.login("test", OPERATOR, "a-new-password").is_ok());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn the_socket_can_change_the_password_without_the_old_one() {
        let root = scratch("trusted");
        let auth = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        auth.change_password(None, "a-new-password", true)
            .expect("change");
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn the_limiter_map_is_bounded() {
        let root = scratch("bounded");
        let auth = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        for index in 0..(MAX_SOURCES + 10) {
            let _ = auth.login(&format!("source-{index}"), OPERATOR, "wrong");
        }
        let inner = auth.inner.lock().expect("credentials");
        assert!(inner.attempts.len() <= MAX_SOURCES);
        drop(inner);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_loose_or_symlinked_credentials_file_is_refused() {
        let root = scratch("mode");
        let auth = Auth::load_or_create(&root, Duration::from_secs(60)).expect("auth");
        let path = auth.credentials_path();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");
        let error = Auth::load_or_create(&root, Duration::from_secs(60))
            .err()
            .expect("loose");
        assert!(error.message().contains("0600"), "{error}");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
        let outside = root.join("outside");
        fs::rename(&path, &outside).expect("rename");
        std::os::unix::fs::symlink(&outside, &path).expect("symlink");
        let error = Auth::load_or_create(&root, Duration::from_secs(60))
            .err()
            .expect("symlink");
        assert!(error.message().contains("not a real"), "{error}");
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
