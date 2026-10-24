//! Installing a bundle: the one operation that puts new code on disk.
//!
//! It runs in the CLI, not behind the socket, and that placement is the security
//! decision. `plugin call` may run a plugin's code, but only code the operator
//! already installed and whose digest the host re-checks on every load. Letting
//! the socket *add* a bundle would turn "may talk to the daemon" into "may run
//! anything as the daemon", a far larger grant than any request in the protocol.
//! Installing is therefore a local filesystem action: it needs the same access
//! that writing the plugin directory by hand would need.
//!
//! The order below is the point of the module. Nothing appears where the registry
//! looks until the archive has been read safely, the manifest has been validated,
//! the digest has matched and the module has been compiled. Extraction happens in
//! a hidden staging directory, and a refusal at any step removes it, so a rejected
//! bundle leaves no half-installed plugin for the next daemon start to find.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use anth_wire::{ErrorKind, WireError};
use sha2::{Digest, Sha256};

use super::manifest::Manifest;
use super::registry::hex;
use super::runtime::Loaded;

/// A bundle may expand to at most this many bytes.
///
/// The limit is on what is *written*, not on what the archive declares: a zip
/// can state any size it likes, so the count is taken as the bytes arrive.
pub const MAX_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;

/// A bundle may hold at most this many entries.
///
/// Bounds the work before any file is created, so an archive with a million
/// entries is refused once rather than a million times.
pub const MAX_ENTRIES: usize = 512;

const MANIFEST: &str = "plugin.toml";
const MODULE: &str = "plugin.wasm";

/// What an install did, for the caller to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
    pub anchors: usize,
    pub slots: usize,
}

/// Whether `name` is a file a bundle may contain.
///
/// An allowlist, not a blocklist: a bundle holds `plugin.toml`, `plugin.wasm` and
/// whatever the manifest serves under `web/`. Anything else is refused, because
/// the directory the registry reads is not a place to drop files the host never
/// looked at.
fn member_allowed(name: &str) -> bool {
    name == MANIFEST || name == MODULE || (name.starts_with("web/") && name.len() > "web/".len())
}

/// The path an archive entry names, or `None` if it could escape the bundle.
///
/// Refuses rather than repairs. `../x` and `/etc/passwd` are the shapes that
/// matter, but `./web/x` is refused too: normalising it would make the name in
/// the archive and the file on disk disagree, and the allowlist would then be
/// approving one string while writing another.
pub(crate) fn safe_member(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\\') {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            // RootDir, Prefix, ParentDir and CurDir all mean the entry is not a
            // plain path inside the bundle.
            _ => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

/// Whether a zip entry is a symlink.
///
/// A symlink is not a file whose bytes can be written: extracting one creates a
/// link, and a later entry under that name would be written through it. The
/// format has no use for links, so they are refused rather than followed.
fn is_symlink(mode: Option<u32>) -> bool {
    mode.is_some_and(|mode| mode & 0o170_000 == 0o120_000)
}

/// Validates one archive entry name and returns the path to write it at.
///
/// The three refusals — not inside the bundle, not a file a bundle holds, named
/// twice — are decided together so a caller cannot apply two of them and forget
/// the third. `zip`'s reader collapses duplicate names, so the last one is
/// tested against this function rather than through an archive: it is defence
/// against the reader changing, and a test that could only assert the reader's
/// behaviour would not cover it.
pub(crate) fn plan_member(raw: &str, seen: &mut HashSet<PathBuf>) -> Result<PathBuf, WireError> {
    let relative = safe_member(raw).ok_or_else(|| {
        WireError::new(
            ErrorKind::Forbidden,
            format!("`{raw}` is not a path inside the bundle"),
        )
    })?;
    let name = relative.to_string_lossy().replace('\\', "/");
    if !member_allowed(&name) {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            format!(
                "`{name}` is not part of a bundle; a bundle holds `{MANIFEST}`, `{MODULE}` and `web/`"
            ),
        ));
    }
    if !seen.insert(relative.clone()) {
        return Err(WireError::invalid(format!(
            "`{name}` appears twice in the bundle, so what it holds depends on read order"
        )));
    }
    Ok(relative)
}

/// A staging directory that removes itself unless the install succeeded.
///
/// Every refusal below returns early, `?` included, and this is what keeps each
/// of those paths from leaving a directory behind.
struct Staging {
    path: PathBuf,
    keep: bool,
}

impl Drop for Staging {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn staging_root(root: &Path) -> Result<Staging, WireError> {
    fs::create_dir_all(root).map_err(|e| {
        WireError::internal(format!(
            "creating the plugin directory {}: {e}; set ANTH_PLUGINS to a directory you can write",
            root.display()
        ))
    })?;
    let unique = format!(
        ".staging-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let path = root.join(unique);
    fs::create_dir(&path)
        .map_err(|e| WireError::internal(format!("staging in {}: {e}", root.display())))?;
    Ok(Staging { path, keep: false })
}

fn aside_name(root: &Path, label: &str) -> PathBuf {
    root.join(format!(
        ".{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ))
}

/// Installs the bundle at `bundle` into the registry root `root`.
///
/// `force` replaces a bundle already installed under the same id. The old
/// directory is renamed aside before the new one moves in, and put back if that
/// move fails, so the root never ends up with neither.
pub fn install(bundle: &Path, root: &Path, force: bool) -> Result<Installed, WireError> {
    let file = fs::File::open(bundle).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => WireError::new(
            ErrorKind::NotFound,
            format!(
                "no bundle at {}; `plugins/<id>/package.sh` writes one under `dist/`",
                bundle.display()
            ),
        ),
        _ => WireError::internal(format!("opening {}: {e}", bundle.display())),
    })?;

    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        WireError::invalid(format!("{} is not a zip bundle: {e}", bundle.display()))
    })?;

    if archive.len() > MAX_ENTRIES {
        return Err(WireError::new(
            ErrorKind::TooLarge,
            format!(
                "the bundle holds {} entries, at most {MAX_ENTRIES} are accepted",
                archive.len()
            ),
        ));
    }

    // Plan first, write later: every refusal that depends only on the archive's
    // own metadata happens before a directory exists at all.
    let mut planned: Vec<(usize, PathBuf)> = Vec::with_capacity(archive.len());
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|e| WireError::invalid(format!("unreadable bundle entry {index}: {e}")))?;
        if entry.is_dir() {
            // Directories are implied by their members; an explicit one is
            // allowed but says nothing on its own.
            continue;
        }
        let raw = entry.name().to_string();
        if is_symlink(entry.unix_mode()) {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                format!("`{raw}` is a symlink; a bundle holds plain files only"),
            ));
        }
        planned.push((index, plan_member(&raw, &mut seen)?));
    }

    let mut staging = staging_root(root)?;

    let mut total = 0u64;
    for (index, relative) in planned {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| WireError::invalid(format!("unreadable bundle entry {index}: {e}")))?;
        let target = staging.path.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| WireError::internal(format!("creating {}: {e}", parent.display())))?;
        }
        // `create_new` refuses a path that already exists, a second line against
        // the duplicate case above; `O_NOFOLLOW` refuses to write through a link.
        let mut out = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&target)
            .map_err(|e| match e.raw_os_error() {
                Some(libc::ELOOP) => WireError::new(
                    ErrorKind::Forbidden,
                    format!(
                        "`{}` would be written through a symlink",
                        relative.display()
                    ),
                ),
                _ => WireError::internal(format!("writing {}: {e}", target.display())),
            })?;

        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = entry
                .read(&mut buffer)
                .map_err(|e| WireError::invalid(format!("reading the bundle: {e}")))?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read as u64);
            if total > MAX_BUNDLE_BYTES {
                return Err(WireError::new(
                    ErrorKind::TooLarge,
                    format!("the bundle expands past {MAX_BUNDLE_BYTES} bytes"),
                ));
            }
            out.write_all(&buffer[..read])
                .map_err(|e| WireError::internal(format!("writing {}: {e}", target.display())))?;
        }
    }

    let manifest_path = staging.path.join(MANIFEST);
    let text = fs::read_to_string(&manifest_path)
        .map_err(|e| WireError::invalid(format!("the bundle has no `{MANIFEST}`: {e}")))?;
    let manifest = Manifest::parse(&text)?;

    // The digest is checked here as well as at load, so an install that would
    // produce a disabled plugin fails now, with the reason, instead of
    // succeeding and leaving the operator to find out from `plugin list`.
    let wasm_path = staging.path.join(MODULE);
    let wasm = fs::read(&wasm_path)
        .map_err(|e| WireError::invalid(format!("the bundle has no `{MODULE}`: {e}")))?;
    let digest = hex(&Sha256::digest(&wasm));
    if digest != manifest.wasm.sha256.to_lowercase() {
        return Err(WireError::invalid(format!(
            "the module digest is {digest}, the manifest approves {}. Re-run package.sh so the digest names these bytes.",
            manifest.wasm.sha256
        )));
    }

    // Compiling proves these bytes are a module this host can drive. A digest
    // says they are the approved bytes; it says nothing about whether they
    // parse, and a bundle that installs then traps on every call is worse than
    // one that never installs.
    Loaded::load(manifest.clone(), &wasm)?;

    let id = manifest.plugin.id.clone();
    let destination = root.join(&id);
    if destination.exists() && !force {
        return Err(WireError::new(
            ErrorKind::Conflict,
            format!(
                "plugin `{id}` is already installed in {}; pass --force to replace it",
                destination.display()
            ),
        ));
    }

    let replaced = if destination.exists() {
        let aside = aside_name(root, "replaced");
        fs::rename(&destination, &aside).map_err(|e| {
            WireError::internal(format!("moving {} aside: {e}", destination.display()))
        })?;
        Some(aside)
    } else {
        None
    };

    if let Err(error) = fs::rename(&staging.path, &destination) {
        if let Some(replaced) = &replaced {
            let _ = fs::rename(replaced, &destination);
        }
        return Err(WireError::internal(format!(
            "moving the bundle into {}: {error}",
            destination.display()
        )));
    }
    staging.keep = true;

    if let Some(replaced) = replaced {
        let _ = fs::remove_dir_all(replaced);
    }

    Ok(Installed {
        id,
        name: manifest.plugin.name.clone(),
        version: manifest.plugin.version.clone(),
        dir: destination,
        anchors: manifest.api.len(),
        slots: manifest.ui.len(),
    })
}

/// The directory one plugin occupies, or `None` if it is not installed.
///
/// `id` arrives from an argument and the caller removes the result recursively,
/// so it is validated here rather than at each call site: `plugin remove
/// ../victim` must not reach a sibling of the registry root. The check lives in
/// the function that builds the path so a future caller cannot forget it.
pub fn plugin_dir(root: &Path, id: &str) -> Result<PathBuf, WireError> {
    super::manifest::validate_id(id)?;
    Ok(root.join(id))
}

/// Removes an installed bundle, and says which one it removed.
///
/// Local for the same reason `install` is local: the socket may not change what
/// the daemon will load next time. Removal is refused for a directory that is
/// not a plugin, so a registry root that also happens to hold something else is
/// not silently pruned entry by entry.
pub fn remove(root: &Path, id: &str) -> Result<PathBuf, WireError> {
    let dir = plugin_dir(root, id)?;
    let metadata = fs::symlink_metadata(&dir).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => WireError::new(
            ErrorKind::NotFound,
            format!("plugin `{id}` is not installed"),
        ),
        _ => WireError::internal(format!("inspecting {}: {e}", dir.display())),
    })?;
    if !metadata.is_dir() {
        return Err(WireError::new(
            ErrorKind::Conflict,
            format!(
                "{} is not a directory, so it is not a plugin",
                dir.display()
            ),
        ));
    }
    if !dir.join(MANIFEST).is_file() {
        return Err(WireError::new(
            ErrorKind::Conflict,
            format!(
                "{} holds no `{MANIFEST}`, so it is not a plugin; refusing to delete it",
                dir.display()
            ),
        ));
    }
    fs::remove_dir_all(&dir)
        .map_err(|e| WireError::internal(format!("removing {}: {e}", dir.display())))?;
    Ok(dir)
}
