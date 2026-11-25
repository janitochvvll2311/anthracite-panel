//! The state tree (§6.8): created once, never inferred.
//!
//! The root is `0711` so a service uid can traverse it; every child is `0700`
//! (`shared` is `0770`, the trust domain) and files are `0600`. A symlink where
//! a file or directory should be is a hard error rather than something to
//! follow.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anth_contract::{Composition, Error};

/// The directories the tree owns, relative to its root.
pub const DIRS: &[&str] = &["apps", "grants", "keys", "data", "shared", "broker"];
/// Directories added after the first tree shipped: a v0.5 tree may lack them
/// until the next boot creates them.
const OPTIONAL_DIRS: &[&str] = &["shared", "broker"];
/// The files the tree owns, relative to its root.
pub const FILES: &[&str] = &[COMPOSITION, AUDIT, anth_auth::CREDENTIALS];

const COMPOSITION: &str = "composition.toml";
const AUDIT: &str = "audit.jsonl";
const DIR_MODE: u32 = 0o700;
const FILE_MODE: u32 = 0o600;
/// The root is traversable but not listable: a service needs to reach its own
/// `{data}` and socket, while every child directory stays `0700`.
const ROOT_MODE: u32 = 0o711;
/// `{shared}` is the trust domain: every service keeps the engine gid.
const SHARED_MODE: u32 = 0o770;

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
    let path = state.join(COMPOSITION);
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
    fs::set_permissions(root, fs::Permissions::from_mode(ROOT_MODE)).map_err(|error| {
        Error::internal(format!(
            "setting permissions on {}: {error}",
            root.display()
        ))
    })?;
    for name in DIRS {
        let path = root.join(name);
        create_dir(&path)?;
        let mode = match *name {
            "shared" => Some(SHARED_MODE),
            "data" => Some(ROOT_MODE),
            _ => None,
        };
        if let Some(mode) = mode {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).map_err(|error| {
                Error::internal(format!(
                    "setting permissions on {}: {error}",
                    path.display()
                ))
            })?;
        }
    }
    let composition = Composition::empty().to_toml()?;
    write_missing(&root.join(COMPOSITION), composition.as_bytes())?;
    write_missing(&root.join(AUDIT), b"")?;
    // First boot mints the operator's recovery file. `load_or_create` is
    // idempotent, so every later boot reads the same document.
    anth_auth::Auth::load_or_create(root, std::time::Duration::from_secs(1))?;
    Ok(())
}

/// Deletes the recovery file so the next boot mints a new password. Missing is
/// success: resetting a tree that never had credentials is still a reset.
pub fn reset_credentials(state: &Path) -> Result<bool, Error> {
    match fs::remove_file(state.join(anth_auth::CREDENTIALS)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(Error::internal(format!(
            "removing the operator credentials: {error}"
        ))),
    }
}

/// Verifies the shape of an existing tree without creating or fixing anything:
/// names, types, no symlinks, and the modes §6.8 requires.
pub fn check(root: &Path) -> Result<StateStatus, Error> {
    match fs::symlink_metadata(root) {
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            check_creatable(root)?;
            return Ok(StateStatus::Missing);
        }
        Err(error) => {
            return Err(inspect_error(root, error));
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
            // A tree written before L1 is still valid for a read-only check;
            // the next boot upgrades it.
            check_mode_any(root, &metadata, &[ROOT_MODE, DIR_MODE])?;
        }
    }

    for name in DIRS {
        let path = root.join(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if OPTIONAL_DIRS.contains(name) {
                    continue;
                }
                return Err(Error::not_found(format!(
                    "{} is missing; start the engine once to create it",
                    path.display()
                )));
            }
            Err(error) => return Err(inspect_error(&path, error)),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::invalid(format!(
                "{} is not a real directory",
                path.display()
            )));
        }
        let accepted: &[u32] = match *name {
            "shared" => &[SHARED_MODE, DIR_MODE],
            "data" => &[ROOT_MODE, DIR_MODE],
            // The broker listener makes its directory 0711 so a service uid can
            // traverse to its own socket without listing the peers.
            "broker" => &[ROOT_MODE, DIR_MODE],
            _ => &[DIR_MODE],
        };
        check_mode_any(&path, &metadata, accepted)?;
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
            inspect_error(path, error)
        }
    })
}

/// A tree the operator cannot read is a denial to explain, not a bug in the
/// engine: `internal` would send them looking at logs for a permissions problem.
fn inspect_error(path: &Path, error: std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        Error::denied(format!("cannot read {}: {error}", path.display()))
    } else {
        Error::internal(format!("inspecting {}: {error}", path.display()))
    }
}

/// A missing root is only "not created yet" if the first boot could create it.
///
/// `access` is the read-only way to ask: it reports both a missing permission
/// and a read-only mount (`EROFS`), without writing a probe file or touching the
/// configured path. The nearest existing ancestor is the one `create_dir_all`
/// would need to write into.
fn check_creatable(root: &Path) -> Result<(), Error> {
    let dir = existing_ancestor(root)?;
    ensure_writable(root, &dir)
}

/// The directory `create_dir_all(root)` would write into.
///
/// A single-component relative path (`state`) has an **empty** parent, which is
/// not a path: it means the current directory. Treating it as nothing is how the
/// check used to pass on a read-only working directory without inspecting it.
fn existing_ancestor(root: &Path) -> Result<PathBuf, Error> {
    let mut ancestor = current_dir_or(root.parent());
    loop {
        match fs::metadata(&ancestor) {
            Ok(metadata) if metadata.is_dir() => return Ok(ancestor),
            Ok(_) => {
                return Err(Error::invalid(format!(
                    "{} exists and is not a directory; {} cannot be created",
                    ancestor.display(),
                    root.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let next = current_dir_or(ancestor.parent());
                if next == ancestor {
                    // `.` (or `/`) does not exist: there is nowhere to write.
                    return Err(Error::invalid(format!(
                        "cannot create {}: {} does not exist",
                        root.display(),
                        ancestor.display()
                    )));
                }
                ancestor = next;
            }
            Err(error) => {
                return Err(inspect_error(&ancestor, error));
            }
        }
    }
}

fn current_dir_or(path: Option<&Path>) -> PathBuf {
    match path {
        Some(path) if !path.as_os_str().is_empty() => path.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn ensure_writable(root: &Path, dir: &Path) -> Result<(), Error> {
    use std::os::unix::ffi::OsStrExt;

    let path = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| Error::invalid(format!("{} contains a NUL byte", dir.display())))?;
    // SAFETY: `path` is a valid NUL-terminated string for the call, and `access`
    // only reads filesystem metadata.
    let writable = unsafe { libc::access(path.as_ptr(), libc::W_OK | libc::X_OK) } == 0;
    if !writable {
        let error = std::io::Error::last_os_error();
        return Err(Error::invalid(format!(
            "cannot create {}: {} is not writable ({error})",
            root.display(),
            dir.display()
        )));
    }
    Ok(())
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

fn check_mode_any(path: &Path, metadata: &fs::Metadata, accepted: &[u32]) -> Result<(), Error> {
    let actual = metadata.permissions().mode() & 0o777;
    if !accepted.contains(&actual) {
        let expected = accepted
            .iter()
            .map(|mode| format!("{mode:04o}"))
            .collect::<Vec<_>>()
            .join(" or ");
        return Err(Error::invalid(format!(
            "{} is {actual:04o}; it should be {expected}",
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
    fn the_broker_directory_may_be_traversable() {
        let root = scratch("broker-traversable");
        ensure(&root).expect("ensure");
        // `broker::start_all` widens it after the first boot.
        fs::set_permissions(root.join("broker"), fs::Permissions::from_mode(ROOT_MODE))
            .expect("chmod");
        check(&root).expect("a booted tree with a 0711 broker is valid");
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn the_tree_is_created_with_the_right_modes() {
        let root = scratch("create");
        ensure(&root).expect("ensure");
        let root_mode = fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        assert_eq!(root_mode, ROOT_MODE, "the root should be 0711");
        for name in DIRS {
            let path = root.join(name);
            assert!(path.is_dir(), "{} should be a directory", path.display());
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            let expected = match *name {
                "shared" => SHARED_MODE,
                "data" => ROOT_MODE,
                _ => DIR_MODE,
            };
            assert_eq!(
                mode,
                expected,
                "{} should be {expected:04o}",
                path.display()
            );
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
    fn a_missing_required_directory_is_not_found_but_shared_is_optional() {
        let root = scratch("check-dirs");
        ensure(&root).expect("ensure");
        fs::remove_dir_all(root.join("shared")).expect("remove shared");
        assert_eq!(
            check(&root).expect("shared is optional"),
            StateStatus::Ready
        );
        fs::remove_dir_all(root.join("keys")).expect("remove keys");
        let error = check(&root).expect_err("keys is required");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
        assert!(error.message().contains("is missing"), "{error}");
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
    fn a_single_component_relative_path_is_checked_against_the_current_directory() {
        // `Path::new("state").parent()` is `Some("")`, and an empty path must
        // mean ".", not "nothing to check".
        assert_eq!(
            existing_ancestor(Path::new("state")).expect("ancestor"),
            PathBuf::from(".")
        );
        assert_eq!(
            existing_ancestor(Path::new("missing/state")).expect("ancestor"),
            PathBuf::from(".")
        );
    }

    #[test]
    fn an_absolute_path_uses_its_nearest_existing_ancestor() {
        let dir = scratch("ancestor");
        fs::create_dir_all(&dir).expect("mkdir");
        assert_eq!(
            existing_ancestor(&dir.join("a/b/state")).expect("ancestor"),
            dir
        );
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn an_unreadable_tree_is_reported_as_denied() {
        // End to end this needs a tree owned by another user (verified as
        // `nobody` in the PR); as the same user a chmod cannot produce EACCES,
        // so the mapping itself is what the test asserts.
        let error = inspect_error(
            Path::new("/tmp/anth/state/apps"),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        assert!(error.message().contains("cannot read"), "{error}");

        let missing = inspect_error(
            Path::new("/tmp/anth/state/apps"),
            std::io::Error::from(std::io::ErrorKind::Other),
        );
        assert_eq!(missing.kind(), anth_contract::ErrorKind::Internal);
    }

    #[test]
    fn a_missing_tree_below_a_file_cannot_be_created() {
        let dir = scratch("blocked");
        fs::create_dir_all(&dir).expect("mkdir");
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"").expect("write");
        let error = check(&blocker.join("state")).expect_err("blocked");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        assert!(error.message().contains("not a directory"), "{error}");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_missing_tree_below_an_unwritable_parent_cannot_be_created() {
        // Root can write anywhere; on a developer machine the check would pass
        // for the wrong reason, so this only runs as a normal user.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let dir = scratch("read-only-parent");
        fs::create_dir_all(&dir).expect("mkdir");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).expect("chmod");
        let error = check(&dir.join("state")).expect_err("read only");
        assert!(error.message().contains("not writable"), "{error}");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("chmod back");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_pre_l1_tree_is_still_valid_for_a_read_only_check() {
        let root = scratch("pre-l1");
        ensure(&root).expect("ensure");
        // A v0.6 tree: 0700 root, 0700 shared and 0700 data.
        fs::set_permissions(&root, fs::Permissions::from_mode(DIR_MODE)).expect("root");
        fs::set_permissions(root.join("shared"), fs::Permissions::from_mode(DIR_MODE))
            .expect("shared");
        fs::set_permissions(root.join("data"), fs::Permissions::from_mode(DIR_MODE)).expect("data");
        check(&root).expect("an old tree is readable");
        fs::remove_dir_all(&root).expect("cleanup");
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
