//! Extension bundles: extraction, signatures, hashes and installation.
//!
//! A bundle is a zip with `manifest.toml`, `wasm/plugin.wasm`, an optional
//! `manifest.sig` and assets under `web/`. Installation is the only moment a
//! extension gains authority, so every check runs before the directory becomes
//! visible under its id:
//!
//! 1. the archive is read with an allowlist, size and entry caps, refusing
//!    symlinks and duplicate names;
//! 2. the detached Ed25519 signature is checked over the raw manifest bytes;
//! 3. the manifest is parsed and validated;
//! 4. the module and every declared web asset are hashed;
//! 5. a changed capability surface needs an explicit approval.
//!
//! A failure removes the staging directory; a refused bundle never becomes a
//! half-installed extension for the next start to find.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

use anth_wire::WireError;
use sha2::{Digest as _, Sha256};

use crate::apps::grants::{AppGrant, GrantStore};
use crate::apps::manifest::{
    AppManifest as Manifest, BIN_MANIFEST_FILE, MANIFEST_FILE, MANIFEST_FILES, SIGNATURE_FILE,
    WASM_FILE, WASM_MANIFEST_FILE, WEB_MANIFEST_FILE, manifest_message,
};
use crate::apps::signature;

pub const MAX_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_BUNDLE_ENTRIES: usize = 512;
pub const MAX_ASSET_BYTES: u64 = 4 * 1024 * 1024;

/// What an install produced.
#[derive(Debug, Clone)]
pub struct BundleInstall {
    pub grant: AppGrant,
    pub manifest: Manifest,
    pub warning: Option<String>,
}

/// A verified bundle loaded from disk.
#[derive(Debug, Clone)]
pub struct LoadedBundle {
    pub manifest: Manifest,
    pub wasm: Option<Vec<u8>>,
    pub dir: PathBuf,
}

impl LoadedBundle {
    pub fn wasm_path(&self) -> PathBuf {
        self.dir.join(WASM_FILE)
    }
}

fn staging_root(root: &Path) -> Result<PathBuf, WireError> {
    fs::create_dir_all(root).map_err(|error| {
        WireError::internal(format!(
            "creating the extension directory {}: {error}",
            root.display()
        ))
    })?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = root.join(format!(".staging-{}-{nanos}", std::process::id()));
    fs::create_dir(&path)
        .map_err(|error| WireError::internal(format!("staging in {}: {error}", root.display())))?;
    Ok(path)
}

fn aside_path(root: &Path, label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    root.join(format!(".{label}-{}-{nanos}", std::process::id()))
}

fn is_allowed_member(name: &str) -> bool {
    name == MANIFEST_FILE
        || name == WASM_MANIFEST_FILE
        || name == WEB_MANIFEST_FILE
        || name == BIN_MANIFEST_FILE
        || name == WASM_FILE
        || name == SIGNATURE_FILE
        || (name.starts_with("web/")
            && (name.ends_with(".js") || name.ends_with(".mjs") || name.ends_with(".css")))
}

fn safe_member(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\\') {
        return None;
    }
    let mut relative = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => return None,
        }
    }
    (!relative.as_os_str().is_empty()).then_some(relative)
}

fn is_symlink(mode: Option<u32>) -> bool {
    mode.is_some_and(|mode| mode & 0o170_000 == 0o120_000)
}

fn extract<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    staging: &Path,
) -> Result<(), WireError> {
    if archive.len() > MAX_BUNDLE_ENTRIES {
        return Err(WireError::too_large(format!(
            "the bundle holds more than {MAX_BUNDLE_ENTRIES} entries"
        )));
    }
    let mut seen = HashSet::new();
    let mut written: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| WireError::invalid(format!("the bundle is corrupt: {error}")))?;
        let raw = entry.name().to_string();
        if raw.ends_with('/') {
            continue;
        }
        let relative = safe_member(&raw).ok_or_else(|| {
            WireError::forbidden(format!("`{raw}` is not a path inside the bundle"))
        })?;
        let name = relative.to_string_lossy().replace('\\', "/");
        if !is_allowed_member(&name) {
            return Err(WireError::forbidden(format!(
                "`{name}` is not part of a bundle; a bundle holds `{MANIFEST_FILE}`, `{WASM_FILE}`, `{SIGNATURE_FILE}` and `web/*.js|mjs|css`"
            )));
        }
        if is_symlink(entry.unix_mode()) {
            return Err(WireError::forbidden(format!(
                "`{name}` is a symlink; a bundle holds regular files"
            )));
        }
        if !seen.insert(relative.clone()) {
            return Err(WireError::invalid(format!(
                "`{name}` appears twice in the bundle"
            )));
        }
        let remaining = MAX_BUNDLE_BYTES.saturating_sub(written);
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(remaining + 1)
            .read_to_end(&mut data)
            .map_err(|error| WireError::invalid(format!("reading `{name}`: {error}")))?;
        written = written.saturating_add(data.len() as u64);
        if written > MAX_BUNDLE_BYTES {
            return Err(WireError::too_large(format!(
                "the bundle expands past {MAX_BUNDLE_BYTES} bytes"
            )));
        }
        let target = staging.join(&relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                WireError::internal(format!("creating {}: {error}", parent.display()))
            })?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&target)
            .map_err(|error| {
                WireError::internal(format!("writing {}: {error}", target.display()))
            })?;
        file.write_all(&data).map_err(|error| {
            WireError::internal(format!("writing {}: {error}", target.display()))
        })?;
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    signature::encode_hex(&Sha256::digest(bytes))
}

fn read_manifest_files(staging: &Path) -> Result<BTreeMap<String, Vec<u8>>, WireError> {
    let mut files = BTreeMap::new();
    for name in MANIFEST_FILES {
        let path = staging.join(name);
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound && name == MANIFEST_FILE => {
                return Err(WireError::invalid(format!(
                    "the bundle has no `{MANIFEST_FILE}`"
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(WireError::internal(format!(
                    "reading {}: {error}",
                    path.display()
                )));
            }
        };
        if bytes.len() > crate::apps::manifest::MAX_MANIFEST_BYTES {
            return Err(WireError::too_large(format!(
                "`{name}` is larger than {} bytes",
                crate::apps::manifest::MAX_MANIFEST_BYTES
            )));
        }
        files.insert(name.to_string(), bytes);
    }
    Ok(files)
}

/// Reads and checks the detached signature.
///
/// Returns the signature hex to freeze in the grant. A present signature is
/// always verified; the switch only decides whether a missing one is fatal.
fn verify_signature(
    staging: &Path,
    manifest: &[u8],
    require_signature: bool,
    public_key: &str,
) -> Result<Option<String>, WireError> {
    let path = staging.join(SIGNATURE_FILE);
    let signature = match fs::read_to_string(&path) {
        Ok(text) => Some(text.trim().to_string()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(WireError::internal(format!(
                "reading {}: {error}",
                path.display()
            )));
        }
    };
    match signature {
        Some(hex) => {
            if public_key.trim().is_empty() {
                return Err(WireError::forbidden(
                    "the bundle is signed but `[extensions] public_key` is empty",
                ));
            }
            signature::verify(public_key, &hex, manifest).map_err(WireError::forbidden)?;
            Ok(Some(hex))
        }
        None if require_signature => Err(WireError::forbidden(
            "the bundle is not signed; add `manifest.sig` or set `[extensions] require_signature = false`",
        )),
        None => Ok(None),
    }
}

fn verify_hash(path: &Path, expected: &str, what: &str) -> Result<Vec<u8>, WireError> {
    let bytes = fs::read(path).map_err(|error| {
        WireError::invalid(format!("reading {what} {}: {error}", path.display()))
    })?;
    let actual = sha256(&bytes);
    if actual != expected.to_lowercase() {
        return Err(WireError::forbidden(format!(
            "the {what} digest is {actual}, the manifest approves {expected}"
        )));
    }
    Ok(bytes)
}

fn verify_web_assets(staging: &Path, manifest: &Manifest) -> Result<(), WireError> {
    for (module, digest) in manifest.web_modules() {
        let relative = safe_member(&module)
            .ok_or_else(|| WireError::invalid(format!("the UI module `{module}` is not safe")))?;
        let path = staging.join(&relative);
        let metadata = fs::metadata(&path).map_err(|error| {
            WireError::invalid(format!("the UI module `{module}` is missing: {error}"))
        })?;
        if !metadata.is_file() || metadata.len() > MAX_ASSET_BYTES {
            return Err(WireError::invalid(format!(
                "the UI module `{module}` is not a regular file under {MAX_ASSET_BYTES} bytes"
            )));
        }
        verify_hash(&path, &digest, "UI module")?;
    }
    Ok(())
}

/// Installs one archive into `root`, writing its grant to `grants`.
pub fn install_reader<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    root: &Path,
    grants: &GrantStore,
    require_signature: bool,
    public_key: &str,
    accept_capabilities: bool,
) -> Result<BundleInstall, WireError> {
    let staging = staging_root(root)?;
    let outcome = (|| {
        extract(archive, &staging)?;
        let manifest_files = read_manifest_files(&staging)?;
        let manifest_bytes = manifest_message(&manifest_files);
        let signature = verify_signature(&staging, &manifest_bytes, require_signature, public_key)?;
        let manifest = Manifest::parse_files(&manifest_files)?;
        let manifest_sha256 = sha256(&manifest_bytes);
        if let Some(wasm) = &manifest.wasm {
            verify_hash(&staging.join(WASM_FILE), &wasm.sha256, "module")?;
        }
        verify_web_assets(&staging, &manifest)?;

        let existing = grants.read(&manifest.app.id)?;
        if let Some(existing) = &existing
            && !existing.approves(&manifest)
            && !accept_capabilities
        {
            return Err(WireError::new(
                anth_wire::ErrorKind::Conflict,
                "app_capabilities_changed",
                "the manifest changes the approved capabilities; run `extension approve --accept-capabilities` to accept them",
            ));
        }
        let mut grant = AppGrant::from_manifest(&manifest, manifest_sha256, signature.clone());
        if let Some(existing) = &existing {
            grant.enabled = existing.enabled;
        }
        Ok((manifest, grant, signature))
    })();

    let (manifest, grant, signature) = match outcome {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let id = manifest.app.id.clone();
    let destination = root.join(&id);
    let aside = aside_path(root, "old");
    let had_existing = destination.exists();
    if had_existing {
        fs::rename(&destination, &aside).map_err(|error| {
            let _ = fs::remove_dir_all(&staging);
            WireError::internal(format!("replacing extension `{id}`: {error}"))
        })?;
    }
    if let Err(error) = fs::rename(&staging, &destination) {
        if had_existing {
            let _ = fs::rename(&aside, &destination);
        }
        let _ = fs::remove_dir_all(&staging);
        return Err(WireError::internal(format!(
            "installing extension `{id}`: {error}"
        )));
    }
    if had_existing {
        let _ = fs::remove_dir_all(&aside);
    }
    if let Err(error) = grants.write(&grant) {
        let _ = fs::remove_dir_all(&destination);
        if had_existing {
            let _ = fs::rename(&aside, &destination);
        }
        return Err(error);
    }
    let warning = signature.is_none().then(|| {
        "installed without a signature because `[extensions] require_signature = false`".to_string()
    });
    Ok(BundleInstall {
        grant,
        manifest,
        warning,
    })
}

pub fn install_file(
    bundle: &Path,
    root: &Path,
    grants: &GrantStore,
    require_signature: bool,
    public_key: &str,
    accept_capabilities: bool,
) -> Result<BundleInstall, WireError> {
    let file = fs::File::open(bundle).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            WireError::not_found(format!("no bundle at {}", bundle.display()))
        } else {
            WireError::internal(format!("opening {}: {error}", bundle.display()))
        }
    })?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| {
        WireError::invalid(format!("{} is not a zip bundle: {error}", bundle.display()))
    })?;
    install_reader(
        &mut archive,
        root,
        grants,
        require_signature,
        public_key,
        accept_capabilities,
    )
}

/// Loads and re-verifies one installed bundle against its grant.
///
/// This runs on every listing and every anchor call. The signature and hashes
/// answer two different questions: the signature proves who approved the
/// manifest, the hashes prove the files still match what that manifest names.
pub fn load(
    root: &Path,
    grant: &AppGrant,
    require_signature: bool,
    public_key: &str,
) -> Result<LoadedBundle, WireError> {
    let dir = root.join(&grant.id);
    let manifest_files = read_manifest_files(&dir)?;
    let manifest_bytes = manifest_message(&manifest_files);
    if sha256(&manifest_bytes) != grant.manifest_sha256.to_lowercase() {
        return Err(WireError::forbidden(format!(
            "extension `{}` has a rewritten manifest; reinstall it to approve the new bytes",
            grant.id
        )));
    }
    match grant.manifest_signature.as_deref() {
        Some(signature) => {
            if public_key.trim().is_empty() {
                return Err(WireError::forbidden(
                    "the grant is signed but `[extensions] public_key` is empty",
                ));
            }
            signature::verify(public_key, signature, &manifest_bytes)
                .map_err(WireError::forbidden)?;
        }
        None if require_signature => {
            return Err(WireError::forbidden(
                "the grant has no signature and `[extensions] require_signature` is on",
            ));
        }
        None => {}
    }
    let manifest = Manifest::parse_files(&manifest_files)?;
    if !grant.approves(&manifest) {
        return Err(WireError::forbidden(format!(
            "extension `{}` declares a capability surface the grant does not approve; run `extension approve --accept-capabilities`",
            grant.id
        )));
    }
    let wasm = match (manifest.wasm.as_ref(), grant.wasm_sha256.as_deref()) {
        (Some(wasm), Some(grant_hash)) => {
            if wasm.sha256.to_lowercase() != grant_hash.to_lowercase() {
                return Err(WireError::forbidden(format!(
                    "extension `{}` module digest does not match the grant",
                    grant.id
                )));
            }
            Some(verify_hash(&dir.join(WASM_FILE), grant_hash, "module")?)
        }
        (None, None) => None,
        _ => {
            return Err(WireError::forbidden(format!(
                "extension `{}` runtime and grant disagree about the wasm module",
                grant.id
            )));
        }
    };
    verify_web_assets(&dir, &manifest)?;
    Ok(LoadedBundle {
        manifest,
        wasm,
        dir,
    })
}

/// Rereads a bundle and replaces its grant with the approved surface.
pub fn approve(
    root: &Path,
    grants: &GrantStore,
    id: &str,
    require_signature: bool,
    public_key: &str,
) -> Result<AppGrant, WireError> {
    let existing = grants.read(id)?;
    let dir = root.join(id);
    if !dir.is_dir() {
        return Err(WireError::not_found(format!(
            "extension `{id}` is not installed"
        )));
    }
    let manifest_files = read_manifest_files(&dir)?;
    let manifest_bytes = manifest_message(&manifest_files);
    let signature = verify_signature(&dir, &manifest_bytes, require_signature, public_key)?;
    let manifest = Manifest::parse_files(&manifest_files)?;
    if let Some(wasm) = &manifest.wasm {
        verify_hash(&dir.join(WASM_FILE), &wasm.sha256, "module")?;
    }
    verify_web_assets(&dir, &manifest)?;
    let mut grant = AppGrant::from_manifest(&manifest, sha256(&manifest_bytes), signature);
    grant.enabled = existing.map(|grant| grant.enabled).unwrap_or(true);
    grants.write(&grant)?;
    Ok(grant)
}

pub fn remove(root: &Path, grants: &GrantStore, id: &str) -> Result<(), WireError> {
    crate::apps::manifest::validate_id(id)?;
    let dir = root.join(id);
    let grant = grants.read(id)?;
    if dir.exists() {
        fs::remove_dir_all(&dir)
            .map_err(|error| WireError::internal(format!("removing {}: {error}", dir.display())))?;
    } else if grant.is_none() {
        return Err(WireError::not_found(format!(
            "extension `{id}` is not installed"
        )));
    }
    grants.remove(id)?;
    Ok(())
}

/// Every installed directory, paired with its grant when readable.
pub fn installed(
    root: &Path,
    grants: &GrantStore,
) -> Result<Vec<(String, Option<AppGrant>)>, WireError> {
    let by_id: BTreeMap<String, AppGrant> = grants
        .load_all()?
        .into_iter()
        .map(|grant| (grant.id.clone(), grant))
        .collect();
    let mut found = BTreeMap::new();
    match fs::read_dir(root) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|error| {
                    WireError::internal(format!("reading {}: {error}", root.display()))
                })?;
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                found.insert(name.to_string(), by_id.get(name).cloned());
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(WireError::internal(format!(
                "reading {}: {error}",
                root.display()
            )));
        }
    }
    for (id, grant) in by_id {
        found.entry(id).or_insert(Some(grant));
    }
    Ok(found.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::app_host::AppService;
    use crate::config::AppsConfig;

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = PathBuf::from("/tmp/anthracite-panel/tests")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root).expect("scratch");
        root
    }

    pub(super) fn key() -> SigningKey {
        SigningKey::from_bytes(&[9u8; 32])
    }

    pub(super) fn sign_manifest(text: &str) -> String {
        let mut files = BTreeMap::new();
        files.insert(MANIFEST_FILE.to_string(), text.as_bytes().to_vec());
        signature::encode_hex(&key().sign(&manifest_message(&files)).to_bytes())
    }

    fn config(require_signature: bool) -> AppsConfig {
        AppsConfig {
            enabled: true,
            require_signature,
            public_key: signature::encode_hex(key().verifying_key().as_bytes()),
            ..AppsConfig::default()
        }
    }

    fn manifest(wasm: &[u8], capability: &str) -> String {
        format!(
            r#"
manifest_version = 2
[app]
id = "org.anth.hello"
alias = "hello"
version = "0.3.0"
name = "Hello"
[runtime]
kind = "wasm"

[wasm]
path = "wasm/plugin.wasm"
abi = 3
sha256 = "{}"

[[capability]]
id = "{capability}"
program = "hostname"

[[api]]
method = "GET"
path = "/status"
"#,
            sha256(wasm)
        )
    }

    pub(super) fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut bytes);
            let options = SimpleFileOptions::default();
            for (name, data) in entries {
                zip.start_file(*name, options).expect("entry");
                zip.write_all(data).expect("write");
            }
            zip.finish().expect("finish");
        }
        bytes.into_inner()
    }

    fn signed_bundle(sign: bool, capability: &str) -> Vec<u8> {
        let wasm = b"\0asm\x01\0\0\0";
        let manifest = manifest(wasm, capability);
        let signature = sign.then(|| sign_manifest(&manifest));
        let mut entries: Vec<(&str, &[u8])> = vec![
            ("manifest.toml", manifest.as_bytes()),
            ("wasm/plugin.wasm", wasm),
        ];
        let signature_bytes;
        if let Some(signature) = &signature {
            signature_bytes = signature.as_bytes();
            entries.push(("manifest.sig", signature_bytes));
        }
        zip(&entries)
    }

    pub(super) fn make_service(name: &str, require_signature: bool) -> (AppService, PathBuf) {
        let root = scratch(name);
        let service = AppService::new(
            config(require_signature),
            root.join("extensions"),
            root.join("extensions.d"),
            std::sync::Arc::new(crate::app_host::exec::DemoExecRunner::new()),
        );
        (service, root)
    }

    #[test]
    fn a_signed_bundle_installs_and_lists_as_enabled() {
        let (service, root) = make_service("extension-signed", true);
        let installed = service
            .install_bytes(&signed_bundle(true, "host.hostname"), false)
            .expect("install");
        assert!(installed.warning.is_none());
        assert_eq!(installed.grant.id, "org.anth.hello");
        assert!(installed.grant.enabled);
        assert!(
            root.join("extensions/org.anth.hello/manifest.toml")
                .is_file()
        );
        assert!(root.join("extensions.d/org.anth.hello.toml").is_file());

        let view = service.snapshot().expect("snapshot");
        assert!(view.enabled);
        assert_eq!(view.apps.len(), 1);
        assert_eq!(view.apps[0].state, "enabled");
        assert_eq!(view.apps[0].alias.as_deref(), Some("hello"));
        assert_eq!(view.apps[0].capabilities, ["host.hostname"]);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_missing_or_wrong_signature_is_refused() {
        let (service, root) = make_service("extension-unsigned", true);
        let error = service
            .install_bytes(&signed_bundle(false, "host.hostname"), false)
            .expect_err("unsigned");
        assert_eq!(error.kind, anth_wire::ErrorKind::Forbidden);
        assert!(error.message.contains("not signed"));
        assert!(!root.join("extensions/org.anth.hello").exists());
        fs::remove_dir_all(root).expect("cleanup");

        let (service, root) = make_service("extension-badsig", true);
        let wasm = b"\0asm\x01\0\0\0";
        let manifest = manifest(wasm, "host.hostname");
        let other = SigningKey::from_bytes(&[8u8; 32]);
        let signature = signature::encode_hex(
            &other
                .sign(&{
                    let mut files = BTreeMap::new();
                    files.insert(MANIFEST_FILE.to_string(), manifest.as_bytes().to_vec());
                    manifest_message(&files)
                })
                .to_bytes(),
        );
        let bundle = zip(&[
            ("manifest.toml", manifest.as_bytes()),
            ("wasm/plugin.wasm", wasm),
            ("manifest.sig", signature.as_bytes()),
        ]);
        assert!(service.install_bytes(&bundle, false).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn an_unsigned_bundle_is_allowed_only_when_the_switch_is_off() {
        let (service, root) = make_service("extension-unsigned-off", false);
        let installed = service
            .install_bytes(&signed_bundle(false, "host.hostname"), false)
            .expect("install");
        assert!(installed.warning.is_some());
        assert_eq!(
            service.snapshot().expect("snapshot").apps[0].state,
            "enabled"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_tampered_module_or_asset_is_refused() {
        let wasm = b"\0asm\x01\0\0\0";
        let manifest = manifest(wasm, "host.hostname");
        let signature = sign_manifest(&manifest);
        let bundle = zip(&[
            ("manifest.toml", manifest.as_bytes()),
            ("wasm/plugin.wasm", b"not wasm"),
            ("manifest.sig", signature.as_bytes()),
        ]);
        let (service, root) = make_service("extension-tampered", true);
        let error = service.install_bytes(&bundle, false).expect_err("tampered");
        assert!(error.message.contains("digest"), "{error}");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_traversal_or_unexpected_member_is_refused() {
        let (service, root) = make_service("extension-traversal", true);
        let bundle = zip(&[
            ("manifest.toml", b"x"),
            ("../escape.wasm", b"x"),
            ("manifest.sig", b"x"),
        ]);
        assert!(service.install_bytes(&bundle, false).is_err());

        let bundle = zip(&[("manifest.toml", b"x"), ("run.sh", b"x")]);
        let error = service.install_bytes(&bundle, false).expect_err("member");
        assert!(error.message.contains("not part of a bundle"), "{error}");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_changed_capability_surface_needs_approval() {
        let (service, root) = make_service("extension-approve", true);
        service
            .install_bytes(&signed_bundle(true, "host.hostname"), false)
            .expect("first install");
        let error = service
            .install_bytes(&signed_bundle(true, "host.uname"), false)
            .expect_err("changed");
        assert_eq!(error.kind, anth_wire::ErrorKind::Conflict);
        assert_eq!(error.code, "app_capabilities_changed");

        let accepted = service
            .install_bytes(&signed_bundle(true, "host.uname"), true)
            .expect("accepted");
        assert_eq!(accepted.grant.capability[0].id, "host.uname");
        assert_eq!(
            service.snapshot().expect("snapshot").apps[0].capabilities,
            ["host.uname"]
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rewriting_the_bundle_after_install_is_reported_as_an_error() {
        let (service, root) = make_service("extension-rewrite", true);
        service
            .install_bytes(&signed_bundle(true, "host.hostname"), false)
            .expect("install");
        fs::write(
            root.join("extensions/org.anth.hello/wasm/plugin.wasm"),
            b"different",
        )
        .expect("rewrite");
        let view = service.snapshot().expect("snapshot");
        assert_eq!(view.apps[0].state, "error");
        assert!(
            view.apps[0]
                .error
                .as_deref()
                .is_some_and(|error| error.contains("digest"))
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_bundle_without_a_grant_can_be_approved() {
        let (service, root) = make_service("extension-unapproved", true);
        service
            .install_bytes(&signed_bundle(true, "host.hostname"), false)
            .expect("install");
        fs::remove_file(root.join("extensions.d/org.anth.hello.toml")).expect("remove grant");
        assert_eq!(
            service.snapshot().expect("snapshot").apps[0].state,
            "unapproved"
        );
        service.approve("org.anth.hello", true).expect("approve");
        assert_eq!(
            service.snapshot().expect("snapshot").apps[0].state,
            "enabled"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn enable_disable_and_remove_update_the_catalogue() {
        let (service, root) = make_service("extension-toggle", true);
        service
            .install_bytes(&signed_bundle(true, "host.hostname"), false)
            .expect("install");
        service
            .set_enabled("org.anth.hello", false)
            .expect("disable");
        assert_eq!(
            service.snapshot().expect("snapshot").apps[0].state,
            "disabled"
        );
        service.set_enabled("org.anth.hello", true).expect("enable");
        assert_eq!(
            service.snapshot().expect("snapshot").apps[0].state,
            "enabled"
        );

        service.remove("org.anth.hello").expect("remove");
        assert!(service.snapshot().expect("snapshot").apps.is_empty());
        fs::remove_dir_all(root).expect("cleanup");
    }
}

#[cfg(test)]
mod proptests {
    use super::safe_member;
    use proptest::prelude::*;
    use std::path::{Component, Path};

    fn text() -> impl Strategy<Value = String> {
        proptest::collection::vec(any::<char>(), 0..256)
            .prop_map(|chars| chars.into_iter().collect())
    }

    proptest! {
        #[test]
        fn an_accepted_member_never_leaves_the_bundle(raw in text()) {
            if let Some(relative) = safe_member(&raw) {
                prop_assert!(!relative.is_absolute());
                prop_assert!(relative
                    .components()
                    .all(|component| matches!(component, Component::Normal(_))));
                prop_assert!(Path::new("/bundle").join(&relative).starts_with("/bundle"));
                prop_assert!(!raw.contains('\\'));
            }
        }
    }
}

#[cfg(test)]
mod bundle_more_tests {
    use super::tests::*;
    use super::*;
    use ed25519_dalek::Signer;

    fn sign_files(files: &BTreeMap<String, Vec<u8>>) -> String {
        signature::encode_hex(&key().sign(&manifest_message(files)).to_bytes())
    }

    #[test]
    fn a_service_bundle_installs_without_a_wasm_module() {
        let (service, root) = make_service("extension-service", true);
        let manifest = r#"
manifest_version = 2
[app]
id = "org.anth.service"
version = "0.4.0"
name = "Service"
[runtime]
kind = "bin"
[bin]
socket = "/run/anth/service.sock"
[[api]]
method = "GET"
path = "/status"
"#;
        let signature = super::tests::sign_manifest(manifest);
        let bundle = zip(&[
            ("manifest.toml", manifest.as_bytes()),
            ("manifest.sig", signature.as_bytes()),
        ]);
        let installed = service.install_bytes(&bundle, false).expect("install");
        assert_eq!(
            installed.grant.bin_socket.as_deref(),
            Some("/run/anth/service.sock")
        );
        assert!(installed.grant.wasm_sha256.is_none());
        let loaded = service.load("org.anth.service").expect("load");
        assert!(loaded.wasm.is_none());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn sidecar_manifest_files_are_signed_and_merged() {
        let (service, root) = make_service("extension-split", true);
        let wasm = b"\0asm\x01\0\0\0";
        let core = format!(
            r#"
manifest_version = 2
[app]
id = "org.anth.split"
version = "0.4.0"
name = "Split"
[runtime]
kind = "wasm"
[wasm]
path = "wasm/plugin.wasm"
abi = 3
sha256 = "{}"
"#,
            sha256(wasm)
        );
        let capabilities =
            b"[[capability]]\nid = \"host.hostname\"\nprogram = \"hostname\"\n".to_vec();
        let asset = b"// split\n";
        let interface = format!(
            "[[api]]\nmethod = \"GET\"\npath = \"/status\"\n[[ui]]\nslot = \"settings.extension\"\ntag = \"anth-split\"\nmodule = \"split.js\"\nsha256 = \"{}\"\n",
            sha256(asset)
        )
        .into_bytes();
        let mut files = BTreeMap::new();
        files.insert(MANIFEST_FILE.to_string(), core.as_bytes().to_vec());
        files.insert(WASM_MANIFEST_FILE.to_string(), capabilities);
        files.insert(WEB_MANIFEST_FILE.to_string(), interface);
        let signature = sign_files(&files);

        let bundle = zip(&[
            ("manifest.toml", core.as_bytes()),
            (
                "wasm/manifest.toml",
                files.get(WASM_MANIFEST_FILE).expect("capabilities"),
            ),
            (
                "web/manifest.toml",
                files.get(WEB_MANIFEST_FILE).expect("interface"),
            ),
            ("wasm/plugin.wasm", wasm),
            ("web/split.js", asset),
            ("manifest.sig", signature.as_bytes()),
        ]);
        let installed = service.install_bytes(&bundle, false).expect("install");
        assert_eq!(installed.manifest.capability.len(), 1);
        assert_eq!(installed.manifest.api.len(), 1);
        assert_eq!(installed.manifest.ui.len(), 1);
        let loaded = service.load("org.anth.split").expect("load");
        assert_eq!(loaded.manifest.ui.len(), 1);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
