//! The registry: installed apps under `state/apps/<id>` (§6.8, §6.1).
//!
//! A bundle becomes visible only after `bundle::extract` has verified it. The
//! move is a rename inside the state tree, so an observer sees the app appear
//! whole or not at all. The registry never grants authority: an installed app
//! still needs a grant to run.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use anth_contract::id::Namespace;
use anth_contract::{AppId, Descriptor, Error, Grant, Manifest, Tier, DESCRIPTOR_VERSION};

use crate::bundle::{self, Options, StagedBundle};
use crate::grant::GrantStore;
use crate::keys::{Distribution, Keyring};
use crate::store;
use crate::{ApprovalMaterial, InstalledApp};

static SWAP: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct Installed {
    pub manifest: Manifest,
    pub descriptor: Descriptor,
}

impl Installed {
    /// The alias the CLI uses: the operator's override, then the manifest's.
    pub fn alias(&self) -> Option<&str> {
        self.descriptor
            .alias
            .as_deref()
            .or_else(|| self.manifest.cli_name())
    }

    /// Whether `grant` was approved for exactly this manifest: same app, same
    /// version, same interface sets. The resolver uses the same rule.
    pub fn grant_matches(&self, grant: &Grant) -> bool {
        let Ok(exports) =
            crate::parse_interfaces(self.manifest.export.iter().map(|e| &e.interface))
        else {
            return false;
        };
        let Ok(imports) =
            crate::parse_interfaces(self.manifest.import.iter().map(|i| &i.interface))
        else {
            return false;
        };
        crate::grant_matches(&self.manifest, grant, &exports, &imports)
    }
}

/// An installed app the catalogue could not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub app: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Registry {
    apps: PathBuf,
    keyring: Keyring,
    distribution: Distribution,
}

/// The catalogue and the compose-side projection of each app.
pub type InstalledSet = (Vec<Installed>, Vec<InstalledApp>, Vec<Problem>);

impl Registry {
    pub fn new(state: &Path) -> Self {
        Self::with_distribution(state, Distribution::embedded())
    }

    /// A registry with an explicit distribution key. The engine uses the
    /// compiled-in one; tests and the release tooling pick their own.
    pub fn with_distribution(state: &Path, distribution: Distribution) -> Self {
        Self {
            apps: state.join("apps"),
            keyring: Keyring::new(state),
            distribution,
        }
    }

    /// The trusted signers. The keyring is part of install, not composition.
    pub fn keyring(&self) -> &Keyring {
        &self.keyring
    }

    /// The distribution key this engine trusts, compiled in (§9.1).
    pub fn distribution(&self) -> &Distribution {
        &self.distribution
    }

    /// The installed tree of one app. Asset serving joins a manifest-declared
    /// relative module under it; callers must not invent a path.
    pub fn app_dir(&self, app: &AppId) -> std::path::PathBuf {
        self.apps.join(app.as_str())
    }

    pub fn dir(&self) -> &Path {
        &self.apps
    }

    /// Verifies and installs a bundle. An id that is already installed is a
    /// conflict; replacing is an explicit operation.
    pub fn install(&self, bundle: &[u8], options: &Options) -> Result<Installed, Error> {
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
        let descriptor = self.descriptor_for(&staged, &id, true, None, options)?;
        // A fallback alias may collide with another fallback (§7.9 resolves it
        // with the full id), but not with an operator alias.
        if let Some(other) = self.alias_conflict(&id, manifest.cli_name(), false)? {
            return Err(Error::conflict(format!(
                "the CLI alias `{}` belongs to `{other}`",
                manifest.cli_name().unwrap_or_default()
            )));
        }
        write_staged_descriptor(staged.root(), &descriptor)?;
        publish(staged, &target)?;
        sync_dir(&self.apps)?;
        Ok(Installed {
            manifest,
            descriptor,
        })
    }

    /// Replaces the bundle of an installed app. The id lives inside the bundle,
    /// so a bundle declaring another one is a conflict, not a rename. The
    /// operator's `enabled` and `alias` survive; authority does not.
    pub fn replace(
        &self,
        app: &AppId,
        bundle: &[u8],
        options: &Options,
    ) -> Result<Installed, Error> {
        let target = self.apps.join(app.as_str());
        let previous = self
            .get(app)?
            .ok_or_else(|| Error::not_found(format!("{} is not installed", target.display())))?;
        let staged = self.extract(bundle, options)?;
        let manifest = staged.manifest().clone();
        if manifest.app.id != app.as_str() {
            return Err(Error::conflict(format!(
                "the bundle declares `{}`, not `{app}`",
                manifest.app.id
            )));
        }
        let descriptor = self.descriptor_for(
            &staged,
            app,
            previous.descriptor.enabled,
            previous.descriptor.alias.clone(),
            options,
        )?;
        // The new manifest may change the fallback alias (its `[app] alias`),
        // and the operator alias may be absent.
        let effective = descriptor.alias.as_deref().or_else(|| manifest.cli_name());
        let operator = descriptor.alias.is_some();
        if let Some(other) = self.alias_conflict(app, effective, operator)? {
            return Err(Error::conflict(format!(
                "`{}` is already the CLI alias of `{other}`",
                effective.unwrap_or_default()
            )));
        }
        write_staged_descriptor(staged.root(), &descriptor)?;
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
        Ok(Installed {
            manifest,
            descriptor,
        })
    }

    pub fn get(&self, app: &AppId) -> Result<Option<Installed>, Error> {
        let target = self.apps.join(app.as_str());
        match fs::symlink_metadata(&target) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(Error::internal(format!(
                "inspecting {}: {error}",
                target.display()
            ))),
            Ok(_) => Ok(Some(read_app(&target, &self.distribution)?)),
        }
    }

    /// Every installed app, sorted by id. Staging and swap directories are
    /// hidden by their leading dot.
    pub fn list(&self) -> Result<Vec<Installed>, Error> {
        let mut installed = Vec::new();
        for entry in self.visible_entries()? {
            let name = entry.file_name();
            let id = AppId::parse(&name.to_string_lossy())?;
            if let Some(app) = self.get(&id)? {
                installed.push(app);
            }
        }
        installed.sort_by(|a, b| a.manifest.app.id.cmp(&b.manifest.app.id));
        Ok(installed)
    }

    /// Every app that can be read, plus one problem per tree that cannot.
    ///
    /// The catalogue must not disappear because one install is broken: the
    /// operator needs the rest of it to repair the broken one.
    pub fn catalogue(&self) -> Result<(Vec<Installed>, Vec<Problem>), Error> {
        let mut apps = Vec::new();
        let mut problems = Vec::new();
        for entry in self.visible_entries()? {
            let app = entry.file_name().to_string_lossy().to_string();
            match read_app(&entry.path(), &self.distribution) {
                Ok(installed) => apps.push(installed),
                Err(error) => problems.push(Problem {
                    app,
                    message: error.message().to_string(),
                }),
            }
        }
        apps.sort_by(|a, b| a.manifest.app.id.cmp(&b.manifest.app.id));
        problems.sort_by(|a, b| a.app.cmp(&b.app));
        Ok((apps, problems))
    }

    /// The app directories a reader may see: staging and swap trees start with
    /// a dot, and a missing tree is empty rather than an error.
    fn visible_entries(&self) -> Result<Vec<fs::DirEntry>, Error> {
        if !store::check_dir(&self.apps)? {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&self.apps).map_err(|error| {
            Error::internal(format!("reading {}: {error}", self.apps.display()))
        })?;
        let mut visible = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                Error::internal(format!("reading {}: {error}", self.apps.display()))
            })?;
            if !entry.file_name().to_string_lossy().starts_with('.') {
                visible.push(entry);
            }
        }
        Ok(visible)
    }

    /// The installed catalogue plus the compose-side view of each app: its
    /// grant (or the fact that it is unreadable), enabled flag and approval
    /// material. Boot and the API use the same projection (§7.12).
    pub fn installed_apps(&self, grants: &GrantStore) -> Result<InstalledSet, Error> {
        let (apps, problems) = self.catalogue()?;
        let mut installed = Vec::with_capacity(apps.len());
        for app in &apps {
            let id = AppId::parse(&app.manifest.app.id)?;
            let (grant, grant_invalid) = match grants.load(&id) {
                Ok(grant) => (grant, false),
                Err(_) => (None, true),
            };
            installed.push(InstalledApp {
                manifest: app.manifest.clone(),
                grant,
                grant_invalid,
                enabled: app.descriptor.enabled,
                material: self.approval_material(&id).ok().flatten(),
            });
        }
        Ok((apps, installed, problems))
    }

    /// The app's WIT files by file name, for `anth app fetch-wit`.
    pub fn wit_files(&self, app: &AppId) -> Result<BTreeMap<String, String>, Error> {
        let mut files = BTreeMap::new();
        for (name, bytes) in self.wit_set(app)? {
            let text = String::from_utf8(bytes)
                .map_err(|error| Error::invalid(format!("{name} is not UTF-8: {error}")))?;
            files.insert(name.strip_prefix("wit/").unwrap_or(&name).to_string(), text);
        }
        Ok(files)
    }

    /// The WIT files in the shape the hash and the signature use.
    fn wit_set(&self, app: &AppId) -> Result<Vec<(String, Vec<u8>)>, Error> {
        let dir = self.apps.join(app.as_str()).join("wit");
        let entries = fs::read_dir(&dir).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::invalid(format!("{} has no WIT files", dir.display()))
            } else {
                Error::internal(format!("reading {}: {error}", dir.display()))
            }
        })?;
        let mut wit = Vec::new();
        for entry in entries {
            let entry = entry
                .map_err(|error| Error::internal(format!("reading {}: {error}", dir.display())))?;
            if !entry
                .file_type()
                .map_err(|error| Error::internal(error.to_string()))?
                .is_file()
            {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".wit") {
                continue;
            }
            let path = entry.path();
            let bytes = fs::read(&path)
                .map_err(|error| Error::internal(format!("reading {}: {error}", path.display())))?;
            wit.push((format!("wit/{name}"), bytes));
        }
        if wit.is_empty() {
            return Err(Error::invalid(format!(
                "{} has no WIT files",
                dir.display()
            )));
        }
        wit.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(wit)
    }

    /// A fingerprint of the WIT set, the manifest and the signature, ready for a
    /// grant (D-Wit-Hash).
    pub fn approval_material(&self, app: &AppId) -> Result<Option<ApprovalMaterial>, Error> {
        let dir = self.apps.join(app.as_str());
        match fs::symlink_metadata(&dir) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(Error::internal(format!(
                    "inspecting {}: {error}",
                    dir.display()
                )));
            }
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(Error::invalid(format!(
                    "{} is not a real app directory",
                    dir.display()
                )));
            }
            Ok(_) => {}
        }
        let manifest_path = dir.join("manifest.toml");
        let manifest = fs::read(&manifest_path).map_err(|error| {
            Error::internal(format!("reading {}: {error}", manifest_path.display()))
        })?;
        let wit = self.wit_set(app)?;
        let wit_sha256 =
            crate::signature::sha256_hex(&anth_contract::signature::wit_message(&wit)?);
        let signature = match fs::read(dir.join("manifest.sig")) {
            Ok(bytes) => Some(
                String::from_utf8(bytes)
                    .map_err(|error| Error::invalid(format!("manifest.sig is not UTF-8: {error}")))?
                    .trim()
                    .to_string(),
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(Error::internal(format!(
                    "reading {}: {error}",
                    dir.join("manifest.sig").display()
                )));
            }
        }
        .filter(|text| !text.is_empty());
        Ok(Some(ApprovalMaterial {
            manifest_sha256: crate::signature::sha256_hex(&manifest),
            wit_sha256,
            signature,
        }))
    }

    /// Apps that import an interface this app exports. Uninstall, disable and
    /// revoke need an acknowledgment when this is not empty (§6.4).
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

    /// Enables or disables the app. This is operator state, not authority: the
    /// grant and the manifest are untouched.
    pub fn set_enabled(&self, app: &AppId, enabled: bool) -> Result<Installed, Error> {
        self.configure(app, Some(enabled), None)
    }

    /// Sets the operator's CLI alias for the app. It overrides the manifest's
    /// alias and must not be ambiguous with another app's effective alias.
    pub fn set_alias(&self, app: &AppId, alias: Option<String>) -> Result<Installed, Error> {
        self.configure(app, None, Some(alias))
    }

    /// Applies the mutable descriptor fields in **one** write, so a request that
    /// changes both cannot leave one of them applied.
    pub fn configure(
        &self,
        app: &AppId,
        enabled: Option<bool>,
        alias: Option<Option<String>>,
    ) -> Result<Installed, Error> {
        let mut installed = self.installed(app)?;
        if let Some(enabled) = enabled {
            installed.descriptor.enabled = enabled;
        }
        if let Some(alias) = alias {
            installed.descriptor.alias = alias;
        }
        let effective = installed
            .descriptor
            .alias
            .as_deref()
            .or_else(|| installed.manifest.cli_name());
        let operator = installed.descriptor.alias.is_some();
        if let Some(other) = self.alias_conflict(app, effective, operator)? {
            return Err(Error::conflict(format!(
                "`{}` is already the CLI alias of `{other}`",
                effective.unwrap_or_default()
            )));
        }
        self.save_descriptor(app, &installed.descriptor)?;
        Ok(installed)
    }

    fn installed(&self, app: &AppId) -> Result<Installed, Error> {
        self.get(app)?
            .ok_or_else(|| Error::not_found(format!("`{app}` is not installed")))
    }

    fn save_descriptor(&self, app: &AppId, descriptor: &Descriptor) -> Result<(), Error> {
        descriptor.validate()?;
        let dir = self.apps.join(app.as_str());
        store::write_atomic(&dir, "descriptor.toml", descriptor.to_toml()?.as_bytes())?;
        sync_dir(&dir)
    }

    /// Whether `alias` on `app` is ambiguous with another installed app.
    ///
    /// A fallback may collide with another fallback (§7.9 resolves it with the
    /// full id); an operator alias may not collide with anything, and no
    /// fallback may shadow another app's operator alias.
    fn alias_conflict(
        &self,
        app: &AppId,
        alias: Option<&str>,
        operator: bool,
    ) -> Result<Option<AppId>, Error> {
        let Some(alias) = alias else {
            return Ok(None);
        };
        for other in self.readable()? {
            if other.manifest.app.id == app.as_str() {
                continue;
            }
            if other.alias() == Some(alias) && (operator || other.descriptor.alias.is_some()) {
                return Ok(Some(AppId::parse(&other.manifest.app.id)?));
            }
        }
        Ok(None)
    }

    /// Extracts into `apps/` with the packages of installed providers added to
    /// the set the bundle is allowed to import.
    fn extract(&self, bundle: &[u8], options: &Options) -> Result<StagedBundle, Error> {
        let mut options = options.clone();
        options.known_packages.extend(self.known_packages()?);
        bundle::extract(
            bundle,
            &self.apps,
            &options,
            &self.keyring,
            &self.distribution,
        )
    }

    /// The descriptor an install produces.
    ///
    /// The tier comes from the signature: only the distribution key mints
    /// `system`, and only a `system` bundle may carry a service without the
    /// third-party switch (`D-Service-Third-Party`).
    fn descriptor_for(
        &self,
        staged: &StagedBundle,
        id: &AppId,
        enabled: bool,
        alias: Option<String>,
        options: &Options,
    ) -> Result<Descriptor, Error> {
        let manifest = staged.manifest();
        let tier = staged.tier();
        check_service_tier(manifest, tier, options)?;
        check_namespace_tier(manifest, tier, &self.distribution)?;
        Ok(Descriptor {
            version: DESCRIPTOR_VERSION,
            app: id.clone(),
            enabled,
            alias,
            signer: staged.signer().map(str::to_string),
            tier,
            installed_at: now(),
        })
    }

    /// Packages of the installed apps, skipping any tree that cannot be read.
    ///
    /// One corrupt install must not stop the operator from installing or
    /// replacing something else; `list` is where corruption is reported. Only
    /// the manifest matters here, so a missing descriptor does not hide a
    /// provider.
    fn known_packages(&self) -> Result<Vec<String>, Error> {
        let mut packages = Vec::new();
        for entry in self.visible_entries()? {
            if let Ok(manifest) = read_manifest(&entry.path()) {
                packages.push(manifest.app.namespace);
            }
        }
        Ok(packages)
    }

    /// The installed apps that can be read, skipping a corrupt tree.
    fn readable(&self) -> Result<Vec<Installed>, Error> {
        let mut apps = Vec::new();
        for entry in self.visible_entries()? {
            if let Ok(installed) = read_app(&entry.path(), &self.distribution) {
                apps.push(installed);
            }
        }
        Ok(apps)
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
    error.kind() == std::io::ErrorKind::Unsupported
        || matches!(
            error.raw_os_error(),
            Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::EOPNOTSUPP)
        )
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

/// Reads a real app directory: no symlink, a directory, and a manifest that
/// names the directory it lives in.
fn read_app(dir: &Path, distribution: &Distribution) -> Result<Installed, Error> {
    let metadata = fs::symlink_metadata(dir)
        .map_err(|error| Error::internal(format!("inspecting {}: {error}", dir.display())))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::invalid(format!(
            "{} is not a real app directory",
            dir.display()
        )));
    }
    read_installed(dir, distribution)
}

/// Reads the manifest and the descriptor and refuses a pair that names two
/// different apps: the directory is one app or it is corrupt.
fn read_installed(dir: &Path, distribution: &Distribution) -> Result<Installed, Error> {
    let manifest = read_manifest(dir)?;
    let descriptor = read_descriptor(dir, distribution)?;
    if descriptor.app.as_str() != manifest.app.id {
        return Err(Error::invalid(format!(
            "{} descriptor names `{}` but the manifest is `{}`",
            dir.display(),
            descriptor.app,
            manifest.app.id
        )));
    }
    check_namespace_tier(&manifest, descriptor.tier, distribution)?;
    let expected = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Error::invalid(format!(
                "{} is not a valid app directory name",
                dir.display()
            ))
        })?;
    if manifest.app.id != expected {
        return Err(Error::invalid(format!(
            "{} declares `{}`, not `{expected}`",
            dir.display(),
            manifest.app.id
        )));
    }
    Ok(Installed {
        manifest,
        descriptor,
    })
}

/// A service is never silently allowed: the tier decides, and the deployment's
/// startup switch decides for third-party (D-Service-Third-Party).
fn check_service_tier(manifest: &Manifest, tier: Tier, options: &Options) -> Result<(), Error> {
    if manifest.service.is_some() && tier == Tier::ThirdParty && !options.allow_third_party_service
    {
        return Err(Error::denied(
            "this engine refuses third-party service bundles; start it with \
             `[services] allow-third-party = true` to opt in",
        ));
    }
    Ok(())
}

/// The first-party namespace is a tier, not a guess (§7.3, D-Namespace).
///
/// The manifest already refuses `anth`, which belongs to the engine. `anthracite`
/// belongs to apps the distribution key signs: a release build refuses it from
/// anything `third-party`, so a random bundle cannot impersonate a shipped app's
/// interface ids. A build without a key has no `system` tier at all — it is the
/// development build, where the shipped apps install as `third-party` — so the
/// rule only bites where there is a release key to compare against.
fn check_namespace_tier(
    manifest: &Manifest,
    tier: Tier,
    distribution: &Distribution,
) -> Result<(), Error> {
    distribution.check()?;
    if tier == Tier::System || distribution.fingerprint().is_none() {
        return Ok(());
    }
    let namespace = manifest
        .app
        .namespace
        .split_once(':')
        .map(|(namespace, _)| namespace)
        .unwrap_or_default();
    if Namespace::of(namespace) == Namespace::FirstParty {
        return Err(Error::denied(format!(
            "`{}` uses the first-party `anthracite` namespace; only a bundle \
             signed by the distribution key may use it",
            manifest.app.namespace
        )));
    }
    Ok(())
}

fn read_descriptor(dir: &Path, distribution: &Distribution) -> Result<Descriptor, Error> {
    let path = dir.join("descriptor.toml");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        // An app installed before the descriptor existed: synthesize the
        // defaults instead of making the whole registry unreadable.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return legacy_descriptor(dir);
        }
        Err(error) => {
            return Err(Error::internal(format!(
                "reading {}: {error}",
                path.display()
            )));
        }
    };
    let descriptor = Descriptor::parse(&text)
        .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
    descriptor
        .validate()
        .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
    check_system_descriptor(dir, &descriptor, distribution)
        .map_err(|error| Error::invalid(format!("{}: {}", path.display(), error.message())))?;
    Ok(descriptor)
}

/// The `system` tier is a signature, not a claim.
///
/// The descriptor is engine-written, but it lives on disk next to a bundle: a
/// hand-edited `tier = "system"` must not grant the privileges of a release
/// signature, and a build without a distribution key has no system tier at all.
/// The stored `manifest.sig` is re-verified, so a rewritten `signer` field
/// proves nothing.
fn check_system_descriptor(
    dir: &Path,
    descriptor: &Descriptor,
    distribution: &Distribution,
) -> Result<(), Error> {
    if descriptor.tier != Tier::System {
        return Ok(());
    }
    distribution.check()?;
    match (distribution.fingerprint(), descriptor.signer.as_deref()) {
        (Some(key), Some(signer)) if key == signer => {}
        (None, _) => {
            return Err(Error::denied(
                "this engine was built without a distribution key, so it has no `system` tier",
            ));
        }
        _ => {
            return Err(Error::denied(
                "the descriptor claims the `system` tier without the distribution signature",
            ));
        }
    }
    let signature = store::read(&dir.join("manifest.sig"))?
        .ok_or_else(|| Error::denied("the `system` descriptor has no bundle signature to check"))?;
    match distribution.verify(&bundle::signed_message(dir)?, &signature)? {
        Some(_) => Ok(()),
        None => Err(Error::denied(
            "the stored bundle is not signed by the distribution key",
        )),
    }
}

/// The descriptor a pre-descriptor install would have had.
fn legacy_descriptor(dir: &Path) -> Result<Descriptor, Error> {
    let manifest = read_manifest(dir)?;
    let installed_at = fs::metadata(dir)
        .and_then(|metadata| metadata.modified())
        .map(|time| humantime::format_rfc3339_seconds(time).to_string())
        .unwrap_or_else(|_| now());
    Ok(Descriptor {
        version: DESCRIPTOR_VERSION,
        app: AppId::parse(&manifest.app.id)?,
        enabled: true,
        alias: None,
        signer: None,
        tier: Tier::ThirdParty,
        installed_at,
    })
}

/// Writes the descriptor into the staging tree before it becomes visible, so an
/// installed app always has one.
fn write_staged_descriptor(root: &Path, descriptor: &Descriptor) -> Result<(), Error> {
    descriptor.validate()?;
    let path = root.join("descriptor.toml");
    let text = descriptor.to_toml()?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(store::FILE_MODE)
        .open(&path)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                Error::invalid(format!(
                    "{} already has a descriptor; the engine owns that file",
                    root.display()
                ))
            } else {
                Error::internal(format!("creating {}: {error}", path.display()))
            }
        })?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| Error::internal(format!("writing {}: {error}", path.display())))?;
    sync_dir(root)
}

fn now() -> String {
    humantime::format_rfc3339_seconds(SystemTime::now()).to_string()
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
    use crate::grant::GrantStore;
    use anth_contract::{Grant, KeyAlgorithm, Risk, TrustedKey, GRANT_VERSION, KEY_VERSION};
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

    fn plain_bundle(id: &str, namespace: &str, alias: Option<&str>) -> Vec<u8> {
        let alias_line = alias
            .map(|alias| format!("alias = \"{alias}\"\n"))
            .unwrap_or_default();
        let manifest = format!(
            "manifest_version = 2\n\n[app]\nid = \"{id}\"\n{alias_line}namespace = \"{namespace}\"\nname = \"Test\"\nversion = \"0.1.0\"\n"
        );
        let wit = format!("package {namespace}@1.0.0;\nworld app {{}}\n");
        zip(&[
            ("manifest.toml".to_string(), manifest.into_bytes()),
            ("wit/test.wit".to_string(), wit.into_bytes()),
        ])
    }

    fn other_bundle() -> Vec<u8> {
        plain_bundle("org.example.other", "example:other", None)
    }

    /// A first-party-shaped app: the `anthracite` namespace with no exports, so
    /// the tier rule is the only thing being tested.
    fn first_party_entries() -> Vec<(String, Vec<u8>)> {
        let manifest = "manifest_version = 2\n\n[app]\nid = \"org.anthracite.notes\"\nnamespace = \"anthracite:notes\"\nname = \"Notes\"\nversion = \"0.1.0\"\n";
        let wit = "package anthracite:notes@1.0.0;\nworld app {}\n";
        vec![
            ("manifest.toml".to_string(), manifest.as_bytes().to_vec()),
            ("wit/notes.wit".to_string(), wit.as_bytes().to_vec()),
        ]
    }

    fn trusted(public_key: &SigningKey) -> TrustedKey {
        TrustedKey {
            version: KEY_VERSION,
            algorithm: KeyAlgorithm::Ed25519,
            public_key: hex(&public_key.verifying_key().to_bytes()),
            label: None,
            added_by: "operator-test".to_string(),
            added_at: "2026-11-13T20:00:00Z".to_string(),
        }
    }

    fn service_bundle() -> Vec<u8> {
        zip(&service_entries())
    }

    fn service_entries() -> Vec<(String, Vec<u8>)> {
        let binary = b"\x7fELF a fake service";
        let manifest = format!(
            "manifest_version = 3\n\n[app]\nid = \"org.example.service\"\nnamespace = \"example:service\"\nname = \"Service\"\nversion = \"0.1.0\"\n\n[service]\npath = \"bin/service\"\nsha256 = \"{}\"\nclass = \"socket\"\nargv = [\"--socket\", \"{{socket}}\"]\n\n[[export]]\ninterface = \"example:service/notes@1.0.0\"\n",
            hex(&Sha256::digest(binary))
        );
        let wit = "package example:service@1.0.0;\ninterface notes { all: func(); }\nworld app { export notes; }\n";
        vec![
            ("manifest.toml".to_string(), manifest.into_bytes()),
            ("wit/service.wit".to_string(), wit.as_bytes().to_vec()),
            ("bin/service".to_string(), binary.to_vec()),
        ]
    }

    /// Signs the canonical message of `entries` and appends `manifest.sig`.
    fn signed(mut entries: Vec<(String, Vec<u8>)>, signing: &SigningKey) -> Vec<u8> {
        let files = entries
            .iter()
            .filter(|(name, _)| name == "manifest.toml" || name.starts_with("wit/"))
            .cloned()
            .collect::<Vec<_>>();
        let message = anth_contract::signature::message(&files).expect("message");
        let signature = crate::signature::signature_base64(&signing.sign(&message).to_bytes());
        entries.push(("manifest.sig".to_string(), signature.into_bytes()));
        zip(&entries)
    }

    fn distribution(signing: &SigningKey) -> Distribution {
        Distribution::from_public_key(Some(&hex(&signing.verifying_key().to_bytes())))
    }

    fn options() -> Options {
        Options {
            max_bundle_bytes: 1 << 20,
            require_signature: false,
            allow_third_party_service: false,
            known_packages: Vec::new(),
        }
    }

    #[test]
    fn a_third_party_service_is_refused_unless_allowed() {
        let state = scratch("service-switch");
        let registry = Registry::new(&state);
        let error = registry
            .install(&service_bundle(), &options())
            .expect_err("third-party service");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        assert!(error.message().contains("allow-third-party"), "{error}");
        assert!(!registry.dir().join("org.example.service").exists());

        let mut allowed = options();
        allowed.allow_third_party_service = true;
        let installed = registry
            .install(&service_bundle(), &allowed)
            .expect("opted in");
        assert_eq!(installed.manifest.app.id, "org.example.service");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_service_replacement_is_gated_too() {
        let state = scratch("service-replace");
        let registry = Registry::new(&state);
        let mut allowed = options();
        allowed.allow_third_party_service = true;
        registry
            .install(&service_bundle(), &allowed)
            .expect("install");
        let app = AppId::parse("org.example.service").unwrap();
        let error = registry
            .replace(&app, &service_bundle(), &options())
            .expect_err("switch is off");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn install_list_get_and_remove() {
        let state = scratch("lifecycle");
        let registry = Registry::new(&state);
        let installed = registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        assert_eq!(installed.manifest.app.id, "org.example.notes");
        assert_eq!(installed.descriptor.tier, Tier::ThirdParty);
        assert!(installed.descriptor.enabled);
        assert!(installed.descriptor.signer.is_none());
        assert!(registry.dir().join("org.example.notes").is_dir());

        let installed = registry
            .get(&AppId::parse("org.example.notes").unwrap())
            .expect("get")
            .expect("installed");
        assert_eq!(installed.manifest.app.version, "0.1.0");

        let list = registry.list().expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].manifest.app.id, "org.example.notes");

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
                .manifest
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
            registry
                .get(&app)
                .expect("get")
                .unwrap()
                .manifest
                .app
                .version,
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
            registry.get(&app).expect("get").unwrap().manifest.app.id,
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
    fn the_descriptor_is_part_of_the_installed_app() {
        let state = scratch("descriptor");
        let registry = Registry::new(&state);
        let installed = registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        let path = registry.dir().join("org.example.notes/descriptor.toml");
        let text = fs::read_to_string(&path).expect("descriptor");
        let descriptor = Descriptor::parse(&text).expect("parse");
        assert_eq!(descriptor, installed.descriptor);
        assert!(descriptor.enabled);
        assert_eq!(descriptor.tier, Tier::ThirdParty);
        assert!(descriptor.signer.is_none());
        assert!(
            humantime::parse_rfc3339(&descriptor.installed_at).is_ok(),
            "installed_at is RFC 3339"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn set_enabled_and_alias_round_trip() {
        let state = scratch("patch");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        let installed = registry.set_enabled(&app, false).expect("disable");
        assert!(!installed.descriptor.enabled);
        let installed = registry
            .set_alias(&app, Some("notes".to_string()))
            .expect("alias");
        assert_eq!(installed.descriptor.alias.as_deref(), Some("notes"));
        let reread = registry.get(&app).expect("get").unwrap();
        assert!(!reread.descriptor.enabled);
        assert_eq!(reread.descriptor.alias.as_deref(), Some("notes"));
        let error = registry
            .set_alias(&app, Some("list".to_string()))
            .expect_err("reserved");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn replace_preserves_enabled_and_alias() {
        let state = scratch("replace-descriptor");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        registry.set_enabled(&app, false).expect("disable");
        registry
            .set_alias(&app, Some("notes".to_string()))
            .expect("alias");
        let newer = b"\0asm a newer component";
        let installed = registry
            .replace(&app, &notes_bundle("0.2.0", newer), &options())
            .expect("replace");
        assert!(!installed.descriptor.enabled);
        assert_eq!(installed.descriptor.alias.as_deref(), Some("notes"));
        assert_eq!(installed.manifest.app.version, "0.2.0");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_descriptor_that_names_another_app_is_refused() {
        let state = scratch("descriptor-mismatch");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        let path = registry.dir().join("org.example.notes/descriptor.toml");
        let text = fs::read_to_string(&path)
            .expect("read")
            .replace("org.example.notes", "org.example.other");
        fs::write(&path, text).expect("write");
        let error = registry.get(&app).expect_err("mismatch");
        assert!(error.message().contains("descriptor names"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_old_install_gets_a_default_descriptor() {
        let state = scratch("legacy");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        fs::remove_file(registry.dir().join("org.example.notes/descriptor.toml"))
            .expect("remove descriptor");
        let installed = registry.get(&app).expect("get").expect("installed");
        assert!(installed.descriptor.enabled);
        assert_eq!(installed.descriptor.tier, Tier::ThirdParty);
        assert!(installed.descriptor.signer.is_none());
        // The next write creates the file again.
        registry.set_enabled(&app, false).expect("disable");
        assert!(registry
            .dir()
            .join("org.example.notes/descriptor.toml")
            .is_file());
        assert!(!registry.get(&app).unwrap().unwrap().descriptor.enabled);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_keyless_build_refuses_a_system_descriptor() {
        let state = scratch("system-keyless");
        let registry = Registry::with_distribution(&state, Distribution::none());
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        let path = registry.dir().join("org.example.notes/descriptor.toml");
        let text = fs::read_to_string(&path)
            .expect("read")
            .replace("tier = \"third-party\"", "tier = \"system\"");
        fs::write(&path, text).expect("write");
        let error = registry.get(&app).expect_err("system tier");
        assert!(error.message().contains("distribution key"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_operator_alias_must_be_unique() {
        let state = scratch("alias-unique");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install notes");
        registry
            .install(&other_bundle(), &options())
            .expect("install other");
        // `other` has no manifest alias, so its effective alias is the last
        // label of its id.
        let error = registry
            .set_alias(&app, Some("other".to_string()))
            .expect_err("duplicate alias");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        registry
            .set_alias(&app, Some("notes".to_string()))
            .expect("free alias");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn the_operator_alias_wins_over_the_manifest() {
        let state = scratch("alias-precedence");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        assert_eq!(registry.get(&app).unwrap().unwrap().alias(), Some("notes"));
        let installed = registry
            .set_alias(&app, Some("custom".to_string()))
            .expect("alias");
        assert_eq!(installed.alias(), Some("custom"));
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_fallback_may_share_another_fallback() {
        let state = scratch("fallback-collision");
        let registry = Registry::new(&state);
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install notes");
        // `com.example.notes` falls back to `notes` too; §7.9 resolves it with
        // the full id, so installing it is allowed.
        registry
            .install(
                &plain_bundle("com.example.notes", "example:sharednotes", None),
                &options(),
            )
            .expect("fallback collision is allowed");
        assert_eq!(registry.list().expect("list").len(), 2);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_install_may_not_shadow_an_operator_alias() {
        let state = scratch("shadow-alias");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        registry
            .set_alias(&app, Some("custom".to_string()))
            .expect("alias");
        let error = registry
            .install(
                &plain_bundle("org.example.custom", "example:custom", None),
                &options(),
            )
            .expect_err("shadowed operator alias");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn clearing_an_alias_checks_the_fallback() {
        let state = scratch("clear-alias");
        let registry = Registry::new(&state);
        let notes = AppId::parse("org.example.notes").unwrap();
        let other = AppId::parse("org.example.other").unwrap();
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install notes");
        registry
            .install(&other_bundle(), &options())
            .expect("install other");
        registry
            .set_alias(&notes, Some("custom".to_string()))
            .expect("notes is custom");
        // `other` may take `notes` now: the notes app no longer uses it.
        registry
            .set_alias(&other, Some("notes".to_string()))
            .expect("notes is free");
        // Clearing notes falls back to `notes`, which `other` now owns.
        let error = registry
            .set_alias(&notes, None)
            .expect_err("fallback shadowed");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn replace_checks_the_new_manifest_alias() {
        let state = scratch("replace-alias");
        let registry = Registry::new(&state);
        let notes = AppId::parse("org.example.notes").unwrap();
        registry
            .install(
                &plain_bundle("org.example.notes", "example:notesalias", None),
                &options(),
            )
            .expect("install notes");
        registry
            .install(&other_bundle(), &options())
            .expect("install other");
        registry
            .set_alias(
                &AppId::parse("org.example.other").unwrap(),
                Some("custom".to_string()),
            )
            .expect("alias");
        // The replacement's manifest alias becomes the app's fallback, and it
        // collides with `other`'s operator alias.
        let error = registry
            .replace(
                &notes,
                &plain_bundle("org.example.notes", "example:notesalias", Some("custom")),
                &options(),
            )
            .expect_err("alias");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        assert_eq!(
            registry.get(&notes).unwrap().unwrap().manifest.app.version,
            "0.1.0",
            "the original stays installed"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn the_catalogue_does_not_follow_a_symlink() {
        let state = scratch("catalogue-symlink");
        let registry = Registry::new(&state);
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        std::os::unix::fs::symlink(
            registry.dir().join("org.example.notes"),
            registry.dir().join("org.example.alias"),
        )
        .expect("symlink");
        let (apps, problems) = registry.catalogue().expect("catalogue");
        assert_eq!(apps.len(), 1, "the link is not a second app");
        assert!(
            problems
                .iter()
                .any(|problem| problem.app == "org.example.alias"),
            "{problems:?}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn the_catalogue_reports_a_directory_that_declares_another_app() {
        let state = scratch("catalogue-mismatch");
        let registry = Registry::new(&state);
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        let copy = registry.dir().join("org.example.other");
        fs::create_dir(&copy).expect("mkdir");
        for name in ["manifest.toml", "descriptor.toml"] {
            fs::copy(
                registry.dir().join("org.example.notes").join(name),
                copy.join(name),
            )
            .expect("copy");
        }
        let wasm = copy.join("wasm");
        fs::create_dir(&wasm).expect("mkdir");
        fs::copy(
            registry.dir().join("org.example.notes/wasm/notes.wasm"),
            wasm.join("notes.wasm"),
        )
        .expect("copy");
        let (apps, problems) = registry.catalogue().expect("catalogue");
        assert_eq!(apps.len(), 1);
        assert!(
            problems
                .iter()
                .any(|problem| problem.app == "org.example.other"),
            "{problems:?}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn grant_matches_requires_the_same_version() {
        let state = scratch("grant-matches");
        let registry = Registry::new(&state);
        let app = AppId::parse("org.example.notes").unwrap();
        let installed = registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        grant_for(&state, &app, "0.1.0");
        let stored = GrantStore::new(&state)
            .load(&app)
            .expect("load")
            .expect("grant");
        assert!(installed.grant_matches(&stored));
        let mut other = stored.clone();
        other.app_version = "9.9.9".to_string();
        assert!(!installed.grant_matches(&other));
        fs::remove_dir_all(&state).expect("cleanup");
    }

    fn grant_for(state: &std::path::Path, app: &AppId, version: &str) {
        let grant = Grant {
            version: GRANT_VERSION,
            app: app.clone(),
            app_version: version.to_string(),
            manifest_sha256: "a".repeat(64),
            wit_sha256: "b".repeat(64),
            signature: None,
            risk: Risk::Read,
            accepted: Vec::new(),
            approved_by: "operator-test".to_string(),
            approved_at: "2026-11-03T20:00:00Z".to_string(),
            export: vec![
                anth_contract::InterfaceId::parse("example:notes/notes@1.0.0").expect("interface"),
            ],
            import: Vec::new(),
        };
        GrantStore::new(state).save(&grant).expect("save grant");
    }

    #[test]
    fn a_signed_bundle_is_installed_with_its_signer() {
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
        let installed = Registry::new(&state)
            .install(&bundle, &options)
            .expect("signed install");
        let fingerprint =
            crate::signature::fingerprint(&hex(&signing.verifying_key().to_bytes())).unwrap();
        assert_eq!(
            installed.descriptor.signer.as_deref(),
            Some(fingerprint.as_str())
        );
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

    #[test]
    fn the_distribution_key_mints_the_system_tier() {
        let state = scratch("distribution-tier");
        let release = SigningKey::from_bytes(&[9u8; 32]);
        let registry = Registry::with_distribution(&state, distribution(&release));
        let bundle = signed(service_entries(), &release);
        // No operator key and no `allow-third-party`: a system service installs.
        let installed = registry
            .install(&bundle, &options())
            .expect("system install");
        assert_eq!(installed.descriptor.tier, Tier::System);
        assert_eq!(
            installed.descriptor.signer.as_deref(),
            registry.distribution().fingerprint()
        );
        assert_eq!(
            installed.manifest.service.expect("service").class.as_str(),
            "socket"
        );
        // The descriptor is read back as system only by a build with the key.
        assert_eq!(
            registry
                .get(&AppId::parse("org.example.service").unwrap())
                .expect("get")
                .expect("installed")
                .descriptor
                .tier,
            Tier::System
        );
        let keyless = Registry::with_distribution(&state, Distribution::none());
        let error = keyless
            .get(&AppId::parse("org.example.service").unwrap())
            .expect_err("no distribution key");
        assert!(
            error.message().contains("without a distribution key"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_keyring_key_cannot_mint_the_system_tier() {
        let state = scratch("system-keyring");
        let release = SigningKey::from_bytes(&[9u8; 32]);
        let third_party = SigningKey::from_bytes(&[3u8; 32]);
        Keyring::new(&state)
            .add(&TrustedKey {
                version: KEY_VERSION,
                algorithm: KeyAlgorithm::Ed25519,
                public_key: hex(&third_party.verifying_key().to_bytes()),
                label: None,
                added_by: "operator-test".to_string(),
                added_at: "2026-11-13T20:00:00Z".to_string(),
            })
            .expect("add key");
        let registry = Registry::with_distribution(&state, distribution(&release));
        let bundle = signed(service_entries(), &third_party);
        // Trusted, so the signature verifies, but it is not the release key:
        // the service switch still applies.
        let error = registry
            .install(&bundle, &options())
            .expect_err("third-party service");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        assert!(error.message().contains("allow-third-party"), "{error}");

        let mut allowed = options();
        allowed.allow_third_party_service = true;
        let installed = registry.install(&bundle, &allowed).expect("opted in");
        assert_eq!(installed.descriptor.tier, Tier::ThirdParty);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_descriptor_cannot_claim_the_system_tier() {
        let state = scratch("system-claim");
        let release = SigningKey::from_bytes(&[9u8; 32]);
        let third_party = SigningKey::from_bytes(&[3u8; 32]);
        Keyring::new(&state)
            .add(&TrustedKey {
                version: KEY_VERSION,
                algorithm: KeyAlgorithm::Ed25519,
                public_key: hex(&third_party.verifying_key().to_bytes()),
                label: None,
                added_by: "operator-test".to_string(),
                added_at: "2026-11-13T20:00:00Z".to_string(),
            })
            .expect("add key");
        let registry = Registry::with_distribution(&state, distribution(&release));
        let mut allowed = options();
        allowed.allow_third_party_service = true;
        registry
            .install(&signed(service_entries(), &third_party), &allowed)
            .expect("install");
        let descriptor = registry.dir().join("org.example.service/descriptor.toml");
        let text = fs::read_to_string(&descriptor).expect("descriptor");
        fs::write(&descriptor, text.replace("third-party", "system")).expect("rewrite");
        let error = registry
            .get(&AppId::parse("org.example.service").unwrap())
            .expect_err("tier claim");
        assert!(
            error.message().contains("distribution signature"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_descriptor_cannot_lend_the_distribution_signature() {
        let state = scratch("system-forged");
        let release = SigningKey::from_bytes(&[9u8; 32]);
        let third_party = SigningKey::from_bytes(&[3u8; 32]);
        let distribution = distribution(&release);
        let release_fingerprint = distribution.fingerprint().expect("fingerprint").to_string();
        let third_party_fingerprint = Keyring::new(&state)
            .add(&TrustedKey {
                version: KEY_VERSION,
                algorithm: KeyAlgorithm::Ed25519,
                public_key: hex(&third_party.verifying_key().to_bytes()),
                label: None,
                added_by: "operator-test".to_string(),
                added_at: "2026-11-13T20:00:00Z".to_string(),
            })
            .expect("add key");
        let registry = Registry::with_distribution(&state, distribution);
        let mut allowed = options();
        allowed.allow_third_party_service = true;
        registry
            .install(&signed(service_entries(), &third_party), &allowed)
            .expect("install");
        // Rewrite both the tier and the signer: the pair now names the release
        // key, but the stored bundle is still signed by the other key.
        let descriptor = registry.dir().join("org.example.service/descriptor.toml");
        let text = fs::read_to_string(&descriptor).expect("descriptor");
        let forged = text
            .replace("third-party", "system")
            .replace(&third_party_fingerprint, &release_fingerprint);
        fs::write(&descriptor, forged).expect("rewrite");
        let error = registry
            .get(&AppId::parse("org.example.service").unwrap())
            .expect_err("forged signer");
        assert!(
            error
                .message()
                .contains("not signed by the distribution key"),
            "{error}"
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_release_build_refuses_the_first_party_namespace_from_third_parties() {
        let state = scratch("namespace-third-party");
        let release = SigningKey::from_bytes(&[9u8; 32]);
        let third_party = SigningKey::from_bytes(&[3u8; 32]);
        Keyring::new(&state)
            .add(&trusted(&third_party))
            .expect("add key");
        let registry = Registry::with_distribution(&state, distribution(&release));
        let error = registry
            .install(&signed(first_party_entries(), &third_party), &options())
            .expect_err("first-party namespace");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        assert!(error.message().contains("first-party"), "{error}");
        assert!(!registry.dir().join("org.anthracite.notes").exists());
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn the_distribution_key_owns_the_first_party_namespace() {
        let state = scratch("namespace-system");
        let release = SigningKey::from_bytes(&[9u8; 32]);
        let registry = Registry::with_distribution(&state, distribution(&release));
        let installed = registry
            .install(&signed(first_party_entries(), &release), &options())
            .expect("system install");
        assert_eq!(installed.descriptor.tier, Tier::System);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_keyless_build_still_installs_the_shipped_apps() {
        let state = scratch("namespace-keyless");
        let registry = Registry::with_distribution(&state, Distribution::none());
        let installed = registry
            .install(&zip(&first_party_entries()), &options())
            .expect("debug install");
        assert_eq!(installed.descriptor.tier, Tier::ThirdParty);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_release_build_refuses_a_first_party_app_installed_as_third_party() {
        let state = scratch("namespace-read");
        let keyless = Registry::with_distribution(&state, Distribution::none());
        keyless
            .install(&zip(&first_party_entries()), &options())
            .expect("debug install");
        let release = SigningKey::from_bytes(&[9u8; 32]);
        let registry = Registry::with_distribution(&state, distribution(&release));
        let error = registry
            .get(&AppId::parse("org.anthracite.notes").unwrap())
            .expect_err("third-party first-party app");
        assert!(error.message().contains("first-party"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_unusable_distribution_key_refuses_installs() {
        let state = scratch("system-invalid");
        let registry =
            Registry::with_distribution(&state, Distribution::from_public_key(Some("not-hex")));
        let error = registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect_err("invalid key");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Internal);
        assert!(
            error.message().contains("invalid distribution key"),
            "{error}"
        );
        assert!(!registry.dir().join("org.example.notes").exists());
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_wrong_mode_root_fails_closed_for_internal_readers() {
        let state = scratch("wrong-mode-readers");
        let registry = Registry::new(&state);
        registry
            .install(&notes_bundle("0.1.0", WASM), &options())
            .expect("install");
        fs::set_permissions(registry.dir(), fs::Permissions::from_mode(0o755)).expect("chmod");
        assert!(registry.readable().is_err());
        assert!(registry.known_packages().is_err());
        fs::remove_dir_all(&state).expect("cleanup");
    }
}
