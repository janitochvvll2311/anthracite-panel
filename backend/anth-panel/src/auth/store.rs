//! Reading and writing the `[auth]` table of `config.toml`.
//!
//! One file holds three things the operator must not lose — the account, the
//! signing key and the live sessions — and it is the same file that holds
//! `socket`, `plugins` and `[terminal]`. So every write here edits the document
//! in place rather than rewriting the file: a `register` must not drop a setting
//! the operator wrote by hand, and a `disable` must not drop the account.
//!
//! `toml_edit` rather than `toml` for that reason. `toml::Value` round-trips
//! through a `BTreeMap` and loses comments and key order, which would turn an
//! edited settings file into a rewritten one. `toml_edit` keeps the document
//! byte-for-byte outside the keys this module touches.
//!
//! # The mode
//!
//! The file is created `0600`. It cannot be: the signing key has to be readable
//! by the daemon to sign, so it is stored in the clear and the file's mode is
//! what protects it. A file already owned by someone else, or world-readable,
//! is reported rather than silently accepted — the daemon logs it and refuses to
//! treat the credential as sound. Loosening is never done for the operator: this
//! module only ever tightens.

use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use super::{Account, AuthError, unhex};

/// The table this module owns.
const TABLE: &str = "auth";

/// The permissions a settings file holding a signing key must have.
const MODE: u32 = 0o600;

/// Reads the account, if the file holds one.
///
/// A missing file is `None`: a fresh checkout has no account, and that is the
/// state that opens the bootstrap. A file that exists but will not parse is an
/// error, because it was written on purpose and guessing at its contents would
/// be guessing at a credential.
pub fn read(config: &Path) -> Result<Option<Account>, AuthError> {
    let doc = document(config)?;
    let Some(auth) = doc.get(TABLE).and_then(|item| item.as_table_like()) else {
        return Ok(None);
    };
    let Some(username) = auth.get("username").and_then(|v| v.as_str()) else {
        return Ok(None);
    };
    let password_hash = auth
        .get("password_hash")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AuthError::internal("`auth.username` is set but `password_hash` is not"))?;
    let key_hex = auth
        .get("signing_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AuthError::internal("`auth.username` is set but `signing_key` is not"))?;
    let signing_key = unhex(key_hex)
        .ok_or_else(|| AuthError::internal("`auth.signing_key` is not hexadecimal"))?;

    Ok(Some(Account::from_parts(
        username.to_string(),
        password_hash.to_string(),
        signing_key,
    )))
}

/// Writes the account, refusing when one is already present.
///
/// The refusal is the bootstrap's whole security: the first request to arrive
/// may register, and every request after it finds the door shut. Checking here
/// rather than at the caller keeps the check and the write in one critical
/// section, so two concurrent first requests cannot both see an empty file.
pub fn write_account(config: &Path, account: &Account) -> Result<(), AuthError> {
    edit(config, |doc| {
        if holds_account(doc) {
            return Err(AuthError::AlreadyRegistered);
        }
        set_account(doc, account);
        Ok(())
    })
}

/// Overwrites the account, for a password change.
///
/// Kept separate from [`write_account`] so the two paths cannot be confused: a
/// caller that wants to *create* cannot reach this one, and this one is only
/// reachable after [`super::login`] has proved the old password.
pub fn write_replace(config: &Path, account: &Account) -> Result<(), AuthError> {
    edit(config, |doc| {
        set_account(doc, account);
        Ok(())
    })
}

/// Whether the parsed document holds an account.
fn holds_account(doc: &toml_edit::DocumentMut) -> bool {
    doc.get(TABLE)
        .and_then(|item| item.as_table_like())
        .and_then(|auth| auth.get("username"))
        .is_some()
}

/// Sets the three account keys, leaving sessions alone.
fn set_account(doc: &mut toml_edit::DocumentMut, account: &Account) {
    let auth = doc
        .entry(TABLE)
        .or_insert(toml_edit::table())
        .as_table_mut()
        .expect("the `auth` key is created as a table");

    auth["username"] = toml_edit::value(account.username());
    auth["password_hash"] = toml_edit::value(account.password_hash());
    auth["signing_key"] = toml_edit::value(super::hex(account.signing_key()));
}

/// Applies `change` to the document and writes it back atomically.
///
/// Shared with [`super::session`] so the two never write the file at once: each
/// call parses, changes and renames, and the rename is what a concurrent reader
/// sees instead of a half-written file. That is not a lock — two writers racing
/// would still lose one edit — but the two writers here are the daemon's own
/// requests, and the operations they perform (add a session, delete a session)
/// are each a single read-modify-write on one process.
pub(crate) fn edit<F>(config: &Path, change: F) -> Result<(), AuthError>
where
    F: FnOnce(&mut toml_edit::DocumentMut) -> Result<(), AuthError>,
{
    let mut doc = document(config)?;
    change(&mut doc)?;
    write_atomic(config, doc.to_string().as_bytes())
}

/// The parsed document, empty when the file does not exist yet.
pub(crate) fn document(config: &Path) -> Result<toml_edit::DocumentMut, AuthError> {
    let text = match fs::read_to_string(config) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(toml_edit::DocumentMut::new());
        }
        Err(error) => {
            return Err(AuthError::internal(format!(
                "reading {}: {error}",
                config.display()
            )));
        }
    };
    text.parse::<toml_edit::DocumentMut>().map_err(|error| {
        AuthError::internal(format!("{} will not parse: {error}", config.display()))
    })
}

/// Writes `bytes` to `config` through a temporary file in the same directory.
///
/// The rename is atomic within a directory, so a reader sees either the old file
/// or the new one. The temporary file is created `0600` from the start rather
/// than chmod-ed afterwards: a window in which a signing key sits in a
/// world-readable file is exactly the window an attacker waits for.
fn write_atomic(config: &Path, bytes: &[u8]) -> Result<(), AuthError> {
    let directory = config.parent().filter(|p| !p.as_os_str().is_empty());
    let name = config
        .file_name()
        .ok_or_else(|| AuthError::internal(format!("{} is not a file name", config.display())))?;

    let mut temporary = PathBuf::from(directory.unwrap_or(Path::new(".")));
    temporary.push(format!(
        ".{}.tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    ));

    let write = || -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(MODE)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, config)
    };

    write().map_err(|error| {
        let _ = fs::remove_file(&temporary);
        AuthError::internal(format!("writing {}: {error}", config.display()))
    })
}

/// The mode `config` currently has, if it exists.
///
/// Separate from the write path so the daemon can report a file it inherited
/// rather than one it created: `register` sets `0600`, but a `config.toml` the
/// operator wrote by hand may already be `0644`, and re-creating it would not
/// tighten it.
pub fn mode(config: &Path) -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(config).ok().map(|meta| meta.mode() & 0o777)
}

/// Tightens `config` to [`MODE`] when it is too readable.
///
/// Called once at startup, so a file that predates this module is brought in
/// line. Returns whether it changed anything, for the caller to log: a silent
/// chmod on the operator's file would be a surprise, and a log line is how they
/// learn their settings file is now owner-only.
pub fn tighten(config: &Path) -> Result<bool, AuthError> {
    let Some(current) = mode(config) else {
        return Ok(false);
    };
    if current & 0o077 == 0 {
        return Ok(false);
    }
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(config, fs::Permissions::from_mode(MODE)).map_err(|error| {
        AuthError::internal(format!(
            "{} is mode {current:04o} and cannot be made {MODE:04o}: {error}",
            config.display()
        ))
    })?;
    Ok(true)
}
