//! The state tree (§6.8): created once, never inferred.
//!
//! Directories are `0700` and files `0600`, and a symlink where a file or
//! directory should be is a hard error rather than something to follow.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anth_contract::Error;

/// The composition written on first boot: empty, but valid.
pub const EMPTY_COMPOSITION: &str = "version = 1\n\n[[profile]]\nid = \"default\"\napps = []\n";

/// Creates the tree if it is missing and checks the shape if it is not.
pub fn ensure(root: &Path) -> Result<(), Error> {
    create_dir(root)?;
    for name in ["apps", "grants", "keys"] {
        create_dir(&root.join(name))?;
    }
    write_missing(&root.join("composition.toml"), EMPTY_COMPOSITION.as_bytes())?;
    write_missing(&root.join("audit.jsonl"), b"")?;
    Ok(())
}

fn create_dir(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(Error::invalid(format!(
                    "{} is a symlink; the state tree must be real directories",
                    path.display()
                )));
            }
            if !metadata.is_dir() {
                return Err(Error::invalid(format!(
                    "{} exists and is not a directory",
                    path.display()
                )));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| {
                Error::internal(format!("creating {}: {error}", path.display()))
            })?;
        }
        Err(error) => {
            return Err(Error::internal(format!(
                "inspecting {}: {error}",
                path.display()
            )));
        }
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
        Error::internal(format!(
            "setting permissions on {}: {error}",
            path.display()
        ))
    })
}

fn write_missing(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::invalid(format!(
                "{} is a symlink; the state tree must be real files",
                path.display()
            )));
        }
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(Error::internal(format!(
                "inspecting {}: {error}",
                path.display()
            )));
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| Error::internal(format!("creating {}: {error}", path.display())))?;
    file.write_all(bytes)
        .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("anth-state-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_tree_is_created_with_the_right_modes() {
        let root = scratch("create");
        ensure(&root).expect("ensure");
        for name in ["apps", "grants", "keys"] {
            let path = root.join(name);
            assert!(path.is_dir(), "{} should be a directory", path.display());
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{} should be 0700", path.display());
        }
        let composition = fs::read_to_string(root.join("composition.toml")).expect("read");
        assert_eq!(composition, EMPTY_COMPOSITION);
        let mode = fs::metadata(root.join("composition.toml"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn an_existing_file_is_not_overwritten() {
        let root = scratch("existing");
        ensure(&root).expect("ensure");
        fs::write(root.join("composition.toml"), "keep me").expect("write");
        ensure(&root).expect("ensure again");
        assert_eq!(
            fs::read_to_string(root.join("composition.toml")).expect("read"),
            "keep me"
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_symlink_is_refused() {
        let root = scratch("symlink");
        fs::create_dir_all(&root).expect("mkdir");
        fs::write(root.join("outside"), b"").expect("write");
        std::os::unix::fs::symlink(root.join("outside"), root.join("apps")).expect("symlink");
        let error = ensure(&root).expect_err("symlink");
        assert!(error.message().contains("symlink"), "{error}");
        fs::remove_dir_all(&root).expect("cleanup");
    }
}
