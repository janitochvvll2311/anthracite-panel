//! Native Files service: a host filesystem confined to an approved root.
//!
//! `--root` and `--trash` are frozen in the approved argv. Every path is
//! normalised, parent symlinks are canonicalised and checked against the root,
//! and a final symlink is resolved before it is followed. The wasm view never
//! receives a host path.

use std::fs::{self, Metadata, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use anth_contract::Error;
use anth_service::Handler;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const READ: &str = "anthracite:fs/read@1.2.0";
const WRITE: &str = "anthracite:fs/write@1.2.0";
const MAX_FILE: u64 = 1 << 20;
const MAX_PAGE: u32 = 500;
const DEFAULT_PAGE: u32 = 50;

struct Service {
    root: PathBuf,
    trash: Mutex<Trash>,
}

#[derive(Clone, Serialize, Deserialize)]
struct TrashItem {
    original: String,
    name: String,
    kind: String,
    size: u64,
    modified_ms: u64,
    stored: String,
}

#[derive(Default, Serialize, Deserialize)]
struct TrashState {
    items: Vec<TrashItem>,
    next: u64,
}

struct Trash {
    objects: PathBuf,
    index: PathBuf,
    state: TrashState,
}

impl Trash {
    fn open(dir: &Path) -> Result<Self, Error> {
        let objects = dir.join("objects");
        fs::create_dir_all(&objects).map_err(|error| io_error("creating the trash", error))?;
        let index = dir.join("index.json");
        let state = if index.exists() {
            let bytes = fs::read(&index).map_err(|error| io_error("reading the trash", error))?;
            serde_json::from_slice(&bytes)
                .map_err(|error| Error::invalid(format!("the trash index is invalid: {error}")))?
        } else {
            TrashState::default()
        };
        Ok(Self {
            objects,
            index,
            state,
        })
    }

    fn items(&self) -> Vec<TrashItem> {
        let mut items = self.state.items.clone();
        items.sort_by(|left, right| left.original.cmp(&right.original));
        items
    }

    fn item(&self, original: &str) -> Option<TrashItem> {
        self.state
            .items
            .iter()
            .find(|item| item.original == original)
            .cloned()
    }

    fn object(&self, stored: &str) -> PathBuf {
        self.objects.join(stored)
    }

    fn add(&mut self, source: &Path, original: &str, metadata: &Metadata) -> Result<(), Error> {
        let id = format!("item-{}", self.state.next);
        self.state.next += 1;
        let object = self.objects.join(&id);
        let (kind, size) = details(source, metadata);
        move_path(source, &object)?;
        self.state.items.push(TrashItem {
            original: original.to_string(),
            name: original
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or("/")
                .to_string(),
            kind,
            size,
            modified_ms: modified_ms(metadata),
            stored: id,
        });
        self.persist()
    }

    fn remove(&mut self, original: &str) {
        self.state.items.retain(|item| item.original != original);
    }

    fn persist(&self) -> Result<(), Error> {
        let bytes = serde_json::to_vec_pretty(&self.state)
            .map_err(|error| Error::internal(format!("encoding the trash: {error}")))?;
        let temporary = self.index.with_extension("json.tmp");
        fs::write(&temporary, bytes).map_err(|error| io_error("writing the trash", error))?;
        fs::rename(&temporary, &self.index).map_err(|error| io_error("committing the trash", error))
    }
}

impl Service {
    fn new(root: &str, trash: &Path) -> Result<Self, Error> {
        let canonical = fs::canonicalize(root)
            .map_err(|error| io_error(&format!("opening root `{root}`"), error))?;
        if !canonical.is_dir() {
            return Err(Error::invalid(format!("`{root}` is not a directory")));
        }
        Ok(Self {
            root: canonical,
            trash: Mutex::new(Trash::open(trash)?),
        })
    }

    fn root(&self) -> Root {
        Root {
            path: self.root.clone(),
        }
    }

    fn list(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let path = text(input, "path")?;
        let directory = root.existing(&path)?;
        let cursor = optional_text(input, "cursor")
            .map(|value| {
                value
                    .parse::<usize>()
                    .map_err(|_| Error::invalid("`cursor` is not a page cursor"))
            })
            .transpose()?
            .unwrap_or(0);
        let limit = optional_number(input, "limit")
            .unwrap_or(DEFAULT_PAGE as u64)
            .clamp(1, MAX_PAGE as u64) as usize;

        let mut entries = Vec::new();
        let iterator = fs::read_dir(&directory)
            .map_err(|error| io_error(&format!("listing `{path}`"), error))?;
        for entry in iterator {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(_) => continue,
            };
            let full = entry.path();
            let metadata = match fs::symlink_metadata(&full) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let (kind, size) = details(&full, &metadata);
            entries.push(json!({
                "name": name,
                "kind": kind,
                "size": size,
                "modified-ms": modified_ms(&metadata),
            }));
        }
        // Directories first, then by name: the order the panel expects.
        entries.sort_by(|left, right| {
            let left_dir = left["kind"] == "dir";
            let right_dir = right["kind"] == "dir";
            right_dir
                .cmp(&left_dir)
                .then_with(|| left["name"].as_str().cmp(&right["name"].as_str()))
        });
        let start = cursor.min(entries.len());
        let end = (start + limit).min(entries.len());
        Ok(json!({
            "entries": entries[start..end],
            "cursor": (end < entries.len()).then(|| end.to_string()),
            "done": end >= entries.len(),
        }))
    }

    fn stat(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let (path, normalized, metadata) = root.lookup(&text(input, "path")?)?;
        let (kind, size) = details(&path, &metadata);
        let name = if normalized == "/" {
            "/".to_string()
        } else {
            normalized.rsplit('/').next().unwrap_or("/").to_string()
        };
        Ok(json!({
            "name": name,
            "kind": kind,
            "size": size,
            "modified-ms": modified_ms(&metadata),
        }))
    }

    fn read_file(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let path = root.existing(&text(input, "path")?)?;
        let metadata = fs::metadata(&path).map_err(|error| io_error("reading metadata", error))?;
        if !metadata.is_file() {
            return Err(Error::invalid(format!(
                "`{}` is not a file",
                path.display()
            )));
        }
        if metadata.len() > MAX_FILE {
            return Err(Error::too_large(format!(
                "`{}` is {} bytes; the limit is {MAX_FILE}",
                path.display(),
                metadata.len()
            )));
        }
        let bytes = fs::read(&path).map_err(|error| io_error("reading the file", error))?;
        if bytes.len() as u64 > MAX_FILE {
            return Err(Error::too_large(format!(
                "`{}` is over the {MAX_FILE}-byte limit",
                path.display()
            )));
        }
        Ok(json!(bytes))
    }

    fn read_link(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let (path, _, metadata) = root.lookup(&text(input, "path")?)?;
        if !metadata.file_type().is_symlink() {
            return Err(Error::invalid(format!(
                "`{}` is not a symbolic link",
                path.display()
            )));
        }
        let target = fs::read_link(&path).map_err(|error| io_error("reading the link", error))?;
        let target = target
            .to_str()
            .ok_or_else(|| Error::invalid("the link target is not UTF-8"))?;
        Ok(json!(target))
    }

    fn trash(&self) -> Result<Value, Error> {
        let trash = self.trash.lock().expect("trash mutex");
        Ok(Value::Array(
            trash
                .items()
                .into_iter()
                .map(|item| {
                    json!({
                        "name": item.name,
                        "kind": item.kind,
                        "size": item.size,
                        "modified-ms": item.modified_ms,
                        "original": item.original,
                    })
                })
                .collect(),
        ))
    }

    fn write_file(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let path = text(input, "path")?;
        let bytes = byte_list(input.get("bytes").unwrap_or(&Value::Null))?;
        if bytes.len() as u64 > MAX_FILE {
            return Err(Error::too_large(format!(
                "the payload is {} bytes; the limit is {MAX_FILE}",
                bytes.len()
            )));
        }
        let (parent, name, normalized) = root.parent(&path)?;
        let mut destination = parent.join(&name);
        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            if metadata.file_type().is_symlink() {
                destination = fs::canonicalize(&destination)
                    .map_err(|error| io_error(&format!("resolving `{normalized}`"), error))?;
                if !destination.starts_with(&self.root) {
                    return Err(Error::denied("the link escapes the approved root"));
                }
            }
            if metadata.is_dir() {
                return Err(Error::invalid(format!("`{normalized}` is a directory")));
            }
        }
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&destination)
            .map_err(|error| io_error(&format!("writing `{normalized}`"), error))?;
        file.write_all(&bytes)
            .map_err(|error| io_error(&format!("writing `{normalized}`"), error))?;
        Ok(json!(bytes.len() as u64))
    }

    fn mkdir(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let (parent, name, normalized) = root.parent(&text(input, "path")?)?;
        let path = parent.join(&name);
        if fs::symlink_metadata(&path).is_ok() {
            return Err(Error::conflict(format!("`{normalized}` already exists")));
        }
        fs::create_dir(&path)
            .map_err(|error| io_error(&format!("creating `{normalized}`"), error))?;
        Ok(Value::Null)
    }

    fn symlink(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let target = text(input, "target")?;
        let (parent, name, normalized) = root.parent(&text(input, "path")?)?;
        let path = parent.join(&name);
        if fs::symlink_metadata(&path).is_ok() {
            return Err(Error::conflict(format!("`{normalized}` already exists")));
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &path)
            .map_err(|error| io_error(&format!("linking `{normalized}`"), error))?;
        #[cfg(not(unix))]
        return Err(Error::unavailable("symbolic links need a Unix host"));
        Ok(Value::Null)
    }

    fn remove(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let path = text(input, "path")?;
        let (target, normalized, metadata) = root.lookup(&path)?;
        if normalized == "/" {
            return Err(Error::invalid("the root cannot be removed"));
        }
        let trash = boolean(input, "trash");
        if trash {
            let mut index = self.trash.lock().expect("trash mutex");
            index.add(&target, &normalized, &metadata)?;
        } else {
            remove_path(&target)?;
        }
        Ok(Value::Null)
    }

    fn rename(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let source = text(input, "source")?;
        let (source_path, source_normalized, _) = root.lookup(&source)?;
        if source_normalized == "/" {
            return Err(Error::invalid("the root cannot be renamed"));
        }
        let (parent, name, normalized) = root.parent(&text(input, "destination")?)?;
        let destination = parent.join(&name);
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(Error::conflict(format!("`{normalized}` already exists")));
        }
        fs::rename(&source_path, &destination)
            .map_err(|error| io_error(&format!("renaming `{source}`"), error))?;
        Ok(Value::Null)
    }

    fn restore(&self, input: &Value) -> Result<Value, Error> {
        let root = self.root();
        let original = text(input, "original")?;
        let (parent, name, normalized) = root.parent(&original)?;
        let destination = parent.join(&name);
        if fs::symlink_metadata(&destination).is_ok() {
            return Err(Error::conflict(format!("`{normalized}` already exists")));
        }
        let mut trash = self.trash.lock().expect("trash mutex");
        let item = trash
            .item(&normalized)
            .ok_or_else(|| Error::not_found(format!("`{normalized}` is not in the trash")))?;
        let object = trash.object(&item.stored);
        move_path(&object, &destination)?;
        trash.remove(&normalized);
        trash.persist()?;
        Ok(Value::Null)
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        match (interface, function) {
            (READ, "list") => self.list(&input),
            (READ, "stat") => self.stat(&input),
            (READ, "read-file") => self.read_file(&input),
            (READ, "read-link") => self.read_link(&input),
            (READ, "trash") => self.trash(),
            (WRITE, "write-file") => self.write_file(&input),
            (WRITE, "mkdir") => self.mkdir(&input),
            (WRITE, "symlink") => self.symlink(&input),
            (WRITE, "remove") => self.remove(&input),
            (WRITE, "rename") => self.rename(&input),
            (WRITE, "restore") => self.restore(&input),
            _ => Err(Error::not_found(format!(
                "`{interface}#{function}` is not exported by the files service"
            ))),
        }
    }
}

struct Root {
    path: PathBuf,
}

impl Root {
    fn parts(&self, input: &str) -> Result<(Vec<String>, String), Error> {
        if input.contains('\0') {
            return Err(Error::invalid("paths cannot contain NUL"));
        }
        if !input.starts_with('/') {
            return Err(Error::invalid(format!("`{input}` is not an absolute path")));
        }
        let mut parts: Vec<String> = Vec::new();
        for part in input.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if parts.pop().is_none() {
                        return Err(Error::denied("the path escapes the approved root"));
                    }
                }
                other => parts.push(other.to_string()),
            }
        }
        let normalized = format!("/{}", parts.join("/"));
        Ok((parts, normalized))
    }

    fn join(&self, parts: &[String]) -> PathBuf {
        let mut path = self.path.clone();
        for part in parts {
            path.push(part);
        }
        path
    }

    fn existing(&self, input: &str) -> Result<PathBuf, Error> {
        let (parts, _) = self.parts(input)?;
        let path = self.join(&parts);
        let canonical = fs::canonicalize(&path)
            .map_err(|error| io_error(&format!("resolving `{input}`"), error))?;
        if !canonical.starts_with(&self.path) {
            return Err(Error::denied("the path escapes the approved root"));
        }
        Ok(canonical)
    }

    /// Resolves the parent, checks it stays inside the root and returns the
    /// final component. The final component may not exist yet.
    fn parent(&self, input: &str) -> Result<(PathBuf, String, String), Error> {
        let (mut parts, normalized) = self.parts(input)?;
        let name = parts
            .pop()
            .ok_or_else(|| Error::invalid("the root has no parent"))?;
        let parent = self.join(&parts);
        let canonical = fs::canonicalize(&parent)
            .map_err(|error| io_error(&format!("resolving `{input}`"), error))?;
        if !canonical.starts_with(&self.path) {
            return Err(Error::denied("the path escapes the approved root"));
        }
        Ok((canonical, name, normalized))
    }

    /// Looks at the entry itself, without following a final symlink.
    fn lookup(&self, input: &str) -> Result<(PathBuf, String, Metadata), Error> {
        let (parts, normalized) = self.parts(input)?;
        let path = if parts.is_empty() {
            self.path.clone()
        } else {
            let (parent, name, _) = self.parent(input)?;
            parent.join(&name)
        };
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| io_error(&format!("resolving `{input}`"), error))?;
        Ok((path, normalized, metadata))
    }
}

fn details(path: &Path, metadata: &Metadata) -> (String, u64) {
    if metadata.file_type().is_symlink() {
        let size = fs::read_link(path)
            .map(|target| target.to_string_lossy().len() as u64)
            .unwrap_or(0);
        ("link".to_string(), size)
    } else if metadata.is_dir() {
        ("dir".to_string(), 0)
    } else {
        ("file".to_string(), metadata.len())
    }
}

fn modified_ms(metadata: &Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn text(input: &Value, field: &str) -> Result<String, Error> {
    input
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("`{field}` is required and must be a string")))
}

fn optional_text(input: &Value, field: &str) -> Option<String> {
    input.get(field).and_then(Value::as_str).map(str::to_string)
}

fn optional_number(input: &Value, field: &str) -> Option<u64> {
    match input.get(field)? {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn boolean(input: &Value, field: &str) -> bool {
    match input.get(field) {
        Some(Value::Bool(value)) => *value,
        Some(Value::String(value)) => value == "true",
        _ => false,
    }
}

fn byte_list(input: &Value) -> Result<Vec<u8>, Error> {
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

fn io_error(context: &str, error: std::io::Error) -> Error {
    use std::io::ErrorKind;
    let message = format!("{context}: {error}");
    match error.kind() {
        ErrorKind::NotFound => Error::not_found(message),
        ErrorKind::PermissionDenied => Error::denied(message),
        ErrorKind::AlreadyExists => Error::conflict(message),
        ErrorKind::InvalidInput | ErrorKind::InvalidData => Error::invalid(message),
        _ => Error::unavailable(message),
    }
}

fn remove_path(path: &Path) -> Result<(), Error> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| io_error("reading metadata", error))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).map_err(|error| io_error("removing the directory", error))
    } else {
        fs::remove_file(path).map_err(|error| io_error("removing the entry", error))
    }
}

fn move_path(source: &Path, destination: &Path) -> Result<(), Error> {
    if fs::rename(source, destination).is_ok() {
        return Ok(());
    }
    copy_path(source, destination)?;
    remove_path(source)
}

fn copy_path(source: &Path, destination: &Path) -> Result<(), Error> {
    let metadata =
        fs::symlink_metadata(source).map_err(|error| io_error("reading metadata", error))?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(source).map_err(|error| io_error("reading the link", error))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, destination)
            .map_err(|error| io_error("copying the link", error))?;
        #[cfg(not(unix))]
        return Err(Error::unavailable("symbolic links need a Unix host"));
        return Ok(());
    }
    if metadata.is_dir() {
        fs::create_dir(destination).map_err(|error| io_error("copying the directory", error))?;
        let entries =
            fs::read_dir(source).map_err(|error| io_error("reading the directory", error))?;
        for entry in entries {
            let entry = entry.map_err(|error| io_error("reading an entry", error))?;
            copy_path(&entry.path(), &destination.join(entry.file_name()))?;
        }
        return Ok(());
    }
    fs::copy(source, destination).map_err(|error| io_error("copying the file", error))?;
    Ok(())
}

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let root = argument("--root").unwrap_or_else(|| "/".to_string());
    let trash = argument("--trash").unwrap_or_else(|| {
        std::env::temp_dir()
            .join("anthracite-panel")
            .join("files-trash")
            .to_string_lossy()
            .to_string()
    });
    let service = match Service::new(&root, Path::new(&trash)) {
        Ok(service) => Arc::new(service),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = anth_service::serve(&socket, service).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn argument(name: &str) -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        if argument == name {
            return args.next();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("files-native-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    fn call(service: &Service, interface: &str, function: &str, input: Value) -> Value {
        service
            .call(interface, function, input)
            .expect("files call")
    }

    #[test]
    fn lists_writes_and_pages_inside_the_root() {
        let root = scratch("list");
        let trash = scratch("list-trash");
        let service = Service::new(root.to_str().expect("root"), &trash).expect("service");
        fs::create_dir(root.join("dir")).expect("dir");
        fs::write(root.join("b.txt"), b"b").expect("b");
        fs::write(root.join("a.txt"), b"a").expect("a");

        let page = call(&service, READ, "list", json!({"path": "/", "limit": 2}));
        assert_eq!(page["entries"][0]["name"], "dir");
        assert_eq!(page["entries"][1]["name"], "a.txt");
        assert_eq!(page["done"], false);
        let cursor = page["cursor"].as_str().expect("cursor");
        let next = call(
            &service,
            READ,
            "list",
            json!({"path": "/", "cursor": cursor, "limit": 2}),
        );
        assert_eq!(next["entries"][0]["name"], "b.txt");
        assert_eq!(next["done"], true);

        let written = call(
            &service,
            WRITE,
            "write-file",
            json!({"path": "/new.txt", "bytes": [104, 105]}),
        );
        assert_eq!(written, json!(2));
        assert_eq!(
            call(&service, READ, "read-file", json!({"path": "/new.txt"})),
            json!([104, 105])
        );
        assert_eq!(
            call(&service, READ, "stat", json!({"path": "/new.txt"}))["kind"],
            "file"
        );
    }

    #[test]
    fn traversal_and_symlink_escape_are_refused() {
        let root = scratch("escape");
        let trash = scratch("escape-trash");
        let service = Service::new(root.to_str().expect("root"), &trash).expect("service");
        let error = service
            .call(READ, "list", json!({"path": "/../etc"}))
            .expect_err("traversal");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);

        let outside = scratch("outside");
        fs::write(outside.join("secret"), b"secret").expect("secret");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("escape")).expect("symlink");
        let error = service
            .call(READ, "list", json!({"path": "/escape"}))
            .expect_err("symlink escape");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
    }

    #[test]
    fn trash_removes_and_restores() {
        let root = scratch("trash");
        let trash = scratch("trash-store");
        let service = Service::new(root.to_str().expect("root"), &trash).expect("service");
        fs::write(root.join("note.txt"), b"hello").expect("note");
        call(
            &service,
            WRITE,
            "remove",
            json!({"path": "/note.txt", "trash": true}),
        );
        assert!(!root.join("note.txt").exists());
        let entries = call(&service, READ, "trash", json!({}));
        assert_eq!(entries[0]["original"], "/note.txt");
        call(&service, WRITE, "restore", json!({"original": "/note.txt"}));
        assert_eq!(fs::read(root.join("note.txt")).expect("restored"), b"hello");
        assert_eq!(call(&service, READ, "trash", json!({})), json!([]));
    }
}
