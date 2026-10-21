//! Filesystem explorer: path containment, listings, reads and atomic writes.
//!
//! Paths on the wire are virtual: `/` is the configured root. Every operation
//! resolves its parent (or itself, for reads) and rejects paths that escape the
//! root through `..` components or symlinks.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{
    FileChunk, FileEntry, FileError, FileErrorKind, FileKind, FileListing, FileWritten,
    MAX_FILE_CHUNK,
};

use crate::config::FilesConfig;
use anth_proto::TrashListing;

pub mod trash;
pub mod volumes;

pub use trash::{TrashDirectory, TrashService};
pub use volumes::{home_paths, live_topdirs, topdirs_from_mountinfo};

static UPLOAD_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A validated view of the host filesystem exposed by the daemon.
#[derive(Debug, Clone)]
pub struct FileService {
    root: PathBuf,
    write: bool,
    max_entries: usize,
    max_upload_bytes: u64,
    /// Whether `[files] trash` is on. Separate from `write`: trashing deletes
    /// an entry from its directory, so it is its own permission.
    trash: bool,
    max_trash_bytes: Option<u64>,
    /// Overrides where the trash lives. Tests point it at a temporary
    /// directory so the trash stays inside the fixture; production reads
    /// mountinfo and the XDG variables.
    trash_paths: Option<TrashPaths>,
}

/// The volumes and directories the trash resolves against.
#[derive(Debug, Clone)]
pub struct TrashPaths {
    pub topdirs: Vec<PathBuf>,
    pub home: PathBuf,
    pub data_home: PathBuf,
}

/// An upload that has not been committed yet.
///
/// Dropping the value removes the temporary file, so a closed connection or an
/// explicit abort cannot leave partial uploads behind.
#[derive(Debug)]
pub struct PendingUpload {
    virtual_path: String,
    final_path: PathBuf,
    temp_path: PathBuf,
    file: Option<File>,
    received: u64,
    max_bytes: u64,
    overwrite: bool,
    committed: bool,
}

impl FileService {
    /// Validates the configured root and limits.
    pub fn new(config: &FilesConfig) -> Result<Self, String> {
        if config.root.is_empty() || !Path::new(&config.root).is_absolute() {
            return Err(format!(
                "files.root must be an absolute path, got `{}`",
                config.root
            ));
        }
        if config.max_entries == 0 {
            return Err("files.max_entries must be greater than zero".to_string());
        }
        if config.max_preview_bytes == 0 {
            return Err("files.max_preview_bytes must be greater than zero".to_string());
        }
        if config.max_upload_bytes == 0 {
            return Err("files.max_upload_bytes must be greater than zero".to_string());
        }

        let root = fs::canonicalize(&config.root)
            .map_err(|e| format!("cannot resolve files.root `{}`: {e}", config.root))?;
        if !root.is_dir() {
            return Err(format!("files.root `{}` is not a directory", config.root));
        }

        Ok(Self {
            root,
            write: config.write,
            max_entries: config.max_entries,
            max_upload_bytes: config.max_upload_bytes,
            trash: config.trash,
            max_trash_bytes: (config.max_trash_bytes > 0).then_some(config.max_trash_bytes),
            trash_paths: None,
        })
    }

    pub fn write_enabled(&self) -> bool {
        self.write
    }

    pub fn max_upload_bytes(&self) -> u64 {
        self.max_upload_bytes
    }

    /// Pins the mount table instead of reading `/proc/self/mountinfo`. The home
    /// and data directories keep following the environment.
    pub fn with_topdirs(self, topdirs: Vec<PathBuf>) -> Self {
        let (home, data_home) = home_paths();
        self.with_trash_paths(TrashPaths {
            topdirs,
            home,
            data_home,
        })
    }

    /// Pins the whole trash view, which is what a test needs: the mount table
    /// alone still sends the home volume's trash to the real `$HOME`.
    pub fn with_trash_paths(mut self, paths: TrashPaths) -> Self {
        self.trash_paths = Some(paths);
        self
    }

    fn topdirs(&self) -> Vec<PathBuf> {
        match &self.trash_paths {
            Some(paths) => paths.topdirs.clone(),
            None => live_topdirs(),
        }
    }

    /// The trash service, bound to the mount table.
    pub fn trash_service(&self) -> TrashService {
        let (home, data_home) = match &self.trash_paths {
            Some(paths) => (paths.home.clone(), paths.data_home.clone()),
            None => home_paths(),
        };
        TrashService::new(
            self.trash,
            self.max_trash_bytes,
            self.max_entries,
            home,
            data_home,
            self.topdirs(),
        )
    }

    /// Lists one directory below the configured root.
    pub fn list(&self, path: &str) -> Result<FileListing, FileError> {
        let path = normalize(path)?;
        let host = self.host_existing(&path)?;
        let metadata = fs::metadata(&host).map_err(|e| io_error(&path, e))?;
        if !metadata.is_dir() {
            return Err(FileError::new(
                FileErrorKind::NotADirectory,
                format!("`{path}` is not a directory"),
            ));
        }

        let read_dir = fs::read_dir(&host).map_err(|e| io_error(&path, e))?;
        let mut entries = Vec::new();

        for item in read_dir {
            let item = item.map_err(|e| io_error(&path, e))?;
            let name = item.file_name().to_string_lossy().into_owned();
            let item_path = join_virtual(&path, &name);
            let item_host = item.path();
            let metadata = fs::symlink_metadata(&item_host).map_err(|e| io_error(&item_path, e))?;
            let file_type = metadata.file_type();
            let kind = if file_type.is_symlink() {
                FileKind::Symlink
            } else if file_type.is_dir() {
                FileKind::Directory
            } else if file_type.is_file() {
                FileKind::File
            } else {
                FileKind::Other
            };
            let target = if file_type.is_symlink() {
                fs::read_link(&item_host)
                    .ok()
                    .map(|target| target.to_string_lossy().into_owned())
            } else {
                None
            };
            let size = if file_type.is_dir() {
                0
            } else {
                metadata.len()
            };

            entries.push(FileEntry {
                hidden: name.starts_with('.'),
                name,
                path: item_path,
                kind,
                size,
                modified_ms: modified_ms(&metadata),
                target,
            });
        }

        entries.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        });

        let total = entries.len();
        let truncated = total > self.max_entries;
        if truncated {
            entries.truncate(self.max_entries);
        }

        Ok(FileListing {
            parent: parent_virtual(&path),
            path,
            entries,
            total,
            truncated,
            warning: truncated.then(|| {
                format!(
                    "showing {} of {total} entries; raise files.max_entries to see more",
                    self.max_entries
                )
            }),
            write_enabled: self.write,
        })
    }

    /// Reads at most one protocol chunk at `offset`.
    pub fn read(&self, path: &str, offset: u64, len: u32) -> Result<FileChunk, FileError> {
        let path = normalize(path)?;
        let host = self.host_existing(&path)?;
        let metadata = fs::metadata(&host).map_err(|e| io_error(&path, e))?;
        if metadata.is_dir() {
            return Err(FileError::new(
                FileErrorKind::IsADirectory,
                format!("`{path}` is a directory"),
            ));
        }
        if !metadata.is_file() {
            return Err(FileError::new(
                FileErrorKind::InvalidPath,
                format!("`{path}` is not a regular file"),
            ));
        }

        let size = metadata.len();
        let offset = offset.min(size);
        let len = len.min(MAX_FILE_CHUNK);
        let mut file = File::open(&host).map_err(|e| io_error(&path, e))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| io_error(&path, e))?;

        let mut bytes = vec![0u8; len as usize];
        let read = file.read(&mut bytes).map_err(|e| io_error(&path, e))?;
        bytes.truncate(read);

        Ok(FileChunk {
            eof: offset + read as u64 >= size,
            path,
            offset,
            size,
            bytes,
        })
    }

    /// Creates a directory. `recursive` maps to `create_dir_all`.
    pub fn mkdir(&self, path: &str, recursive: bool) -> Result<String, FileError> {
        self.require_write()?;
        let path = normalize(path)?;
        if path == "/" {
            return Err(FileError::new(
                FileErrorKind::AlreadyExists,
                "the filesystem root already exists",
            ));
        }

        let host = if recursive {
            self.host_create_any(&path)?
        } else {
            self.host_create(&path)?
        };
        if recursive {
            fs::create_dir_all(&host).map_err(|e| io_error(&path, e))?;
        } else {
            fs::create_dir(&host).map_err(|e| io_error(&path, e))?;
        }
        Ok(path)
    }

    /// Renames one entry. Refuses to overwrite an existing destination.
    pub fn rename(&self, from: &str, to: &str) -> Result<(String, String), FileError> {
        self.require_write()?;
        let from = normalize(from)?;
        let to = normalize(to)?;
        if from == "/" || to == "/" {
            return Err(FileError::new(
                FileErrorKind::InvalidPath,
                "the filesystem root cannot be renamed",
            ));
        }

        let from_host = self.host_entry(&from)?;
        let to_host = self.host_create(&to)?;
        fs::symlink_metadata(&from_host).map_err(|e| io_error(&from, e))?;

        match fs::symlink_metadata(&to_host) {
            Ok(_) => {
                return Err(FileError::new(
                    FileErrorKind::AlreadyExists,
                    format!("`{to}` already exists"),
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&to, e)),
        }

        fs::rename(&from_host, &to_host).map_err(|e| io_error(&from, e))?;
        Ok((from, to))
    }

    /// Deletes a file, symlink or directory. Non-empty directories need
    /// `recursive`.
    pub fn delete(&self, path: &str, recursive: bool) -> Result<String, FileError> {
        self.require_write()?;
        let path = normalize(path)?;
        if path == "/" {
            return Err(FileError::new(
                FileErrorKind::InvalidPath,
                "the filesystem root cannot be deleted",
            ));
        }

        let host = self.host_entry(&path)?;
        let metadata = fs::symlink_metadata(&host).map_err(|e| io_error(&path, e))?;
        if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            if recursive {
                fs::remove_dir_all(&host).map_err(|e| io_error(&path, e))?;
            } else {
                fs::remove_dir(&host).map_err(|e| io_error(&path, e))?;
            }
        } else {
            fs::remove_file(&host).map_err(|e| io_error(&path, e))?;
        }

        Ok(path)
    }

    /// Moves one entry to the trash of the volume holding it.
    ///
    /// Refuses when the volume has no usable trash directory rather than
    /// quietly making the delete permanent: a user who picked "move to trash"
    /// should not lose the file because a mount was read-only.
    pub fn trash(&self, path: &str) -> Result<(String, String), FileError> {
        self.require_write()?;
        if !self.trash {
            return Err(FileError::new(
                FileErrorKind::Disabled,
                "the trash is disabled; enable `[files] trash`".to_string(),
            ));
        }
        let path = normalize(path)?;
        if path == "/" {
            return Err(FileError::new(
                FileErrorKind::InvalidPath,
                "the filesystem root cannot be trashed",
            ));
        }

        let host = self.host_entry(&path)?;
        // `symlink_metadata` so a dangling symlink can be trashed, and so a
        // symlink is moved rather than the directory it points at.
        fs::symlink_metadata(&host).map_err(|e| io_error(&path, e))?;

        let topdir = self.topdir_of(&host).ok_or_else(|| {
            FileError::new(
                FileErrorKind::Disabled,
                format!("no mount point covers `{path}`, so it has no trash"),
            )
        })?;

        let service = self.trash_service();
        let id = service.move_to_trash(&host, &path, &topdir)?;
        Ok((path, id))
    }

    /// Restores one trashed entry to its recorded path.
    pub fn restore(&self, id: &str, overwrite: bool) -> Result<(String, String), FileError> {
        self.require_write()?;
        let service = self.trash_service();
        let entry = service.find(id)?;

        // The recorded path was written by whoever deleted the file, which may
        // be another application, so it is untrusted input: normalize rejects
        // `..`, and the host resolution below keeps it inside the root.
        let path = normalize(&entry.original_path)?;
        if path == "/" {
            return Err(FileError::new(
                FileErrorKind::InvalidPath,
                "the filesystem root is not a restore target",
            ));
        }
        let host_target = self.host_create(&path)?;

        service.restore(id, &host_target, overwrite)?;
        Ok((id.to_string(), path))
    }

    /// Empties every trash directory the daemon can see.
    pub fn empty_trash(&self) -> Result<usize, FileError> {
        self.require_write()?;
        self.trash_service().empty()
    }

    /// Lists every trash directory the daemon can see.
    pub fn trash_listing(&self) -> Result<TrashListing, FileError> {
        self.trash_service().list()
    }

    /// The longest mount point that contains `host`, which is the volume whose
    /// trash the entry belongs in.
    fn topdir_of(&self, host: &Path) -> Option<PathBuf> {
        let mut best: Option<PathBuf> = None;
        for topdir in self.topdirs() {
            if host.starts_with(&topdir) {
                let longer = best.as_ref().is_none_or(|current| {
                    topdir.components().count() > current.components().count()
                });
                if longer {
                    best = Some(topdir);
                }
            }
        }
        best
    }

    /// Opens a temporary file next to the destination.
    pub fn start_upload(&self, path: &str, overwrite: bool) -> Result<PendingUpload, FileError> {
        self.require_write()?;
        let path = normalize(path)?;
        if path == "/" {
            return Err(FileError::new(
                FileErrorKind::IsADirectory,
                "cannot upload over the filesystem root",
            ));
        }

        let final_path = self.host_create(&path)?;
        match fs::symlink_metadata(&final_path) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                return Err(FileError::new(
                    FileErrorKind::IsADirectory,
                    format!("`{path}` is a directory"),
                ));
            }
            Ok(_) if !overwrite => {
                return Err(FileError::new(
                    FileErrorKind::AlreadyExists,
                    format!("`{path}` already exists"),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&path, e)),
        }

        let parent = final_path
            .parent()
            .ok_or_else(|| FileError::new(FileErrorKind::InvalidPath, "path has no parent"))?;
        let temp_path = unique_temp_path(parent);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|e| io_error(&path, e))?;

        Ok(PendingUpload {
            virtual_path: path,
            final_path,
            temp_path,
            file: Some(file),
            received: 0,
            max_bytes: self.max_upload_bytes,
            overwrite,
            committed: false,
        })
    }

    fn require_write(&self) -> Result<(), FileError> {
        if self.write {
            Ok(())
        } else {
            Err(FileError::new(
                FileErrorKind::Disabled,
                "file writes are disabled; set [files] write = true",
            ))
        }
    }

    /// Resolves an existing path and follows symlinks, rejecting escapes.
    fn host_existing(&self, path: &str) -> Result<PathBuf, FileError> {
        let relative = path.trim_start_matches('/');
        let candidate = if relative.is_empty() {
            self.root.clone()
        } else {
            self.root.join(relative)
        };
        let canonical = fs::canonicalize(&candidate).map_err(|e| io_error(path, e))?;
        if !canonical.starts_with(&self.root) {
            return Err(FileError::new(
                FileErrorKind::InvalidPath,
                format!("`{path}` escapes the configured filesystem root"),
            ));
        }
        Ok(canonical)
    }

    /// Resolves the parent without following the entry itself, so a symlink can
    /// be inspected, renamed or deleted without touching its target.
    fn host_entry(&self, path: &str) -> Result<PathBuf, FileError> {
        let (parent, name) = split_virtual(path)?;
        Ok(self.host_existing(parent)?.join(name))
    }

    /// Resolves a destination whose final component may not exist yet.
    fn host_create(&self, path: &str) -> Result<PathBuf, FileError> {
        let (parent, name) = split_virtual(path)?;
        Ok(self.host_existing(parent)?.join(name))
    }

    /// Resolves a path whose intermediate components may not exist, as needed
    /// by recursive mkdir. Existing components are canonicalized and checked
    /// against the root; a missing component starts the remaining suffix.
    fn host_create_any(&self, path: &str) -> Result<PathBuf, FileError> {
        let relative = path.trim_start_matches('/');
        if relative.is_empty() {
            return Ok(self.root.clone());
        }

        let mut current = self.root.clone();
        for component in relative.split('/') {
            let candidate = current.join(component);
            match fs::canonicalize(&candidate) {
                Ok(canonical) => {
                    if !canonical.starts_with(&self.root) {
                        return Err(FileError::new(
                            FileErrorKind::InvalidPath,
                            format!("`{path}` escapes the configured filesystem root"),
                        ));
                    }
                    current = canonical;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    current = candidate;
                }
                Err(e) => return Err(io_error(path, e)),
            }
        }
        Ok(current)
    }
}

impl PendingUpload {
    /// Appends one chunk, enforcing the per-chunk and total upload limits.
    pub fn write_chunk(&mut self, bytes: &[u8]) -> Result<u64, FileError> {
        if bytes.len() > MAX_FILE_CHUNK as usize {
            return Err(FileError::new(
                FileErrorKind::TooLarge,
                format!(
                    "chunk is {} bytes; the maximum is {MAX_FILE_CHUNK}",
                    bytes.len()
                ),
            ));
        }

        let received = self
            .received
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| FileError::new(FileErrorKind::TooLarge, "upload size overflow"))?;
        if received > self.max_bytes {
            return Err(FileError::new(
                FileErrorKind::TooLarge,
                format!("upload exceeds files.max_upload_bytes ({})", self.max_bytes),
            ));
        }

        let file = self
            .file
            .as_mut()
            .ok_or_else(|| FileError::new(FileErrorKind::Io, "upload is already closed"))?;
        file.write_all(bytes)
            .map_err(|e| io_error(&self.virtual_path, e))?;
        self.received = received;
        Ok(received)
    }

    /// Flushes, closes and atomically renames the temporary file.
    pub fn finish(mut self) -> Result<FileWritten, FileError> {
        let mut file = self
            .file
            .take()
            .ok_or_else(|| FileError::new(FileErrorKind::Io, "upload is already closed"))?;
        file.flush().map_err(|e| io_error(&self.virtual_path, e))?;
        file.sync_all()
            .map_err(|e| io_error(&self.virtual_path, e))?;
        drop(file);

        match fs::symlink_metadata(&self.final_path) {
            Ok(_) if !self.overwrite => {
                return Err(FileError::new(
                    FileErrorKind::AlreadyExists,
                    format!("`{}` already exists", self.virtual_path),
                ));
            }
            Ok(metadata) if metadata.file_type().is_dir() => {
                return Err(FileError::new(
                    FileErrorKind::IsADirectory,
                    format!("`{}` is a directory", self.virtual_path),
                ));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&self.virtual_path, e)),
        }

        fs::rename(&self.temp_path, &self.final_path)
            .map_err(|e| io_error(&self.virtual_path, e))?;
        self.committed = true;

        Ok(FileWritten {
            path: self.virtual_path.clone(),
            size: self.received,
        })
    }
}

impl Drop for PendingUpload {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temp_path);
        }
    }
}

/// Normalizes a virtual path, collapsing empty and `.` components and rejecting
/// `..`.
pub fn normalize(path: &str) -> Result<String, FileError> {
    if path.contains('\0') {
        return Err(FileError::new(
            FileErrorKind::InvalidPath,
            "paths cannot contain NUL bytes",
        ));
    }

    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                return Err(FileError::new(
                    FileErrorKind::InvalidPath,
                    "paths cannot contain `..`",
                ));
            }
            value => parts.push(value),
        }
    }

    if parts.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(format!("/{}", parts.join("/")))
    }
}

fn split_virtual(path: &str) -> Result<(&str, &str), FileError> {
    if path == "/" {
        return Err(FileError::new(
            FileErrorKind::InvalidPath,
            "the filesystem root is not a valid entry",
        ));
    }
    let index = path
        .rfind('/')
        .ok_or_else(|| FileError::new(FileErrorKind::InvalidPath, "path must be absolute"))?;
    let parent = if index == 0 { "/" } else { &path[..index] };
    let name = &path[index + 1..];
    if name.is_empty() || name == "." || name == ".." {
        return Err(FileError::new(
            FileErrorKind::InvalidPath,
            "path has no valid final component",
        ));
    }
    Ok((parent, name))
}

fn join_virtual(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn parent_virtual(path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }
    match path.rfind('/') {
        Some(0) => Some("/".to_string()),
        Some(index) => Some(path[..index].to_string()),
        None => None,
    }
}

fn modified_ms(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

fn unique_temp_path(parent: &Path) -> PathBuf {
    let counter = UPLOAD_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    parent.join(format!(
        ".anth-upload-{}-{nanos}-{counter}",
        std::process::id()
    ))
}

fn io_error(path: &str, error: std::io::Error) -> FileError {
    let kind = match error.kind() {
        std::io::ErrorKind::NotFound => FileErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => FileErrorKind::PermissionDenied,
        std::io::ErrorKind::AlreadyExists => FileErrorKind::AlreadyExists,
        _ => match error.raw_os_error() {
            Some(libc::ENOTDIR) => FileErrorKind::NotADirectory,
            Some(libc::EISDIR) => FileErrorKind::IsADirectory,
            Some(libc::ENOTEMPTY) => FileErrorKind::DirectoryNotEmpty,
            _ => FileErrorKind::Io,
        },
    };
    FileError::new(kind, format!("`{path}`: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_virtual_paths() {
        assert_eq!(normalize("").unwrap(), "/");
        assert_eq!(normalize("/").unwrap(), "/");
        assert_eq!(normalize("foo").unwrap(), "/foo");
        assert_eq!(normalize("//foo/./bar//").unwrap(), "/foo/bar");
        assert_eq!(parent_virtual("/foo/bar").as_deref(), Some("/foo"));
        assert_eq!(parent_virtual("/foo").as_deref(), Some("/"));
        assert_eq!(parent_virtual("/"), None);
    }

    #[test]
    fn rejects_parent_components_and_nul() {
        assert_eq!(
            normalize("../etc").unwrap_err().kind,
            FileErrorKind::InvalidPath
        );
        assert_eq!(
            normalize("/foo/../bar").unwrap_err().kind,
            FileErrorKind::InvalidPath
        );
        assert_eq!(
            normalize("/foo\0bar").unwrap_err().kind,
            FileErrorKind::InvalidPath
        );
    }

    #[test]
    fn split_rejects_the_root() {
        assert_eq!(
            split_virtual("/").unwrap_err().kind,
            FileErrorKind::InvalidPath
        );
        assert_eq!(split_virtual("/foo").unwrap(), ("/", "foo"));
    }
}
