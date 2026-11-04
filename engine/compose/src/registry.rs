//! The registry: installed apps under `state/apps/<id>` (§6.8, §6.1).
//!
//! A bundle becomes visible only after `bundle::extract` has verified it. The
//! move is a rename inside the state tree, so an observer sees the app appear
//! whole or not at all. The registry never grants authority: an installed app
//! still needs a grant to run.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anth_contract::{AppId, Error, Manifest};

use crate::bundle::{self, Options, StagedBundle};
use crate::keys::Keyring;
use crate::store;

static SWAP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Registry {
    apps: PathBuf,
    keyring: Keyring,
}

impl Registry {
    pub fn new(state: &Path) -> Self {
        Self {
            apps: state.join("apps"),
            keyring: Keyring::new(state),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.apps
    }

    /// Verifies and installs a bundle. An id that is already installed is a
    /// conflict; replacing is an explicit operation.
    pub fn install(&self, bundle: &[u8], options: &Options) -> Result<Manifest, Error> {
        store::ensure_dir(&self.apps)?;
        let staged = self.extract(bundle, options)?;
        let manifest = staged.manifest().clone();
        let id = AppId::parse(&manifest.app.id)?;
        let target = self.apps.join(id.as_str());
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(Error::conflict(format!("`{id}` is already installed")));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(Error::internal(format!(
                    "inspecting {}: {error}",
                    target.display()
                )));
            }
        }
        publish(staged, &target)?;
        sync_dir(&self.apps)?;
        Ok(manifest)
    }

    /// Replaces the bundle of an installed app. The id lives inside the bundle,
    /// so a bundle declaring another one is a conflict, not a rename.
    pub fn replace(
        &self,
        app: &AppId,
        bundle: &[u8],
        options: &Options,
    ) -> Result<Manifest, Error> {
        let target = self.apps.join(app.as_str());
        check_installed(&target)?;
        let staged = self.extract(bundle, options)?;
        let manifest = staged.manifest().clone();
        if manifest.app.id != app.as_str() {
            return Err(Error::conflict(format!(
                "the bundle declares `{}`, not `{app}`",
                manifest.app.id
            )));
        }
        let staged_root = staged.into_root();
        // RENAME_EXCHANGE swaps the two trees in one step: the app is never
        // missing, even if the engine dies. Filesystems without it fall back to
        // two renames with a short window and a rollback.
        match exchange(&staged_root, &target) {
            Ok(()) => {
                let _ = fs::remove_dir_all(&staged_root);
            }
            Err(error) if unsupported(&error) => {
                let old = self.unique_swap_path(app)?;
                fs::rename(&target, &old).map_err(|error| {
                    Error::internal(format!("staging {}: {error}", target.display()))
                })?;
                if let Err(error) = fs::rename(&staged_root, &target) {
                    let _ = fs::rename(&old, &target);
                    let _ = fs::remove_dir_all(&staged_root);
                    return Err(Error::internal(format!(
                        "publishing {}: {error}",
                        target.display()
                    )));
                }
                let _ = fs::remove_dir_all(&old);
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&staged_root);
                return Err(Error::internal(format!(
                    "exchanging {}: {error}",
                    target.display()
                )));
            }
        }
        sync_dir(&self.apps)?;
        Ok(manifest)
    }

    pub fn get(&self, app: &AppId) -> Result<Option<Manifest>, Error> {
        let target = self.apps.join(app.as_str());
        match fs::symlink_metadata(&target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::internal(format!(
                "inspecting {}: {error}",
                target.display()
            ))),
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(Error::invalid(format!(
                        "{} is not a real app directory",
                        target.display()
                    )));
                }
                read_manifest(&target)
                    .and_then(|manifest| {
                        if manifest.app.id != app.as_str() {
                            return Err(Error::invalid(format!(
                                "{} declares `{}`, not `{app}`",
                                target.display(),
                                manifest.app.id
                            )));
                        }
                        Ok(manifest)
                    })
                    .map(Some)
            }
        }
    }

    /// Every installed app, sorted by id. Staging and swap directories are
    /// hidden by their leading dot.
    pub fn list(&self) -> Result<Vec<Manifest>, Error> {
        if !store::check_dir(&self.apps)? {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&self.apps).map_err(|error| {
            Error::internal(format!("reading {}: {error}", self.apps.display()))
        })?;
        let mut manifests = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                Error::internal(format!("reading {}: {error}", self.apps.display()))
            })?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let id = AppId::parse(&name)?;
            if let Some(manifest) = self.get(&id)? {
                manifests.push(manifest);
            }
        }
        manifests.sort_by(|a, b| a.app.id.cmp(&b.app.id));
        Ok(manifests)
    }

    /// Removes the installed bundle. The grant is a separate document and the
    /// caller decides whether it goes too.
    pub fn remove(&self, app: &AppId) -> Result<bool, Error> {
        let target = self.apps.join(app.as_str());
        match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(Error::invalid(format!(
                        "{} is not a real app directory",
                        target.display()
                    )));
                }
                fs::remove_dir_all(&target).map_err(|error| {
                    Error::internal(format!("removing {}: {error}", target.display()))
                })?;
                sync_dir(&self.apps)?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::internal(format!(
                "inspecting {}: {error}",
                target.display()
            ))),
        }
    }

    /// Extracts into `apps/` with the packages of installed providers added to
    /// the set the bundle is allowed to import.
    fn extract(&self, bundle: &[u8], options: &Options) -> Result<StagedBundle, Error> {
        let mut options = options.clone();
        options.known_packages.extend(self.known_packages());
        bundle::extract(bundle, &self.apps, &options, &self.keyring)
    }

    /// Packages of the installed apps, skipping any tree that cannot be read.
    ///
    /// One corrupt install must not stop the operator from installing or
    /// replacing something else; `list` is where corruption is reported.
    fn known_packages(&self) -> Vec<String> {
        let mut packages = Vec::new();
        let Ok(entries) = fs::read_dir(&self.apps) else {
            return packages;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            if let Ok(manifest) = read_manifest(&entry.path()) {
                packages.push(manifest.app.namespace);
            }
        }
        packages
    }

    /// A free `.old-*` name. A crashed process can leave one behind, and a
    /// recycled pid must not make the fallback swap fail forever.
    fn unique_swap_path(&self, app: &AppId) -> Result<PathBuf, Error> {
        for _ in 0..100 {
            let candidate = self.apps.join(format!(
                ".old-{}-{}-{}",
                app,
                std::process::id(),
                SWAP.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::symlink_metadata(&candidate) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(candidate);
                }
                Ok(_) => continue,
                Err(error) => {
                    return Err(Error::internal(format!(
                        "inspecting {}: {error}",
                        candidate.display()
                    )));
                }
            }
        }
        Err(Error::internal(format!(
            "could not find a free swap name for `{app}`"
        )))
    }
}

fn publish(staged: StagedBundle, target: &Path) -> Result<(), Error> {
    let root = staged.into_root();
    if let Err(error) = fs::rename(&root, target) {
        let _ = fs::remove_dir_all(&root);
        // Two installs can race past the existence check; the loser sees an
        // existing directory, which is a conflict rather than an internal error.
        if error.kind() == std::io::ErrorKind::AlreadyExists
            || error.raw_os_error() == Some(libc::ENOTEMPTY)
        {
            return Err(Error::conflict(format!(
                "{} is already installed",
                target.display()
            )));
        }
        return Err(Error::internal(format!(
            "publishing {}: {error}",
            target.display()
        )));
    }
    Ok(())
}

/// Swaps two paths atomically (Linux `renameat2` with `RENAME_EXCHANGE`).
#[cfg(target_os = "linux")]
fn exchange(left: &Path, right: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let left = CString::new(left.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let right = CString::new(right.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    // SAFETY: both pointers are valid NUL-terminated paths for the call, and
    // `renameat2` only touches the two directories.
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            left.as_ptr(),
            libc::AT_FDCWD,
            right.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn exchange(_: &Path, _: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// The filesystem or kernel does not implement `RENAME_EXCHANGE`.
fn unsupported(error: &std::io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::EOPNOTSUPP)
    )
}

fn check_installed(target: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(Error::invalid(format!(
            "{} is not a real app directory",
            target.display()
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::not_found(
            format!("{} is not installed", target.display()),
        )),
        Err(error) => Err(Error::internal(format!(
            "inspecting {}: {error}",
            target.display()
        ))),
    }
}

fn read_manifest(dir: &Path) -> Result<Manifest, Error> {
    let path = dir.join("manifest.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::invalid(format!(
                "{} has no manifest; the installed tree is corrupt",
                dir.display()
            )));
        }
        Err(error) => {
            return Err(Error::internal(format!(
                "reading {}: {error}",
                path.display()
            )));
        }
    };
    let manifest = Manifest::parse(&text)
        .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
    manifest
        .validate()
        .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
    Ok(manifest)
}

/// Makes the rename durable before answering: a published app is on disk.
fn sync_dir(dir: &Path) -> Result<(), Error> {
    fs::File::open(dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|error| Error::internal(format!("syncing {}: {error}", dir.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{KeyAlgorithm, TrustedKey, KEY_VERSION};
    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};
    use std::io::{Cursor, Write};
    use std::os::unix::fs::PermissionsExt;
    use zip::write::SimpleFileOptions;

    const WASM: &[u8] = b"\0asm a fake component";
    const WIT: &str = "package example:notes@1.0.0;\ninterface notes { all: func(); }\nworld app { export notes; }\n";

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-registry-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).expect("chmod");
        dir
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn notes(version: &str, wasm: &[u8]) -> String {
        format!(
            "manifest_version = 2\n\n[app]\nid = \"org.example.notes\"\nnamespace = \"example:notes\"\nname = \"Notes\"\nversion = \"{version}\"\n\n[wasm]\npath = \"wasm/notes.wasm\"\nsha256 = \"{}\"\n\n[[export]]\ninterface = \"example:notes/notes@1.0.0\"\n",
            hex(&Sha256::digest(wasm))
        )
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

    fn notes_bundle(version: &str, wasm: &[u8]) -> Vec<u8> {
        zip(&[
            (
                "manifest.toml".to_string(),
                notes(version, wasm).into_bytes(),
            ),
            ("wit/notes.wit".to_string(), WIT.as_bytes().to_vec()),
            ("wasm/notes.wasm".to_string(), wasm.to_vec()),
        ])
    }

    fn options() -> Options {
        Options {
            max_bundle_bytes: 1 << 20,
            require_signature: false,
            known_packages: Vec::new(),
        }
    }

    #[test]
    fn install_list_get_and_remove() {
        let state = scratch("lifecycle");
        let registry = Registry::new(&state);
        let manifest = registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        assert_eq!(manifest.app.id, "org.example.notes");
        assert!(registry.dir().join("org.example.notes").is_dir());

        let installed = registry
            .get(&AppId::parse("org.example.notes").unwrap())
            .expect("get")
            .expect("installed");
        assert_eq!(installed.app.version, "0.1.0");

        let list = registry.list().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].app.id, "org.example.notes");

        let app = AppId::parse("org.example.notes").unwrap();
        assert!(registry.remove(&app).expect("remove"));
        assert!(!registry.remove(&app).expect("already removed"));
        assert!(registry.list().expect("list").is_empty());
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn installing_the_same_id_twice_is_a_conflict() {
        let state = scratch("conflict");
        let registry = Registry::new(&state);
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        let error = registry
            .install(&notes_bundle("0.2.0", WASM), &options())
            .expect_err("conflict");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        assert_eq!(
            registry
                .get(&AppId::parse("org.example.notes").unwrap())
                .expect("get")
                .unwrap()
                .app
                .version,
            "0.1.0",
            "the original stays installed"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn replace_swaps_the_bundle() {
        let state = scratch("replace");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        let newer = b"\0asm a newer component";
        registry
            .replace(&app, &notes_bundle("0.2.0", newer), &options())
            .expect("replace");
        assert_eq!(
            registry.get(&app).expect("get").unwrap().app.version,
            "0.2.0"
        );
        let installed =
            fs::read(registry.dir().join("org.example.notes/wasm/notes.wasm")).expect("wasm");
        assert_eq!(installed, newer, "the component was replaced too");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn replace_with_another_id_is_a_conflict() {
        let state = scratch("replace-id");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        // Build a bundle for a different id by rewriting the manifest.
        let changed = notes("0.1.0", WASM).replace("org.example.notes", "org.example.other");
        let bundle = zip(&[
            ("manifest.toml".to_string(), changed.into_bytes()),
            ("wit/notes.wit".to_string(), WIT.as_bytes().to_vec()),
            ("wasm/notes.wasm".to_string(), WASM.to_vec()),
        ]);
        let error = registry.replace(&app, &bundle, &options()).expect_err("id");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        assert_eq!(
            registry.get(&app).expect("get").unwrap().app.id,
            "org.example.notes"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn replace_of_a_missing_app_is_not_found() {
        let state = scratch("replace-missing");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        let error = registry
            .replace(&app, &notes_bundle("0.1.0", WASM), &options())
            .expect_err("missing");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_installed_provider_may_be_imported() {
        let state = scratch("provider");
        let registry = Registry::new(&state);
        let provider = "manifest_version = 2\n\n[app]\nid = \"org.example.provider\"\nnamespace = \"example:provider\"\nname = \"Provider\"\nversion = \"0.1.0\"\n\n[wasm]\npath = \"wasm/p.wasm\"\nsha256 = \"SHA\"\n\n[[export]]\ninterface = \"example:provider/greeting@1.0.0\"\n"
            .replace("SHA", &hex(&Sha256::digest(b"provider wasm")));
        let provider_wit = "package example:provider@1.0.0;\ninterface greeting { hi: func(); }\nworld app { export greeting; }\n";
        registry
            .install(
                &zip(&[
                    ("manifest.toml".to_string(), provider.into_bytes()),
                    ("wit/p.wit".to_string(), provider_wit.as_bytes().to_vec()),
                    ("wasm/p.wasm".to_string(), b"provider wasm".to_vec()),
                ]),
                &options(),
            )
            .expect("install provider");

        let consumer = "manifest_version = 2\n\n[app]\nid = \"org.example.consumer\"\nnamespace = \"example:consumer\"\nname = \"Consumer\"\nversion = \"0.1.0\"\n\n[[import]]\ninterface = \"example:provider/greeting@1.0.0\"\n";
        let consumer_wit = "package example:consumer@1.0.0;\nworld app { import example:provider/greeting@1.0.0; }\n";
        registry
            .install(
                &zip(&[
                    ("manifest.toml".to_string(), consumer.as_bytes().to_vec()),
                    ("wit/c.wit".to_string(), consumer_wit.as_bytes().to_vec()),
                ]),
                &options(),
            )
            .expect("the installed provider is known");
        assert_eq!(registry.list().expect("list").len(), 2);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn get_refuses_a_manifest_under_the_wrong_directory() {
        let state = scratch("wrong-dir");
        let registry = Registry::new(&state);
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        fs::rename(
            registry.dir().join("org.example.notes"),
            registry.dir().join("org.example.other"),
        )
        .expect("rename");
        let error = registry
            .get(&AppId::parse("org.example.other").unwrap())
            .expect_err("mismatch");
        assert!(error.message().contains("declares"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_corrupt_tree_does_not_block_installing_something_else() {
        let state = scratch("corrupt");
        let registry = Registry::new(&state);
        store::ensure_dir(registry.dir()).expect("apps");
        fs::create_dir(registry.dir().join("org.example.broken")).expect("dir");
        fs::write(
            registry.dir().join("org.example.broken/manifest.toml"),
            b"not a manifest",
        )
        .expect("write");
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install despite the corrupt tree");
        assert!(
            registry.list().is_err(),
            "list still reports the corrupt tree"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn replace_leaves_no_swap_directories() {
        let state = scratch("no-leftovers");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        registry
            .replace(&app, &notes_bundle("0.2.0", WASM), &options())
            .expect("replace");
        let leftovers: Vec<_> = fs::read_dir(registry.dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_signed_install_needs_a_trusted_key() {
        let state = scratch("signed");
        let keyring = Keyring::new(&state);
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
        let manifest = notes("0.1.0", WASM);
        let message = anth_contract::signature::message(&[
            ("manifest.toml".to_string(), manifest.as_bytes().to_vec()),
            ("wit/notes.wit".to_string(), WIT.as_bytes().to_vec()),
        ])
        .expect("message");
        let signature = crate::signature::signature_base64(&signing.sign(&message).to_bytes());
        let bundle = zip(&[
            ("manifest.toml".to_string(), manifest.into_bytes()),
            ("wit/notes.wit".to_string(), WIT.as_bytes().to_vec()),
            ("wasm/notes.wasm".to_string(), WASM.to_vec()),
            ("manifest.sig".to_string(), signature.into_bytes()),
        ]);
        let mut options = options();
        options.require_signature = true;
        Registry::new(&state)
            .install(&bundle, &options)
            .expect("signed install");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_signed_install_without_the_key_is_denied() {
        let state = scratch("signed-denied");
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let manifest = notes("0.1.0", WASM);
        let message = anth_contract::signature::message(&[
            ("manifest.toml".to_string(), manifest.as_bytes().to_vec()),
            ("wit/notes.wit".to_string(), WIT.as_bytes().to_vec()),
        ])
        .expect("message");
        let signature = crate::signature::signature_base64(&signing.sign(&message).to_bytes());
        let bundle = zip(&[
            ("manifest.toml".to_string(), manifest.into_bytes()),
            ("wit/notes.wit".to_string(), WIT.as_bytes().to_vec()),
            ("wasm/notes.wasm".to_string(), WASM.to_vec()),
            ("manifest.sig".to_string(), signature.into_bytes()),
        ]);
        let mut options = options();
        options.require_signature = true;
        let error = Registry::new(&state)
            .install(&bundle, &options)
            .expect_err("untrusted");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        assert!(Registry::new(&state).list().expect("list").is_empty());
        fs::remove_dir_all(&state).expect("cleanup");
    }
}
