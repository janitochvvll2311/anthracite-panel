//! File access under one configured root.
//!
//! Every request resolves the path twice. A lexical pass rejects `..` and
//! absolute names before any I/O; `canonicalize` then resolves symlinks, and
//! the result must stay under the root. The second check is the one that stops
//! a symlink from escaping; the first keeps an obviously hostile name from
//! reaching the filesystem at all.
//!
//! Deleting is the one write this slice supports. It is gated by
//! `files.write`, and it unlinks the final name instead of canonicalizing it:
//! resolving a symlink before `unlink` would delete its target.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use anth_wire::{ErrorKind, FileChunk, FileEntry, Page, PageRequest, WireError};

pub struct FileService {
    root: PathBuf,
    write: bool,
    max_entries: usize,
    max_preview_bytes: u64,
}

impl FileService {
    /// Builds the service. `root` is canonicalized once so every later check
    /// compares against the same absolute path.
    pub fn new(
        root: impl AsRef<Path>,
        write: bool,
        max_entries: usize,
        max_preview_bytes: u64,
    ) -> Result<Self, String> {
        let requested = root.as_ref();
        let root = fs::canonicalize(requested)
            .map_err(|error| format!("the file root {}: {error}", requested.display()))?;
        if !root.is_dir() {
            return Err(format!(
                "the file root {} is not a directory",
                root.display()
            ));
        }
        Ok(Self {
            root,
            write,
            max_entries: max_entries.max(1),
            max_preview_bytes: max_preview_bytes.max(1),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Lists one page of a directory, sorted by name.
    pub fn list(&self, path: &str, request: &PageRequest) -> Result<Page<FileEntry>, WireError> {
        if !request.valid() {
            return Err(WireError::invalid(
                "the page limit or cursor is out of bounds",
            ));
        }
        let display = display_path(path);
        let directory = self.resolve(path)?;
        let metadata = fs::metadata(&directory).map_err(|error| io_error(&display, error))?;
        if !metadata.is_dir() {
            return Err(WireError::invalid(format!("{display} is not a directory")));
        }

        let mut entries = Vec::new();
        let reader = fs::read_dir(&directory).map_err(|error| io_error(&display, error))?;
        for entry in reader {
            let entry = entry.map_err(|error| io_error(&display, error))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.is_empty() {
                continue;
            }
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                // A file that vanished between `read_dir` and `stat` is not a
                // reason to fail the whole listing.
                Err(_) => continue,
            };
            let kind = if metadata.file_type().is_symlink() {
                "symlink"
            } else if metadata.is_dir() {
                "dir"
            } else if metadata.is_file() {
                "file"
            } else {
                "other"
            };
            entries.push(FileEntry {
                name,
                kind: kind.to_string(),
                size: if metadata.is_file() {
                    metadata.len()
                } else {
                    0
                },
                modified_ms: modified_ms(&metadata),
            });
        }

        if entries.len() > self.max_entries {
            return Err(WireError::new(
                ErrorKind::TooLarge,
                "too_many_entries",
                format!(
                    "{display} has more than {} entries; use a narrower path",
                    self.max_entries
                ),
            ));
        }

        entries.sort_by(|left, right| left.name.cmp(&right.name));
        let limit = usize::from(request.limit());
        let start = match request.cursor.as_deref() {
            Some(cursor) => entries.partition_point(|entry| entry.name.as_str() <= cursor),
            None => 0,
        };
        let end = start.saturating_add(limit).min(entries.len());
        let items: Vec<FileEntry> = entries[start..end].to_vec();
        if end < entries.len() {
            let next = items.last().map(|entry| entry.name.clone());
            if let Some(next) = next {
                return Ok(Page::with_next(items, next));
            }
        }
        Ok(Page::new(items))
    }

    /// Reads one bounded chunk of a UTF-8 text file.
    pub fn read(
        &self,
        path: &str,
        offset: u64,
        max_bytes: Option<u64>,
    ) -> Result<FileChunk, WireError> {
        let display = display_path(path);
        let resolved = self.resolve(path)?;
        let metadata = fs::metadata(&resolved).map_err(|error| io_error(&display, error))?;
        if !metadata.is_file() {
            return Err(WireError::invalid(format!(
                "{display} is not a regular file"
            )));
        }
        let size = metadata.len();
        if offset > size {
            return Err(WireError::invalid(format!(
                "the offset {offset} is past the end of {display} ({size} bytes)"
            )));
        }
        let requested = max_bytes.unwrap_or(self.max_preview_bytes);
        if requested == 0 {
            return Err(WireError::invalid("max_bytes must be greater than zero"));
        }
        let take = requested.min(self.max_preview_bytes).min(size - offset);
        let capacity = usize::try_from(take)
            .map_err(|_| WireError::internal("the preview is too large for this platform"))?;
        let mut buffer = Vec::with_capacity(capacity);
        let mut file = File::open(&resolved).map_err(|error| io_error(&display, error))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| io_error(&display, error))?;
        file.take(take)
            .read_to_end(&mut buffer)
            .map_err(|error| io_error(&display, error))?;
        let text = String::from_utf8(buffer).map_err(|_| {
            WireError::new(
                ErrorKind::Unsupported,
                "not_text",
                format!("{display} is not UTF-8 text"),
            )
        })?;
        let eof = offset + u64::try_from(text.len()).unwrap_or(u64::MAX) >= size;
        Ok(FileChunk {
            path: display,
            offset,
            size,
            eof,
            text,
        })
    }

    /// Deletes one regular file or symlink under the root.
    ///
    /// The final component is never canonicalized: unlinking a symlink must
    /// remove the link, not the file it points at. The parent is resolved and
    /// checked against the root, and directories are refused.
    pub fn delete(&self, path: &str) -> Result<String, WireError> {
        if !self.write {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "write_disabled",
                "file changes are disabled; set files.write to true",
            ));
        }
        let display = display_path(path);
        let (parent, name) = split_parent(path)?;
        let canonical_parent =
            fs::canonicalize(self.root.join(parent)).map_err(|error| io_error(&display, error))?;
        if !canonical_parent.starts_with(&self.root) {
            return Err(escape("the path escapes the file root"));
        }
        let target = canonical_parent.join(name);
        let metadata = fs::symlink_metadata(&target).map_err(|error| io_error(&display, error))?;
        if metadata.is_dir() {
            return Err(WireError::invalid(format!(
                "{display} is a directory; only files can be deleted"
            )));
        }
        fs::remove_file(&target).map_err(|error| io_error(&display, error))?;
        Ok(display)
    }

    /// Resolves a request path to a canonical path under the root.
    fn resolve(&self, requested: &str) -> Result<PathBuf, WireError> {
        let display = display_path(requested);
        let relative = requested.trim_start_matches('/');
        let mut joined = self.root.clone();
        for component in Path::new(relative).components() {
            match component {
                Component::Normal(part) => joined.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    return Err(escape("the path may not contain `..`"));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(escape("the path must be relative to the file root"));
                }
            }
        }

        let canonical = fs::canonicalize(&joined).map_err(|error| io_error(&display, error))?;
        if !canonical.starts_with(&self.root) {
            return Err(escape("the path escapes the file root"));
        }
        Ok(canonical)
    }
}

/// Splits a request path into a relative parent and a final name.
///
/// `..`, absolute components and the root itself are refused before any I/O.
fn split_parent(requested: &str) -> Result<(PathBuf, OsString), WireError> {
    let relative = Path::new(requested.trim_start_matches('/'));
    let mut parent = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parent.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(escape("the path may not contain `..`"));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(escape("the path must be relative to the file root"));
            }
        }
    }
    let Some(name) = parent.file_name().map(OsString::from) else {
        return Err(WireError::invalid("the file root cannot be deleted"));
    };
    parent.pop();
    Ok((parent, name))
}

/// The path as the client sees it, with `/` separators and no leading slash.
fn display_path(requested: &str) -> String {
    let trimmed = requested.trim_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn escape(message: &str) -> WireError {
    WireError::new(ErrorKind::Forbidden, "path_escape", message)
}

fn modified_ms(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn io_error(path: &str, error: std::io::Error) -> WireError {
    match error.kind() {
        std::io::ErrorKind::NotFound => WireError::not_found(format!("{path} does not exist")),
        std::io::ErrorKind::PermissionDenied => {
            WireError::forbidden(format!("permission denied for {path}"))
        }
        _ => WireError::internal(format!("reading {path}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = PathBuf::from("/tmp/anthracite-panel/tests")
            .join(format!("files-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root).expect("create scratch");
        root
    }

    fn service(root: &Path) -> FileService {
        FileService::new(root, false, 100, 64).expect("service")
    }

    fn writable(root: &Path) -> FileService {
        FileService::new(root, true, 100, 64).expect("service")
    }

    #[test]
    fn a_directory_lists_in_name_order_and_pages() {
        let root = scratch("list");
        fs::write(root.join("b.txt"), b"b").expect("write");
        fs::write(root.join("a.txt"), b"a").expect("write");
        fs::create_dir(root.join("dir")).expect("dir");
        let files = service(&root);

        let first = files
            .list(
                "/",
                &PageRequest {
                    limit: Some(2),
                    cursor: None,
                },
            )
            .expect("first");
        assert_eq!(
            first
                .items
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a.txt", "b.txt"]
        );
        assert_eq!(first.next.as_deref(), Some("b.txt"));

        let second = files
            .list(
                "/",
                &PageRequest {
                    limit: Some(2),
                    cursor: first.next.clone(),
                },
            )
            .expect("second");
        assert_eq!(
            second
                .items
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["dir"]
        );
        assert_eq!(second.next, None);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn traversal_and_absolute_paths_are_refused() {
        let root = scratch("traversal");
        let files = service(&root);
        assert_eq!(
            files
                .list("../", &PageRequest::default())
                .expect_err("parent")
                .code,
            "path_escape"
        );
        assert_eq!(
            files
                .list("/../", &PageRequest::default())
                .expect_err("parent")
                .code,
            "path_escape"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_symlink_out_of_the_root_is_refused() {
        let root = scratch("symlink");
        let outside = scratch("symlink-outside");
        fs::write(outside.join("secret.txt"), b"secret").expect("write");
        symlink(&outside, root.join("escape")).expect("symlink");
        let files = service(&root);

        let error = files
            .list("/escape", &PageRequest::default())
            .expect_err("escaped");
        assert_eq!(error.kind, ErrorKind::Forbidden);
        fs::remove_dir_all(root).expect("cleanup");
        fs::remove_dir_all(outside).expect("cleanup");
    }

    #[test]
    fn reading_a_text_file_respects_offset_and_the_cap() {
        let root = scratch("read");
        fs::write(root.join("note.txt"), b"hello world").expect("write");
        let files = service(&root);

        let chunk = files.read("note.txt", 0, Some(5)).expect("read");
        assert_eq!(chunk.text, "hello");
        assert!(!chunk.eof);
        assert_eq!(chunk.size, 11);

        let next = files.read("note.txt", 5, Some(64)).expect("read rest");
        assert_eq!(next.text, " world");
        assert!(next.eof);

        let empty = files.read("note.txt", 11, None).expect("at end");
        assert!(empty.eof);
        assert!(empty.text.is_empty());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_non_regular_or_non_text_file_is_refused() {
        let root = scratch("refuse");
        fs::create_dir(root.join("dir")).expect("dir");
        fs::write(root.join("binary.bin"), [0xff, 0xfe, 0x00]).expect("write");
        let files = service(&root);

        assert_eq!(
            files.read("dir", 0, None).expect_err("dir").kind,
            ErrorKind::Invalid
        );
        assert_eq!(
            files.read("binary.bin", 0, None).expect_err("binary").kind,
            ErrorKind::Unsupported
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_missing_path_is_not_found() {
        let root = scratch("missing");
        let files = service(&root);
        assert_eq!(
            files
                .list("nope", &PageRequest::default())
                .expect_err("missing")
                .kind,
            ErrorKind::NotFound
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn deleting_a_file_requires_write_and_unlinks_it() {
        let root = scratch("delete");
        fs::write(root.join("note.txt"), b"hello").expect("write");
        let files = writable(&root);

        assert_eq!(files.delete("/note.txt").expect("delete"), "note.txt");
        assert!(!root.join("note.txt").exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn deleting_is_refused_when_writes_are_disabled() {
        let root = scratch("delete-off");
        fs::write(root.join("note.txt"), b"hello").expect("write");
        let files = service(&root);

        assert_eq!(
            files.delete("note.txt").expect_err("disabled").code,
            "write_disabled"
        );
        assert!(root.join("note.txt").exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn deleting_a_directory_or_escaping_path_is_refused() {
        let root = scratch("delete-guard");
        fs::create_dir(root.join("dir")).expect("dir");
        let files = writable(&root);

        assert_eq!(
            files.delete("dir").expect_err("dir").kind,
            ErrorKind::Invalid
        );
        assert_eq!(
            files.delete("../etc/hosts").expect_err("parent").code,
            "path_escape"
        );
        assert_eq!(
            files.delete("/").expect_err("root").kind,
            ErrorKind::Invalid
        );
        assert!(root.join("dir").exists());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn deleting_a_symlink_removes_the_link_and_keeps_the_target() {
        let root = scratch("delete-symlink");
        let target = root.join("target.txt");
        fs::write(&target, b"keep me").expect("write");
        symlink(&target, root.join("link")).expect("symlink");
        let files = writable(&root);

        files.delete("link").expect("delete");
        assert!(!root.join("link").exists());
        assert!(target.exists());

        fs::remove_dir_all(root).expect("cleanup");
    }
}
