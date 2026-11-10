//! Reading an app bundle: zip safety, the closed layout, hashes, signature and
//! the WIT package (§7.1–§7.3).
//!
//! Verification happens in a staging directory under `state/apps` and the
//! caller decides when it becomes visible. Nothing here grants authority: a
//! verified bundle is an installed candidate, still without a grant.

use std::collections::BTreeSet;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io::{Cursor, Read};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anth_contract::manifest::parse_call;
use anth_contract::{catalog, signature, Error, InterfaceId, Manifest};
use sha2::{Digest, Sha256};
use wit_parser::{Type, TypeDefKind, UnresolvedPackage, UnresolvedPackageGroup};

use crate::keys::Keyring;
use crate::store;

/// A bundle that unpacks to more entries than this is refused before reading.
const MAX_FILES: usize = 512;
/// Per-file uncompressed cap, and the total is the same as the body cap.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

static STAGING: AtomicU64 = AtomicU64::new(0);

/// What the caller allows for this install.
#[derive(Debug, Clone)]
pub struct Options {
    /// `apps.max-bundle-bytes`: the compressed body and the uncompressed total.
    pub max_bundle_bytes: u64,
    /// `apps.require-signature`.
    pub require_signature: bool,
    /// `services.allow-third-party`: a third-party bundle with `[service]` is
    /// refused at install unless the deployment opted in (D-Service-Third-Party).
    pub allow_third_party_service: bool,
    /// Packages the bundle may import besides the native set: the installed
    /// providers' `namespace:package` names.
    pub known_packages: Vec<String>,
}

#[derive(Debug)]
pub struct StagedBundle {
    root: PathBuf,
    manifest: Manifest,
    signer: Option<String>,
    keep: bool,
}

impl StagedBundle {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The fingerprint that signed, or `None` for an accepted unsigned bundle.
    pub fn signer(&self) -> Option<&str> {
        self.signer.as_deref()
    }

    /// Hands the directory to the caller; `Drop` will no longer remove it.
    pub fn into_root(mut self) -> PathBuf {
        self.keep = true;
        self.root.clone()
    }
}

impl Drop for StagedBundle {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

/// Unpacks and verifies `bundle` into a staging directory under `staging_parent`.
pub fn extract(
    bundle: &[u8],
    staging_parent: &Path,
    options: &Options,
    keyring: &Keyring,
) -> Result<StagedBundle, Error> {
    if bundle.len() as u64 > options.max_bundle_bytes {
        return Err(Error::too_large(format!(
            "the bundle is {} bytes; the limit is {}",
            bundle.len(),
            options.max_bundle_bytes
        )));
    }
    store::ensure_dir(staging_parent)?;
    // A crashed install can leave `.bundle-*` behind, and a recycled pid could
    // collide with the next attempt: retry with a fresh name instead of failing.
    let mut root = None;
    for _ in 0..100 {
        let candidate = staging_parent.join(format!(
            ".bundle-{}-{}",
            std::process::id(),
            STAGING.fetch_add(1, Ordering::Relaxed)
        ));
        match DirBuilder::new().mode(store::DIR_MODE).create(&candidate) {
            Ok(()) => {
                root = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(Error::internal(format!(
                    "creating {}: {error}",
                    candidate.display()
                )));
            }
        }
    }
    let root = root.ok_or_else(|| {
        Error::internal(format!(
            "could not create a staging directory under {}",
            staging_parent.display()
        ))
    })?;
    match verify(&root, bundle, options, keyring) {
        Ok(staged) => Ok(staged),
        Err(error) => {
            let _ = fs::remove_dir_all(&root);
            Err(error)
        }
    }
}

fn verify(
    root: &Path,
    bundle: &[u8],
    options: &Options,
    keyring: &Keyring,
) -> Result<StagedBundle, Error> {
    unpack(root, bundle, options.max_bundle_bytes)?;

    let manifest_text = read_required(root, "manifest.toml")?;
    let text = std::str::from_utf8(&manifest_text)
        .map_err(|error| Error::invalid(format!("`manifest.toml` is not UTF-8: {error}")))?;
    let manifest = Manifest::parse(text)?;
    manifest.validate()?;

    check_layout(root)?;
    check_wit(root, &manifest, &options.known_packages)?;
    check_hashes(root, &manifest)?;

    let signer = match store::read(&root.join("manifest.sig"))? {
        Some(text) => {
            let message = signed_message(root)?;
            Some(keyring.verify(&message, &text)?)
        }
        None if options.require_signature => {
            return Err(Error::denied(
                "the bundle is not signed and this engine requires a signature",
            ));
        }
        None => None,
    };

    sync_tree(root)?;

    Ok(StagedBundle {
        root: root.to_path_buf(),
        manifest,
        signer,
        keep: false,
    })
}

fn unpack(root: &Path, bundle: &[u8], max_total: u64) -> Result<(), Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bundle))
        .map_err(|error| Error::invalid(format!("the bundle is not a zip: {error}")))?;
    if archive.len() > MAX_FILES {
        return Err(Error::too_large(format!(
            "the bundle has {} entries; the limit is {MAX_FILES}",
            archive.len()
        )));
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
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
        // Zip directory entries carry a trailing slash; the path is the same as
        // the name without it, so normalize before the checks and the dedup.
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
            create_dir_private(&path)?;
            continue;
        }
        if let Some(parent) = path.parent() {
            create_dir_private(parent)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(store::FILE_MODE)
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

/// The layout is closed: the manifest, its signature, WIT sources, the
/// component and the UI modules. Anything else is a payload nobody declared.
fn check_layout(root: &Path) -> Result<(), Error> {
    for (relative, is_file) in walk(root)? {
        if !is_file {
            continue;
        }
        if relative == "descriptor.toml" {
            return Err(Error::invalid(
                "the bundle must not carry `descriptor.toml`; the engine writes it",
            ));
        }
        let ok = relative == "manifest.toml"
            || relative == "manifest.sig"
            || relative.strip_prefix("wit/").is_some_and(|name| {
                !name.is_empty() && !name.contains('/') && name.ends_with(".wit")
            })
            || (relative.starts_with("wasm/")
                && !relative["wasm/".len()..].contains('/')
                && relative.ends_with(".wasm"))
            || (relative.starts_with("bin/")
                && !relative["bin/".len()..].contains('/')
                && !relative["bin/".len()..].starts_with('.'))
            || (relative.starts_with("web/")
                && !relative["web/".len()..].contains('/')
                && (relative.ends_with(".js")
                    || relative.ends_with(".mjs")
                    || relative.ends_with(".css")));
        if !ok {
            return Err(Error::invalid(format!(
                "`{relative}` is not part of the bundle layout"
            )));
        }
    }
    Ok(())
}

/// One package, one world, an identity that matches the manifest, and only
/// imports the engine can actually resolve.
fn check_wit(root: &Path, manifest: &Manifest, known: &[String]) -> Result<(), Error> {
    let wit = root.join("wit");
    if !wit.is_dir() {
        return Err(Error::invalid("the bundle has no `wit/` directory"));
    }
    let group = UnresolvedPackageGroup::parse_dir(&wit)
        .map_err(|error| Error::invalid(format!("the WIT package is not valid: {error:#}")))?;
    if !group.nested.is_empty() {
        return Err(Error::invalid(
            "`wit/` must hold exactly one package, not nested packages",
        ));
    }
    let package = &group.main;
    if package.worlds.len() != 1 {
        return Err(Error::invalid(format!(
            "`wit/` must declare exactly one world, and it declares {}",
            package.worlds.len()
        )));
    }
    let (namespace, name) = manifest
        .app
        .namespace
        .split_once(':')
        .ok_or_else(|| Error::invalid("the manifest namespace is not `namespace:package`"))?;
    if package.name.namespace != namespace || package.name.name != name {
        return Err(Error::invalid(format!(
            "the WIT package is `{}`, and the manifest says `{}`",
            package.name, manifest.app.namespace
        )));
    }
    // WIT has no per-interface version: the package version is the version of
    // the interface set, and every exported id carries it. The app's own
    // version is independent (D-Interface-Versioning).
    let package_version = package.name.version.clone().ok_or_else(|| {
        Error::invalid("the WIT package must declare a version; interface ids carry it")
    })?;
    // Full `use` resolution needs the host catalog and the installed providers;
    // what this can decide is that the native interface exists at that version,
    // and that any other package has a provider at all.
    for (foreign, interfaces) in &package.foreign_deps {
        let package_id = format!("{}:{}", foreign.namespace, foreign.name);
        // `anth:error` is a package of types injected when parsing, never
        // granted and never risked (§9.2); it has no version to pin.
        if package_id == "anth:error" {
            continue;
        }
        let provided: Vec<(String, anth_contract::Version)> = catalog::NATIVE
            .iter()
            .filter_map(|entry| InterfaceId::parse(entry.id).ok())
            .filter(|interface| interface.package_id() == package_id)
            .map(|interface| (interface.to_string(), interface.version().clone()))
            .collect();
        if provided.is_empty() {
            if !known.iter().any(|known| known == &package_id) {
                return Err(Error::invalid(format!(
                    "`{package_id}` is imported by the WIT package but is neither native nor installed"
                )));
            }
            continue;
        }
        let version = provided[0].1.clone();
        if foreign.version.as_ref() != Some(&version) {
            return Err(Error::invalid(format!(
                "`{package_id}` is imported at {}, and the engine provides {version}",
                foreign
                    .version
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| "no version".to_string())
            )));
        }
        for interface in interfaces.keys() {
            let id = format!("{package_id}/{interface}@{version}");
            if !provided.iter().any(|(declared, _)| declared == &id) {
                return Err(Error::invalid(format!("`{id}` is not in the native set")));
            }
        }
    }
    for export in &manifest.export {
        let id = InterfaceId::parse(&export.interface)?;
        if id.namespace() != namespace || id.package() != name {
            return Err(Error::invalid(format!(
                "`{}` is exported from a package the bundle does not declare",
                export.interface
            )));
        }
        if id.version() != &package_version {
            return Err(Error::invalid(format!(
                "`{}` is exported at version {}, and the WIT package is {package_version}",
                export.interface,
                id.version()
            )));
        }
        let declared = package
            .interfaces
            .iter()
            .any(|(_, interface)| interface.name.as_deref() == Some(id.interface()));
        if !declared {
            return Err(Error::invalid(format!(
                "`{}` is exported by the manifest but not declared in `wit/`",
                export.interface
            )));
        }
    }
    for route in &manifest.route {
        let (id, function) = parse_call(&route.call)?;
        let id = InterfaceId::parse(&id)?;
        let interface = package
            .interfaces
            .iter()
            .find_map(|(_, interface)| {
                (interface.name.as_deref() == Some(id.interface())).then_some(interface)
            })
            .ok_or_else(|| {
                Error::invalid(format!(
                    "route `{}` calls `{}`, which `wit/` does not declare",
                    route.path, id
                ))
            })?;
        let declared = interface.functions.get(&function).ok_or_else(|| {
            Error::invalid(format!(
                "route `{}` calls `{function}`, which `{}` does not declare",
                route.path, id
            ))
        })?;
        let element = declared
            .result
            .as_ref()
            .and_then(|result| stream_element(package, result));
        if route.stream {
            let element = element.ok_or_else(|| {
                Error::invalid(format!(
                    "route `{}` declares `stream = true` but `{function}` does not return a stream",
                    route.path
                ))
            })?;
            if route.events {
                if is_bytes(package, element) {
                    return Err(Error::invalid(format!(
                        "route `{}` declares `events = true` but `{function}` streams bytes",
                        route.path
                    )));
                }
            } else if !is_bytes(package, element) {
                return Err(Error::invalid(format!(
                    "route `{}` streams records and needs `events = true`",
                    route.path
                )));
            }
        } else if element.is_some() {
            return Err(Error::invalid(format!(
                "route `{}` calls the stream `{function}` without `stream = true`",
                route.path
            )));
        }
        if let Some(write) = &route.write {
            let (write_id, write_function) = parse_call(write)?;
            let write_id = InterfaceId::parse(&write_id)?;
            let interface = package
                .interfaces
                .iter()
                .find_map(|(_, interface)| {
                    (interface.name.as_deref() == Some(write_id.interface())).then_some(interface)
                })
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "route `{}` writes through `{}`, which `wit/` does not declare",
                        route.path, write_id
                    ))
                })?;
            let declared = interface.functions.get(&write_function).ok_or_else(|| {
                Error::invalid(format!(
                    "route `{}` writes through `{write_function}`, which `{}` does not declare",
                    route.path, write_id
                ))
            })?;
            if declared
                .result
                .as_ref()
                .is_some_and(|result| is_stream(package, result))
            {
                return Err(Error::invalid(format!(
                    "route `{}` writes through the stream `{write_function}`",
                    route.path
                )));
            }
        }
    }
    Ok(())
}

/// Whether a WIT result is a stream, either directly or as the success half of a
/// `result<stream<T>, E>` (§20).
fn is_stream(package: &UnresolvedPackage, ty: &Type) -> bool {
    match ty {
        Type::Id(id) => match &package.types[*id].kind {
            TypeDefKind::Stream(_) => true,
            TypeDefKind::Result(result) => {
                result.ok.as_ref().is_some_and(|ok| is_stream(package, ok))
            }
            _ => false,
        },
        _ => false,
    }
}

/// The element type of a stream result, unwrapping `result<stream<T>, E>`.
fn stream_element<'a>(package: &'a UnresolvedPackage, ty: &'a Type) -> Option<&'a Type> {
    match ty {
        Type::Id(id) => match &package.types[*id].kind {
            TypeDefKind::Stream(element) => element.as_ref(),
            TypeDefKind::Result(result) => result
                .ok
                .as_ref()
                .and_then(|ok| stream_element(package, ok)),
            _ => None,
        },
        _ => None,
    }
}

/// `list<u8>` is the byte stream the chunked transport carries.
fn is_bytes(package: &UnresolvedPackage, ty: &Type) -> bool {
    match ty {
        Type::Id(id) => matches!(&package.types[*id].kind, TypeDefKind::List(Type::U8)),
        _ => false,
    }
}

/// Every hash in the manifest is checked before the bundle becomes visible, and
/// the component and UI directories hold nothing undeclared.
fn check_hashes(root: &Path, manifest: &Manifest) -> Result<(), Error> {
    let mut declared: BTreeSet<String> = BTreeSet::new();
    if let Some(wasm) = &manifest.wasm {
        declared.insert(wasm.path.clone());
        check_hash(root, &wasm.path, &wasm.sha256)?;
    }
    if let Some(service) = &manifest.service {
        declared.insert(service.path.clone());
        check_hash(root, &service.path, &service.sha256)?;
        // Verified bytes, owner-executable, nobody else: the supervisor launches
        // this path directly and the bundle directory is already `0700`.
        fs::set_permissions(root.join(&service.path), fs::Permissions::from_mode(0o700)).map_err(
            |error| Error::internal(format!("marking `{}` executable: {error}", service.path)),
        )?;
    }
    for slot in &manifest.slot {
        declared.insert(slot.module.clone());
        check_hash(root, &slot.module, &slot.sha256)?;
    }
    for (relative, is_file) in walk(root)? {
        if is_file
            && (relative.starts_with("wasm/")
                || relative.starts_with("bin/")
                || relative.starts_with("web/"))
            && !declared.contains(&relative)
        {
            return Err(Error::invalid(format!(
                "`{relative}` is in the bundle but not declared in the manifest"
            )));
        }
    }
    Ok(())
}

fn check_hash(root: &Path, relative: &str, expected: &str) -> Result<(), Error> {
    let bytes = fs::read(root.join(relative)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::invalid(format!(
                "the manifest declares `{relative}`, which the bundle does not contain"
            ))
        } else {
            Error::internal(format!("reading `{relative}`: {error}"))
        }
    })?;
    let actual = hex(&Sha256::digest(&bytes));
    if actual != expected {
        return Err(Error::invalid(format!(
            "`{relative}` does not match its manifest hash"
        )));
    }
    Ok(())
}

fn signed_message(root: &Path) -> Result<Vec<u8>, Error> {
    let mut files = vec![(
        "manifest.toml".to_string(),
        read_required(root, "manifest.toml")?,
    )];
    for (relative, is_file) in walk(root)? {
        if is_file && relative.starts_with("wit/") {
            files.push((relative.clone(), read_required(root, &relative)?));
        }
    }
    signature::message(&files)
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

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Syncs every directory of the staged tree, children first, so the entries a
/// later rename publishes are on disk.
fn sync_tree(root: &Path) -> Result<(), Error> {
    for (relative, is_dir) in walk(root)?.into_iter().rev() {
        if !is_dir {
            continue;
        }
        let dir = root.join(&relative);
        fs::File::open(&dir)
            .and_then(|dir| dir.sync_all())
            .map_err(|error| Error::internal(format!("syncing {}: {error}", dir.display())))?;
    }
    fs::File::open(root)
        .and_then(|dir| dir.sync_all())
        .map_err(|error| Error::internal(format!("syncing {}: {error}", root.display())))
}

/// Creates `path` and every missing parent with `0700`, never with the umask.
fn create_dir_private(path: &Path) -> Result<(), Error> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => return Ok(()),
        Ok(_) => {
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
    if let Some(parent) = path.parent() {
        create_dir_private(parent)?;
    }
    match DirBuilder::new().mode(store::DIR_MODE).create(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(Error::internal(format!(
            "creating {}: {error}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{KeyAlgorithm, TrustedKey, KEY_VERSION};
    use ed25519_dalek::{Signer, SigningKey};
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    const WASM: &[u8] = b"\0asm a fake component";
    const WIT: &str = "package example:notes@1.0.0;\n\ninterface notes {\n  all: func() -> list<string>;\n}\n\nworld app {\n  export notes;\n}\n";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-bundle-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("chmod");
        dir
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn manifest(wasm: &[u8]) -> String {
        format!(
            "manifest_version = 2\n\n[app]\nid = \"org.example.notes\"\nnamespace = \"example:notes\"\nname = \"Notes\"\nversion = \"0.1.0\"\n\n[wasm]\npath = \"wasm/notes.wasm\"\nsha256 = \"{}\"\n\n[[export]]\ninterface = \"example:notes/notes@1.0.0\"\n",
            hex(&Sha256::digest(wasm))
        )
    }

    fn entries(manifest: &str, wit: &str, wasm: &[u8]) -> Vec<(String, Vec<u8>)> {
        vec![
            ("manifest.toml".to_string(), manifest.as_bytes().to_vec()),
            ("wit/notes.wit".to_string(), wit.as_bytes().to_vec()),
            ("wasm/notes.wasm".to_string(), wasm.to_vec()),
        ]
    }

    fn service_manifest(binary: &[u8]) -> String {
        format!(
            "manifest_version = 3\n\n[app]\nid = \"org.example.notes\"\nnamespace = \"example:notes\"\nname = \"Notes\"\nversion = \"0.1.0\"\n\n[service]\npath = \"bin/notes\"\nsha256 = \"{}\"\nclass = \"socket\"\nargv = [\"--socket\", \"{{socket}}\"]\n\n[[export]]\ninterface = \"example:notes/notes@1.0.0\"\n",
            hex(&Sha256::digest(binary))
        )
    }

    fn service_entries(manifest: &str, wit: &str, binary: &[u8]) -> Vec<(String, Vec<u8>)> {
        vec![
            ("manifest.toml".to_string(), manifest.as_bytes().to_vec()),
            ("wit/notes.wit".to_string(), wit.as_bytes().to_vec()),
            ("bin/notes".to_string(), binary.to_vec()),
        ]
    }

    fn zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer
                .start_file(name, SimpleFileOptions::default())
                .expect("start");
            writer.write_all(bytes).expect("write");
        }
        writer.finish().expect("finish").into_inner()
    }

    fn options(require_signature: bool) -> Options {
        Options {
            max_bundle_bytes: 1 << 20,
            require_signature,
            allow_third_party_service: false,
            known_packages: Vec::new(),
        }
    }

    fn keyring(state: &Path) -> (Keyring, SigningKey) {
        let keyring = Keyring::new(state);
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        keyring
            .add(&TrustedKey {
                version: KEY_VERSION,
                algorithm: KeyAlgorithm::Ed25519,
                public_key: hex(&signing.verifying_key().to_bytes()),
                label: None,
                added_by: "operator-test".to_string(),
                added_at: "2026-11-03T20:00:00Z".to_string(),
            })
            .expect("add key");
        (keyring, signing)
    }

    fn sign(manifest: &str, wit: &str, signing: &SigningKey) -> String {
        let message = anth_contract::signature::message(&[
            ("manifest.toml".to_string(), manifest.as_bytes().to_vec()),
            ("wit/notes.wit".to_string(), wit.as_bytes().to_vec()),
        ])
        .expect("message");
        let signature = signing.sign(&message);
        crate::signature::signature_base64(&signature.to_bytes())
    }

    #[test]
    fn an_unsigned_bundle_is_accepted_when_signatures_are_off() {
        let state = scratch("unsigned");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let staged = extract(
            &zip(&entries(&manifest, WIT, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect("staged");
        assert_eq!(staged.manifest().app.id, "org.example.notes");
        assert!(staged.signer().is_none());
        let root = staged.root().to_path_buf();
        assert!(root.join("manifest.toml").is_file());
        drop(staged);
        assert!(!root.exists(), "the staging directory is removed");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_unsigned_bundle_is_denied_when_required() {
        let state = scratch("required");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let error = extract(
            &zip(&entries(&manifest, WIT, WASM)),
            &state,
            &options(true),
            &keyring,
        )
        .expect_err("unsigned");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_signed_bundle_names_the_signer() {
        let state = scratch("signed");
        let (keyring, signing) = keyring(&state);
        let manifest = manifest(WASM);
        let signature = sign(&manifest, WIT, &signing);
        let mut files = entries(&manifest, WIT, WASM);
        files.push(("manifest.sig".to_string(), signature.into_bytes()));
        let staged = extract(&zip(&files), &state, &options(true), &keyring).expect("staged");
        let expected =
            crate::signature::fingerprint(&hex(&signing.verifying_key().to_bytes())).unwrap();
        assert_eq!(staged.signer(), Some(expected.as_str()));
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_component_that_does_not_match_its_hash_is_refused() {
        let state = scratch("hash");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let error = extract(
            &zip(&entries(&manifest, WIT, b"another component")),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("hash");
        assert!(error.message().contains("manifest hash"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_undeclared_file_is_refused() {
        let state = scratch("undeclared");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut files = entries(&manifest, WIT, WASM);
        files.push(("bin/helper".to_string(), b"payload".to_vec()));
        let error =
            extract(&zip(&files), &state, &options(false), &keyring).expect_err("undeclared");
        assert!(
            error.message().contains("not declared in the manifest"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_file_outside_the_layout_is_refused() {
        let state = scratch("layout");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut files = entries(&manifest, WIT, WASM);
        files.push(("lib/helper.so".to_string(), b"payload".to_vec()));
        let error = extract(&zip(&files), &state, &options(false), &keyring).expect_err("layout");
        assert!(
            error.message().contains("not part of the bundle layout"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_service_bundle_verifies_and_is_executable() {
        let state = scratch("service");
        let keyring = Keyring::new(&state);
        let binary = b"\x7fELF a fake service";
        let manifest = service_manifest(binary);
        let staged = extract(
            &zip(&service_entries(&manifest, WIT, binary)),
            &state,
            &options(false),
            &keyring,
        )
        .expect("service bundle");
        let path = staged.root().join("bin/notes");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert_eq!(
            staged.manifest().service.as_ref().unwrap().path,
            "bin/notes"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_service_binary_that_does_not_match_its_hash_is_refused() {
        let state = scratch("service-hash");
        let keyring = Keyring::new(&state);
        let manifest = service_manifest(b"\x7fELF the real service");
        let error = extract(
            &zip(&service_entries(
                &manifest,
                WIT,
                b"\x7fELF a swapped service",
            )),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("service hash");
        assert!(error.message().contains("manifest hash"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_undeclared_service_binary_is_refused() {
        let state = scratch("service-undeclared");
        let keyring = Keyring::new(&state);
        let binary = b"\x7fELF a fake service";
        let manifest = service_manifest(binary);
        let mut files = service_entries(&manifest, WIT, binary);
        files.push(("bin/helper".to_string(), b"payload".to_vec()));
        let error = extract(&zip(&files), &state, &options(false), &keyring)
            .expect_err("undeclared service file");
        assert!(error.message().contains("not declared"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_bin_subdirectory_is_refused() {
        let state = scratch("service-subdir");
        let keyring = Keyring::new(&state);
        let binary = b"\x7fELF a fake service";
        let manifest = service_manifest(binary);
        let mut files = service_entries(&manifest, WIT, binary);
        files.push(("bin/lib/notes.so".to_string(), b"payload".to_vec()));
        let error =
            extract(&zip(&files), &state, &options(false), &keyring).expect_err("subdirectory");
        assert!(
            error.message().contains("not part of the bundle layout"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_undeclared_component_is_refused() {
        let state = scratch("extra-wasm");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut files = entries(&manifest, WIT, WASM);
        files.push(("wasm/other.wasm".to_string(), b"extra".to_vec()));
        let error = extract(&zip(&files), &state, &options(false), &keyring).expect_err("extra");
        assert!(error.message().contains("not declared"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_bundle_may_not_carry_its_own_descriptor() {
        // The descriptor is engine-written install metadata, never signed
        // bundle content.
        let state = scratch("bundle-descriptor");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut files = entries(&manifest, WIT, WASM);
        files.push(("descriptor.toml".to_string(), b"version = 1\n".to_vec()));
        let error = extract(&zip(&files), &state, &options(false), &keyring)
            .expect_err("bundled descriptor");
        assert!(error.message().contains("must not carry"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_path_that_escapes_the_bundle_is_refused() {
        let state = scratch("traversal");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut files = entries(&manifest, WIT, WASM);
        files.push(("../evil".to_string(), b"x".to_vec()));
        let error =
            extract(&zip(&files), &state, &options(false), &keyring).expect_err("traversal");
        assert!(error.message().contains("escapes the bundle"), "{error}");
        assert!(!state.join("..").join("evil").exists());
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_symlink_entry_is_refused() {
        let state = scratch("symlink");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries(&manifest, WIT, WASM) {
            writer
                .start_file(name, SimpleFileOptions::default())
                .expect("start");
            writer.write_all(&bytes).expect("write");
        }
        writer
            .add_symlink("wit/link", "manifest.toml", SimpleFileOptions::default())
            .expect("symlink");
        let bundle = writer.finish().expect("finish").into_inner();
        let error = extract(&bundle, &state, &options(false), &keyring).expect_err("symlink");
        assert!(error.message().contains("symlink"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn two_worlds_are_refused() {
        let state = scratch("worlds");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let wit = "package example:notes@1.0.0;\ninterface notes { all: func(); }\nworld app { export notes; }\nworld other { export notes; }\n";
        let error = extract(
            &zip(&entries(&manifest, wit, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("worlds");
        assert!(error.message().contains("exactly one world"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_package_that_does_not_match_the_manifest_is_refused() {
        let state = scratch("package");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let wit = "package example:other@0.1.0;\ninterface notes { all: func(); }\nworld app { export notes; }\n";
        let error = extract(
            &zip(&entries(&manifest, wit, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("package");
        assert!(error.message().contains("WIT package"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_export_not_declared_in_wit_is_refused() {
        let state = scratch("export");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM).replace(
            "interface = \"example:notes/notes@1.0.0\"",
            "interface = \"example:notes/other@1.0.0\"",
        );
        let error = extract(
            &zip(&entries(&manifest, WIT, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("export");
        assert!(
            error.message().contains("not declared in `wit/`"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_version_the_wit_does_not_implement_is_refused() {
        let state = scratch("export-version");
        let keyring = Keyring::new(&state);
        let manifest =
            manifest(WASM).replace("example:notes/notes@1.0.0", "example:notes/notes@9.9.9");
        let error = extract(
            &zip(&entries(&manifest, WIT, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("version");
        assert!(error.message().contains("exported at version"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_nested_wit_directory_is_refused() {
        // `wit/` holds the package and nothing else: a vendored copy is a
        // second truth, not a dependency.
        let state = scratch("nested-wit");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut files = entries(&manifest, WIT, WASM);
        files.push((
            "wit/vendor/other.wit".to_string(),
            b"package other:things@1.0.0;\n".to_vec(),
        ));
        let error = extract(&zip(&files), &state, &options(false), &keyring).expect_err("layout");
        assert!(
            error.message().contains("not part of the bundle layout"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_dot_component_is_refused_before_it_reaches_the_filesystem() {
        let state = scratch("dot");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut files = entries(&manifest, WIT, WASM);
        files.push(("wit/./notes.wit".to_string(), WIT.as_bytes().to_vec()));
        let error = extract(&zip(&files), &state, &options(false), &keyring).expect_err("dot");
        assert!(error.message().contains("plain path"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_native_import_is_allowed() {
        let state = scratch("native-import");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let wit = "package example:notes@1.0.0;\ninterface notes { all: func(); }\nworld app { import anth:env/info@1.0.0; export notes; }\n";
        extract(
            &zip(&entries(&manifest, wit, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect("the native set is always available");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_unknown_import_is_refused() {
        let state = scratch("unknown-import");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let wit = "package example:notes@1.0.0;\ninterface notes { all: func(); }\nworld app { import example:other/x@1.0.0; export notes; }\n";
        let error = extract(
            &zip(&entries(&manifest, wit, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("unknown import");
        assert!(
            error.message().contains("neither native nor installed"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_installed_provider_may_be_imported() {
        let state = scratch("known-import");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let wit = "package example:notes@1.0.0;\ninterface notes { all: func(); }\nworld app { import example:other/x@1.0.0; export notes; }\n";
        let mut options = options(false);
        options.known_packages = vec!["example:other".to_string()];
        extract(
            &zip(&entries(&manifest, wit, WASM)),
            &state,
            &options,
            &keyring,
        )
        .expect("the provider is installed");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn explicit_directory_entries_are_accepted() {
        // Zips written by common tools include `wit/` and `wasm/` entries. The
        // trailing slash is not a path segment.
        let state = scratch("dir-entries");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for dir in ["wit", "wasm"] {
            writer
                .add_directory(dir, SimpleFileOptions::default())
                .expect("dir");
        }
        for (name, bytes) in entries(&manifest, WIT, WASM) {
            writer
                .start_file(name, SimpleFileOptions::default())
                .expect("start");
            writer.write_all(&bytes).expect("write");
        }
        let bundle = writer.finish().expect("finish").into_inner();
        extract(&bundle, &state, &options(false), &keyring).expect("directory entries");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_native_import_with_another_version_is_refused() {
        let state = scratch("native-version");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let wit = "package example:notes@1.0.0;\ninterface notes { all: func(); }\nworld app { import anth:env/info@2.0.0; export notes; }\n";
        let error = extract(
            &zip(&entries(&manifest, wit, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("version");
        assert!(error.message().contains("engine provides 1.0.0"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_native_interface_that_does_not_exist_is_refused() {
        let state = scratch("native-interface");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let wit = "package example:notes@1.0.0;\ninterface notes { all: func(); }\nworld app { import anth:env/nonexistent@1.0.0; export notes; }\n";
        let error = extract(
            &zip(&entries(&manifest, wit, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("interface");
        assert!(error.message().contains("not in the native set"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn the_extracted_directories_are_0700() {
        let state = scratch("dir-modes");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let staged = extract(
            &zip(&entries(&manifest, WIT, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect("staged");
        for relative in ["wit", "wasm"] {
            let mode = fs::metadata(staged.root().join(relative))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700, "{relative}");
        }
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_bundle_over_the_limit_is_refused() {
        let state = scratch("large");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let bundle = zip(&entries(&manifest, WIT, WASM));
        let error = extract(
            &bundle,
            &state,
            &Options {
                max_bundle_bytes: 10,
                require_signature: false,
                allow_third_party_service: false,
                known_packages: Vec::new(),
            },
            &keyring,
        )
        .expect_err("large");
        assert_eq!(error.kind(), anth_contract::ErrorKind::TooLarge);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_failed_extraction_leaves_no_staging_directory() {
        let state = scratch("cleanup");
        let keyring = Keyring::new(&state);
        let manifest = manifest(WASM);
        let error = extract(
            &zip(&entries(&manifest, WIT, b"wrong")),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("hash");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        let leftovers: Vec<_> = fs::read_dir(&state)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with(".bundle-"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn changing_the_wit_after_signing_is_denied() {
        let state = scratch("changed-wit");
        let (keyring, signing) = keyring(&state);
        let manifest = manifest(WASM);
        let signature = sign(&manifest, WIT, &signing);
        let changed = WIT.replace("all: func() -> list<string>;", "all: func() -> string;");
        let mut files = entries(&manifest, &changed, WASM);
        files.push(("manifest.sig".to_string(), signature.into_bytes()));
        let error = extract(&zip(&files), &state, &options(true), &keyring).expect_err("tampered");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    fn with_route(manifest: &str, call: &str, stream: bool) -> String {
        format!(
            "{manifest}[[route]]\nmethod = \"GET\"\npath = \"/stream\"\ncall = \"{call}\"\nstream = {stream}\n"
        )
    }

    const STREAM_WIT: &str = "package example:notes@1.0.0;\n\ninterface notes {\n  follow: func() -> stream<list<u8>>;\n}\n\nworld app {\n  export notes;\n}\n";

    #[test]
    fn a_stream_route_must_match_a_wit_stream() {
        let state = scratch("stream-route");
        let keyring = Keyring::new(&state);
        let manifest = with_route(&manifest(WASM), "example:notes/notes@1.0.0#follow", true);
        extract(
            &zip(&entries(&manifest, STREAM_WIT, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect("stream route");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_result_wrapped_stream_is_accepted() {
        let state = scratch("stream-result");
        let keyring = Keyring::new(&state);
        let wit = "package example:notes@1.0.0;\n\ninterface notes {\n  follow: func() -> result<stream<list<u8>>, string>;\n}\n\nworld app {\n  export notes;\n}\n";
        let manifest = with_route(&manifest(WASM), "example:notes/notes@1.0.0#follow", true);
        extract(
            &zip(&entries(&manifest, wit, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect("result stream");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_stream_flag_on_a_plain_function_is_refused() {
        let state = scratch("stream-mismatch");
        let keyring = Keyring::new(&state);
        let manifest = with_route(&manifest(WASM), "example:notes/notes@1.0.0#all", true);
        let error = extract(
            &zip(&entries(&manifest, WIT, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("not a stream");
        assert!(
            error.message().contains("does not return a stream"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_stream_result_needs_the_route_flag() {
        let state = scratch("stream-no-flag");
        let keyring = Keyring::new(&state);
        let manifest = with_route(&manifest(WASM), "example:notes/notes@1.0.0#follow", false);
        let error = extract(
            &zip(&entries(&manifest, STREAM_WIT, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("missing flag");
        assert!(
            error.message().contains("without `stream = true`"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_route_function_must_exist_in_wit() {
        let state = scratch("route-function");
        let keyring = Keyring::new(&state);
        let manifest = with_route(&manifest(WASM), "example:notes/notes@1.0.0#missing", false);
        let error = extract(
            &zip(&entries(&manifest, WIT, WASM)),
            &state,
            &options(false),
            &keyring,
        )
        .expect_err("missing function");
        assert!(error.message().contains("does not declare"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }
}
