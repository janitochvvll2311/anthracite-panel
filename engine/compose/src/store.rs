//! File rules shared by the state stores: `0700` directories, `0600` files,
//! atomic replacement and no symlinks.
//!
//! Grants and trusted keys are different documents with the same physical
//! contract, so the contract lives here once. A store names a directory and a
//! file; it never invents a path from user input without validating it first.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anth_contract::Error;

pub(crate) const DIR_MODE: u32 = 0o700;
pub(crate) const FILE_MODE: u32 = 0o600;

/// A counter so two writes in one process never share a temporary name.
static TEMP: AtomicU64 = AtomicU64::new(0);

/// Creates the directory with `0700`, or verifies the existing one.
pub(crate) fn ensure_dir(dir: &Path) -> Result<(), Error> {
    if check_dir(dir)? {
        return Ok(());
    }
    fs::create_dir_all(dir)
        .map_err(|error| Error::internal(format!("creating {}: {error}", dir.display())))?;
    fs::set_permissions(dir, fs::Permissions::from_mode(DIR_MODE)).map_err(|error| {
        Error::internal(format!("setting permissions on {}: {error}", dir.display()))
    })
}

/// `true` when the directory exists and is safe; `false` when it is missing.
/// A symlink, a non-directory or a loose mode is an error: a read must never
/// silently follow a link or trust a world-readable tree.
pub(crate) fn check_dir(dir: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(dir) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(Error::invalid(format!(
                    "{} is a symlink; the state tree must be real directories",
                    dir.display()
                )));
            }
            if !metadata.is_dir() {
                return Err(Error::invalid(format!(
                    "{} is not a directory",
                    dir.display()
                )));
            }
            let mode = metadata.permissions().mode() & 0o777;
            if mode != DIR_MODE {
                return Err(Error::invalid(format!(
                    "{} is {mode:04o}; it should be {DIR_MODE:04o}",
                    dir.display()
                )));
            }
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::internal(format!(
            "inspecting {}: {error}",
            dir.display()
        ))),
    }
}

/// Reads a store file, refusing a symlink. `None` means the file is missing.
pub(crate) fn read(path: &Path) -> Result<Option<String>, Error> {
    reject_symlink(path)?;
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::internal(format!(
            "reading {}: {error}",
            path.display()
        ))),
    }
}

/// Writes `bytes` to `dir/name` atomically: a reader sees the old document or
/// the new one, never a half-written file.
pub(crate) fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    ensure_dir(dir)?;
    write_bytes(dir, name, bytes)
}

/// The state root is `0711` (traversable for service uids) and still owns
/// `composition.toml`; a subdirectory must stay `0700`.
pub(crate) fn write_atomic_root(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    match fs::symlink_metadata(dir) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(Error::invalid(format!(
                    "{} is not a real directory",
                    dir.display()
                )));
            }
            let mode = metadata.permissions().mode() & 0o777;
            if mode != 0o711 {
                return Err(Error::invalid(format!(
                    "{} is {mode:04o}; the state root should be 0711",
                    dir.display()
                )));
            }
        }
        Err(error) => {
            return Err(Error::internal(format!(
                "inspecting {}: {error}",
                dir.display()
            )))
        }
    }
    write_bytes(dir, name, bytes)
}

fn write_bytes(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let path = dir.join(name);
    reject_symlink(&path)?;
    let temp = dir.join(format!(
        ".{name}.tmp.{}.{}",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_file(&temp);
    let result = write_private(&temp, bytes)
        .and_then(|()| fs::rename(&temp, &path).map_err(|error| (temp.clone(), error)));
    match result {
        Ok(()) => Ok(()),
        Err((temp, error)) => {
            let _ = fs::remove_file(&temp);
            Err(Error::internal(format!(
                "writing {}: {error}",
                path.display()
            )))
        }
    }
}

/// Removes a store file. `false` means there was nothing to remove.
pub(crate) fn remove(path: &Path) -> Result<bool, Error> {
    reject_symlink(path)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::internal(format!(
            "removing {}: {error}",
            path.display()
        ))),
    }
}

/// Every `*.toml` in the directory, sorted. A missing directory is empty.
pub(crate) fn toml_paths(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    if !check_dir(dir)? {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(dir)
        .map_err(|error| Error::internal(format!("reading {}: {error}", dir.display())))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| Error::internal(format!("reading {}: {error}", dir.display())))?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn reject_symlink(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::invalid(format!(
            "{} is a symlink; the state tree must be real files",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::internal(format!(
            "inspecting {}: {error}",
            path.display()
        ))),
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<(), (PathBuf, std::io::Error)> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(FILE_MODE)
        .open(path)
        .map_err(|error| (path.to_path_buf(), error))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|error| (path.to_path_buf(), error))
}
