//! The state tree (§6.8): created once, never inferred.
//!
//! Directories are `0700` and files `0600`, and a symlink where a file or
//! directory should be is a hard error rather than something to follow.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use anth_contract::{Composition, Error};

/// The directories the tree owns, relative to its root.
pub const DIRS: &[&str] = &["apps", "grants", "keys"];
/// The files the tree owns, relative to its root.
pub const FILES: &[&str] = &["composition.toml", "audit.jsonl"];

const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;

/// What `check` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateStatus {
    /// The root does not exist yet: the first boot creates it, so this is not an
    /// error to report before the engine has ever run.
    Missing,
    /// The tree exists and has the shape §6.8 asks for.
    Ready,
}

/// Reads the composition, or returns the empty one when the file does not exist.
///
/// The contract crate parses documents; the daemon is what touches the disk
/// (§6.6).
pub fn load_composition(state: &Path) -> Result<Composition, Error> {
    let path = state.join("composition.toml");
    match fs::read_to_string(&path) {
        Ok(text) => Composition::parse(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Composition::empty()),
        Err(error) => Err(Error::internal(format!(
            "reading {}: {error}",
            path.display()
        ))),
    }
}

/// Creates the tree if it is missing and checks the shape if it is not.
pub fn ensure(root: &Path) -> Result<(), Error> {
    create_dir(root)?;
    for name in DIRS {
        create_dir(&root.join(name))?;
    }
    let composition = toml::to_string_pretty(&Composition::empty())
        .map_err(|error| Error::internal(format!("rendering the composition: {error}")))?;
    write_missing(&root.join("composition.toml"), composition.as_bytes())?;
    write_missing(&root.join("audit.jsonl"), b"")?;
    Ok(())
}

/// Verifies the shape of an existing tree without creating or fixing anything:
/// names, types, no symlinks, and the modes §6.8 requires.
pub fn check(root: &Path) -> Result<StateStatus, Error> {
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(StateStatus::Missing);
        }
        Err(error) => {
            return Err(Error::internal(format!(
                "inspecting {}: {error}",
                root.display()
            )));
        }
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(Error::invalid(format!(
                    "{} is a symlink; the state tree must be a real directory",
                    root.display()
                )));
            }
            if !metadata.is_dir() {
                return Err(Error::invalid(format!(
                    "{} is not a directory",
                    root.display()
                )));
            }
            check_mode(root, &metadata, DIR_MODE)?;
        }
    }

    for name in DIRS {
        let path = root.join(name);
        let metadata = inspect(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::invalid(format!(
                "{} is not a real directory",
                path.display()
            )));
        }
        check_mode(&path, &metadata, DIR_MODE)?;
    }

    for name in FILES {
        let path = root.join(name);
        let metadata = inspect(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(Error::invalid(format!(
                "{} is not a real file",
                path.display()
            )));
        }
        check_mode(&path, &metadata, FILE_MODE)?;
    }

    Ok(StateStatus::Ready)
}

fn inspect(path: &Path) -> Result<fs::Metadata, Error> {
    fs::symlink_metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::not_found(format!(
                "{} is missing; start the engine once to create it",
                path.display()
            ))
        } else {
            Error::internal(format!("inspecting {}: {error}", path.display()))
        }
    })
}

fn check_mode(path: &Path, metadata: &fs::Metadata, expected: u32) -> Result<(), Error> {
    let actual = metadata.permissions().mode() & 0o777;
    if actual != expected {
        return Err(Error::invalid(format!(
            "{} is {actual:04o}; it should be {expected:04o}",
            path.display()
        )));
    }
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
    fs::set_permissions(path, fs::Permissions::from_mode(DIR_MODE)).map_err(|error| {
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
        .mode(FILE_MODE)
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
        for name in DIRS {
            let path = root.join(name);
            assert!(path.is_dir(), "{} should be a directory", path.display());
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, DIR_MODE, "{} should be 0700", path.display());
        }
        let composition = fs::read_to_string(root.join("composition.toml")).expect("read");
        let parsed = Composition::parse(&composition).expect("parse");
        assert_eq!(parsed, Composition::empty());
        for name in FILES {
            let mode = fs::metadata(root.join(name)).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, FILE_MODE, "{name} should be 0600");
        }
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_created_tree_checks_clean() {
        let root = scratch("check");
        ensure(&root).expect("ensure");
        assert_eq!(check(&root).expect("check"), StateStatus::Ready);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_missing_tree_is_reported_as_missing_and_not_created() {
        let root = scratch("check-missing");
        assert_eq!(check(&root).expect("check"), StateStatus::Missing);
        assert!(
            !root.exists(),
            "check must not create the tree; boot does that"
        );
    }

    #[test]
    fn a_world_writable_directory_is_refused() {
        let root = scratch("mode-dir");
        ensure(&root).expect("ensure");
        fs::set_permissions(root.join("apps"), fs::Permissions::from_mode(0o755)).expect("chmod");
        let error = check(&root).expect_err("mode");
        assert!(error.message().contains("0755"), "{error}");
        assert!(error.message().contains("0700"), "{error}");
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_world_readable_file_is_refused() {
        let root = scratch("mode-file");
        ensure(&root).expect("ensure");
        fs::set_permissions(root.join("audit.jsonl"), fs::Permissions::from_mode(0o644))
            .expect("chmod");
        let error = check(&root).expect_err("mode");
        assert!(error.message().contains("0644"), "{error}");
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn an_existing_root_with_a_missing_entry_is_an_error() {
        let root = scratch("incomplete");
        ensure(&root).expect("ensure");
        fs::remove_file(root.join("audit.jsonl")).expect("rm");
        let error = check(&root).expect_err("incomplete");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_missing_composition_is_empty() {
        let root = scratch("composition");
        // Deliberately no `ensure`: the point is the file that does not exist.
        fs::create_dir_all(&root).expect("mkdir");
        let composition = load_composition(&root).expect("load");
        assert_eq!(composition, Composition::empty());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn an_existing_composition_is_read() {
        let root = scratch("composition-present");
        ensure(&root).expect("ensure");
        fs::write(
            root.join("composition.toml"),
            "version = 1\n\n[[profile]]\nid = \"default\"\napps = [\"org.example.notes\"]\n",
        )
        .expect("write");
        let composition = load_composition(&root).expect("load");
        assert_eq!(composition.profile[0].apps, ["org.example.notes"]);
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
