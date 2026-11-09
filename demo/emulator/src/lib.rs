//! Deterministic fixtures for the demo services (D-Host-Emulator).
//!
//! One binary backs one bundle per app: the bundles carry the real app's WIT
//! package and manifest id, and only the binary is shared. The handler is
//! deliberately synchronous and in-memory, so a restart resets the fixture and
//! every run renders the same tree, sessions and outputs.

use std::collections::BTreeMap;
use std::sync::Mutex;

use anth_contract::Error;
use anth_service::Handler;
use serde_json::{json, Value};

mod accounts;
pub mod bundle;
mod net;
mod proc;
mod storage;

const FS_READ: &str = "anthracite:fs/read@1.1.0";
const FS_WRITE: &str = "anthracite:fs/write@1.1.0";
const TERMINAL: &str = "anthracite:terminal/terminal@1.0.0";
const PROC: &str = "anthracite:proc/proc@1.0.0";
const NET: &str = "anthracite:net/net@1.0.0";
const STORAGE: &str = "anthracite:storage/storage@1.0.0";
const ACCOUNTS: &str = "anthracite:accounts/accounts@1.0.0";

/// Every fixture entry shares one timestamp: a demo must not depend on the clock.
const MODIFIED_MS: u64 = 1_760_000_000_000;
const MAX_FILE: usize = 1 << 20;
const MAX_PAGE: u32 = 500;
const DEFAULT_PAGE: u32 = 50;
const PROMPT: &str = "operator@anthracite:~$ ";
const HELP: &str = "help: ls, cat, echo, help, exit\r\n";

/// The service handler: filesystem and terminal state, reset on restart.
pub struct Emulator {
    fs: Mutex<Fs>,
    terminals: Mutex<Terminals>,
    procs: Mutex<proc::ProcFixture>,
    net: net::NetFixture,
    storage: storage::StorageFixture,
    accounts: accounts::AccountsFixture,
}

impl Default for Emulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Emulator {
    pub fn new() -> Self {
        Self {
            fs: Mutex::new(Fs::demo()),
            terminals: Mutex::new(Terminals::default()),
            procs: Mutex::new(proc::ProcFixture::demo()),
            net: net::NetFixture::new(),
            storage: storage::StorageFixture::new(),
            accounts: accounts::AccountsFixture::new(),
        }
    }

    pub fn trashed(&self) -> Vec<String> {
        self.fs
            .lock()
            .expect("the fixture mutex is not poisoned")
            .trashed()
    }
}

impl Handler for Emulator {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        match interface {
            FS_READ => self.fs_read(function, &input),
            FS_WRITE => self.fs_write(function, &input),
            TERMINAL => self.terminal(function, &input),
            PROC => self
                .procs
                .lock()
                .expect("the fixture mutex is not poisoned")
                .handle(function, &input),
            NET => self.net.handle(function, &input),
            STORAGE => self.storage.handle(function, &input),
            ACCOUNTS => self.accounts.handle(function, &input),
            _ => Err(Error::not_found(format!(
                "`{interface}` is not exported by the emulator"
            ))),
        }
    }
}

enum Node {
    Dir,
    File(Vec<u8>),
}

struct Fs {
    nodes: BTreeMap<String, Node>,
    /// Subtrees removed with `trash = true`, kept for listing and restore.
    trash: Vec<Trashed>,
}

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
        ] {
            fs.nodes.insert(dir.to_string(), Node::Dir);
        }
        for (path, text) in [
            ("/etc/hostname", "anthracite-demo\n"),
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
        fs
    }

    fn node(&self, path: &str) -> Result<&Node, Error> {
        self.nodes
            .get(path)
            .ok_or_else(|| Error::not_found(format!("`{path}` does not exist")))
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
}

impl Term {
    fn push(&mut self, bytes: Vec<u8>) {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.chunks.push((seq, bytes));
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
                let node = fs.node(&path)?;
                if !matches!(node, Node::Dir) {
                    return Err(Error::invalid(format!("`{path}` is not a directory")));
                }
                let children = fs.children(&path);
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
                Ok(entry(&path, fs.node(&path)?))
            }
            "read-file" => {
                let path = normalize(&text(input, "path")?)?;
                let Node::File(bytes) = fs.node(&path)? else {
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
                let path = normalize(&text(input, "path")?)?;
                if path == "/" {
                    return Err(Error::invalid("the root is not a file"));
                }
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
                let path = normalize(&text(input, "path")?)?;
                if path == "/" {
                    return Err(Error::conflict("the root already exists"));
                }
                if fs.nodes.contains_key(&path) {
                    return Err(Error::conflict(format!("`{path}` already exists")));
                }
                let parent =
                    parent(&path).ok_or_else(|| Error::invalid("the root is not a directory"))?;
                if !matches!(fs.node(parent)?, Node::Dir) {
                    return Err(Error::invalid(format!("`{parent}` is not a directory")));
                }
                fs.nodes.insert(path, Node::Dir);
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
                term.push(data);
                if newline {
                    let trimmed = line.trim();
                    if trimmed == "exit" {
                        term.push(b"logout\r\n".to_vec());
                        term.closed = true;
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
                    }
                    "KILL" => {
                        term.push(b"Killed\r\n".to_vec());
                        term.closed = true;
                    }
                    // WINCH only asks the shell to reflow; the fixture has no
                    // pixel grid, so the resize itself is the observable effect.
                    _ => {}
                }
                Ok(Value::Null)
            }
            "close" => {
                let id = text(input, "id")?;
                terminals
                    .sessions
                    .remove(&id)
                    .ok_or_else(|| Error::not_found(format!("session `{id}` does not exist")))?;
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

    fn call(emulator: &Emulator, function: &str, input: Value) -> Value {
        emulator
            .call(FS_READ, function, input)
            .expect("the fixture answers")
    }

    #[test]
    fn the_tree_is_sorted_and_paginates() {
        let emulator = Emulator::new();
        let page = call(&emulator, "list", json!({"path": "/", "limit": 3}));
        assert_eq!(page["entries"][0]["name"], "etc");
        assert_eq!(page["entries"][1]["name"], "home");
        assert_eq!(page["entries"][2]["name"], "tmp");
        assert_eq!(page["done"], false);
        let cursor = page["cursor"].as_str().expect("cursor");
        let next = call(
            &emulator,
            "list",
            json!({"path": "/", "cursor": cursor, "limit": 3}),
        );
        assert_eq!(next["entries"][0]["name"], "var");
        assert_eq!(next["done"], true);
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
}
