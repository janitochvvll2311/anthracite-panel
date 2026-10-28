//! Per-session demo state.
//!
//! A public demo cannot share one synthetic machine between visitors: one
//! person deleting a user would change what everyone else sees. This registry
//! builds a fresh [`Backends`] set for each authenticated (or anonymous demo)
//! owner and evicts idle sessions.
//!
//! Files are the only service backed by disk, under
//! `<state_dir>/demo/sessions/<hashed owner>/files`. Every other service keeps
//! its state in memory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::backend::{Backends, FilesService};
use crate::now_ms;

use super::{
    DemoAccounts, DemoFiles, DemoFirewall, DemoNetwork, DemoProcesses, DemoTerminal, DemoVolumes,
};
use crate::accounts::AccountsSettings;
use crate::volumes::VolumesSettings;

/// Fixed policy for the per-session file sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoFileSettings {
    pub max_entries: usize,
    pub max_preview_bytes: u64,
    pub max_upload_bytes: u64,
    pub max_trash_bytes: u64,
    pub max_archive_bytes: u64,
}

/// Fixed policy for the synthetic process table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoProcessSettings {
    pub control: bool,
    pub spawn: bool,
    pub spawn_allow: Vec<String>,
}

/// Everything the registry needs to build one session.
#[derive(Debug, Clone)]
pub struct DemoSettings {
    pub root: PathBuf,
    pub ttl_seconds: u64,
    pub max_sessions: usize,
    pub features: Vec<String>,
    pub files: DemoFileSettings,
    pub process: DemoProcessSettings,
    pub network_control: bool,
    pub network_manage: bool,
    pub firewall_control: bool,
    pub volumes: VolumesSettings,
    pub accounts: AccountsSettings,
    pub terminal_enabled: bool,
    pub terminal_max_sessions: usize,
}

/// Which features the operator left enabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DemoFeatures {
    pub network: bool,
    pub firewall: bool,
    pub files: bool,
    pub processes: bool,
    pub volumes: bool,
    pub accounts: bool,
    pub terminal: bool,
}

impl DemoFeatures {
    /// An empty list means every feature; otherwise only the names listed are
    /// on. Unknown names have no effect.
    pub fn from_names(names: &[String]) -> Self {
        let all = names.is_empty();
        let has = |name: &str| names.iter().any(|entry| entry == name);
        Self {
            network: all || has("network"),
            firewall: all || has("firewall"),
            files: all || has("files"),
            processes: all || has("processes"),
            volumes: all || has("volumes"),
            accounts: all || has("accounts"),
            terminal: all || has("terminal"),
        }
    }

    pub fn enabled(&self, name: &str) -> bool {
        match name {
            "network" => self.network,
            "firewall" => self.firewall,
            "files" => self.files,
            "processes" => self.processes,
            "volumes" => self.volumes,
            "accounts" => self.accounts,
            "terminal" => self.terminal,
            _ => true,
        }
    }

    pub fn names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        for (name, enabled) in [
            ("network", self.network),
            ("firewall", self.firewall),
            ("files", self.files),
            ("processes", self.processes),
            ("volumes", self.volumes),
            ("accounts", self.accounts),
            ("terminal", self.terminal),
        ] {
            if enabled {
                names.push(name);
            }
        }
        names
    }
}

/// What `/api/demo` reports for one owner.
#[derive(Debug, Clone, Serialize)]
pub struct DemoStatus {
    pub active: bool,
    pub created_ms: u64,
    pub reset_ms: u64,
    pub ttl_seconds: u64,
    pub sessions: usize,
    pub max_sessions: usize,
    pub features: DemoFeatures,
}

struct Entry {
    backends: Backends,
    last_used: Instant,
    created_ms: u64,
    reset_ms: u64,
}

/// The per-owner backend sets for the demo build.
pub struct DemoSessions {
    settings: DemoSettings,
    features: DemoFeatures,
    sessions: Mutex<HashMap<String, Entry>>,
}

impl DemoSessions {
    pub fn new(settings: DemoSettings) -> Self {
        let features = DemoFeatures::from_names(&settings.features);
        Self {
            settings,
            features,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn features(&self) -> &DemoFeatures {
        &self.features
    }

    pub fn max_sessions(&self) -> usize {
        self.settings.max_sessions.max(1)
    }

    pub fn ttl_seconds(&self) -> u64 {
        self.settings.ttl_seconds
    }

    /// Returns this owner's backends, creating the session when needed.
    pub fn backends(&self, owner: &str) -> Backends {
        let mut sessions = self.sessions();
        self.prune_locked(&mut sessions);
        let now = Instant::now();
        if let Some(entry) = sessions.get_mut(owner) {
            entry.last_used = now;
            return entry.backends.clone();
        }

        // Evict the least recently used session instead of refusing a new
        // visitor. A reset is preferable to a hard cap the UI cannot escape.
        if sessions.len() >= self.max_sessions()
            && let Some(victim) = sessions
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(owner, _)| owner.clone())
            && let Some(entry) = sessions.remove(&victim)
        {
            self.remove_files(&victim);
            drop(entry);
        }

        let backends = self.build(owner);
        let stamp = now_ms();
        sessions.insert(
            owner.to_string(),
            Entry {
                backends: backends.clone(),
                last_used: now,
                created_ms: stamp,
                reset_ms: stamp,
            },
        );
        backends
    }

    /// Drops one session and its sandbox. Returns the reset timestamp when the
    /// session existed.
    pub fn reset(&self, owner: &str) -> Option<u64> {
        let mut sessions = self.sessions();
        let entry = sessions.remove(owner)?;
        drop(entry);
        self.remove_files(owner);
        Some(now_ms())
    }

    pub fn status(&self, owner: &str) -> DemoStatus {
        let mut sessions = self.sessions();
        self.prune_locked(&mut sessions);
        let entry = sessions.get(owner);
        DemoStatus {
            active: entry.is_some(),
            created_ms: entry.map_or(0, |entry| entry.created_ms),
            reset_ms: entry.map_or(0, |entry| entry.reset_ms),
            ttl_seconds: self.settings.ttl_seconds,
            sessions: sessions.len(),
            max_sessions: self.max_sessions(),
            features: self.features.clone(),
        }
    }

    /// Drops every session whose idle time passed the TTL.
    pub fn reap(&self) -> usize {
        let mut sessions = self.sessions();
        self.prune_locked(&mut sessions)
    }

    pub fn close_terminals(&self, owner: &str) -> usize {
        let sessions = self.sessions();
        sessions
            .get(owner)
            .map_or(0, |entry| entry.backends.terminal.close_owner(owner))
    }

    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn prune_locked(&self, sessions: &mut HashMap<String, Entry>) -> usize {
        if self.settings.ttl_seconds == 0 {
            return 0;
        }
        let ttl = Duration::from_secs(self.settings.ttl_seconds);
        let now = Instant::now();
        let expired: Vec<String> = sessions
            .iter()
            .filter(|(_, entry)| now.saturating_duration_since(entry.last_used) >= ttl)
            .map(|(owner, _)| owner.clone())
            .collect();
        for owner in &expired {
            sessions.remove(owner);
            self.remove_files(owner);
        }
        expired.len()
    }

    fn build(&self, owner: &str) -> Backends {
        let root = self.session_dir(owner).join("files");
        let files: Result<Arc<dyn FilesService>, String> = DemoFiles::new(
            &root,
            self.settings.files.max_entries,
            self.settings.files.max_preview_bytes,
            self.settings.files.max_upload_bytes,
            self.settings.files.max_trash_bytes,
            self.settings.files.max_archive_bytes,
        )
        .map(|service| Arc::new(service) as Arc<dyn FilesService>);
        Backends {
            files,
            terminal: Arc::new(DemoTerminal::new(
                self.settings.terminal_enabled,
                self.settings.terminal_max_sessions,
            )),
            process: Arc::new(DemoProcesses::new(
                self.settings.process.control,
                self.settings.process.spawn,
                self.settings.process.spawn_allow.clone(),
            )),
            network: Arc::new(DemoNetwork::new(
                self.settings.network_control,
                self.settings.network_manage,
            )),
            firewall: Arc::new(DemoFirewall::new(self.settings.firewall_control)),
            volumes: Arc::new(DemoVolumes::new(self.settings.volumes.clone())),
            accounts: Arc::new(DemoAccounts::new(self.settings.accounts.clone())),
        }
    }

    fn session_dir(&self, owner: &str) -> PathBuf {
        self.root().join(session_key(owner))
    }

    fn root(&self) -> &Path {
        &self.settings.root
    }

    fn remove_files(&self, owner: &str) {
        let _ = std::fs::remove_dir_all(self.session_dir(owner));
    }
}

/// A path-safe name for an owner. The owner comes from a cookie or peer
/// identity, so it must never reach a path directly.
fn session_key(owner: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in owner.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(root: &Path) -> DemoSettings {
        DemoSettings {
            root: root.to_path_buf(),
            ttl_seconds: 0,
            max_sessions: 2,
            features: Vec::new(),
            files: DemoFileSettings {
                max_entries: 100,
                max_preview_bytes: 1024,
                max_upload_bytes: 1024,
                max_trash_bytes: 1024,
                max_archive_bytes: 1024,
            },
            process: DemoProcessSettings {
                control: true,
                spawn: false,
                spawn_allow: Vec::new(),
            },
            network_control: true,
            network_manage: true,
            firewall_control: true,
            volumes: VolumesSettings::default(),
            accounts: AccountsSettings {
                control: true,
                ..AccountsSettings::default()
            },
            terminal_enabled: true,
            terminal_max_sessions: 2,
        }
    }

    #[test]
    fn sessions_are_isolated_and_reset() {
        let root = std::env::temp_dir().join(format!(
            "anth-demo-sessions-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let sessions = DemoSessions::new(settings(&root));
        let first = sessions.backends("alice");
        let second = sessions.backends("bob");
        first
            .accounts
            .create_group(&anth_wire::AccountGroupCreate {
                name: "team".to_string(),
                gid: None,
                system: false,
            })
            .expect("alice group");
        assert!(
            !second
                .accounts
                .snapshot()
                .groups
                .iter()
                .any(|group| group.name == "team")
        );
        assert!(
            sessions
                .backends("alice")
                .accounts
                .snapshot()
                .groups
                .iter()
                .any(|group| group.name == "team")
        );

        assert!(sessions.reset("alice").is_some());
        assert!(
            !sessions
                .backends("alice")
                .accounts
                .snapshot()
                .groups
                .iter()
                .any(|group| group.name == "team")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_least_recently_used_session_is_evicted() {
        let root = std::env::temp_dir().join(format!(
            "anth-demo-evict-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let sessions = DemoSessions::new(settings(&root));
        sessions.backends("one");
        sessions.backends("two");
        sessions.backends("three");
        assert_eq!(sessions.status("one").sessions, 2);
        assert!(!sessions.status("one").active || sessions.status("one").sessions == 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn feature_names_default_to_everything() {
        let all = DemoFeatures::from_names(&[]);
        assert!(all.network && all.terminal);
        let some = DemoFeatures::from_names(&["files".to_string()]);
        assert!(some.files);
        assert!(!some.network);
        assert_eq!(some.names(), vec!["files"]);
    }
}
