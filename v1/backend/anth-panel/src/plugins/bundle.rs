//! Plugin bundles: reading a zip, freezing its recipe, and installing it.
//!
//! The two jobs here are confining the archive and freezing the grant. The
//! member guard is the same [`safe_member`](crate::files::archive::safe_member)
//! the file manager uses, so there is one answer to "may this name be written",
//! not two that can drift. Writes additionally open with `O_NOFOLLOW`, because
//! a check followed by a create is two syscalls and a concurrent writer could
//! swap a directory for a link in between.
//!
//! Installation is the only moment a plugin gains authority. It reads the
//! manifest, validates it against host policy, hashes the module, writes the
//! bundle to disk and records a grant whose capability list is the *normalized*
//! one from the manifest — never the text of the file, which the plugin could
//! rewrite later.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

use super::manifest::{
    InstalledPlugin, MAX_MANIFEST_BYTES, Manifest, PluginError, PluginErrorKind, PluginGrant,
    WASM_FILE,
};
use super::runtime;
use super::signature;

/// Detached Ed25519 signature over `plugin.wasm`, hex encoded.
const SIGNATURE_FILE: &str = "plugin.sig";

/// Largest bundle accepted. A wasm module plus web assets; anything larger is a
/// mistake or an attack, and the limit is applied while extracting.
pub const MAX_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;

/// Maximum number of entries in one bundle.
pub const MAX_BUNDLE_ENTRIES: usize = 512;

/// Every file a bundle may contain. A name outside this list is refused rather
/// than skipped: a bundle carrying something unexpected is a bundle the admin
/// should look at, not one to install minus a file.
fn is_allowed_member(name: &str) -> bool {
    name == "plugin.toml"
        || name == "plugin.wasm"
        || name == SIGNATURE_FILE
        || (name.starts_with("web/") && !name.ends_with('/'))
}

fn io_error(detail: impl Into<String>) -> PluginError {
    PluginError::new(PluginErrorKind::Io, detail)
}

/// One bundle on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub plugin: InstalledPlugin,
    pub root: PathBuf,
}

impl Installed {
    /// Path to the wasm module.
    pub fn wasm_path(&self) -> PathBuf {
        self.root.join(WASM_FILE)
    }
}

/// The result of a successful install.
///
/// `warning` is `Some` only when the bundle was accepted without a verified
/// signature, which happens when `[plugins] require_signature = false`. The CLI
/// and the web UI both surface it; an unverified install must never look like a
/// verified one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleInstall {
    pub installed: Installed,
    pub grant: PluginGrant,
    pub warning: Option<String>,
}

/// Reads a bundle and installs it under `plugins_root/<id>`.
///
/// The extraction goes to a temporary directory first, so a bundle that fails
/// halfway — a bad hash, a refused member — leaves no half-installed plugin
/// that a later load would try to run. `require_signature` and `public_key`
/// decide whether `plugin.sig` is mandatory and which key checks it.
pub fn install(
    bundle: &Path,
    plugins_root: &Path,
    require_signature: bool,
    public_key: &str,
) -> Result<BundleInstall, PluginError> {
    let staging = staging_dir(plugins_root)?;

    let result = extract_bundle(bundle, &staging).and_then(|()| {
        let text = fs::read_to_string(staging.join("plugin.toml"))
            .map_err(|error| io_error(format!("the bundle has no `plugin.toml`: {error}")))?;
        let manifest = Manifest::parse(&text)?;
        if manifest.wasm.path.trim() != WASM_FILE {
            return Err(PluginError::invalid(format!(
                "`wasm.path` must be `{WASM_FILE}`"
            )));
        }
        Ok(manifest)
    });

    let manifest = match result {
        Ok(manifest) => manifest,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };

    let destination = plugin_dir(plugins_root, &manifest.plugin.id)?;
    if destination.exists() {
        let _ = fs::remove_dir_all(&staging);
        return Err(PluginError::new(
            PluginErrorKind::Conflict,
            format!("plugin `{}` is already installed", manifest.plugin.id),
        ));
    }

    let grant = PluginGrant::from_manifest(&manifest);
    // Hash against the grant, not the manifest, so `install` and `load` apply
    // one rule: the digest the admin approved is the one that must match.
    if let Err(error) = verify_wasm(&staging, &grant.wasm_sha256) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    // Then check it is wasm the host can load. A digest proves the bytes are the
    // ones the admin approved; it says nothing about whether they parse. Refusing
    // here is the difference between a rejected upload and a plugin that is
    // listed, hashes correctly, and fails on every request.
    if let Err(error) = verify_module(&staging) {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    // The digest proves the bytes match what the manifest names; the signature
    // proves the author the admin trusts produced them. They answer different
    // questions, so install checks both.
    let warning = match verify_signature(&staging, require_signature, public_key) {
        Ok(warning) => warning,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };

    fs::rename(&staging, &destination)
        .map_err(|error| io_error(format!("could not install the bundle: {error}")))?;

    Ok(BundleInstall {
        installed: installed_view(&manifest, &grant, destination),
        grant,
        warning,
    })
}

/// Checks `plugin.sig` against the configured key.
///
/// Returns the warning to show when the bundle was accepted without a verified
/// signature. A signature that *is* present is always checked, even when the
/// requirement is switched off: the flag exists to allow an unsigned bundle, not
/// to accept a signature that does not match.
pub fn verify_signature(
    root: &Path,
    require_signature: bool,
    public_key: &str,
) -> Result<Option<String>, PluginError> {
    let wasm = root.join(WASM_FILE);
    let wasm_bytes = fs::read(&wasm)
        .map_err(|error| io_error(format!("could not read `{}`: {error}", wasm.display())))?;
    let signature_path = root.join(SIGNATURE_FILE);
    let signature = match fs::read_to_string(&signature_path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(io_error(format!(
                "could not read `{SIGNATURE_FILE}`: {error}"
            )));
        }
    };
    let key = public_key.trim();

    if require_signature {
        if key.is_empty() {
            return Err(PluginError::denied(
                "no plugin signing key is configured; set `[plugins] public_key` or `require_signature = false`",
            ));
        }
        let Some(signature) = signature else {
            return Err(PluginError::denied(
                "the bundle is not signed; it has no `plugin.sig`",
            ));
        };
        signature::verify(key, &signature, &wasm_bytes).map_err(PluginError::denied)?;
        return Ok(None);
    }

    let warning = match (signature, key.is_empty()) {
        (Some(signature), false) => {
            signature::verify(key, &signature, &wasm_bytes).map_err(PluginError::denied)?;
            None
        }
        (Some(_), true) => Some(
            "installed without verifying the signature: `[plugins] public_key` is empty"
                .to_string(),
        ),
        (None, _) => Some(
            "installed without a signature because `[plugins] require_signature = false`"
                .to_string(),
        ),
    };
    Ok(warning)
}

/// Re-reads an installed plugin and confirms its module still matches `grant`.
///
/// This is what makes the freeze real. A manifest is a file inside the bundle,
/// so a plugin directory that a local user can write is a plugin directory that
/// can be re-pointed; the hash is checked on every load rather than trusted.
pub fn load(root: &Path, grant: &PluginGrant) -> Result<Installed, PluginError> {
    let text = fs::read_to_string(root.join("plugin.toml"))
        .map_err(|error| io_error(format!("could not read the manifest: {error}")))?;
    let manifest = Manifest::parse(&text)?;

    if manifest.wasm.path.trim() != WASM_FILE {
        return Err(PluginError::new(
            PluginErrorKind::Tampered,
            format!("`wasm.path` must be `{WASM_FILE}`"),
        ));
    }
    if manifest.plugin.id != grant.id {
        return Err(PluginError::new(
            PluginErrorKind::Tampered,
            format!(
                "`{}` declares `{}`, the grant is for `{}`",
                root.display(),
                manifest.plugin.id,
                grant.id
            ),
        ));
    }
    // The expected digest comes from the grant, never from the manifest. A
    // manifest lives inside a bundle an attacker can write, so comparing the
    // module to the manifest's own hash would let a rewritten module travel
    // with a rewritten hash and stay consistent while the grant still names the
    // old one. The grant is the only digest the admin approved.
    verify_wasm(root, &grant.wasm_sha256)?;

    if !manifest.plugin.version.eq(&grant.version) {
        return Err(PluginError::new(
            PluginErrorKind::Tampered,
            format!(
                "`{}` is version {}, the grant froze {}",
                manifest.plugin.id, manifest.plugin.version, grant.version
            ),
        ));
    }
    // The hash proves these are the approved bytes; it does not prove they load.
    // Probing here too, not only at install, is what makes the daemon's view and
    // the CLI's view agree about a plugin being usable.
    verify_module(root)?;

    Ok(installed_view(&manifest, grant, root.to_path_buf()))
}

/// Builds the public view, taking the capability list from the grant.
///
/// The manifest is content; the grant is policy. A field the admin reads, and
/// that the host side will iterate when it wires execution, must never
/// advertise authority the grant does not carry — a plugin able to add
/// `wg.delete` to its own manifest would otherwise be *shown* as holding it.
fn installed_view(manifest: &Manifest, grant: &PluginGrant, root: PathBuf) -> Installed {
    Installed {
        plugin: InstalledPlugin {
            id: manifest.plugin.id.clone(),
            version: manifest.plugin.version.clone(),
            name: manifest.plugin.name.clone(),
            enabled: grant.enabled,
            capabilities: grant.allowed_ids(),
            api: manifest.api.clone(),
            ui: manifest.ui.clone(),
        },
        root,
    }
}

/// Confirms the module parses and implements this host's ABI.
fn verify_module(root: &Path) -> Result<(), PluginError> {
    let path = root.join(WASM_FILE);
    let bytes = fs::read(&path)
        .map_err(|error| io_error(format!("could not read `{}`: {error}", path.display())))?;
    runtime::probe(&bytes).map_err(|error| {
        PluginError::new(
            PluginErrorKind::Invalid,
            format!("`{WASM_FILE}` cannot be loaded: {}", error.detail),
        )
    })
}

/// Public form of [`verify_wasm`], for a caller that has a digest to check
/// before recording it.
pub fn verify(root: &Path, expected: &str) -> Result<(), PluginError> {
    verify_wasm(root, expected)
}

/// Confirms the module's bytes match `expected`.
///
/// The path is the fixed [`WASM_FILE`], not `manifest.wasm.path`: the path is
/// validated as staying inside the bundle, but loading through it would let a
/// rewritten manifest point the hash check at a different file.
fn verify_wasm(root: &Path, expected: &str) -> Result<(), PluginError> {
    let path = root.join(WASM_FILE);
    let bytes = fs::read(&path)
        .map_err(|error| io_error(format!("could not read `{}`: {error}", path.display())))?;
    let actual = hex(&Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(PluginError::new(
            PluginErrorKind::Tampered,
            format!("the module hashes to {actual}, the grant says {expected}"),
        ));
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The canonical directory for one plugin.
///
/// The id is validated here rather than at each call site because the result is
/// used for recursive deletion: `plugin remove ../victim` must not be able to
/// reach a sibling of the plugin root, and a future caller cannot forget a
/// check that lives inside the function it calls.
pub fn plugin_dir(plugins_root: &Path, id: &str) -> Result<PathBuf, PluginError> {
    crate::plugins::manifest::validate_id(id)?;
    Ok(plugins_root.join(id))
}

fn staging_dir(plugins_root: &Path) -> Result<PathBuf, PluginError> {
    fs::create_dir_all(plugins_root).map_err(|error| {
        io_error(format!(
            "could not create `{}`: {error}",
            plugins_root.display()
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
    let path = plugins_root.join(unique);
    fs::create_dir(&path)
        .map_err(|error| io_error(format!("could not stage the bundle: {error}")))?;
    Ok(path)
}

/// Extracts a bundle into `destination`, refusing anything unexpected first.
///
/// The whole archive is inspected before a byte is written: a bundle that names
/// an entry it may not contain is rejected as a unit. Writing as we went would
/// leave those earlier entries behind, and "installed with one file missing" is
/// a worse state than "not installed".
fn extract_bundle(bundle: &Path, destination: &Path) -> Result<(), PluginError> {
    let file = File::open(bundle)
        .map_err(|error| io_error(format!("could not open `{}`: {error}", bundle.display())))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| PluginError::invalid(format!("not a zip bundle: {error}")))?;

    if zip.len() > MAX_BUNDLE_ENTRIES {
        return Err(PluginError::invalid(format!(
            "the bundle has {} entries, at most {MAX_BUNDLE_ENTRIES} are allowed",
            zip.len()
        )));
    }

    let mut planned: Vec<(String, PathBuf, usize)> = Vec::with_capacity(zip.len());
    let mut total = 0u64;

    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|error| PluginError::invalid(format!("unreadable entry: {error}")))?;
        let raw = entry.name().to_string();

        if entry.is_dir() {
            // Directories are implied by their members; an explicit one is
            // allowed but never materialized on its own.
            continue;
        }

        let relative = crate::files::archive::safe_member(&raw)
            .ok_or_else(|| PluginError::denied(format!("`{raw}` is not a safe bundle path")))?;
        let name = relative.to_string_lossy().replace('\\', "/");

        if !is_allowed_member(&name) {
            return Err(PluginError::denied(format!(
                "`{name}` is not part of a bundle; a bundle holds `plugin.toml`, `plugin.wasm`, `plugin.sig` and `web/`"
            )));
        }
        if name == "plugin.toml" && entry.size() > MAX_MANIFEST_BYTES as u64 {
            return Err(PluginError::invalid(format!(
                "`plugin.toml` is larger than {MAX_MANIFEST_BYTES} bytes"
            )));
        }

        total = total.saturating_add(entry.size());
        if total > MAX_BUNDLE_BYTES {
            return Err(PluginError::invalid(format!(
                "the bundle expands past {MAX_BUNDLE_BYTES} bytes"
            )));
        }
        planned.push((name, relative, index));
    }

    for (name, relative, index) in planned {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| PluginError::invalid(format!("unreadable entry: {error}")))?;
        let target = destination.join(&relative);

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                io_error(format!("could not create `{}`: {error}", parent.display()))
            })?;
        }

        let mut out = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&target)
            .map_err(|error| match error.raw_os_error() {
                Some(libc::ELOOP) => {
                    PluginError::denied(format!("`{name}` would be written through a symlink"))
                }
                _ => io_error(format!("could not write `{name}`: {error}")),
            })?;

        // Copy with a ceiling rather than trusting the declared size: a zip can
        // lie about `size()`, and the count above would then be fantasy.
        let mut written = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = entry
                .read(&mut buffer)
                .map_err(|error| io_error(format!("could not read `{name}`: {error}")))?;
            if read == 0 {
                break;
            }
            written = written.saturating_add(read as u64);
            if written > MAX_BUNDLE_BYTES {
                return Err(PluginError::invalid(format!(
                    "`{name}` expands past {MAX_BUNDLE_BYTES} bytes"
                )));
            }
            out.write_all(&buffer[..read])
                .map_err(|error| io_error(format!("could not write `{name}`: {error}")))?;
        }
    }

    Ok(())
}

/// Collects the installed plugins under `plugins_root`, one directory per id.
///
/// A directory whose manifest cannot be read is reported rather than skipped:
/// silently hiding a broken plugin makes a missing feature look like a bug in
/// the panel.
pub fn list(plugins_root: &Path, grants: &[PluginGrant]) -> (Vec<InstalledPlugin>, Vec<String>) {
    let mut found = Vec::new();
    let mut problems = Vec::new();

    let entries = match fs::read_dir(plugins_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (found, problems),
        Err(error) => {
            problems.push(format!(
                "could not read `{}`: {error}",
                plugins_root.display()
            ));
            return (found, problems);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }

        match grants.iter().find(|grant| grant.id == name) {
            None => problems.push(format!("`{name}` has no grant and will not load")),
            Some(grant) => match load(&path, grant) {
                Ok(installed) => {
                    let mut plugin = installed.plugin;
                    plugin.enabled = grant.enabled;
                    found.push(plugin);
                }
                Err(error) => problems.push(format!("`{name}`: {error}")),
            },
        }
    }

    found.sort_by(|left, right| left.id.cmp(&right.id));
    (found, problems)
}

/// Removes one plugin directory. The caller drops the grant separately, since
/// the config is the source of truth for authority.
pub fn remove(plugins_root: &Path, id: &str) -> Result<(), PluginError> {
    let root = plugin_dir(plugins_root, id)?;
    match fs::remove_dir_all(&root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(PluginError::new(
            PluginErrorKind::Missing,
            format!("plugin `{id}` is not installed"),
        )),
        Err(error) => Err(io_error(format!(
            "could not remove `{}`: {error}",
            root.display()
        ))),
    }
}

/// Reads a grant list keyed by id, which is how the config stores them.
pub fn grants_by_id(grants: &[PluginGrant]) -> BTreeMap<&str, &PluginGrant> {
    grants
        .iter()
        .map(|grant| (grant.id.as_str(), grant))
        .collect()
}

#[cfg(test)]
mod tests;
