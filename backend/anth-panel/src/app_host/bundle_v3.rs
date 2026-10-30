//! Install a v3 bundle: extract, verify and stage it, but do not grant it.
//!
//! Installation stores content and a manifest. The grant is created later by
//! `app approve`, after the operator has seen the normalized authority and the
//! imports resolve against the host catalog.
//!
//! A v3 bundle is currently one root `manifest.toml`; layer merging is a later
//! step, so a layer file is refused instead of half-merged.

use std::fs;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};

use anth_wire::WireError;
use zip::ZipArchive;

use super::bundle;
use crate::apps::capability::{Implementation, ManifestV3};
use crate::apps::manifest::{
    BIN_MANIFEST_FILE, MANIFEST_FILE, MANIFEST_FILES, WASM_MANIFEST_FILE, WEB_MANIFEST_FILE,
    manifest_message, validate_id,
};

/// What an install produced. There is deliberately no grant.
#[derive(Debug, Clone)]
pub struct InstalledV3 {
    pub manifest: ManifestV3,
    pub manifest_sha256: String,
    pub manifest_signature: Option<String>,
    pub warning: Option<String>,
    pub dir: PathBuf,
}

/// Reads the `manifest_version` of a bundle without installing it.
pub fn manifest_version(bytes: &[u8]) -> Result<u32, WireError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| WireError::invalid(format!("the upload is not a zip bundle: {error}")))?;
    read_version(&mut archive)
}

pub fn manifest_version_file(path: &Path) -> Result<u32, WireError> {
    let file = fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            WireError::not_found(format!("no bundle at {}", path.display()))
        } else {
            WireError::internal(format!("opening {}: {error}", path.display()))
        }
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| WireError::invalid(format!("{} is not a zip: {error}", path.display())))?;
    read_version(&mut archive)
}

fn read_version<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<u32, WireError> {
    let mut entry = archive.by_name(MANIFEST_FILE).map_err(|error| {
        WireError::invalid(format!("the bundle has no `{MANIFEST_FILE}`: {error}"))
    })?;
    let mut text = String::new();
    entry
        .by_ref()
        .take(crate::apps::manifest::MAX_MANIFEST_BYTES as u64)
        .read_to_string(&mut text)
        .map_err(|error| WireError::invalid(format!("reading `{MANIFEST_FILE}`: {error}")))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| WireError::invalid(format!("`{MANIFEST_FILE}` is not TOML: {error}")))?;
    value
        .get("manifest_version")
        .and_then(toml::Value::as_integer)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| WireError::invalid("the manifest has no usable `manifest_version`"))
}

pub fn install_bytes(
    bytes: &[u8],
    root: &Path,
    require_signature: bool,
    public_key: &str,
) -> Result<InstalledV3, WireError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| WireError::invalid(format!("the upload is not a zip bundle: {error}")))?;
    install_reader(&mut archive, root, require_signature, public_key)
}

pub fn install_file(
    path: &Path,
    root: &Path,
    require_signature: bool,
    public_key: &str,
) -> Result<InstalledV3, WireError> {
    let file = fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            WireError::not_found(format!("no bundle at {}", path.display()))
        } else {
            WireError::internal(format!("opening {}: {error}", path.display()))
        }
    })?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| WireError::invalid(format!("{} is not a zip: {error}", path.display())))?;
    install_reader(&mut archive, root, require_signature, public_key)
}

pub fn install_reader<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    root: &Path,
    require_signature: bool,
    public_key: &str,
) -> Result<InstalledV3, WireError> {
    let staging = bundle::staging_root(root)?;
    let staged = install_staged(archive, &staging, require_signature, public_key);
    let (manifest, manifest_sha256, manifest_signature, warning) = match staged {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    let destination = root.join(&manifest.app.id);
    if destination.exists() {
        let _ = fs::remove_dir_all(&staging);
        return Err(WireError::new(
            anth_wire::ErrorKind::Conflict,
            "app_exists",
            format!(
                "an app named `{}` is already installed; remove it first",
                manifest.app.id
            ),
        ));
    }
    fs::rename(&staging, &destination).map_err(|error| {
        let _ = fs::remove_dir_all(&staging);
        WireError::internal(format!(
            "moving the app into {}: {error}",
            destination.display()
        ))
    })?;
    Ok(InstalledV3 {
        manifest,
        manifest_sha256,
        manifest_signature,
        warning,
        dir: destination,
    })
}

type Staged = (ManifestV3, String, Option<String>, Option<String>);

fn install_staged(
    archive: &mut ZipArchive<impl Read + Seek>,
    staging: &Path,
    require_signature: bool,
    public_key: &str,
) -> Result<Staged, WireError> {
    bundle::extract(archive, staging)?;
    let files = bundle::read_manifest_files(staging)?;
    for layer in [WASM_MANIFEST_FILE, WEB_MANIFEST_FILE, BIN_MANIFEST_FILE] {
        if files.contains_key(layer) {
            return Err(WireError::new(
                anth_wire::ErrorKind::Unsupported,
                "layered_manifest",
                "v3 layer manifests are not supported yet; use one root manifest.toml",
            ));
        }
    }
    if !MANIFEST_FILES.contains(&MANIFEST_FILE) {
        return Err(WireError::internal("the manifest file list is broken"));
    }
    let root = files
        .get(MANIFEST_FILE)
        .ok_or_else(|| WireError::invalid(format!("the bundle has no `{MANIFEST_FILE}`")))?;
    let text = std::str::from_utf8(root)
        .map_err(|error| WireError::invalid(format!("the manifest is not UTF-8: {error}")))?;
    let manifest = ManifestV3::parse(text).map_err(WireError::invalid)?;
    validate_id(&manifest.app.id)?;

    let message = manifest_message(&files);
    let signature = bundle::verify_signature(staging, &message, require_signature, public_key)?;
    let manifest_sha256 = bundle::sha256(&message);

    // The module must be present and match the manifest.
    bundle::verify_hash(
        &staging.join(&manifest.wasm.path),
        &manifest.wasm.sha256,
        "module",
    )?;

    let mut helpers = Vec::new();
    for export in &manifest.export {
        let path = match &export.implementation {
            Implementation::Binary(binary)
                if binary.source == crate::apps::capability::BinarySource::Bundle =>
            {
                Some(&binary.path)
            }
            Implementation::Service(service)
                if service.source == crate::apps::capability::BinarySource::Bundle =>
            {
                Some(&service.path)
            }
            _ => None,
        };
        if let Some(path) = path {
            check_relative(path)?;
            let digest = match &export.implementation {
                Implementation::Binary(binary) => &binary.sha256,
                Implementation::Service(service) => &service.sha256,
                _ => unreachable!(),
            };
            bundle::verify_hash(&staging.join(path), digest, "helper")?;
            helpers.push(path.clone());
        }
    }
    reject_extra_helpers(staging, &helpers)?;

    for slot in &manifest.ui {
        check_relative(&slot.module)?;
        let path = staging.join(&slot.module);
        let metadata = fs::metadata(&path).map_err(|error| {
            WireError::invalid(format!(
                "the UI module `{}` is missing: {error}",
                slot.module
            ))
        })?;
        if !metadata.is_file() || metadata.len() > bundle::MAX_ASSET_BYTES {
            return Err(WireError::invalid(format!(
                "the UI module `{}` is not a regular file under {} bytes",
                slot.module,
                bundle::MAX_ASSET_BYTES
            )));
        }
        bundle::verify_hash(&path, &slot.sha256, "UI module")?;
    }

    let warning = signature.is_none().then(|| {
        "installed without a signature because `[apps] require_signature = false`".to_string()
    });
    Ok((manifest, manifest_sha256, signature, warning))
}

fn check_relative(path: &str) -> Result<(), WireError> {
    if path.is_empty() || path.starts_with('/') || path.contains("..") || path.contains('\\') {
        return Err(WireError::invalid(format!(
            "`{path}` is not a relative bundle path"
        )));
    }
    Ok(())
}

fn reject_extra_helpers(staging: &Path, declared: &[String]) -> Result<(), WireError> {
    let dir = staging.join("helpers");
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(WireError::internal(format!(
                "reading {}: {error}",
                dir.display()
            )));
        }
    };
    for entry in entries {
        let entry = entry
            .map_err(|error| WireError::internal(format!("reading {}: {error}", dir.display())))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| WireError::internal(format!("reading {}: {error}", path.display())))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(WireError::forbidden(format!(
                "{} is not a regular helper file",
                path.display()
            )));
        }
        let relative = path
            .strip_prefix(staging)
            .map(|value| value.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if !declared.iter().any(|name| name == &relative) {
            return Err(WireError::forbidden(format!(
                "the helper `{relative}` is not declared by any export"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn sha(bytes: &[u8]) -> String {
        bundle::sha256(bytes)
    }

    fn manifest(wasm: &[u8], module: &[u8]) -> String {
        format!(
            r#"
manifest_version = 3
[app]
id = "org.example.v3"
version = "0.1.0"
name = "V3"
[runtime]
kind = "wasm"
[wasm]
path = "wasm/plugin.wasm"
sha256 = "{}"
abi = 4

[[export]]
id = "org.example.v3.status"
version = 1
input = {{ type = "record", fields = {{}} }}
output = {{ type = "record", fields = {{}} }}
[export.implementation]
kind = "wasm"
entry = "anth_http"

[[api]]
method = "GET"
path = "/status"
export = "org.example.v3.status"

[[ui]]
slot = "settings.app"
tag = "anth-v3"
module = "web/v3.js"
sha256 = "{}"
"#,
            sha(wasm),
            sha(module)
        )
    }

    fn bundle_bytes(text: &str, wasm: &[u8], module: &[u8]) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file(MANIFEST_FILE, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
            zip.start_file("wasm/plugin.wasm", options).unwrap();
            zip.write_all(wasm).unwrap();
            zip.start_file("web/v3.js", options).unwrap();
            zip.write_all(module).unwrap();
            zip.finish().unwrap();
        }
        bytes.into_inner()
    }

    fn root(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "anth-v3-bundle-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn an_unsigned_v3_bundle_installs_without_a_grant() {
        let root = root("install");
        let wasm = b"\0asm\x01\0\0\0";
        let module = b"export const v = 3;";
        let text = manifest(wasm, module);
        let bytes = bundle_bytes(&text, wasm, module);
        assert_eq!(manifest_version(&bytes).unwrap(), 3);
        let installed = install_bytes(&bytes, &root, false, "").expect("install");
        assert_eq!(installed.manifest.app.id, "org.example.v3");
        assert!(installed.manifest_signature.is_none());
        assert!(installed.dir.join("manifest.toml").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_wrong_ui_digest_is_refused() {
        let root = root("digest");
        let wasm = b"\0asm\x01\0\0\0";
        let module = b"export const v = 3;";
        let text = manifest(wasm, b"different");
        let bytes = bundle_bytes(&text, wasm, module);
        let error = install_bytes(&bytes, &root, false, "").expect_err("digest");
        assert!(error.message.contains("UI module"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_undeclared_helper_is_refused() {
        let root = root("helper");
        let wasm = b"\0asm\x01\0\0\0";
        let module = b"export const v = 3;";
        let text = manifest(wasm, module);
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut bytes);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file(MANIFEST_FILE, options).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
            zip.start_file("wasm/plugin.wasm", options).unwrap();
            zip.write_all(wasm).unwrap();
            zip.start_file("web/v3.js", options).unwrap();
            zip.write_all(module).unwrap();
            zip.start_file("helpers/extra", options).unwrap();
            zip.write_all(b"extra").unwrap();
            zip.finish().unwrap();
        }
        let error = install_bytes(&bytes.into_inner(), &root, false, "").expect_err("helper");
        assert!(error.message.contains("not declared"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }
}
