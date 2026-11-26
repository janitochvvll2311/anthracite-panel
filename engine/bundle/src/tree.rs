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
