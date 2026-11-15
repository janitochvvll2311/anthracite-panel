//! The grant store: one TOML document per app under `state/grants` (§6.8).
//!
//! The state tree owns the directory; this module owns the files in it. Only an
//! approved grant is written, and a grant is read back by app id, so a grant
//! that names a different app than its file is refused rather than trusted.

use std::path::{Path, PathBuf};

use anth_contract::{AppId, Error, Grant};

use crate::store;

#[derive(Debug, Clone)]
pub struct GrantStore {
    dir: PathBuf,
}

impl GrantStore {
    pub fn new(state: &Path) -> Self {
        Self {
            dir: state.join("grants"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self, app: &AppId) -> PathBuf {
        self.dir.join(format!("{app}.toml"))
    }

    /// Reads the grant for `app`, or `None` when the app was never approved.
    pub fn load(&self, app: &AppId) -> Result<Option<Grant>, Error> {
        if !store::check_dir(&self.dir)? {
            return Ok(None);
        }
        let path = self.path(app);
        let Some(text) = store::read(&path)? else {
            return Ok(None);
        };
        let grant = Grant::parse(&text)
            .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
        grant
            .validate()
            .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
        if &grant.app != app {
            return Err(Error::invalid(format!(
                "{} holds the grant for `{}`, not `{app}`",
                path.display(),
                grant.app
            )));
        }
        Ok(Some(grant))
    }

    /// Writes the grant atomically: a reader sees the old document or the new
    /// one, never a half-written file.
    pub fn save(&self, grant: &Grant) -> Result<(), Error> {
        grant.validate()?;
        store::write_atomic(
            &self.dir,
            &format!("{}.toml", grant.app),
            grant.to_toml()?.as_bytes(),
        )
    }

    /// Whether a grant document exists, without parsing it: a corrupt grant can
    /// still be revoked.
    pub fn contains(&self, app: &AppId) -> Result<bool, Error> {
        if !store::check_dir(&self.dir)? {
            return Ok(false);
        }
        Ok(store::read(&self.path(app))?.is_some())
    }

    /// Removes the grant. `false` means there was nothing to remove.
    pub fn remove(&self, app: &AppId) -> Result<bool, Error> {
        if !store::check_dir(&self.dir)? {
            return Ok(false);
        }
        store::remove(&self.path(app))
    }

    /// Every grant, sorted by app id. A missing directory is an empty store.
    pub fn all(&self) -> Result<Vec<Grant>, Error> {
        let mut grants = Vec::new();
        for path in store::toml_paths(&self.dir)? {
            let Some(text) = store::read(&path)? else {
                continue;
            };
            let grant = Grant::parse(&text)
                .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
            grant
                .validate()
                .map_err(|error| Error::invalid(format!("{}: {error}", path.display())))?;
            if path.file_stem().and_then(|stem| stem.to_str()) != Some(grant.app.as_str()) {
                return Err(Error::invalid(format!(
                    "{} holds the grant for `{}`",
                    path.display(),
                    grant.app
                )));
            }
            grants.push(grant);
        }
        Ok(grants)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::{InterfaceId, Risk};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-grant-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn grant(app: &str) -> Grant {
        Grant {
            version: anth_contract::GRANT_VERSION,
            app: AppId::parse(app).expect("app id"),
            app_version: "0.1.0".to_string(),
            manifest_sha256: "a".repeat(64),
            wit_sha256: "b".repeat(64),
            signature: Some("c2lnbmF0dXJl".to_string()),
            risk: Risk::Write,
            accepted: vec![Risk::Write],
            approved_by: "operator-3f2a".to_string(),
            approved_at: "2026-11-03T20:00:00Z".to_string(),
            export: vec![InterfaceId::parse("example:notes/list@1.0.0").expect("interface")],
            import: vec![InterfaceId::parse("anth:store/kv@1.0.0").expect("interface")],
            roots: Vec::new(),
            listen: Vec::new(),
            egress: Vec::new(),
        }
    }

    #[test]
    fn a_grant_round_trips_through_the_store() {
        let state = scratch("round-trip");
        let store = GrantStore::new(&state);
        let grant = grant("org.example.notes");
        store.save(&grant).expect("save");
        assert_eq!(store.load(&grant.app).expect("load"), Some(grant));
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn the_directory_is_0700_and_the_file_is_0600() {
        let state = scratch("modes");
        let store = GrantStore::new(&state);
        let grant = grant("org.example.notes");
        store.save(&grant).expect("save");
        let dir_mode = fs::metadata(store.dir()).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        let file_mode = fs::metadata(store.path(&grant.app))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_unapproved_app_has_no_grant() {
        let state = scratch("missing");
        let store = GrantStore::new(&state);
        assert_eq!(
            store
                .load(&AppId::parse("org.example.notes").unwrap())
                .expect("load"),
            None
        );
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn saving_again_replaces_the_grant() {
        let state = scratch("replace");
        let store = GrantStore::new(&state);
        let mut grant = grant("org.example.notes");
        store.save(&grant).expect("save");
        grant.approved_by = "operator-9c1b".to_string();
        store.save(&grant).expect("save again");
        assert_eq!(
            store.load(&grant.app).expect("load").unwrap().approved_by,
            "operator-9c1b"
        );
        let leftovers: Vec<_> = fs::read_dir(store.dir())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_grant_files_name_must_match_its_app() {
        let state = scratch("mismatch");
        let store = GrantStore::new(&state);
        store.save(&grant("org.example.notes")).expect("save");
        let other = AppId::parse("org.example.other").unwrap();
        fs::copy(
            store.path(&grant("org.example.notes").app),
            store.path(&other),
        )
        .expect("copy");
        let error = store.load(&other).expect_err("mismatch");
        assert!(error.message().contains("holds the grant"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_symlink_target_is_refused() {
        let state = scratch("symlink");
        let outside = state.join("outside.toml");
        let store = GrantStore::new(&state);
        store.save(&grant("org.example.notes")).expect("save");
        let path = store.path(&AppId::parse("org.example.other").unwrap());
        std::os::unix::fs::symlink(&outside, &path).expect("symlink");
        let error = store
            .save(&grant("org.example.other"))
            .expect_err("symlink");
        assert!(error.message().contains("is a symlink"), "{error}");
        assert!(!outside.exists(), "the link must not be followed");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn an_unapproved_grant_is_not_written() {
        let state = scratch("invalid");
        let store = GrantStore::new(&state);
        let mut grant = grant("org.example.notes");
        grant.accepted.clear();
        store.save(&grant).expect_err("invalid grant");
        assert_eq!(store.load(&grant.app).expect("load"), None);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_symlink_is_refused_on_load() {
        let state = scratch("load-link");
        let outside = state.join("outside.toml");
        let store = GrantStore::new(&state);
        fs::create_dir_all(store.dir()).expect("grants");
        fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o700)).expect("chmod");
        let path = store.path(&AppId::parse("org.example.other").unwrap());
        std::os::unix::fs::symlink(&outside, &path).expect("symlink");
        let error = store
            .load(&AppId::parse("org.example.other").unwrap())
            .expect_err("symlink");
        assert!(error.message().contains("is a symlink"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn all_refuses_a_file_named_for_another_app() {
        let state = scratch("all-mismatch");
        let store = GrantStore::new(&state);
        store.save(&grant("org.example.notes")).expect("save");
        fs::copy(
            store.path(&AppId::parse("org.example.notes").unwrap()),
            store.dir().join("org.example.other.toml"),
        )
        .expect("copy");
        let error = store.all().expect_err("mismatch");
        assert!(error.message().contains("holds the grant"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_document_that_does_not_validate_is_not_loaded() {
        let state = scratch("invalid-load");
        let store = GrantStore::new(&state);
        store.save(&grant("org.example.notes")).expect("save");
        let path = store.path(&AppId::parse("org.example.notes").unwrap());
        let mut broken = grant("org.example.notes");
        broken.accepted.clear();
        fs::write(&path, broken.to_toml().expect("render")).expect("write");
        let error = store
            .load(&broken.app)
            .expect_err("an invalid grant is not authority");
        assert!(error.message().contains("--accept-write"), "{error}");
        let error = store.all().expect_err("all validates too");
        assert!(error.message().contains("--accept-write"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_loose_grants_directory_is_refused_on_read() {
        let state = scratch("loose");
        let store = GrantStore::new(&state);
        let grant = grant("org.example.notes");
        store.save(&grant).expect("save");
        fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o755)).expect("chmod");
        let error = store.load(&grant.app).expect_err("loose");
        assert!(error.message().contains("0755"), "{error}");
        fs::set_permissions(store.dir(), fs::Permissions::from_mode(0o700)).expect("chmod back");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn a_symlinked_grants_directory_is_refused_on_read() {
        let state = scratch("dir-symlink");
        let real = state.join("real");
        fs::create_dir_all(&real).expect("mkdir");
        std::os::unix::fs::symlink(&real, state.join("grants")).expect("symlink");
        let store = GrantStore::new(&state);
        let error = store
            .load(&AppId::parse("org.example.notes").unwrap())
            .expect_err("symlinked directory");
        assert!(error.message().contains("is a symlink"), "{error}");
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn remove_reports_whether_there_was_something() {
        let state = scratch("remove");
        let store = GrantStore::new(&state);
        let grant = grant("org.example.notes");
        assert!(!store.remove(&grant.app).expect("nothing"));
        store.save(&grant).expect("save");
        assert!(store.remove(&grant.app).expect("removed"));
        assert_eq!(store.load(&grant.app).expect("load"), None);
        fs::remove_dir_all(&state).expect("cleanup");
    }

    #[test]
    fn all_returns_every_grant_in_order() {
        let state = scratch("all");
        let store = GrantStore::new(&state);
        store.save(&grant("org.example.two")).expect("save");
        store.save(&grant("org.example.one")).expect("save");
        let apps: Vec<String> = store
            .all()
            .expect("all")
            .into_iter()
            .map(|grant| grant.app.to_string())
            .collect();
        assert_eq!(apps, ["org.example.one", "org.example.two"]);
        fs::remove_dir_all(&state).expect("cleanup");
    }
}
