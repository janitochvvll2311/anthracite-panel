//! Operator state for the registry: which bundles are switched off.
//!
//! Kept beside the bundles, in one file per registry, and never inside a bundle
//! directory. A bundle's contents are approved by digest, so a preference stored
//! there would either have to be excluded from that approval or would let an
//! archive arrive already disabled. One file also means one place to look when a
//! plugin is off, and `remove` only has to delete the entry.
//!
//! The file records **only** the negative: a plugin absent from it is on. That
//! way a new bundle is enabled by default, and a file that was never written
//! needs no special case. Reconciliation is the other direction — an id in the
//! file with no directory on disk is ignored on read and pruned on the next
//! write, so uninstalling does not leave a tombstone that a later reinstall
//! would silently inherit.
//!
//! Every write goes to a temporary file and is renamed into place, so a daemon
//! reading the file while `disable` runs sees the old contents or the new ones,
//! never a half-written file.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::Path;

use anth_wire::WireError;

/// The state file, hidden so a registry listing shows bundles and nothing else.
pub const STATE_FILE: &str = ".state.toml";

/// The ids the operator switched off.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct State {
    disabled: BTreeSet<String>,
}

impl State {
    /// Reads the state beside `root`.
    ///
    /// A missing file is an empty state, which is the same as a registry nobody
    /// has changed. A malformed one is also empty rather than fatal: refusing to
    /// start because a preference file is corrupt would take the whole panel
    /// down over a setting, and every plugin would then be judged by the
    /// manifest alone, which is the conservative reading.
    pub fn load(root: &Path) -> Self {
        let path = root.join(STATE_FILE);
        let Ok(text) = fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str::<File>(&text) {
            Ok(file) => Self {
                disabled: file.disabled.into_iter().collect(),
            },
            Err(_) => Self::default(),
        }
    }

    pub fn is_disabled(&self, id: &str) -> bool {
        self.disabled.contains(id)
    }

    /// Switches one id off, or back on, and writes the file.
    ///
    /// The id is validated first: it is about to be interpolated into a TOML
    /// string, and the same grammar that keeps it inside the registry directory
    /// keeps it from breaking the file. Prunes ids that no longer have a
    /// directory in the same write, so the file cannot grow a set of entries for
    /// bundles that were removed.
    pub fn set(&mut self, root: &Path, id: &str, disabled: bool) -> Result<bool, WireError> {
        super::manifest::validate_id(id)?;
        let changed = if disabled {
            self.disabled.insert(id.to_string())
        } else {
            self.disabled.remove(id)
        };
        if !changed {
            return Ok(false);
        }
        self.retain_installed(root);
        self.write(root)?;
        Ok(true)
    }

    /// Drops ids with no bundle on disk, so a removed plugin cannot be
    /// resurrected as disabled by a stale entry.
    fn retain_installed(&mut self, root: &Path) {
        self.disabled
            .retain(|id| super::install::plugin_dir(root, id).is_ok_and(|dir| dir.is_dir()));
    }

    fn write(&self, root: &Path) -> Result<(), WireError> {
        let path = root.join(STATE_FILE);
        let text = self.render();
        let temp = path.with_extension("toml.tmp");

        let mut file = fs::File::create(&temp)
            .map_err(|e| WireError::internal(format!("writing {}: {e}", temp.display())))?;
        file.write_all(text.as_bytes())
            .map_err(|e| WireError::internal(format!("writing {}: {e}", temp.display())))?;
        // Flushed before the rename so a crash cannot leave an empty file where
        // a valid one used to be.
        file.sync_all()
            .map_err(|e| WireError::internal(format!("flushing {}: {e}", temp.display())))?;
        drop(file);

        fs::rename(&temp, &path).map_err(|e| {
            let _ = fs::remove_file(&temp);
            WireError::internal(format!("installing {}: {e}", path.display()))
        })
    }

    /// The file as text. Written by hand rather than through a serializer so the
    /// comment explaining the file survives a rewrite by a human.
    fn render(&self) -> String {
        let mut out = String::from(
            "# Plugins the operator switched off. A plugin not listed here is on.\n\
             # Managed by `anth-panel plugin disable` / `enable`.\n",
        );
        if self.disabled.is_empty() {
            return out;
        }
        out.push_str("\ndisabled = [\n");
        for id in &self.disabled {
            out.push_str(&format!("  \"{id}\",\n"));
        }
        out.push_str("]\n");
        out
    }
}

/// The on-disk shape. Only `disabled` exists: absence means enabled.
#[derive(serde::Deserialize, Default)]
struct File {
    #[serde(default)]
    disabled: Vec<String>,
}
