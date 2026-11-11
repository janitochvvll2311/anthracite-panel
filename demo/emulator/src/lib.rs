//! Deterministic fixtures for the demo services (D-Host-Emulator).
//!
//! One binary backs one bundle per app: the bundles carry the real app's WIT
//! package and manifest id, and only the binary is shared. Host state (files,
//! accounts, network and storage) is one JSON snapshot all demo bundles point
//! at (`state/emulator/host.json`), read and written under a cross-process lock so
//! a mutation in one app is visible in the rest; runtime state (processes and
//! terminal sessions) resets on restart.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use anth_contract::Error;
use anth_service::{ChunkStream, Handler};
use futures_core::Stream;
use serde_json::{json, Value};

mod accounts;
pub mod bundle;
mod home;
mod net;
mod proc;
mod storage;
mod world;

const FS_READ: &str = "anthracite:fs/read@1.2.0";
const FS_WRITE: &str = "anthracite:fs/write@1.2.0";
const TERMINAL: &str = "anthracite:terminal/terminal@1.0.0";
const STREAM: &str = "anthracite:terminal/subscription@1.0.0";
const PROC: &str = "anthracite:proc/proc@1.1.0";
const HOME: &str = "anthracite:home/host@1.0.0";
const NET: &str = "anthracite:net/net@1.2.0";
const STORAGE: &str = "anthracite:storage/storage@1.2.0";
const ACCOUNTS: &str = "anthracite:accounts/accounts@1.1.0";

/// Every fixture entry shares one timestamp: a demo must not depend on the clock.
const MODIFIED_MS: u64 = 1_760_000_000_000;
const MAX_FILE: usize = 1 << 20;
const MAX_PAGE: u32 = 500;
const DEFAULT_PAGE: u32 = 50;
const PROMPT: &str = "operator@anthracite:~$ ";
const HELP: &str = "help: ls, cat, echo, help, exit\r\n";

/// The service handler: persistent host state plus runtime processes/sessions.
pub struct Emulator {
    fs: Mutex<Fs>,
    terminals: Arc<Mutex<Terminals>>,
    procs: Mutex<proc::ProcFixture>,
    net: Arc<Mutex<net::NetFixture>>,
    storage: Mutex<storage::StorageFixture>,
    accounts: Mutex<accounts::AccountsFixture>,
    state_path: Option<PathBuf>,
    state_lock: Mutex<()>,
}

impl Default for Emulator {
    fn default() -> Self {
        Self::new()
    }
}

/// The persistent snapshot. Only fs, accounts, net and storage are durable;
/// processes and terminal sessions are runtime.
#[derive(serde::Serialize, serde::Deserialize)]
struct State {
    fs: Fs,
    accounts: accounts::AccountsFixture,
    net: net::NetFixture,
    storage: storage::StorageFixture,
}

#[derive(serde::Serialize)]
struct StateRef<'a> {
    fs: &'a Fs,
    accounts: &'a accounts::AccountsFixture,
    net: &'a net::NetFixture,
    storage: &'a storage::StorageFixture,
}

/// Releases the cross-process `flock` when the call finishes.
struct StateGuard {
    file: std::fs::File,
}

impl Drop for StateGuard {
    fn drop(&mut self) {
        // SAFETY: the descriptor is valid until the guard drops.
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

impl Emulator {
    pub fn new() -> Self {
        let storage = storage::StorageFixture::new();
        let mut fs = Fs::demo();
        for (device, mountpoint) in storage.mounted_devices() {
            if mountpoint == "/" {
                continue;
            }
            fs.mount(&mountpoint, &storage.files_for(&device))
                .expect("the seed mountpoints are valid");
        }
        Self {
            fs: Mutex::new(fs),
            terminals: Arc::new(Mutex::new(Terminals::default())),
            procs: Mutex::new(proc::ProcFixture::demo()),
            net: Arc::new(Mutex::new(net::NetFixture::new())),
            storage: Mutex::new(storage),
            accounts: Mutex::new(accounts::AccountsFixture::new()),
            state_path: None,
            state_lock: Mutex::new(()),
        }
    }

    /// Opens a persistent snapshot if `state` names one, or seeds the fixtures.
    pub fn open(state: Option<PathBuf>) -> Result<Self, Error> {
        let mut emulator = Self::new();
        if let Some(path) = &state {
            if path.exists() {
                let bytes = std::fs::read(path).map_err(|error| {
                    Error::internal(format!("reading {}: {error}", path.display()))
                })?;
                let loaded: State = serde_json::from_slice(&bytes).map_err(|error| {
                    Error::invalid(format!(
                        "{} is not a valid state snapshot: {error}",
                        path.display()
                    ))
                })?;
                emulator.fs = Mutex::new(loaded.fs);
                emulator.accounts = Mutex::new(loaded.accounts);
                emulator.net = Arc::new(Mutex::new(loaded.net));
                emulator.storage = Mutex::new(loaded.storage);
            }
        }
        emulator.state_path = state;
        Ok(emulator)
    }

    /// Serializes one call across processes that share the same state file.
    fn lock_state(&self) -> Result<Option<StateGuard>, Error> {
        let Some(path) = &self.state_path else {
            return Ok(None);
        };
        let lock_path = path.with_extension("lock");
        if let Some(parent) = lock_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::internal(format!("creating {}: {error}", parent.display()))
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|error| {
                Error::internal(format!("opening {}: {error}", lock_path.display()))
            })?;
        // SAFETY: `file` is open for the lifetime of the guard; `flock` has no
        // memory preconditions and the descriptor is valid.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(Error::internal(format!(
                "locking {}: {}",
                lock_path.display(),
                std::io::Error::last_os_error()
            )));
        }
        Ok(Some(StateGuard { file }))
    }

    /// Reloads the shared snapshot before a call so another app's mutation is
    /// visible. The caller holds the cross-process lock.
    fn reload(&self) -> Result<(), Error> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        let bytes = std::fs::read(path)
            .map_err(|error| Error::internal(format!("reading {}: {error}", path.display())))?;
        let loaded: State = serde_json::from_slice(&bytes).map_err(|error| {
            Error::internal(format!(
                "{} is not a valid state snapshot: {error}",
                path.display()
            ))
        })?;
        *self.fs.lock().expect("the fixture mutex is not poisoned") = loaded.fs;
        *self
            .accounts
            .lock()
            .expect("the fixture mutex is not poisoned") = loaded.accounts;
        *self.net.lock().expect("the fixture mutex is not poisoned") = loaded.net;
        *self
            .storage
            .lock()
            .expect("the fixture mutex is not poisoned") = loaded.storage;
        Ok(())
    }

    /// A mountpoint becomes a directory in the shared files tree; a new user's
    /// home is created too, so Files sees what Storage and Accounts did.
    fn apply_cross_domain(
        &self,
        interface: &str,
        function: &str,
        input: &Value,
        detached_device: Option<&str>,
    ) -> Result<(), Error> {
        if interface == STORAGE {
            let mountpoint = input.get("mountpoint").and_then(Value::as_str);
            if function == "attach" {
                let device = input.get("device").and_then(Value::as_str).unwrap_or("");
                let files = self
                    .storage
                    .lock()
                    .expect("the fixture mutex is not poisoned")
                    .files_for(device);
                if let Some(mountpoint) = mountpoint {
                    self.fs
                        .lock()
                        .expect("the fixture mutex is not poisoned")
                        .mount(mountpoint, &files)?;
                }
            } else if function == "detach" {
                if let Some(mountpoint) = mountpoint {
                    let files = self
                        .fs
                        .lock()
                        .expect("the fixture mutex is not poisoned")
                        .unmount(mountpoint);
                    if let Some(device) = detached_device {
                        self.storage
                            .lock()
                            .expect("the fixture mutex is not poisoned")
                            .set_files(device, files);
                    }
                }
            }
        }
        if interface == ACCOUNTS && function == "create-user" {
            if let Some(home) = input.get("home").and_then(Value::as_str) {
                self.fs
                    .lock()
                    .expect("the fixture mutex is not poisoned")
                    .ensure_dir(home)?;
            }
        }
        Ok(())
    }

    pub fn trashed(&self) -> Vec<String> {
        self.fs
            .lock()
            .expect("the fixture mutex is not poisoned")
            .trashed()
    }

    /// Writes the durable state atomically (`state.json.tmp` then rename).
    fn persist(&self) -> Result<(), Error> {
        let Some(path) = &self.state_path else {
            return Ok(());
        };
        let fs = self.fs.lock().expect("the fixture mutex is not poisoned");
        let accounts = self
            .accounts
            .lock()
            .expect("the fixture mutex is not poisoned");
        let net = self.net.lock().expect("the fixture mutex is not poisoned");
        let storage = self
            .storage
            .lock()
            .expect("the fixture mutex is not poisoned");
        let bytes = serde_json::to_vec_pretty(&StateRef {
            fs: &fs,
            accounts: &accounts,
            net: &net,
            storage: &storage,
        })
        .map_err(|error| Error::internal(format!("encoding host state: {error}")))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::internal(format!("creating {}: {error}", parent.display()))
            })?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, &bytes).map_err(|error| {
            Error::internal(format!("writing {}: {error}", temporary.display()))
        })?;
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| Error::internal(format!("setting {} to 0600: {error}", temporary.display())),
        )?;
        std::fs::rename(&temporary, path)
            .map_err(|error| Error::internal(format!("publishing {}: {error}", path.display())))
    }
}

impl Handler for Emulator {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        let _serial = self
            .state_lock
            .lock()
            .expect("the state lock is not poisoned");
        let _guard = self.lock_state()?;
        self.reload()?;
        let detached_device = if interface == STORAGE && function == "detach" {
            input
                .get("mountpoint")
                .and_then(Value::as_str)
                .and_then(|mountpoint| {
                    self.storage
                        .lock()
                        .expect("the fixture mutex is not poisoned")
                        .device_for(mountpoint)
                })
        } else {
            None
        };
        let mutating = match interface {
            FS_WRITE => true,
            NET => !matches!(function, "interfaces" | "routes"),
            STORAGE => !matches!(function, "devices" | "mounts"),
            ACCOUNTS => !matches!(function, "users" | "groups"),
            _ => false,
        };
        let result = match interface {
            FS_READ => self.fs_read(function, &input),
            FS_WRITE => self.fs_write(function, &input),
            TERMINAL => self.terminal(function, &input),
            PROC => self
                .procs
                .lock()
                .expect("the fixture mutex is not poisoned")
                .handle(function, &input),
            HOME => home::handle(function, &input, self.state_path.as_deref()),
            NET => self
                .net
                .lock()
                .expect("the fixture mutex is not poisoned")
                .handle(function, &input),
            STORAGE => self
                .storage
                .lock()
                .expect("the fixture mutex is not poisoned")
                .handle(function, &input),
            ACCOUNTS => self
                .accounts
                .lock()
                .expect("the fixture mutex is not poisoned")
                .handle(function, &input),
            _ => Err(Error::not_found(format!(
                "`{interface}` is not exported by the emulator"
            ))),
        };
        if mutating && result.is_ok() {
            self.apply_cross_domain(interface, function, &input, detached_device.as_deref())?;
            self.persist()?;
        }
        result
    }

    fn stream(&self, interface: &str, function: &str, input: Value) -> Result<ChunkStream, Error> {
        if interface == HOME {
            return match function {
                "watch" => home::watch(&input, self.state_path.clone()),
                other => Err(Error::not_found(format!(
                    "`anthracite:home/host#{other}` is not implemented"
                ))),
            };
        }
        if interface == NET {
            return match function {
                "watch" => net::watch(&input, Arc::clone(&self.net)),
                other => Err(Error::not_found(format!(
                    "`anthracite:net/net#{other}` is not implemented"
                ))),
            };
        }
        if interface != STREAM {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the emulator"
            )));
        }
        match function {
            "follow" => {
                let id = text(&input, "id")?;
                let cursor = number(&input, "seq").unwrap_or(0);
                {
                    let terminals = self
                        .terminals
                        .lock()
                        .expect("the fixture mutex is not poisoned");
                    if !terminals.sessions.contains_key(&id) {
                        return Err(Error::not_found(format!("session `{id}` does not exist")));
                    }
                }
                Ok(Box::pin(DemoStream {
                    terminals: Arc::clone(&self.terminals),
                    id,
                    cursor,
                    done: false,
                }))
            }
            other => Err(Error::not_found(format!(
                "`anthracite:terminal/stream#{other}` is not implemented"
            ))),
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum Node {
    Dir,
    File(Vec<u8>),
    /// A symbolic link stored as an absolute target inside the fixture root.
    Link(String),
}

/// One entry on a block device. Directories are captured too so an empty
/// directory created while a volume is mounted survives a remount.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum FsEntry {
    Dir,
    File(Vec<u8>),
    Link(String),
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Fs {
    nodes: BTreeMap<String, Node>,
    /// Subtrees removed with `trash = true`, kept for listing and restore.
    trash: Vec<Trashed>,
    /// What a mountpoint hid when a device was mounted over it. Restored on
    /// unmount, so mounting never destroys the underlying tree.
    #[serde(default)]
    hidden: BTreeMap<String, BTreeMap<String, Node>>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Trashed {
    original: String,
    node: Node,
    descendants: Vec<(String, Node)>,
}

impl Fs {
    fn demo() -> Self {
        let mut fs = Self {
            nodes: BTreeMap::new(),
            trash: Vec::new(),
            hidden: BTreeMap::new(),
        };
        for dir in [
            "/",
            "/etc",
            "/home",
            "/home/operator",
            "/home/operator/deep",
            "/tmp",
            "/var",
            "/var/log",
        ]
        .into_iter()
        .chain(world::directories().iter().copied())
        {
            fs.nodes.insert(dir.to_string(), Node::Dir);
        }
        for (path, text) in [
            (
                "/home/operator/notes.txt",
                "The emulator resets on restart.\n",
            ),
            (
                "/home/operator/readme.md",
                "# Welcome\n\nThis tree is deterministic.\n",
            ),
            ("/home/operator/deep/answer.txt", "42\n"),
            ("/var/log/emulator.log", "started\n"),
            ("/tmp/scratch.txt", "temporary\n"),
        ] {
            fs.nodes
                .insert(path.to_string(), Node::File(text.as_bytes().to_vec()));
        }
        fs.nodes.insert(
            "/etc/hostname".to_string(),
            Node::File(format!("{}\n", world::HOSTNAME).into_bytes()),
        );
        fs
    }

    fn node(&self, path: &str) -> Result<&Node, Error> {
        self.nodes
            .get(path)
            .ok_or_else(|| Error::not_found(format!("`{path}` does not exist")))
    }

    /// Follows symlinks in every component and returns the canonical path.
    ///
    /// A missing tail is preserved: callers that create entries use the result
    /// as the insertion path, while callers that read require it to exist.
    fn resolve_path(&self, path: &str) -> Result<String, Error> {
        let mut parts: Vec<String> = path
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
            .map(str::to_string)
            .collect();
        let mut current = String::from("/");
        let mut hops = 0usize;
        let mut index = 0usize;
        while index < parts.len() {
            let part = &parts[index];
            if part == ".." {
                current = parent(&current).unwrap_or("/").to_string();
                index += 1;
                continue;
            }
            let candidate = if current == "/" {
                format!("/{part}")
            } else {
                format!("{current}/{part}")
            };
            match self.nodes.get(&candidate) {
                Some(Node::Link(target)) => {
                    if !target.starts_with('/') {
                        return Err(Error::internal(format!(
                            "`{candidate}` has a relative target"
                        )));
                    }
                    hops += 1;
                    if hops > 16 {
                        return Err(Error::invalid(format!(
                            "`{path}` has too many symbolic links"
                        )));
                    }
                    let mut replacement: Vec<String> = target
                        .split('/')
                        .filter(|part| !part.is_empty() && *part != ".")
                        .map(str::to_string)
                        .collect();
                    replacement.extend(parts[index + 1..].iter().cloned());
                    parts = replacement;
                    current = String::from("/");
                    index = 0;
                }
                Some(_) => {
                    current = candidate;
                    index += 1;
                }
                None => {
                    current = candidate;
                    index += 1;
                }
            }
        }
        Ok(current)
    }

    fn children(&self, path: &str) -> Vec<(String, &Node)> {
        let mut children: Vec<(String, &Node)> = self
            .nodes
            .iter()
            .filter(|(candidate, _)| candidate.as_str() != path && parent(candidate) == Some(path))
            .map(|(candidate, node)| (name(candidate).to_string(), node))
            .collect();
        // Directories first, then by name: the order the panel expects.
        children.sort_by(|left, right| {
            let left_dir = matches!(left.1, Node::Dir);
            let right_dir = matches!(right.1, Node::Dir);
            right_dir.cmp(&left_dir).then_with(|| left.0.cmp(&right.0))
        });
        children
    }

    fn remove(&mut self, path: &str, trash: bool) -> Result<(), Error> {
        if path == "/" {
            return Err(Error::invalid("the root cannot be removed"));
        }
        self.node(path)?;
        let prefix = format!("{path}/");
        let keys: Vec<String> = self
            .nodes
            .keys()
            .filter(|candidate| candidate.as_str() == path || candidate.starts_with(&prefix))
            .cloned()
            .collect();
        let mut removed: BTreeMap<String, Node> = BTreeMap::new();
        for key in keys {
            if let Some(node) = self.nodes.remove(&key) {
                removed.insert(key, node);
            }
        }
        if trash {
            let node = removed
                .remove(path)
                .expect("the removed subtree contains its root");
            self.trash.push(Trashed {
                original: path.to_string(),
                node,
                descendants: removed.into_iter().collect(),
            });
        }
        Ok(())
    }

    fn restore(&mut self, original: &str) -> Result<(), Error> {
        let index = self
            .trash
            .iter()
            .position(|entry| entry.original == original)
            .ok_or_else(|| Error::not_found(format!("`{original}` is not in the trash")))?;
        let entry = self.trash.remove(index);
        let mut conflicts = entry
            .descendants
            .iter()
            .any(|(key, _)| self.nodes.contains_key(key));
        conflicts |= self.nodes.contains_key(original);
        if conflicts {
            self.trash.insert(index, entry);
            return Err(Error::conflict(format!(
                "`{original}` is occupied; move the new entry first"
            )));
        }
        self.nodes.insert(original.to_string(), entry.node);
        for (key, node) in entry.descendants {
            self.nodes.insert(key, node);
        }
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), Error> {
        if from == "/" || to == "/" {
            return Err(Error::invalid("the root cannot be renamed"));
        }
        if from == to {
            return Ok(());
        }
        if self.nodes.contains_key(to) {
            return Err(Error::conflict(format!("`{to}` already exists")));
        }
        self.node(from)?;
        let parent = parent(to).ok_or_else(|| Error::invalid("the root is not a directory"))?;
        if !matches!(self.node(parent)?, Node::Dir) {
            return Err(Error::invalid(format!("`{parent}` is not a directory")));
        }
        if to.starts_with(&format!("{from}/")) {
            return Err(Error::invalid("a directory cannot be moved inside itself"));
        }
        let prefix = format!("{from}/");
        let keys: Vec<String> = self
            .nodes
            .keys()
            .filter(|candidate| candidate.as_str() == from || candidate.starts_with(&prefix))
            .cloned()
            .collect();
        let destinations: Vec<String> = keys
            .iter()
            .map(|key| format!("{to}{}", &key[from.len()..]))
            .collect();
        if destinations.iter().any(|key| self.nodes.contains_key(key)) {
            return Err(Error::conflict(format!(
                "`{to}` has entries that would collide"
            )));
        }
        let moved: Vec<(String, Node)> = keys
            .into_iter()
            .filter_map(|key| self.nodes.remove(&key).map(|node| (key, node)))
            .collect();
        for (old, node) in moved {
            let destination = format!("{to}{}", &old[from.len()..]);
            self.nodes.insert(destination, node);
        }
        Ok(())
    }

    /// Overlays a device filesystem at a mountpoint. The subtree that was
    /// underneath is saved and restored on unmount; mounting never destroys it.
    pub fn mount(
        &mut self,
        mountpoint: &str,
        files: &BTreeMap<String, FsEntry>,
    ) -> Result<(), Error> {
        let root = mountpoint.trim_end_matches('/').to_string();
        self.ensure_dir(mountpoint)?;
        let hidden = self.subtree(mountpoint);
        self.hidden.insert(root, hidden);
        self.clear_subtree(mountpoint);
        for (relative, entry) in files {
            let relative = relative.trim_matches('/');
            if relative.is_empty() || relative.contains("..") {
                continue;
            }
            let path = format!("{}/{}", mountpoint.trim_end_matches('/'), relative);
            if let Some((parent, _)) = path.rsplit_once('/') {
                self.ensure_dir(parent)?;
            }
            match entry {
                FsEntry::Dir => {
                    self.ensure_dir(&path)?;
                }
                FsEntry::File(bytes) => {
                    self.nodes.insert(path, Node::File(bytes.clone()));
                }
                FsEntry::Link(target) => {
                    self.nodes.insert(path, Node::Link(target.clone()));
                }
            }
        }
        Ok(())
    }

    /// Captures the volume contents and restores what the mount hid.
    pub fn unmount(&mut self, mountpoint: &str) -> BTreeMap<String, FsEntry> {
        let root = mountpoint.trim_end_matches('/').to_string();
        let entries = self.collect_entries(mountpoint);
        self.clear_subtree(mountpoint);
        if let Some(hidden) = self.hidden.remove(&root) {
            self.nodes.extend(hidden);
        }
        let _ = self.ensure_dir(mountpoint);
        entries
    }

    /// Everything a mountpoint currently hides, captured before the overlay.
    fn subtree(&self, mountpoint: &str) -> BTreeMap<String, Node> {
        let prefix = format!("{}/", mountpoint.trim_end_matches('/'));
        self.nodes
            .iter()
            .filter_map(|(path, node)| {
                path.strip_prefix(&prefix)?;
                Some((path.clone(), node.clone()))
            })
            .collect()
    }

    /// The visible entries under a mountpoint, relative to it, directories
    /// included so empty directories survive a remount.
    fn collect_entries(&self, mountpoint: &str) -> BTreeMap<String, FsEntry> {
        let prefix = format!("{}/", mountpoint.trim_end_matches('/'));
        self.nodes
            .iter()
            .filter_map(|(path, node)| {
                let relative = path.strip_prefix(&prefix)?;
                let entry = match node {
                    Node::Dir => FsEntry::Dir,
                    Node::File(bytes) => FsEntry::File(bytes.clone()),
                    Node::Link(target) => FsEntry::Link(target.clone()),
                };
                Some((relative.to_string(), entry))
            })
            .collect()
    }

    /// Removes everything under a mountpoint but keeps the directory itself.
    pub fn clear_subtree(&mut self, mountpoint: &str) {
        let prefix = format!("{}/", mountpoint.trim_end_matches('/'));
        self.nodes.retain(|path, _| !path.starts_with(&prefix));
        let _ = self.ensure_dir(mountpoint);
    }

    pub fn ensure_dir(&mut self, path: &str) -> Result<(), Error> {
        if !path.starts_with('/') || path.contains("..") || path.contains('\0') {
            return Err(Error::invalid("`path` must be an absolute path"));
        }
        let mut current = String::new();
        for part in path.split('/').filter(|part| !part.is_empty()) {
            current.push('/');
            current.push_str(part);
            match self.nodes.get(&current) {
                Some(Node::Dir) => {}
                Some(_) => return Err(Error::conflict(format!("`{current}` is not a directory"))),
                None => {
                    self.nodes.insert(current.clone(), Node::Dir);
                }
            }
        }
        Ok(())
    }

    /// The paths removed with `trash = true`, oldest first. Deterministic like
    /// the rest of the fixture, and available to the emulator's tests.
    pub fn trashed(&self) -> Vec<String> {
        self.trash
            .iter()
            .map(|entry| entry.original.clone())
            .collect()
    }
}

#[derive(Default)]
struct Terminals {
    next: u64,
    sessions: BTreeMap<String, Term>,
}

struct Term {
    shell: String,
    owner: String,
    cols: u32,
    rows: u32,
    /// Output chunks, each with the sequence a reader uses as its cursor.
    chunks: Vec<(u64, Vec<u8>)>,
    next_seq: u64,
    /// Set by `exit`: the shell ended, but the queued output is still readable.
    closed: bool,
    /// Streams parked until this session produces output or ends.
    waiters: Vec<Waker>,
}

impl Term {
    fn push(&mut self, bytes: Vec<u8>) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.chunks.push((seq, bytes));
        self.wake();
    }

    fn wake(&mut self) {
        let waiters = std::mem::take(&mut self.waiters);
        for waker in waiters {
            waker.wake();
        }
    }

    fn view(&self, id: &str) -> Value {
        json!({
            "id": id,
            "shell": self.shell,
            "owner": self.owner,
            // The last sequence the service emitted; a reader starts at 0 and
            // advances by one per chunk it consumes.
            "seq": self.next_seq.saturating_sub(1),
        })
    }
}

/// One live follower of a scripted terminal. It replays retained chunks from its
/// cursor and then parks until `push` wakes it.
struct DemoStream {
    terminals: Arc<Mutex<Terminals>>,
    id: String,
    cursor: u64,
    done: bool,
}

impl Stream for DemoStream {
    type Item = Result<Vec<u8>, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.done {
            return Poll::Ready(None);
        }
        let mut terminals = this
            .terminals
            .lock()
            .expect("the fixture mutex is not poisoned");
        let Some(term) = terminals.sessions.get_mut(&this.id) else {
            this.done = true;
            return Poll::Ready(Some(Err(Error::not_found(format!(
                "session `{}` does not exist",
                this.id
            )))));
        };
        if let Some((seq, data)) = term.chunks.iter().find(|(seq, _)| *seq >= this.cursor) {
            this.cursor = seq + 1;
            return Poll::Ready(Some(Ok(data.clone())));
        }
        if term.closed {
            this.done = true;
            return Poll::Ready(None);
        }
        if !term
            .waiters
            .iter()
            .any(|waker| waker.will_wake(context.waker()))
        {
            term.waiters.push(context.waker().clone());
        }
        Poll::Pending
    }
}

fn parent(path: &str) -> Option<&str> {
    if path == "/" {
        return None;
    }
    let index = path.rfind('/')?;
    Some(if index == 0 { "/" } else { &path[..index] })
}

fn name(path: &str) -> &str {
    if path == "/" {
        return "/";
    }
    path.rsplit('/').next().unwrap_or(path)
}

fn normalize(path: &str) -> Result<String, Error> {
    if !path.starts_with('/') {
        return Err(Error::invalid(format!("`{path}` is not an absolute path")));
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(Error::invalid(format!("`{path}` escapes the fixture root")));
                }
            }
            other => parts.push(other),
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

fn entry(path: &str, node: &Node) -> Value {
    match node {
        Node::Dir => json!({
            "name": name(path),
            "kind": "dir",
            "size": 0,
            "modified-ms": MODIFIED_MS,
        }),
        Node::File(bytes) => json!({
            "name": name(path),
            "kind": "file",
            "size": bytes.len() as u64,
            "modified-ms": MODIFIED_MS,
        }),
        Node::Link(target) => json!({
            "name": name(path),
            "kind": "link",
            "size": target.len() as u64,
            "modified-ms": MODIFIED_MS,
        }),
    }
}

pub(crate) fn text(input: &Value, field: &str) -> Result<String, Error> {
    input
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("`{field}` is required and must be a string")))
}

pub(crate) fn optional_text(input: &Value, field: &str) -> Option<String> {
    input.get(field).and_then(Value::as_str).map(str::to_string)
}

pub(crate) fn number(input: &Value, field: &str) -> Option<u64> {
    match input.get(field)? {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

pub(crate) fn boolean(input: &Value, field: &str) -> bool {
    match input.get(field) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => value == "true",
        _ => false,
    }
}

pub(crate) fn bytes(input: &Value) -> Result<Vec<u8>, Error> {
    let values = input
        .as_array()
        .ok_or_else(|| Error::invalid("`bytes` is a list of u8"))?;
    values
        .iter()
        .map(|value| {
            value
                .as_u64()
                .filter(|value| *value <= u64::from(u8::MAX))
                .map(|value| value as u8)
                .ok_or_else(|| Error::invalid("`bytes` holds a value that is not a u8"))
        })
        .collect()
}

impl Emulator {
    fn fs_read(&self, function: &str, input: &Value) -> Result<Value, Error> {
        let fs = self.fs.lock().expect("the fixture mutex is not poisoned");
        match function {
            "list" => {
                let path = normalize(&text(input, "path").unwrap_or_else(|_| "/".to_string()))?;
                let resolved = fs.resolve_path(&path)?;
                let node = fs.node(&resolved)?;
                if !matches!(node, Node::Dir) {
                    return Err(Error::invalid(format!("`{path}` is not a directory")));
                }
                let children = fs.children(&resolved);
                let start = optional_text(input, "cursor")
                    .map(|cursor| cursor.parse::<usize>())
                    .transpose()
                    .map_err(|_| Error::invalid("`cursor` is not a page cursor"))?
                    .unwrap_or(0);
                let limit = number(input, "limit").unwrap_or(u64::from(DEFAULT_PAGE));
                let limit = limit.clamp(1, u64::from(MAX_PAGE)) as usize;
                let start = start.min(children.len());
                let end = (start + limit).min(children.len());
                let entries: Vec<Value> = children[start..end]
                    .iter()
                    .map(|(child, node)| entry(child, node))
                    .collect();
                Ok(json!({
                    "entries": entries,
                    "cursor": (end < children.len()).then(|| (end as u64).to_string()),
                    "done": end >= children.len(),
                }))
            }
            "stat" => {
                let path = normalize(&text(input, "path")?)?;
                let resolved = fs.resolve_path(&path)?;
                Ok(entry(&path, fs.node(&resolved)?))
            }
            "read-file" => {
                let path = normalize(&text(input, "path")?)?;
                let resolved = fs.resolve_path(&path)?;
                let Node::File(bytes) = fs.node(&resolved)? else {
                    return Err(Error::invalid(format!("`{path}` is not a file")));
                };
                if bytes.len() > MAX_FILE {
                    return Err(Error::too_large(format!(
                        "`{path}` is {} bytes; the fixture limit is {MAX_FILE}",
                        bytes.len()
                    )));
                }
                Ok(Value::Array(
                    bytes
                        .iter()
                        .map(|byte| Value::from(u64::from(*byte)))
                        .collect(),
                ))
            }
            "read-link" => {
                let path = normalize(&text(input, "path")?)?;
                let resolved = fs.resolve_path(&path)?;
                if resolved == path {
                    return Err(Error::invalid(format!("`{path}` is not a symbolic link")));
                }
                Ok(Value::String(resolved))
            }
            "trash" => {
                let mut entries = Vec::with_capacity(fs.trash.len());
                for trashed in &fs.trash {
                    let mut value = entry(&trashed.original, &trashed.node);
                    if let Value::Object(object) = &mut value {
                        object.insert(
                            "original".to_string(),
                            Value::String(trashed.original.clone()),
                        );
                    }
                    entries.push(value);
                }
                Ok(Value::Array(entries))
            }
            other => Err(Error::not_found(format!(
                "`anthracite:fs/read#{other}` is not implemented"
            ))),
        }
    }

    fn fs_write(&self, function: &str, input: &Value) -> Result<Value, Error> {
        let mut fs = self.fs.lock().expect("the fixture mutex is not poisoned");
        match function {
            "write-file" => {
                let requested = normalize(&text(input, "path")?)?;
                if requested == "/" {
                    return Err(Error::invalid("the root is not a file"));
                }
                let path = fs.resolve_path(&requested)?;
                let Some(parent) = parent(&path).map(str::to_string) else {
                    return Err(Error::invalid("the root is not a file"));
                };
                if !matches!(fs.node(&parent)?, Node::Dir) {
                    return Err(Error::invalid(format!("`{parent}` is not a directory")));
                }
                if matches!(fs.node(&path), Ok(Node::Dir)) {
                    return Err(Error::invalid(format!("`{path}` is a directory")));
                }
                let bytes = bytes(input.get("bytes").unwrap_or(&Value::Null))?;
                if bytes.len() > MAX_FILE {
                    return Err(Error::too_large(format!(
                        "the write is {} bytes; the fixture limit is {MAX_FILE}",
                        bytes.len()
                    )));
                }
                let length = bytes.len() as u64;
                fs.nodes.insert(path, Node::File(bytes));
                Ok(Value::from(length))
            }
            "mkdir" => {
                let requested = normalize(&text(input, "path")?)?;
                if requested == "/" {
                    return Err(Error::conflict("the root already exists"));
                }
                let path = fs.resolve_path(&requested)?;
                if fs.nodes.contains_key(&path) {
                    return Err(Error::conflict(format!("`{requested}` already exists")));
                }
                let parent =
                    parent(&path).ok_or_else(|| Error::invalid("the root is not a directory"))?;
                if !matches!(fs.node(parent)?, Node::Dir) {
                    return Err(Error::invalid(format!("`{parent}` is not a directory")));
                }
                fs.nodes.insert(path, Node::Dir);
                Ok(Value::Null)
            }
            "symlink" => {
                let requested = normalize(&text(input, "path")?)?;
                if requested == "/" {
                    return Err(Error::invalid("the root cannot become a symbolic link"));
                }
                let target = normalize(&text(input, "target")?)?;
                let target = fs.resolve_path(&target)?;
                if !fs.nodes.contains_key(&target) {
                    return Err(Error::not_found(format!(
                        "the symlink target `{target}` does not exist"
                    )));
                }
                let path = fs.resolve_path(&requested)?;
                if fs.nodes.contains_key(&path) {
                    return Err(Error::conflict(format!("`{requested}` already exists")));
                }
                let parent =
                    parent(&path).ok_or_else(|| Error::invalid("the root is not a directory"))?;
                if !matches!(fs.node(parent)?, Node::Dir) {
                    return Err(Error::invalid(format!("`{parent}` is not a directory")));
                }
                fs.nodes.insert(path, Node::Link(target));
                Ok(Value::Null)
            }
            "remove" => {
                let path = normalize(&text(input, "path")?)?;
                fs.remove(&path, boolean(input, "trash"))?;
                Ok(Value::Null)
            }
            "rename" => {
                let from = normalize(&text(input, "source")?)?;
                let to = normalize(&text(input, "destination")?)?;
                fs.rename(&from, &to)?;
                Ok(Value::Null)
            }
            "restore" => {
                let original = normalize(&text(input, "original")?)?;
                fs.restore(&original)?;
                Ok(Value::Null)
            }
            other => Err(Error::not_found(format!(
                "`anthracite:fs/write#{other}` is not implemented"
            ))),
        }
    }

    fn terminal(&self, function: &str, input: &Value) -> Result<Value, Error> {
        let mut terminals = self
            .terminals
            .lock()
            .expect("the fixture mutex is not poisoned");
        match function {
            "open" => {
                let shell = text(input, "shell")?;
                if shell.trim().is_empty() {
                    return Err(Error::invalid("`shell` cannot be empty"));
                }
                terminals.next += 1;
                let id = format!("t{}", terminals.next);
                let mut term = Term {
                    shell,
                    owner: "operator".to_string(),
                    cols: number(input, "cols").unwrap_or(80).max(1) as u32,
                    rows: number(input, "rows").unwrap_or(24).max(1) as u32,
                    chunks: Vec::new(),
                    next_seq: 0,
                    closed: false,
                    waiters: Vec::new(),
                };
                term.push(PROMPT.as_bytes().to_vec());
                let view = term.view(&id);
                terminals.sessions.insert(id, term);
                Ok(view)
            }
            "list" => Ok(Value::Array(
                terminals
                    .sessions
                    .iter()
                    .filter(|(_, term)| !term.closed)
                    .map(|(id, term)| term.view(id))
                    .collect(),
            )),
            "read" => {
                let id = text(input, "id")?;
                let term = terminals
                    .sessions
                    .get(&id)
                    .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
                let cursor = number(input, "seq").unwrap_or(0);
                if let Some((seq, data)) = term.chunks.iter().find(|(seq, _)| *seq >= cursor) {
                    return Ok(json!({
                        "seq": seq,
                        "data": data.iter().map(|byte| Value::from(u64::from(*byte))).collect::<Vec<_>>(),
                    }));
                }
                if term.closed {
                    return Err(Error::not_found(format!("session `{id}` has ended")));
                }
                Ok(Value::Null)
            }
            "write" => {
                let id = text(input, "id")?;
                let term = terminals
                    .sessions
                    .get_mut(&id)
                    .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
                if term.closed {
                    return Err(Error::not_found(format!("session `{id}` has ended")));
                }
                let data = bytes(input.get("data").unwrap_or(&Value::Null))?;
                let line = String::from_utf8_lossy(&data).to_string();
                let newline = line.ends_with('\n') || line.ends_with('\r');
                // The fixture only echoes visible text (and newlines). Control
                // bytes and escape sequences are consumed silently, like a
                // line editor that has no matching binding.
                let escape = data.contains(&0x1b);
                let visible = data.iter().any(|byte| *byte >= 0x20 && *byte != 0x7f);
                if !escape && (visible || newline) {
                    term.push(data);
                }
                if newline {
                    let trimmed = line.trim();
                    if trimmed == "exit" {
                        term.push(b"logout\r\n".to_vec());
                        term.closed = true;
                        term.wake();
                        return Ok(Value::Null);
                    }
                    let response = match trimmed {
                        "help" => HELP.to_string(),
                        "ls" => "deep  notes.txt  readme.md\r\n".to_string(),
                        "cat" => "usage: cat <file>\r\n".to_string(),
                        "" => String::new(),
                        other => format!("{other}: command not found\r\n"),
                    };
                    if !response.is_empty() {
                        term.push(response.into_bytes());
                    }
                    term.push(PROMPT.as_bytes().to_vec());
                }
                Ok(Value::Null)
            }
            "resize" => {
                let id = text(input, "id")?;
                let cols = number(input, "cols").unwrap_or(0);
                let rows = number(input, "rows").unwrap_or(0);
                if cols == 0 || rows == 0 {
                    return Err(Error::invalid(
                        "`cols` and `rows` must be greater than zero",
                    ));
                }
                let term = terminals
                    .sessions
                    .get_mut(&id)
                    .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
                if term.closed {
                    return Err(Error::not_found(format!("session `{id}` has ended")));
                }
                term.cols = cols as u32;
                term.rows = rows as u32;
                Ok(Value::Null)
            }
            "signal" => {
                let id = text(input, "id")?;
                let signal = text(input, "signal")?;
                if !matches!(signal.as_str(), "HUP" | "INT" | "TERM" | "KILL" | "WINCH") {
                    return Err(Error::invalid(format!("`{signal}` is not allowlisted")));
                }
                let term = terminals
                    .sessions
                    .get_mut(&id)
                    .filter(|term| !term.closed)
                    .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
                match signal.as_str() {
                    "INT" => {
                        term.push(b"^C\r\n".to_vec());
                        term.push(PROMPT.as_bytes().to_vec());
                    }
                    "HUP" | "TERM" => {
                        term.push(b"Session terminated\r\n".to_vec());
                        term.closed = true;
                        term.wake();
                    }
                    "KILL" => {
                        term.push(b"Killed\r\n".to_vec());
                        term.closed = true;
                        term.wake();
                    }
                    // WINCH only asks the shell to reflow; the fixture has no
                    // pixel grid, so the resize itself is the observable effect.
                    _ => {}
                }
                Ok(Value::Null)
            }
            "close" => {
                let id = text(input, "id")?;
                let mut term = terminals
                    .sessions
                    .remove(&id)
                    .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
                term.wake();
                Ok(Value::Null)
            }
            other => Err(Error::not_found(format!(
                "`anthracite:terminal/terminal#{other}` is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_contract::ErrorKind;
    use std::time::Duration;

    fn call(emulator: &Emulator, function: &str, input: Value) -> Value {
        emulator
            .call(FS_READ, function, input)
            .expect("the fixture answers")
    }

    #[test]
    fn the_tree_is_sorted_and_paginates() {
        let emulator = Emulator::new();
        let page = call(&emulator, "list", json!({"path": "/", "limit": 3}));
        assert_eq!(page["entries"][0]["name"], "boot");
        assert_eq!(page["entries"][1]["name"], "etc");
        assert_eq!(page["entries"][2]["name"], "home");
        assert_eq!(page["done"], false);
        let cursor = page["cursor"].as_str().expect("cursor");
        let next = call(
            &emulator,
            "list",
            json!({"path": "/", "cursor": cursor, "limit": 3}),
        );
        assert_eq!(next["entries"][0]["name"], "media");
        assert_eq!(next["done"], false);
        let cursor = next["cursor"].as_str().expect("cursor");
        let last = call(
            &emulator,
            "list",
            json!({"path": "/", "cursor": cursor, "limit": 3}),
        );
        assert_eq!(last["entries"][0]["name"], "usr");
        assert_eq!(last["entries"][1]["name"], "var");
        assert_eq!(last["done"], true);
    }

    #[test]
    fn read_write_mkdir_and_remove_round_trip() {
        let emulator = Emulator::new();
        let bytes = call(&emulator, "read-file", json!({"path": "/etc/hostname"}));
        assert_eq!(
            bytes,
            json!([97, 110, 116, 104, 114, 97, 99, 105, 116, 101, 45, 100, 101, 109, 111, 10])
        );
        let written = emulator
            .call(
                FS_WRITE,
                "write-file",
                json!({"path": "/tmp/new.txt", "bytes": [104, 105]}),
            )
            .expect("write");
        assert_eq!(written, json!(2));
        assert_eq!(
            call(&emulator, "read-file", json!({"path": "/tmp/new.txt"})),
            json!([104, 105])
        );
        emulator
            .call(FS_WRITE, "mkdir", json!({"path": "/tmp/dir"}))
            .expect("mkdir");
        assert_eq!(
            call(&emulator, "stat", json!({"path": "/tmp/dir"}))["kind"],
            "dir"
        );
        emulator
            .call(
                FS_WRITE,
                "remove",
                json!({"path": "/tmp/new.txt", "trash": true}),
            )
            .expect("remove");
        assert_eq!(emulator.trashed(), vec!["/tmp/new.txt".to_string()]);
        let error = emulator
            .call(FS_READ, "stat", json!({"path": "/tmp/new.txt"}))
            .expect_err("removed");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn symlinks_resolve_for_reads_and_write_through() {
        let emulator = Emulator::new();
        emulator
            .call(
                FS_WRITE,
                "write-file",
                json!({"path": "/tmp/blank.txt", "bytes": []}),
            )
            .expect("blank file");
        assert_eq!(
            call(&emulator, "read-file", json!({"path": "/tmp/blank.txt"})),
            json!([])
        );

        emulator
            .call(
                FS_WRITE,
                "symlink",
                json!({"path": "/tmp/blank.link", "target": "/tmp/blank.txt"}),
            )
            .expect("file symlink");
        assert_eq!(
            call(&emulator, "stat", json!({"path": "/tmp/blank.link"}))["kind"],
            "file"
        );
        assert_eq!(
            call(&emulator, "read-link", json!({"path": "/tmp/blank.link"})),
            json!("/tmp/blank.txt")
        );
        let entries = call(&emulator, "list", json!({"path": "/tmp"}));
        let link = entries["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .find(|entry| entry["name"] == "blank.link")
            .expect("link row");
        assert_eq!(link["kind"], "link");

        emulator
            .call(
                FS_WRITE,
                "write-file",
                json!({"path": "/tmp/blank.link", "bytes": [104, 105]}),
            )
            .expect("write through link");
        assert_eq!(
            call(&emulator, "read-file", json!({"path": "/tmp/blank.txt"})),
            json!([104, 105])
        );

        emulator
            .call(
                FS_WRITE,
                "symlink",
                json!({"path": "/tmp/home.link", "target": "/home/operator"}),
            )
            .expect("dir symlink");
        let page = call(&emulator, "list", json!({"path": "/tmp/home.link"}));
        assert!(page["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|entry| entry["name"] == "notes.txt"));

        emulator
            .call(
                FS_WRITE,
                "remove",
                json!({"path": "/tmp/blank.link", "trash": false}),
            )
            .expect("remove link");
        assert_eq!(
            call(&emulator, "read-file", json!({"path": "/tmp/blank.txt"})),
            json!([104, 105])
        );
        let error = emulator
            .call(
                FS_WRITE,
                "symlink",
                json!({"path": "/tmp/missing.link", "target": "/tmp/missing"}),
            )
            .expect_err("missing target");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn rename_moves_files_and_subtrees() {
        let emulator = Emulator::new();
        emulator
            .call(
                FS_WRITE,
                "rename",
                json!({"source": "/home/operator/deep", "destination": "/home/operator/deeper"}),
            )
            .expect("rename dir");
        let error = emulator
            .call(FS_READ, "stat", json!({"path": "/home/operator/deep"}))
            .expect_err("old path");
        assert_eq!(error.kind(), ErrorKind::NotFound);
        assert_eq!(
            call(
                &emulator,
                "read-file",
                json!({"path": "/home/operator/deeper/answer.txt"})
            ),
            json!([52, 50, 10])
        );

        emulator
            .call(
                FS_WRITE,
                "rename",
                json!({"source": "/tmp/scratch.txt", "destination": "/tmp/renamed.txt"}),
            )
            .expect("rename file");
        assert_eq!(
            call(&emulator, "read-file", json!({"path": "/tmp/renamed.txt"})),
            json!([116, 101, 109, 112, 111, 114, 97, 114, 121, 10])
        );

        let error = emulator
            .call(
                FS_WRITE,
                "rename",
                json!({"source": "/tmp/renamed.txt", "destination": "/etc/hostname"}),
            )
            .expect_err("occupied");
        assert_eq!(error.kind(), ErrorKind::Conflict);
    }

    #[test]
    fn trash_lists_and_restores_a_subtree() {
        let emulator = Emulator::new();
        emulator
            .call(
                FS_WRITE,
                "remove",
                json!({"path": "/home/operator/deep", "trash": true}),
            )
            .expect("trash");
        assert_eq!(
            emulator
                .call(FS_READ, "stat", json!({"path": "/home/operator/deep"}))
                .expect_err("trashed")
                .kind(),
            ErrorKind::NotFound
        );
        let trash = emulator
            .call(FS_READ, "trash", json!({}))
            .expect("trash list");
        assert_eq!(trash[0]["original"], "/home/operator/deep");
        assert_eq!(trash[0]["kind"], "dir");

        emulator
            .call(
                FS_WRITE,
                "restore",
                json!({"original": "/home/operator/deep"}),
            )
            .expect("restore");
        assert_eq!(
            call(
                &emulator,
                "read-file",
                json!({"path": "/home/operator/deep/answer.txt"})
            ),
            json!([52, 50, 10])
        );
        let error = emulator
            .call(
                FS_WRITE,
                "restore",
                json!({"original": "/home/operator/deep"}),
            )
            .expect_err("already restored");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn traversal_is_refused() {
        let emulator = Emulator::new();
        let error = emulator
            .call(FS_READ, "list", json!({"path": "/../etc"}))
            .expect_err("escape");
        assert_eq!(error.kind(), ErrorKind::Invalid);
    }

    #[test]
    fn the_terminal_echoes_and_returns_chunks() {
        let emulator = Emulator::new();
        let session = emulator
            .call(
                TERMINAL,
                "open",
                json!({"shell": "/bin/sh", "cols": 80, "rows": 24}),
            )
            .expect("open");
        let id = session["id"].as_str().expect("id");
        assert_eq!(session["owner"], "operator");
        let first = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 0}))
            .expect("read");
        assert_eq!(first["seq"], 0);
        assert_eq!(first["data"][0], 111);

        emulator
            .call(
                TERMINAL,
                "write",
                json!({"id": id, "data": [104, 101, 108, 112, 10]}),
            )
            .expect("write");
        let help = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 1}))
            .expect("read");
        assert_eq!(help["seq"], 1);
        assert!(help["data"].as_array().expect("bytes").len() > 4);

        emulator
            .call(TERMINAL, "close", json!({"id": id}))
            .expect("close");
        let error = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 0}))
            .expect_err("closed");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn a_stream_replays_and_follows_the_scripted_terminal() {
        use futures_util::StreamExt;

        let emulator = Emulator::new();
        let session = emulator
            .call(
                TERMINAL,
                "open",
                json!({"shell": "/bin/sh", "cols": 80, "rows": 24}),
            )
            .expect("open");
        let id = session["id"].as_str().expect("id").to_string();
        let mut stream = emulator
            .stream(STREAM, "follow", json!({"id": id, "seq": 0}))
            .expect("follow");
        let first = stream.next().await.expect("chunk").expect("ok");
        assert_eq!(first[0], 111, "the prompt starts with `o`");

        emulator
            .call(
                TERMINAL,
                "write",
                json!({"id": id, "data": [104, 101, 108, 112, 10]}),
            )
            .expect("write");
        let mut output = Vec::new();
        while let Ok(Some(Ok(chunk))) =
            tokio::time::timeout(Duration::from_millis(200), stream.next()).await
        {
            output.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&output).contains("help: ls") {
                break;
            }
        }
        assert!(
            String::from_utf8_lossy(&output).contains("help: ls"),
            "{}",
            String::from_utf8_lossy(&output)
        );

        emulator
            .call(TERMINAL, "close", json!({"id": id}))
            .expect("close");
        let error = stream.next().await.expect("item").expect_err("closed");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn a_signal_interrupts_or_ends_the_session() {
        let emulator = Emulator::new();
        let session = emulator
            .call(
                TERMINAL,
                "open",
                json!({"shell": "/bin/sh", "cols": 80, "rows": 24}),
            )
            .expect("open");
        let id = session["id"].as_str().expect("id");
        emulator
            .call(TERMINAL, "signal", json!({"id": id, "signal": "INT"}))
            .expect("int");
        let interrupt = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 1}))
            .expect("^C");
        assert_eq!(interrupt["seq"], 1);
        emulator
            .call(TERMINAL, "signal", json!({"id": id, "signal": "TERM"}))
            .expect("term");
        let terminated = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 3}))
            .expect("terminated");
        assert_eq!(terminated["seq"], 3);
        let error = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 4}))
            .expect_err("ended");
        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn exit_delivers_logout_then_ends_the_session() {
        let emulator = Emulator::new();
        let session = emulator
            .call(
                TERMINAL,
                "open",
                json!({"shell": "/bin/sh", "cols": 80, "rows": 24}),
            )
            .expect("open");
        let id = session["id"].as_str().expect("id");
        emulator
            .call(
                TERMINAL,
                "write",
                json!({"id": id, "data": [101, 120, 105, 116, 10]}),
            )
            .expect("exit");
        let echo = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 1}))
            .expect("echo");
        assert_eq!(echo["seq"], 1);
        let logout = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 2}))
            .expect("logout");
        assert_eq!(logout["seq"], 2);
        let error = emulator
            .call(TERMINAL, "read", json!({"id": id, "seq": 3}))
            .expect_err("ended");
        assert_eq!(error.kind(), ErrorKind::NotFound);
        let list = emulator.call(TERMINAL, "list", json!({})).expect("list");
        assert_eq!(list, json!([]));
    }

    #[test]
    fn host_state_survives_a_reopen() {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-emulator-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("state.json");
        let emulator = Emulator::open(Some(path.clone())).expect("open");

        emulator
            .call(
                FS_WRITE,
                "write-file",
                json!({"path": "/tmp/persisted.txt", "bytes": [104, 105]}),
            )
            .expect("write");
        emulator
            .call(
                ACCOUNTS,
                "create-user",
                json!({"name": "persisted", "uid": 1500, "gid": 1500, "home": "/home/persisted", "shell": "/bin/sh", "system": false}),
            )
            .expect("user");
        emulator
            .call(NET, "set-up", json!({"name": "wlan0", "up": true}))
            .expect("up");
        emulator
            .call(
                STORAGE,
                "attach",
                json!({"device": "/dev/sdc", "mountpoint": "/media/sd", "filesystem": "exfat"}),
            )
            .expect("mount");
        drop(emulator);

        let reopened = Emulator::open(Some(path)).expect("reopen");
        assert_eq!(
            reopened
                .call(FS_READ, "read-file", json!({"path": "/tmp/persisted.txt"}))
                .expect("read"),
            json!([104, 105])
        );
        let users = reopened.call(ACCOUNTS, "users", json!({})).expect("users");
        assert!(users
            .as_array()
            .expect("users")
            .iter()
            .any(|user| user["name"] == "persisted"));
        let interfaces = reopened
            .call(NET, "interfaces", json!({}))
            .expect("interfaces");
        assert_eq!(interfaces[2]["up"], true);
        let mounts = reopened.call(STORAGE, "mounts", json!({})).expect("mounts");
        assert!(mounts
            .as_array()
            .expect("mounts")
            .iter()
            .any(|mount| mount["mountpoint"] == "/media/sd"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_fixtures_describe_one_host() {
        let emulator = Emulator::new();
        for mount in world::mounts() {
            emulator
                .call(FS_READ, "stat", json!({"path": mount.mountpoint}))
                .expect("mountpoint in the files tree");
        }
        for user in world::users() {
            emulator
                .call(FS_READ, "stat", json!({"path": user.home}))
                .expect("user home in the files tree");
        }
        let net = emulator
            .call(NET, "interfaces", json!({}))
            .expect("net interfaces");
        let net_names: Vec<&str> = net
            .as_array()
            .expect("interfaces")
            .iter()
            .filter_map(|interface| interface["name"].as_str())
            .collect();
        let home = emulator.call(HOME, "info", json!({})).expect("home info");
        let home_names: Vec<&str> = home["networks"]
            .as_array()
            .expect("home networks")
            .iter()
            .filter_map(|interface| interface["name"].as_str())
            .collect();
        assert_eq!(net_names, home_names);

        let storage = emulator
            .call(STORAGE, "mounts", json!({}))
            .expect("storage mounts");
        let storage_mounts: Vec<&str> = storage
            .as_array()
            .expect("mounts")
            .iter()
            .filter_map(|mount| mount["mountpoint"].as_str())
            .collect();
        let home_mounts: Vec<&str> = home["disks"]
            .as_array()
            .expect("home disks")
            .iter()
            .filter_map(|disk| disk["mount"].as_str())
            .collect();
        assert_eq!(storage_mounts, home_mounts);

        let accounts = emulator
            .call(ACCOUNTS, "users", json!({}))
            .expect("accounts users");
        let names: Vec<&str> = accounts
            .as_array()
            .expect("users")
            .iter()
            .filter_map(|user| user["name"].as_str())
            .collect();
        for user in world::users() {
            assert!(names.contains(&user.name), "{}", user.name);
        }
    }

    #[test]
    fn mounting_over_content_hides_it_and_unmounting_restores_it() {
        let emulator = Emulator::new();
        emulator
            .call(
                STORAGE,
                "attach",
                json!({"device": "/dev/sdc", "mountpoint": "/home", "filesystem": "exfat"}),
            )
            .expect("mount over /home");
        assert_eq!(
            emulator
                .call(FS_READ, "stat", json!({"path": "/home/operator/notes.txt"}))
                .expect_err("hidden by the mount")
                .kind(),
            ErrorKind::NotFound
        );
        emulator
            .call(FS_READ, "stat", json!({"path": "/home/notes.txt"}))
            .expect("the volume is visible");
        emulator
            .call(STORAGE, "detach", json!({"mountpoint": "/home"}))
            .expect("unmount");
        call(
            &emulator,
            "stat",
            json!({"path": "/home/operator/notes.txt"}),
        );
        assert_eq!(
            emulator
                .call(FS_READ, "stat", json!({"path": "/home/notes.txt"}))
                .expect_err("the volume is hidden")
                .kind(),
            ErrorKind::NotFound
        );
    }

    #[test]
    fn empty_directories_on_a_volume_survive_remount() {
        let emulator = Emulator::new();
        emulator
            .call(FS_WRITE, "mkdir", json!({"path": "/media/demo/empty"}))
            .expect("mkdir on the volume");
        emulator
            .call(STORAGE, "detach", json!({"mountpoint": "/media/demo"}))
            .expect("unmount");
        emulator
            .call(
                STORAGE,
                "attach",
                json!({"device": "/dev/sdb1", "mountpoint": "/media/demo", "filesystem": "exfat"}),
            )
            .expect("remount");
        call(&emulator, "stat", json!({"path": "/media/demo/empty"}));
    }

    #[test]
    fn unmounting_hides_and_remounting_restores_device_files() {
        let emulator = Emulator::new();
        let readme = call(
            &emulator,
            "read-file",
            json!({"path": "/media/demo/README.txt"}),
        );
        emulator
            .call(
                FS_WRITE,
                "write-file",
                json!({"path": "/media/demo/new.txt", "bytes": [104, 105]}),
            )
            .expect("write on the mounted volume");
        emulator
            .call(STORAGE, "detach", json!({"mountpoint": "/media/demo"}))
            .expect("unmount");
        assert_eq!(
            emulator
                .call(FS_READ, "stat", json!({"path": "/media/demo/README.txt"}))
                .expect_err("hidden")
                .kind(),
            ErrorKind::NotFound
        );
        call(&emulator, "stat", json!({"path": "/media/demo"}));
        emulator
            .call(
                STORAGE,
                "attach",
                json!({"device": "/dev/sdb1", "mountpoint": "/media/demo", "filesystem": "exfat"}),
            )
            .expect("remount");
        assert_eq!(
            call(
                &emulator,
                "read-file",
                json!({"path": "/media/demo/README.txt"}),
            ),
            readme
        );
        assert_eq!(
            call(
                &emulator,
                "read-file",
                json!({"path": "/media/demo/new.txt"}),
            ),
            json!([104, 105])
        );
    }

    #[test]
    fn shared_state_is_visible_across_apps() {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("shared-state-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("host.json");
        let first = Emulator::open(Some(path.clone())).expect("first");
        let second = Emulator::open(Some(path)).expect("second");

        first
            .call(
                STORAGE,
                "attach",
                json!({"device": "/dev/sdc", "mountpoint": "/media/sd", "filesystem": "exfat"}),
            )
            .expect("attach");
        let mounts = second
            .call(STORAGE, "mounts", json!({}))
            .expect("second sees the mount");
        assert!(mounts
            .as_array()
            .expect("mounts")
            .iter()
            .any(|mount| mount["mountpoint"] == "/media/sd"));
        second
            .call(FS_READ, "stat", json!({"path": "/media/sd"}))
            .expect("mountpoint is a directory in the files tree");
        let notes = second
            .call(FS_READ, "read-file", json!({"path": "/media/sd/notes.txt"}))
            .expect("the device files are visible");
        assert!(!notes.as_array().expect("bytes").is_empty());

        second
            .call(NET, "set-up", json!({"name": "wlan0", "up": true}))
            .expect("interface up");
        let info = first.call(HOME, "info", json!({})).expect("home");
        let wlan = info["networks"]
            .as_array()
            .expect("networks")
            .iter()
            .find(|network| network["name"] == "wlan0")
            .expect("wlan0");
        assert_eq!(wlan["state"], "up");

        second
            .call(
                ACCOUNTS,
                "create-user",
                json!({"name": "demo", "uid": 1500, "gid": 1500, "home": "/home/demo", "shell": "/bin/sh", "system": false}),
            )
            .expect("create user");
        first
            .call(FS_READ, "stat", json!({"path": "/home/demo"}))
            .expect("home directory is visible");

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_net_fixture_streams_moving_snapshots() {
        use futures_util::StreamExt;

        let emulator = Emulator::new();
        let mut stream = emulator
            .stream(NET, "watch", json!({"interval-ms": 1000}))
            .expect("watch");
        let first: Value =
            serde_json::from_slice(&stream.next().await.expect("chunk").expect("ok"))
                .expect("json");
        let second: Value =
            serde_json::from_slice(&stream.next().await.expect("chunk").expect("ok"))
                .expect("json");
        assert!(first["interfaces"].is_array());
        assert!(first["routes"].is_array());
        assert!(
            second["interfaces"][1]["rx-bytes"].as_u64().expect("rx")
                > first["interfaces"][1]["rx-bytes"].as_u64().expect("rx")
        );
    }

    #[tokio::test]
    async fn the_home_fixture_answers_info_and_watch_through_the_handler() {
        use futures_util::StreamExt;

        let emulator = Emulator::new();
        let info = emulator.call(HOME, "info", json!({})).expect("info");
        assert_eq!(info["hostname"], "anthracite-demo");
        assert_eq!(info["cpu"]["cores"], 8);

        let mut stream = emulator
            .stream(HOME, "watch", json!({"interval-ms": 1000}))
            .expect("watch");
        let chunk = stream.next().await.expect("chunk").expect("ok");
        let snapshot: Value = serde_json::from_slice(&chunk).expect("json");
        assert_eq!(snapshot["hostname"], "anthracite-demo");
        assert!(snapshot["uptime-seconds"].as_u64().expect("uptime") >= 273_720);
    }
}
