//! Confined zip unpacking for an app bundle.

use std::path::Path;

use anth_error::Error;

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

    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.is_dir() {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(
                    |error| Error::internal(format!("chmod {}: {error}", path.display())),
                )?;
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
