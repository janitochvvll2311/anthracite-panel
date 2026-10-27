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
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use anth_wire::{
    ArchiveFormat, ArchiveListing, ErrorKind, FileChunk, FileEntry, FileWritten, Page, PageRequest,
    TrashListing, WireError,
};

pub mod archive;
mod error;
mod trash;
mod volumes;

pub use trash::{TrashDirectory, TrashService};

use error::{FileError, FileErrorKind};

pub struct FileService {
    root: PathBuf,
    write: bool,
    max_entries: usize,
    max_preview_bytes: u64,
    max_write_bytes: u64,
    /// Whether `[files] trash` is on. Separate from `write`: trashing removes
    /// an entry from its directory, so it is its own permission.
    trash: bool,
    max_trash_bytes: Option<u64>,
    /// Ceiling on the uncompressed size one extraction may produce.
    max_archive_bytes: u64,
    /// Overrides where the trash lives, so tests stay inside their fixture.
    trash_paths: Option<TrashPaths>,
}

/// The volumes and directories the trash resolves against.
#[derive(Debug, Clone)]
pub struct TrashPaths {
    pub topdirs: Vec<PathBuf>,
    pub home: PathBuf,
    pub data_home: PathBuf,
}

impl FileService {
    /// Builds the service. `root` is canonicalized once so every later check
    /// compares against the same absolute path.
    pub fn new(
        root: impl AsRef<Path>,
        write: bool,
        max_entries: usize,
        max_preview_bytes: u64,
        max_write_bytes: u64,
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
            max_write_bytes: max_write_bytes.max(1),
            trash: false,
            max_trash_bytes: Some(1024 * 1024 * 1024),
            max_archive_bytes: 4 * 1024 * 1024 * 1024,
            trash_paths: None,
        })
    }

    /// Enables the trash and its retention ceiling (`0` keeps every entry).
    pub fn with_trash(mut self, enabled: bool, max_bytes: u64) -> Self {
        self.trash = enabled;
        self.max_trash_bytes = (max_bytes > 0).then_some(max_bytes);
        self
    }

    /// Sets the uncompressed ceiling for one archive extraction.
    pub fn with_archive_limit(mut self, max_bytes: u64) -> Self {
        self.max_archive_bytes = max_bytes.max(1);
        self
    }

    /// Pins the trash view; production reads mountinfo and the XDG variables.
    pub fn with_trash_paths(mut self, paths: TrashPaths) -> Self {
        self.trash_paths = Some(paths);
        self
    }

    fn topdirs(&self) -> Vec<PathBuf> {
        match &self.trash_paths {
            Some(paths) => paths.topdirs.clone(),
            None => volumes::live_topdirs(),
        }
    }

    /// The trash service, bound to the current mount table.
    pub fn trash_service(&self) -> TrashService {
        let (home, data_home) = match &self.trash_paths {
            Some(paths) => (paths.home.clone(), paths.data_home.clone()),
            None => volumes::home_paths(),
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

    /// Lists one page of a directory: directories first, then everything else,
    /// each group by name.
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

        entries.sort_by(|left, right| entry_key(left).cmp(&entry_key(right)));
        let limit = usize::from(request.limit());
        let start = match request.cursor.as_deref() {
            Some(cursor) => {
                let key = cursor_key(cursor);
                entries.partition_point(|entry| entry_key(entry) <= key)
            }
            None => 0,
        };
        let end = start.saturating_add(limit).min(entries.len());
        let items: Vec<FileEntry> = entries[start..end].to_vec();
        if end < entries.len() {
            let next = items.last().map(|entry| {
                let group = if entry.kind == "dir" { 'd' } else { 'f' };
                format!("{group}:{}", entry.name)
            });
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
        self.require_write()?;
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

    /// Moves one entry to the trash of the volume holding it.
    ///
    /// Refuses when the volume has no usable trash directory rather than
    /// quietly making the delete permanent.
    pub fn trash(&self, path: &str) -> Result<(String, String), WireError> {
        self.require_write()?;
        if !self.trash {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "trash_disabled",
                "the trash is disabled; enable `[files] trash`",
            ));
        }
        let display = display_path(path);
        if display == "/" {
            return Err(WireError::invalid("the file root cannot be trashed"));
        }
        if display.split('/').any(|part| part == "..") {
            return Err(escape("the path may not contain `..`"));
        }

        let host = self.host_entry(&display).map_err(trash_wire)?;
        // `symlink_metadata` so a dangling symlink can be trashed, and so a
        // symlink is moved rather than the directory it points at.
        fs::symlink_metadata(&host)
            .map_err(|error| file_io_error(&display, error))
            .map_err(trash_wire)?;

        let topdir = self.topdir_of(&host).ok_or_else(|| {
            WireError::new(
                ErrorKind::Forbidden,
                "trash_unavailable",
                format!("no mount point covers {display}, so it has no trash"),
            )
        })?;

        let id = self
            .trash_service()
            .move_to_trash(&host, &format!("/{display}"), &topdir)
            .map_err(trash_wire)?;
        Ok((display, id))
    }

    /// Restores one trashed entry to its recorded path.
    ///
    /// The recorded path comes from whichever application deleted the entry,
    /// so it is untrusted: `display_path` and the host resolution below keep it
    /// inside the configured root.
    pub fn restore(&self, id: &str, overwrite: bool) -> Result<(String, String), WireError> {
        self.require_write()?;
        let service = self.trash_service();
        let entry = service.find(id).map_err(trash_wire)?;
        let path = display_path(&entry.original_path);
        if path == "/" {
            return Err(WireError::invalid("the file root is not a restore target"));
        }
        let host_target = self.host_create(&path).map_err(trash_wire)?;
        service
            .restore(id, &host_target, overwrite)
            .map_err(trash_wire)?;
        Ok((id.to_string(), path))
    }

    /// Empties every trash directory the daemon can see.
    pub fn empty_trash(&self) -> Result<usize, WireError> {
        self.require_write()?;
        self.trash_service().empty().map_err(trash_wire)
    }

    /// Lists every trash directory the daemon can see.
    pub fn trash_listing(&self) -> Result<TrashListing, WireError> {
        self.trash_service().list().map_err(trash_wire)
    }

    /// The longest mount point that contains `host`.
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

    /// Compresses one entry.
    pub fn create_archive(
        &self,
        path: &str,
        destination: Option<&str>,
        format: Option<ArchiveFormat>,
    ) -> Result<String, WireError> {
        self.require_write()?;
        let path = display_path(path);
        let source = self.host_existing(&path).map_err(archive_wire)?;

        let (destination, format) = match destination {
            Some(given) => {
                let destination = display_path(given);
                let format = format
                    .or_else(|| archive::detect(Path::new(&destination), None))
                    .ok_or_else(|| {
                        WireError::new(
                            ErrorKind::Invalid,
                            "archive_format",
                            format!("cannot tell the archive format of {destination}"),
                        )
                    })?;
                (destination, format)
            }
            None => {
                let format = format.unwrap_or(ArchiveFormat::TarGz);
                let suffix = match format {
                    ArchiveFormat::Zip => "zip",
                    ArchiveFormat::Tar => "tar",
                    ArchiveFormat::TarGz => "tar.gz",
                    ArchiveFormat::TarXz => "tar.xz",
                    ArchiveFormat::SevenZ => {
                        return Err(WireError::new(
                            ErrorKind::Unsupported,
                            "archive_format",
                            "creating a .7z archive is not supported; extract it instead",
                        ));
                    }
                };
                (format!("{path}.{suffix}"), format)
            }
        };

        if destination == path {
            return Err(WireError::invalid(
                "an archive cannot be written over its own source",
            ));
        }
        let host_destination = self.host_create(&destination).map_err(archive_wire)?;
        match fs::symlink_metadata(&host_destination) {
            Ok(_) => {
                return Err(WireError::new(
                    ErrorKind::Conflict,
                    "exists",
                    format!("{destination} already exists"),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(WireError::from(file_io_error(&destination, error))),
        }

        archive::create(&source, &host_destination, format)
            .map_err(|error| archive_wire(replace_path(error, &destination)))?;
        Ok(destination)
    }

    /// Lists one archive without extracting it.
    pub fn list_archive(&self, path: &str) -> Result<ArchiveListing, WireError> {
        let path = display_path(path);
        let host = self.host_existing(&path).map_err(archive_wire)?;
        let magic = read_magic(&host);
        let format = archive::detect(Path::new(&path), magic.as_deref()).ok_or_else(|| {
            WireError::new(
                ErrorKind::Invalid,
                "archive_format",
                format!("{path} is not an archive this daemon can read"),
            )
        })?;

        let entries = archive::list(&host, format, self.max_entries)
            .map_err(|error| archive_wire(replace_path(error, &path)))?;
        let total = entries.len();
        let truncated = total >= self.max_entries;
        Ok(ArchiveListing {
            path,
            format: Some(format),
            entries,
            total,
            truncated,
            warning: truncated.then(|| {
                format!("showing the first {total} entries; raise files.max_entries to see more")
            }),
        })
    }

    /// Extracts one archive into a directory.
    pub fn extract_archive(
        &self,
        path: &str,
        into: Option<&str>,
    ) -> Result<(String, Vec<String>), WireError> {
        self.require_write()?;
        let path = display_path(path);
        let host = self.host_existing(&path).map_err(archive_wire)?;
        let magic = read_magic(&host);
        let format = archive::detect(Path::new(&path), magic.as_deref()).ok_or_else(|| {
            WireError::new(
                ErrorKind::Invalid,
                "archive_format",
                format!("{path} is not an archive this daemon can read"),
            )
        })?;

        let destination = match into {
            Some(given) => display_path(given),
            None => {
                let stem = path.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(&path);
                let stem = stem.strip_suffix(".tar").unwrap_or(stem);
                if stem.is_empty() {
                    path.clone()
                } else {
                    stem.to_string()
                }
            }
        };
        if destination == "/" {
            return Err(WireError::invalid(
                "refusing to extract an archive over the file root",
            ));
        }

        let host_destination = self.host_create_any(&destination).map_err(archive_wire)?;
        fs::create_dir_all(&host_destination)
            .map_err(|error| file_io_error(&destination, error))
            .map_err(archive_wire)?;

        let limits = archive::ArchiveLimits {
            max_total_bytes: self.max_archive_bytes,
            max_entries: self.max_entries,
            max_entry_bytes: self.max_archive_bytes,
        };
        let expected_root = Path::new(&destination)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let written = archive::extract_with(
            &host,
            format,
            &host_destination,
            limits,
            expected_root.as_deref(),
        )
        .map_err(|error| archive_wire(replace_path(error, &destination)))?;

        Ok((destination, written))
    }

    /// Resolves an existing path and follows symlinks, rejecting escapes.
    fn host_existing(&self, path: &str) -> Result<PathBuf, FileError> {
        let relative = path.trim_start_matches('/');
        let candidate = if relative.is_empty() {
            self.root.clone()
        } else {
            self.root.join(relative)
        };
        let canonical = fs::canonicalize(&candidate).map_err(|error| file_io_error(path, error))?;
        if !canonical.starts_with(&self.root) {
            return Err(FileError::new(
                FileErrorKind::InvalidPath,
                format!("{path} escapes the configured file root"),
            ));
        }
        Ok(canonical)
    }

    /// Resolves the parent without following the entry itself.
    fn host_entry(&self, path: &str) -> Result<PathBuf, FileError> {
        let (parent, name) = split_virtual(path)?;
        Ok(self.host_existing(&parent.to_string_lossy())?.join(name))
    }

    /// Resolves a destination whose final component may not exist yet.
    fn host_create(&self, path: &str) -> Result<PathBuf, FileError> {
        let (parent, name) = split_virtual(path)?;
        Ok(self.host_existing(&parent.to_string_lossy())?.join(name))
    }

    /// Resolves a path whose intermediate components may not exist.
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
                            format!("{path} escapes the configured file root"),
                        ));
                    }
                    current = canonical;
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    current = candidate;
                }
                Err(error) => return Err(file_io_error(path, error)),
            }
        }
        Ok(current)
    }

    /// Writes one UTF-8 text file under the root.
    ///
    /// A create never overwrites; an edit requires an existing regular file.
    /// Symlinks that resolve outside the root are refused, and the final
    /// component is resolved before writing so a link cannot redirect it.
    pub fn write(&self, path: &str, content: &str, create: bool) -> Result<FileWritten, WireError> {
        self.write_bytes(path, content.as_bytes(), create)
    }

    /// Writes arbitrary bytes. Used by uploads; the guards are the same.
    pub fn write_bytes(
        &self,
        path: &str,
        bytes: &[u8],
        create: bool,
    ) -> Result<FileWritten, WireError> {
        self.require_write()?;
        let display = display_path(path);
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if size > self.max_write_bytes {
            return Err(WireError::new(
                ErrorKind::TooLarge,
                "too_large",
                format!(
                    "the content is larger than the {} byte limit",
                    self.max_write_bytes
                ),
            ));
        }

        let (parent, name) = split_parent(path)?;
        let canonical_parent =
            fs::canonicalize(self.root.join(parent)).map_err(|error| io_error(&display, error))?;
        if !canonical_parent.starts_with(&self.root) {
            return Err(escape("the path escapes the file root"));
        }
        let target = canonical_parent.join(name);

        if create {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(|error| match error.kind() {
                    std::io::ErrorKind::AlreadyExists => WireError::new(
                        ErrorKind::Conflict,
                        "exists",
                        format!("{display} already exists"),
                    ),
                    _ => io_error(&display, error),
                })?;
            file.write_all(bytes)
                .map_err(|error| io_error(&display, error))?;
            file.sync_all().map_err(|error| io_error(&display, error))?;
            return Ok(FileWritten {
                path: display,
                size,
            });
        }

        let metadata = fs::symlink_metadata(&target).map_err(|error| io_error(&display, error))?;
        if metadata.is_dir() {
            return Err(WireError::invalid(format!(
                "{display} is a directory; only files can be written"
            )));
        }
        let resolved = if metadata.file_type().is_symlink() {
            let resolved = fs::canonicalize(&target).map_err(|error| io_error(&display, error))?;
            if !resolved.starts_with(&self.root) {
                return Err(escape("the path escapes the file root"));
            }
            if !resolved.is_file() {
                return Err(WireError::invalid(format!(
                    "{display} is not a regular file"
                )));
            }
            resolved
        } else {
            if !metadata.is_file() {
                return Err(WireError::invalid(format!(
                    "{display} is not a regular file"
                )));
            }
            target
        };
        fs::write(&resolved, bytes).map_err(|error| io_error(&display, error))?;
        Ok(FileWritten {
            path: display,
            size,
        })
    }

    fn require_write(&self) -> Result<(), WireError> {
        if self.write {
            return Ok(());
        }
        Err(WireError::new(
            ErrorKind::Forbidden,
            "write_disabled",
            "file changes are disabled; set files.write to true",
        ))
    }

    /// Reads an image under the root for the HTTP preview.
    ///
    /// Only raster formats plus SVG are accepted. SVG is served with a
    /// sandboxing CSP so opening the URL directly cannot run scripts.
    pub fn image(&self, path: &str) -> Result<(Vec<u8>, &'static str), WireError> {
        let limit = self.max_write_bytes.min(32 * 1024 * 1024);
        let (resolved, display) = self.regular_file(path, limit, "image")?;
        let extension = resolved
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .unwrap_or_default();
        let content_type = match extension.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            "avif" => "image/avif",
            "ico" => "image/x-icon",
            "svg" => "image/svg+xml",
            _ => {
                return Err(WireError::new(
                    ErrorKind::Unsupported,
                    "not_image",
                    format!("{display} is not a supported image"),
                ));
            }
        };
        let bytes = fs::read(&resolved).map_err(|error| io_error(&display, error))?;
        Ok((bytes, content_type))
    }

    /// Reads any regular file as raw bytes for the download route.
    pub fn download(&self, path: &str) -> Result<Vec<u8>, WireError> {
        let (resolved, display) = self.regular_file(path, self.max_write_bytes, "download")?;
        fs::read(&resolved).map_err(|error| io_error(&display, error))
    }

    /// Resolves `path` to a regular file and enforces `limit`.
    fn regular_file(
        &self,
        path: &str,
        limit: u64,
        kind: &str,
    ) -> Result<(PathBuf, String), WireError> {
        let display = display_path(path);
        let resolved = self.resolve(path)?;
        let metadata = fs::metadata(&resolved).map_err(|error| io_error(&display, error))?;
        if !metadata.is_file() {
            return Err(WireError::invalid(format!(
                "{display} is not a regular file"
            )));
        }
        if metadata.len() > limit {
            return Err(WireError::new(
                ErrorKind::TooLarge,
                "too_large",
                format!("{display} is larger than the {limit} byte {kind} limit"),
            ));
        }
        Ok((resolved, display))
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
        return Err(WireError::invalid("the file root cannot be changed"));
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

/// Splits a request path for the trash and archive helpers.
fn split_virtual(requested: &str) -> Result<(PathBuf, OsString), FileError> {
    let relative = Path::new(requested.trim_start_matches('/'));
    let mut parent = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parent.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(FileError::new(
                    FileErrorKind::InvalidPath,
                    "the path may not contain `..`",
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(FileError::new(
                    FileErrorKind::InvalidPath,
                    "the path must be relative to the file root",
                ));
            }
        }
    }
    let Some(name) = parent.file_name().map(OsString::from) else {
        return Err(FileError::new(
            FileErrorKind::InvalidPath,
            "the file root cannot be changed",
        ));
    };
    parent.pop();
    Ok((parent, name))
}

fn file_io_error(path: &str, error: io::Error) -> FileError {
    let kind = match error.kind() {
        io::ErrorKind::NotFound => FileErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => FileErrorKind::PermissionDenied,
        io::ErrorKind::AlreadyExists => FileErrorKind::AlreadyExists,
        _ => match error.raw_os_error() {
            Some(libc::ENOTDIR) => FileErrorKind::NotADirectory,
            Some(libc::EISDIR) => FileErrorKind::IsADirectory,
            Some(libc::ENOTEMPTY) => FileErrorKind::DirectoryNotEmpty,
            _ => FileErrorKind::Io,
        },
    };
    FileError::new(kind, format!("{path}: {error}"))
}

fn trash_wire(error: FileError) -> WireError {
    if error.kind == FileErrorKind::Disabled {
        WireError::new(ErrorKind::Forbidden, "trash_disabled", error.detail)
    } else {
        error.into()
    }
}

fn archive_wire(error: FileError) -> WireError {
    if error.kind == FileErrorKind::Disabled {
        WireError::new(ErrorKind::Unsupported, "archive_format", error.detail)
    } else {
        error.into()
    }
}

fn read_magic(path: &Path) -> Option<Vec<u8>> {
    let mut buffer = [0u8; 512];
    let mut file = File::open(path).ok()?;
    let read = file.read(&mut buffer).ok()?;
    Some(buffer[..read].to_vec())
}

/// Rewrites the host path inside an archive error to the virtual path.
fn replace_path(mut error: FileError, virtual_path: &str) -> FileError {
    if let Some(index) = error.detail.find("`: ") {
        error.detail = format!("`{virtual_path}`: {}", &error.detail[index + 3..]);
    }
    error
}

fn escape(message: &str) -> WireError {
    WireError::new(ErrorKind::Forbidden, "path_escape", message)
}

/// Directories sort before every other kind; names break ties.
fn entry_key(entry: &FileEntry) -> (u8, &str) {
    (u8::from(entry.kind != "dir"), entry.name.as_str())
}

/// Cursors carry the group and the name so a page boundary stays correct even
/// if the last entry disappears between pages. A cursor without a group is a
/// legacy name and sorts before the file group.
fn cursor_key(cursor: &str) -> (u8, &str) {
    if let Some(name) = cursor.strip_prefix("d:") {
        (0, name)
    } else if let Some(name) = cursor.strip_prefix("f:") {
        (1, name)
    } else {
        (0, cursor)
    }
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
        FileService::new(root, false, 100, 64, 1024).expect("service")
    }

    fn writable(root: &Path) -> FileService {
        FileService::new(root, true, 100, 64, 1024).expect("service")
    }

    #[test]
    fn a_directory_lists_directories_first_and_pages() {
        let root = scratch("list");
        fs::write(root.join("b.txt"), b"b").expect("write");
        fs::write(root.join("a.txt"), b"a").expect("write");
        fs::write(root.join("0.txt"), b"0").expect("write");
        fs::create_dir(root.join("dir")).expect("dir");
        fs::create_dir(root.join("zdir")).expect("dir");
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
            vec!["dir", "zdir"]
        );
        assert_eq!(first.next.as_deref(), Some("d:zdir"));

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
            vec!["0.txt", "a.txt"]
        );
        assert_eq!(second.next.as_deref(), Some("f:a.txt"));

        let third = files
            .list(
                "/",
                &PageRequest {
                    limit: Some(2),
                    cursor: second.next.clone(),
                },
            )
            .expect("third");
        assert_eq!(
            third
                .items
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["b.txt"]
        );
        assert_eq!(third.next, None);

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

    #[test]
    fn creates_and_replaces_files_but_never_overwrites_on_create() {
        let root = scratch("write");
        let files = writable(&root);

        let written = files.write("note.txt", "hello", true).expect("create");
        assert_eq!(written.path, "note.txt");
        assert_eq!(written.size, 5);
        assert_eq!(
            fs::read_to_string(root.join("note.txt")).expect("read"),
            "hello"
        );

        files.write("note.txt", "bye", false).expect("replace");
        assert_eq!(
            fs::read_to_string(root.join("note.txt")).expect("read"),
            "bye"
        );
        assert_eq!(
            files
                .write("note.txt", "again", true)
                .expect_err("exists")
                .code,
            "exists"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn writing_requires_write_and_refuses_a_directory() {
        let root = scratch("write-guard");
        fs::create_dir(root.join("dir")).expect("dir");

        let read_only = service(&root);
        assert_eq!(
            read_only
                .write("note.txt", "x", true)
                .expect_err("off")
                .code,
            "write_disabled"
        );

        let files = writable(&root);
        assert_eq!(
            files.write("dir", "x", false).expect_err("dir").kind,
            ErrorKind::Invalid
        );
        assert_eq!(
            files
                .write("../escape.txt", "x", true)
                .expect_err("escape")
                .code,
            "path_escape"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn writing_respects_the_size_cap_and_symlink_containment() {
        let root = scratch("write-cap");
        let outside = scratch("write-cap-outside");
        fs::write(outside.join("secret.txt"), b"secret").expect("write");
        symlink(outside.join("secret.txt"), root.join("link")).expect("symlink");
        let files = writable(&root);

        assert_eq!(
            files
                .write("big.txt", &"x".repeat(2048), true)
                .expect_err("cap")
                .kind,
            ErrorKind::TooLarge
        );
        assert_eq!(
            files
                .write("link", "overwrite", false)
                .expect_err("symlink")
                .code,
            "path_escape"
        );
        assert_eq!(
            fs::read_to_string(outside.join("secret.txt")).expect("read"),
            "secret"
        );

        fs::remove_dir_all(root).expect("cleanup");
        fs::remove_dir_all(outside).expect("cleanup");
    }

    #[test]
    fn reading_an_image_returns_its_content_type() {
        let root = scratch("image");
        fs::write(root.join("logo.png"), [0x89, b'P', b'N', b'G']).expect("png");
        fs::write(root.join("note.txt"), b"hello").expect("text");
        let files = service(&root);

        let (bytes, content_type) = files.image("logo.png").expect("image");
        assert_eq!(content_type, "image/png");
        assert_eq!(bytes, [0x89, b'P', b'N', b'G']);
        assert_eq!(
            files.image("note.txt").expect_err("not image").code,
            "not_image"
        );
        assert_eq!(
            files.image("../logo.png").expect_err("escape").code,
            "path_escape"
        );

        fs::remove_dir_all(root).expect("cleanup");
    }
}
