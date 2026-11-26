//! The signed set of a bundle tree: `manifest.toml` plus every `wit/**` file.

use std::fs;
use std::path::Path;

use anth_error::Error;

use crate::message::message;

/// The canonical message of a bundle tree: `manifest.toml` plus every `wit/**`
/// file, sorted by name (§9.1, D-Signature-Message).
pub fn signed_message(root: &Path) -> Result<Vec<u8>, Error> {
    let mut files = vec![(
        "manifest.toml".to_string(),
        read_required(root, "manifest.toml")?,
    )];
    for (relative, is_file) in walk(root)? {
        if is_file && relative.starts_with("wit/") {
            files.push((relative.clone(), read_required(root, &relative)?));
        }
    }
    message(&files)
}

fn read_required(root: &Path, relative: &str) -> Result<Vec<u8>, Error> {
    fs::read(root.join(relative)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::invalid(format!("the bundle has no `{relative}`"))
        } else {
            Error::internal(format!("reading `{relative}`: {error}"))
        }
    })
}

/// Every file under `root`, relative and with forward slashes, sorted.
fn walk(root: &Path) -> Result<Vec<(String, bool)>, Error> {
    fn visit(base: &Path, dir: &Path, out: &mut Vec<(String, bool)>) -> Result<(), Error> {
        let entries = fs::read_dir(dir)
            .map_err(|error| Error::internal(format!("reading {}: {error}", dir.display())))?;
        for entry in entries {
            let entry = entry
                .map_err(|error| Error::internal(format!("reading {}: {error}", dir.display())))?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|error| {
                Error::internal(format!("inspecting {}: {error}", path.display()))
            })?;
            if metadata.file_type().is_symlink() {
                return Err(Error::invalid(format!(
                    "`{}` is a symlink; bundles hold real files",
                    path.display()
                )));
            }
            let relative = path
                .strip_prefix(base)
                .expect("the path is under the root")
                .to_str()
                .ok_or_else(|| Error::invalid(format!("`{}` is not UTF-8", path.display())))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            if metadata.is_dir() {
                out.push((relative, false));
                visit(base, &path, out)?;
            } else {
                out.push((relative, true));
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    visit(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

/// A bundle that unpacks to more entries than this is refused before reading.
const MAX_FILES: usize = 512;
/// Per-file uncompressed cap; the total is the caller's body cap.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// Unpacks a bundle zip into an empty staging directory. Paths are confined,
/// symlinks are refused, files are created `0600` and every cap is enforced
/// while reading, never after buffering the whole entry.
pub fn unpack(root: &Path, bundle: &[u8], max_total: u64) -> Result<(), Error> {
    use std::io::{Cursor, Read};
    use std::os::unix::fs::OpenOptionsExt;

    let mut archive = zip::ZipArchive::new(Cursor::new(bundle))
        .map_err(|error| Error::invalid(format!("the bundle is not a zip: {error}")))?;
    if archive.len() > MAX_FILES {
        return Err(Error::too_large(format!(
            "the bundle has {} entries; the limit is {MAX_FILES}",
            archive.len()
        )));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| Error::invalid(format!("reading the bundle: {error}")))?;
        let name = entry
            .enclosed_name()
            .ok_or_else(|| Error::invalid(format!("`{}` escapes the bundle", entry.name())))?;
        let mut relative = name
            .to_str()
            .ok_or_else(|| Error::invalid(format!("`{}` is not UTF-8", entry.name())))?
            .to_string();
        while relative.ends_with('/') {
            relative.pop();
        }
        if relative.contains('\\') || relative.contains('\0') {
            return Err(Error::invalid(format!(
                "`{relative}` is not a plain path inside the bundle"
            )));
        }
        if relative
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(Error::invalid(format!(
                "`{relative}` is not a plain path inside the bundle"
            )));
        }
        if !seen.insert(relative.clone()) {
            return Err(Error::invalid(format!(
                "`{relative}` appears twice in the bundle"
            )));
        }
        if let Some(mode) = entry.unix_mode() {
            if mode & 0o170000 == 0o120000 {
                return Err(Error::invalid(format!(
                    "`{relative}` is a symlink; bundles hold real files"
                )));
            }
        }
        let path = root.join(&name);
        if entry.is_dir() {
            private_dir(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            private_dir(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|error| Error::internal(format!("creating {}: {error}", path.display())))?;
        let copied = std::io::copy(&mut entry.by_ref().take(MAX_FILE_BYTES + 1), &mut file)
            .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))?;
        if copied > MAX_FILE_BYTES {
            return Err(Error::too_large(format!(
                "`{relative}` is larger than {MAX_FILE_BYTES} bytes"
            )));
        }
        file.sync_all()
            .map_err(|error| Error::internal(format!("syncing {}: {error}", path.display())))?;
        total += copied;
        if total > max_total {
            return Err(Error::too_large(format!(
                "the bundle expands to more than {max_total} bytes"
            )));
        }
    }
    Ok(())
}

fn private_dir(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    match std::fs::metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
                return Ok(());
            }
            return Err(Error::invalid(format!(
                "{} exists and is not a directory",
                path.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::internal(format!(
                "inspecting {}: {error}",
                path.display()
            )));
        }
    }
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(|error| Error::internal(format!("creating {}: {error}", path.display())))
}
