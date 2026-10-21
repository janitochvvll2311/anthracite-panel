//! Archive creation, extraction and listing.
//!
//! Everything runs in-process. The runtime image ships no `7z` or `xz`, and its
//! `tar` is BusyBox, so an extraction that delegated to a binary would rest its
//! security boundary on behaviour this project cannot pin in a test. Here the
//! guard is a pure function over entry names, and every format funnels through
//! it.
//!
//! Formats are detected from the file name and, when that is ambiguous, from
//! the magic bytes. Supported for reading: `zip`, `tar`, `tar.gz`, `tar.xz`,
//! `tar.bz2` is deliberately absent, and `7z`. Supported for writing: `zip`,
//! `tar`, `tar.gz` and `tar.xz`. `.7z` creation is not offered because the crate
//! exposes no control over the filter chain, so the output would not match what
//! users expect of a `.7z`.

use std::fs::{self, File};
use std::io::{self, BufReader, Read, Seek, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anth_proto::{ArchiveEntry, ArchiveFormat, FileError, FileErrorKind};

/// Bounds applied to one extraction.
///
/// A zip bomb is a small archive that expands without limit, so the limits are
/// enforced while writing rather than after: the extractor counts bytes as they
/// arrive and stops mid-entry.
#[derive(Debug, Clone, Copy)]
pub struct ArchiveLimits {
    /// Maximum bytes written by one extraction.
    pub max_total_bytes: u64,
    /// Maximum number of entries one archive may contain.
    pub max_entries: usize,
    /// Maximum bytes one entry may expand to.
    pub max_entry_bytes: u64,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: 4 * 1024 * 1024 * 1024,
            max_entries: 100_000,
            max_entry_bytes: 1024 * 1024 * 1024,
        }
    }
}

/// Detects the format from a path, falling back to the file's magic bytes.
pub fn detect(path: &Path, magic: Option<&[u8]>) -> Option<ArchiveFormat> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    if name.ends_with(".zip") {
        return Some(ArchiveFormat::Zip);
    }
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        return Some(ArchiveFormat::TarGz);
    }
    if name.ends_with(".tar.xz") || name.ends_with(".txz") {
        return Some(ArchiveFormat::TarXz);
    }
    if name.ends_with(".7z") {
        return Some(ArchiveFormat::SevenZ);
    }
    if name.ends_with(".tar") {
        return Some(ArchiveFormat::Tar);
    }

    // The extension is authoritative when it is one this module knows, so the
    // magic sniff only helps a file that was renamed or has no extension.
    let magic = magic?;
    if magic.starts_with(b"PK\x03\x04") || magic.starts_with(b"PK\x05\x06") {
        return Some(ArchiveFormat::Zip);
    }
    if magic.starts_with(b"7z\xbc\xaf\x27\x1c") {
        return Some(ArchiveFormat::SevenZ);
    }
    if magic.starts_with(&[0xfd, b'7', b'z', b'X', b'Z', 0x00]) {
        return Some(ArchiveFormat::TarXz);
    }
    if magic.starts_with(&[0x1f, 0x8b]) {
        return Some(ArchiveFormat::TarGz);
    }
    if magic.len() >= 262 && &magic[257..262] == b"ustar" {
        return Some(ArchiveFormat::Tar);
    }
    None
}

/// Resolves one archive member name to a safe relative path.
///
/// Returns `None` when the name must be skipped: an absolute path, a `..`
/// component, a Windows drive letter, a NUL byte or an empty name. Absolute
/// paths are refused rather than stripped, because stripping silently rewrites
/// what the archive said and hides the attempt.
pub fn safe_member(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('\0') {
        return None;
    }
    // A backslash is a separator on Windows and a legal filename character on
    // Unix; treating it as a separator here would let `..\..\x` through on a
    // host that later reads the file. Refuse it outright.
    if name.contains('\\') {
        return None;
    }
    let trimmed = name.trim_start_matches("./");
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return None;
    }
    if trimmed.len() >= 2 && trimmed.as_bytes()[1] == b':' {
        return None;
    }

    let mut parts: Vec<&str> = Vec::new();
    for part in trimmed.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.iter().collect())
}

/// Reported when an archive decompresses past `files.max_archive_bytes`.
///
/// Carried inside an [`io::Error`] so the extractor can tell "the archive is a
/// bomb" apart from an ordinary read failure and answer `TooLarge`.
#[derive(Debug)]
struct ExpandsPastLimit {
    limit: u64,
}

impl std::fmt::Display for ExpandsPastLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the archive expands past files.max_archive_bytes ({})",
            self.limit
        )
    }
}

impl std::error::Error for ExpandsPastLimit {}

impl From<ExpandsPastLimit> for io::Error {
    fn from(error: ExpandsPastLimit) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, error)
    }
}

/// Decompresses `.xz` on demand and stops at the configured ceiling.
///
/// `lzma_rs::xz_decompress` needs a `Write` sink, and a `Vec` sink grows to the
/// *whole* uncompressed size before a single entry is read, so listing a 58 KB
/// archive could cost hundreds of megabytes. The work therefore runs on a
/// worker feeding a bounded channel: peak memory follows the channel depth, not
/// the archive, and the reader stops as soon as the byte count passes the limit.
struct BoundedXz {
    chunks: std::sync::mpsc::Receiver<Vec<u8>>,
    worker: Option<std::thread::JoinHandle<Result<(), lzma_rs::error::Error>>>,
    chunk: Vec<u8>,
    position: usize,
    produced: u64,
    limit: u64,
    done: bool,
}

impl BoundedXz {
    /// The channel holds a few small chunks, so a fast decompressor blocks
    /// instead of buffering the archive ahead of the tar reader.
    const DEPTH: usize = 4;
    const CHUNK: usize = 64 * 1024;

    fn new<R: Read + Send + 'static>(input: R, limit: u64) -> Self {
        let (sender, chunks) = std::sync::mpsc::sync_channel(Self::DEPTH);
        let worker = std::thread::spawn(move || {
            let mut sink = ChannelWriter {
                sender,
                pending: Vec::with_capacity(Self::CHUNK),
            };
            lzma_rs::xz_decompress(&mut BufReader::new(input), &mut sink)
        });
        Self {
            chunks,
            worker: Some(worker),
            chunk: Vec::new(),
            position: 0,
            produced: 0,
            limit,
            done: false,
        }
    }

    /// Joins the worker once the channel closes, surfacing a decompression
    /// failure that would otherwise look like a clean end of stream.
    fn finish(&mut self) -> io::Result<()> {
        self.done = true;
        match self.worker.take() {
            Some(worker) => match worker.join() {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("xz decompression failed: {error}"),
                )),
                Err(_) => Err(io::Error::other("the xz decompression worker panicked")),
            },
            None => Ok(()),
        }
    }
}

impl Read for BoundedXz {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.position >= self.chunk.len() {
            if self.done {
                return Ok(0);
            }
            match self.chunks.recv() {
                Ok(next) => {
                    self.produced += next.len() as u64;
                    if self.produced > self.limit {
                        return Err(ExpandsPastLimit { limit: self.limit }.into());
                    }
                    self.chunk = next;
                    self.position = 0;
                }
                Err(_) => {
                    self.finish()?;
                    return Ok(0);
                }
            }
        }

        let available = self.chunk.len() - self.position;
        let take = available.min(out.len());
        out[..take].copy_from_slice(&self.chunk[self.position..self.position + take]);
        self.position += take;
        Ok(take)
    }
}

impl Drop for BoundedXz {
    fn drop(&mut self) {
        // Dropping the receiver is what unblocks a worker waiting to send, so
        // an abandoned archive cannot leak a thread.
        let worker = self.worker.take();
        drop(std::mem::replace(
            &mut self.chunks,
            std::sync::mpsc::sync_channel(1).1,
        ));
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

/// Buffers a `Write` stream into fixed chunks for [`BoundedXz`].
struct ChannelWriter {
    sender: std::sync::mpsc::SyncSender<Vec<u8>>,
    pending: Vec<u8>,
}

impl Write for ChannelWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() >= BoundedXz::CHUNK {
            let chunk = std::mem::replace(&mut self.pending, Vec::with_capacity(BoundedXz::CHUNK));
            self.sender
                .send(chunk)
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the reader went away"))?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let chunk = std::mem::take(&mut self.pending);
            self.sender
                .send(chunk)
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the reader went away"))?;
        }
        Ok(())
    }
}

impl Drop for ChannelWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

/// How much of an archive to accept, tracked while extracting.
struct Budget {
    limits: ArchiveLimits,
    total: u64,
    entries: usize,
}

impl Budget {
    fn new(limits: ArchiveLimits) -> Self {
        Self {
            limits,
            total: 0,
            entries: 0,
        }
    }

    fn admit_entry(&mut self) -> Result<(), FileError> {
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(FileError::new(
                FileErrorKind::TooLarge,
                format!(
                    "the archive holds more than {} entries",
                    self.limits.max_entries
                ),
            ));
        }
        Ok(())
    }

    /// Copies one member into `writer`, stopping at the per-entry and total
    /// ceilings. The check happens per chunk, so an entry that lies about its
    /// declared size cannot fill the disk before being caught.
    fn copy(
        &mut self,
        mut reader: impl Read,
        writer: &mut impl Write,
        member: &str,
    ) -> Result<u64, FileError> {
        let mut buffer = [0u8; 64 * 1024];
        let mut written = 0u64;
        loop {
            let read = reader
                .read(&mut buffer)
                .map_err(|e| stream_error(member, e))?;
            if read == 0 {
                break;
            }
            written += read as u64;
            self.total += read as u64;
            if written > self.limits.max_entry_bytes {
                return Err(FileError::new(
                    FileErrorKind::TooLarge,
                    format!(
                        "`{member}` expands past the {} byte per-entry limit",
                        self.limits.max_entry_bytes
                    ),
                ));
            }
            if self.total > self.limits.max_total_bytes {
                return Err(FileError::new(
                    FileErrorKind::TooLarge,
                    format!(
                        "the archive expands past the {} byte limit",
                        self.limits.max_total_bytes
                    ),
                ));
            }
            writer
                .write_all(&buffer[..read])
                .map_err(|e| extract_error(member, e))?;
        }
        Ok(written)
    }
}

fn extract_error(member: &str, error: impl std::fmt::Display) -> FileError {
    FileError::new(FileErrorKind::Io, format!("`{member}`: {error}"))
}

/// Turns a read failure from the decompression stream into the right kind.
///
/// A stream that expands past the ceiling is not an I/O fault, so it must not
/// reach the user as one: `TooLarge` is what the HTTP layer maps to `413` and
/// what the editor can explain.
/// Maps a tar header failure, which is where the bomb surfaces when the
/// ceiling is hit while the next header is read.
fn arch_error(error: io::Error) -> FileError {
    stream_error("archive", error)
}

fn stream_error(member: &str, error: io::Error) -> FileError {
    if let Some(limit) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ExpandsPastLimit>())
    {
        return FileError::new(FileErrorKind::TooLarge, limit.to_string());
    }
    extract_error(member, error)
}

fn io_error(path: &Path, error: std::io::Error) -> FileError {
    let kind = match error.kind() {
        std::io::ErrorKind::NotFound => FileErrorKind::NotFound,
        std::io::ErrorKind::PermissionDenied => FileErrorKind::PermissionDenied,
        _ => FileErrorKind::Io,
    };
    FileError::new(kind, format!("`{}`: {error}", path.display()))
}

/// Creates `parent`, refusing to follow a symlink that would place the write
/// outside `destination`.
///
/// `safe_member` already removed `..`, but an archive may still contain a
/// symlink entry followed by a file beneath it. Creating every intermediate
/// directory and checking that it is a real directory closes that hole.
/// Resolves the directory that will hold `relative`, creating it as needed.
///
/// Every intermediate component is checked, but that is only half the guard: a
/// symlink can also sit at the *final* component, where `File::create` would
/// follow it and write outside the destination. [`write_target`] is the other
/// half, and both are needed because `prepare_parent` cannot see the file name.
fn prepare_parent(destination: &Path, relative: &Path) -> Result<PathBuf, FileError> {
    let mut current = destination.to_path_buf();
    let parent = relative.parent().unwrap_or(Path::new(""));
    for component in parent.components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        current = current.join(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(FileError::new(
                        FileErrorKind::InvalidPath,
                        format!(
                            "`{}` is not a directory; the archive would write through it",
                            current.display()
                        ),
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|e| io_error(&current, e))?;
            }
            Err(error) => return Err(io_error(&current, error)),
        }
    }
    Ok(current)
}

/// Returns the final path for one member, refusing to write through a link.
///
/// An archive cannot store a link *and* write through it, because links are
/// skipped during extraction. A link can still already exist in the
/// destination, though, and the attacker only needs a writable destination to
/// plant one. Checking the final component here means a pre-existing symlink is
/// an error instead of a redirect, which is the half of the escape
/// `prepare_parent` alone cannot cover.
fn write_target(destination: &Path, relative: &Path) -> Result<PathBuf, FileError> {
    let parent = prepare_parent(destination, relative)?;
    let name = relative
        .file_name()
        .map(|n| n.to_owned())
        .unwrap_or_default();
    let target = parent.join(name);

    match fs::symlink_metadata(&target) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(FileError::new(
            FileErrorKind::InvalidPath,
            format!(
                "`{}` is a symlink; the archive would write through it",
                target.display()
            ),
        )),
        Ok(metadata) if metadata.is_dir() => Err(FileError::new(
            FileErrorKind::InvalidPath,
            format!("`{}` is a directory", target.display()),
        )),
        Ok(_) => Ok(target),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(target),
        Err(error) => Err(io_error(&target, error)),
    }
}

/// Creates an archive from one virtual path.
pub fn create(
    source: &Path,
    archive: &Path,
    format: ArchiveFormat,
) -> Result<ArchiveEntry, FileError> {
    let file = File::create(archive).map_err(|e| io_error(archive, e))?;
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".to_string());

    let entry = match format {
        ArchiveFormat::Zip => {
            let mut writer = zip::ZipWriter::new(file);
            write_zip(&mut writer, source, &name)?;
            writer.finish().map_err(|e| zip_error(archive, e))?;
            ArchiveEntry {
                name: name.clone(),
                kind: anth_proto::FileKind::File,
                size: 0,
                compressed: None,
            }
        }
        ArchiveFormat::Tar => {
            let mut builder = tar::Builder::new(file);
            append_tar(&mut builder, source, &name)?;
            builder.finish().map_err(|e| io_error(archive, e))?;
            ArchiveEntry {
                name: name.clone(),
                kind: anth_proto::FileKind::File,
                size: 0,
                compressed: None,
            }
        }
        ArchiveFormat::TarGz => {
            let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
            let mut builder = tar::Builder::new(encoder);
            append_tar(&mut builder, source, &name)?;
            let encoder = builder.into_inner().map_err(|e| io_error(archive, e))?;
            encoder.finish().map_err(|e| io_error(archive, e))?;
            ArchiveEntry {
                name: name.clone(),
                kind: anth_proto::FileKind::File,
                size: 0,
                compressed: None,
            }
        }
        ArchiveFormat::TarXz => {
            let mut raw = Vec::new();
            {
                let mut builder = tar::Builder::new(&mut raw);
                append_tar(&mut builder, source, &name)?;
                builder.finish().map_err(|e| io_error(archive, e))?;
            }
            let mut compressed = Vec::new();
            lzma_rs::xz_compress(&mut raw.as_slice(), &mut compressed)
                .map_err(|e| extract_error(&archive.display().to_string(), e))?;
            let mut file = file;
            file.write_all(&compressed)
                .map_err(|e| io_error(archive, e))?;
            ArchiveEntry {
                name: name.clone(),
                kind: anth_proto::FileKind::File,
                size: 0,
                compressed: None,
            }
        }
        ArchiveFormat::SevenZ => {
            return Err(FileError::new(
                FileErrorKind::Disabled,
                "creating a .7z archive is not supported; extract it instead".to_string(),
            ));
        }
    };

    Ok(ArchiveEntry {
        size: file_metadata_size(archive).unwrap_or(0),
        ..entry
    })
}

fn file_metadata_size(path: &Path) -> Option<u64> {
    fs::metadata(path).ok().map(|metadata| metadata.len())
}

fn zip_error(path: &Path, error: zip::result::ZipError) -> FileError {
    match error {
        zip::result::ZipError::Io(io) => io_error(path, io),
        other => FileError::new(FileErrorKind::Io, format!("`{}`: {other}", path.display())),
    }
}

/// Adds `path` and its children to a tar builder under `prefix`.
fn append_tar<W: Write>(
    builder: &mut tar::Builder<W>,
    path: &Path,
    prefix: &str,
) -> Result<(), FileError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| io_error(path, e))?;
    if metadata.file_type().is_symlink() {
        // Storing a symlink verbatim is safe: extraction refuses to follow one,
        // and dropping it would silently change the archive's contents.
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        header.set_cksum();
        let target = fs::read_link(path).map_err(|e| io_error(path, e))?;
        builder
            .append_link(&mut header, prefix, target)
            .map_err(|e| io_error(path, e))?;
        return Ok(());
    }

    if metadata.is_dir() {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(metadata.permissions().mode());
        header.set_mtime(modified_secs(&metadata));
        header.set_cksum();
        builder
            .append_data(&mut header, format!("{prefix}/"), &[][..])
            .map_err(|e| io_error(path, e))?;

        let mut children: Vec<PathBuf> = fs::read_dir(path)
            .map_err(|e| io_error(path, e))?
            .flatten()
            .map(|item| item.path())
            .collect();
        children.sort();
        for child in children {
            let Some(name) = child.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            append_tar(builder, &child, &format!("{prefix}/{name}"))?;
        }
        return Ok(());
    }

    let mut file = File::open(path).map_err(|e| io_error(path, e))?;
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(metadata.len());
    header.set_mode(metadata.permissions().mode());
    header.set_mtime(modified_secs(&metadata));
    header.set_cksum();
    builder
        .append_data(&mut header, prefix, &mut file)
        .map_err(|e| io_error(path, e))?;
    Ok(())
}

fn modified_secs(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn write_zip<W: Write + Seek>(
    writer: &mut zip::ZipWriter<W>,
    path: &Path,
    prefix: &str,
) -> Result<(), FileError> {
    let metadata = fs::symlink_metadata(path).map_err(|e| io_error(path, e))?;
    if metadata.file_type().is_symlink() {
        // Zip stores a symlink as a regular file whose body is the target and
        // whose mode carries `S_IFLNK`. Writing it as a plain file keeps the
        // archive honest without teaching the extractor a second traversal
        // rule.
        let target = fs::read_link(path).map_err(|e| io_error(path, e))?;
        writer
            .start_file(prefix, zip::write::SimpleFileOptions::default())
            .map_err(|e| zip_error(path, e))?;
        writer
            .write_all(target.to_string_lossy().as_bytes())
            .map_err(|e| io_error(path, e))?;
        return Ok(());
    }

    if metadata.is_dir() {
        writer
            .add_directory(
                format!("{prefix}/"),
                zip::write::SimpleFileOptions::default(),
            )
            .map_err(|e| zip_error(path, e))?;
        let mut children: Vec<PathBuf> = fs::read_dir(path)
            .map_err(|e| io_error(path, e))?
            .flatten()
            .map(|item| item.path())
            .collect();
        children.sort();
        for child in children {
            let Some(name) = child.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            write_zip(writer, &child, &format!("{prefix}/{name}"))?;
        }
        return Ok(());
    }

    writer
        .start_file(prefix, zip::write::SimpleFileOptions::default())
        .map_err(|e| zip_error(path, e))?;
    let mut file = File::open(path).map_err(|e| io_error(path, e))?;
    std::io::copy(&mut file, writer).map_err(|e| io_error(path, e))?;
    Ok(())
}

/// Extracts `archive` into `destination`.
///
/// `destination` must already exist and must not be the archive itself. Every
/// member is passed through [`safe_member`]; a refused member is an error, not
/// a skip, so the caller learns the archive tried something rather than getting
/// a silently partial tree.
pub fn extract(
    archive: &Path,
    format: ArchiveFormat,
    destination: &Path,
    limits: ArchiveLimits,
) -> Result<Vec<String>, FileError> {
    extract_with(archive, format, destination, limits, None)
}

/// Extracts, dropping the wrapper directory when the caller already named it.
///
/// `expect_root` exists so `docs.zip`, whose members are `docs/a.txt`, does not
/// land in `docs/docs/a.txt` when the destination is already `docs`. Stripping
/// only happens when the archive's single common root *equals* that name, so
/// an archive named after its own wrapper is unwrapped while an unrelated
/// destination keeps the tree exactly as stored. Names are validated by
/// [`safe_member`] before a component is dropped, so stripping can only remove
/// a component that was already accepted.
pub fn extract_with(
    archive: &Path,
    format: ArchiveFormat,
    destination: &Path,
    limits: ArchiveLimits,
    expect_root: Option<&str>,
) -> Result<Vec<String>, FileError> {
    let mut budget = Budget::new(limits);
    let strip = match expect_root {
        Some(expected) => match common_root(archive, format)? {
            Some(root) if root == expected => Some(root),
            _ => None,
        },
        None => None,
    }
    .filter(|_| expect_root.is_some());
    let mut written = Vec::new();

    match format {
        ArchiveFormat::Zip => extract_zip(
            archive,
            destination,
            &mut budget,
            &mut written,
            strip.as_deref(),
        )?,
        ArchiveFormat::Tar => {
            let file = File::open(archive).map_err(|e| io_error(archive, e))?;
            extract_tar(
                BufReader::new(file),
                destination,
                &mut budget,
                &mut written,
                strip.as_deref(),
            )?;
        }
        ArchiveFormat::TarGz => {
            let file = File::open(archive).map_err(|e| io_error(archive, e))?;
            let decoder = flate2::read::GzDecoder::new(BufReader::new(file));
            extract_tar(
                decoder,
                destination,
                &mut budget,
                &mut written,
                strip.as_deref(),
            )?;
        }
        ArchiveFormat::TarXz => {
            extract_tar(
                xz_reader(archive, budget.limits.max_total_bytes)?,
                destination,
                &mut budget,
                &mut written,
                strip.as_deref(),
            )?;
        }
        ArchiveFormat::SevenZ => extract_7z(
            archive,
            destination,
            &mut budget,
            &mut written,
            strip.as_deref(),
        )?,
    }

    Ok(written)
}

fn extract_tar<R: Read>(
    reader: R,
    destination: &Path,
    budget: &mut Budget,
    written: &mut Vec<String>,
    strip: Option<&str>,
) -> Result<(), FileError> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries().map_err(arch_error)?;

    for entry in entries {
        let mut entry = entry.map_err(arch_error)?;
        let name = entry
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(relative) = resolve(&name, strip)? else {
            continue;
        };
        budget.admit_entry()?;

        let entry_type = entry.header().entry_type();

        if entry_type.is_dir() {
            let target = prepare_parent(destination, &relative)?.join(
                relative
                    .file_name()
                    .map(|n| n.to_owned())
                    .unwrap_or_default(),
            );
            fs::create_dir_all(&target).map_err(|e| io_error(&target, e))?;
            written.push(relative.to_string_lossy().into_owned());
            continue;
        }
        if entry_type.is_symlink() || entry_type.is_hard_link() {
            // Links are materialised as files only when the archive also
            // contains their target; storing them would let a later entry write
            // through them, which is the classic tar escape.
            written.push(format!("{} (link skipped)", relative.display()));
            continue;
        }
        if !entry_type.is_file() {
            continue;
        }

        let target = write_target(destination, &relative)?;
        let mut file = File::create(&target).map_err(|e| io_error(&target, e))?;
        budget.copy(&mut entry, &mut file, &name)?;
        written.push(relative.to_string_lossy().into_owned());
    }
    Ok(())
}

fn extract_zip(
    archive: &Path,
    destination: &Path,
    budget: &mut Budget,
    written: &mut Vec<String>,
    strip: Option<&str>,
) -> Result<(), FileError> {
    let file = File::open(archive).map_err(|e| io_error(archive, e))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| zip_error(archive, e))?;

    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| zip_error(archive, e))?;
        let name = entry.name().to_string();
        let Some(relative) = resolve(&name, strip)? else {
            continue;
        };
        budget.admit_entry()?;

        if entry.is_dir() {
            let target = prepare_parent(destination, &relative)?.join(
                relative
                    .file_name()
                    .map(|n| n.to_owned())
                    .unwrap_or_default(),
            );
            fs::create_dir_all(&target).map_err(|e| io_error(&target, e))?;
            written.push(relative.to_string_lossy().into_owned());
            continue;
        }

        let target = write_target(destination, &relative)?;
        let mut file = File::create(&target).map_err(|e| io_error(&target, e))?;
        budget.copy(&mut entry, &mut file, &name)?;
        written.push(relative.to_string_lossy().into_owned());
    }
    Ok(())
}

fn extract_7z(
    archive: &Path,
    destination: &Path,
    budget: &mut Budget,
    written: &mut Vec<String>,
    strip: Option<&str>,
) -> Result<(), FileError> {
    let file = File::open(archive).map_err(|e| io_error(archive, e))?;
    let mut reader =
        sevenz_rust2::SevenZReader::new(BufReader::new(file), sevenz_rust2::Password::empty())
            .map_err(|e| extract_error(&archive.display().to_string(), e))?;

    let mut failure: Option<FileError> = None;
    reader
        .for_each_entries(|entry, body| {
            // A refused member stops the walk; the error is carried out through
            // `failure` because the callback can only answer a bool.
            if failure.is_some() {
                return Ok(false);
            }
            let name = entry.name().to_string();
            let relative = match resolve(&name, strip) {
                Ok(Some(relative)) => relative,
                Ok(None) => return Ok(true),
                Err(error) => {
                    failure = Some(error);
                    return Ok(false);
                }
            };
            if let Err(error) = budget.admit_entry() {
                failure = Some(error);
                return Ok(false);
            }

            if entry.is_directory() {
                let target = match prepare_parent(destination, &relative) {
                    Ok(parent) => parent.join(
                        relative
                            .file_name()
                            .map(|n| n.to_owned())
                            .unwrap_or_default(),
                    ),
                    Err(error) => {
                        failure = Some(error);
                        return Ok(false);
                    }
                };
                if let Err(error) = fs::create_dir_all(&target) {
                    failure = Some(io_error(&target, error));
                    return Ok(false);
                }
                written.push(relative.to_string_lossy().into_owned());
                return Ok(true);
            }

            let target = match write_target(destination, &relative) {
                Ok(target) => target,
                Err(error) => {
                    failure = Some(error);
                    return Ok(false);
                }
            };
            let mut file = match File::create(&target) {
                Ok(file) => file,
                Err(error) => {
                    failure = Some(io_error(&target, error));
                    return Ok(false);
                }
            };
            match budget.copy(body, &mut file, &name) {
                Ok(_) => {
                    written.push(relative.to_string_lossy().into_owned());
                    Ok(true)
                }
                Err(error) => {
                    failure = Some(error);
                    Ok(false)
                }
            }
        })
        .map_err(|e| extract_error(&archive.display().to_string(), e))?;

    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Walks a tar stream for the listing. Separate from `extract_tar` because it
/// reads headers only and must not touch the filesystem.
/// Ceiling for a listing, which only reads headers.
///
/// `list` takes no limits argument because the headers are cheap; the stream
/// still has to be bounded, and a listing needs far less than an extraction.
const MAX_LIST_BYTES: u64 = 1024 * 1024 * 1024;

/// Opens an `.xz` archive as a bounded stream.
fn xz_reader(archive: &Path, limit: u64) -> Result<BoundedXz, FileError> {
    let file = File::open(archive).map_err(|e| io_error(archive, e))?;
    Ok(BoundedXz::new(file, limit))
}

fn list_tar<R: Read>(
    reader: R,
    max_entries: usize,
    entries: &mut Vec<ArchiveEntry>,
) -> Result<(), FileError> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(arch_error)? {
        if entries.len() >= max_entries {
            break;
        }
        let entry = entry.map_err(arch_error)?;
        let name = entry
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let kind = if entry.header().entry_type().is_dir() {
            anth_proto::FileKind::Directory
        } else {
            anth_proto::FileKind::File
        };
        entries.push(ArchiveEntry {
            name,
            kind,
            size: entry.size(),
            compressed: None,
        });
    }
    Ok(())
}

/// Validates a member name and applies the optional root strip.
///
/// `Ok(None)` means the member became empty once the root was dropped — the
/// directory entry matching the root itself — and should be skipped.
fn resolve(name: &str, strip: Option<&str>) -> Result<Option<PathBuf>, FileError> {
    let relative = safe_member(name).ok_or_else(|| unsafe_member(name))?;
    let Some(strip) = strip else {
        return Ok(Some(relative));
    };
    if relative == Path::new(strip) {
        return Ok(None);
    }
    match relative.strip_prefix(strip) {
        Ok(rest) if !rest.as_os_str().is_empty() => Ok(Some(rest.to_path_buf())),
        Ok(_) => Ok(None),
        // The archive mixes a rooted and a loose layout; keep the full path
        // rather than silently relocating half of it.
        Err(_) => Ok(Some(relative)),
    }
}

/// The single leading component every member shares, if there is one.
///
/// Returns `None` for an empty archive and for one whose members disagree, so
/// only an archive that is unambiguously wrapped in a directory is stripped.
fn common_root(archive: &Path, format: ArchiveFormat) -> Result<Option<String>, FileError> {
    let entries = list(archive, format, usize::MAX)?;
    let mut root: Option<String> = None;
    for entry in &entries {
        let trimmed = entry.name.trim_start_matches("./");
        let first = match trimmed.split('/').next() {
            Some(first) if !first.is_empty() => first.to_string(),
            _ => return Ok(None),
        };
        match &root {
            None => root = Some(first),
            Some(existing) if *existing == first => {}
            Some(_) => return Ok(None),
        }
    }
    // A single loose file is not a wrapper directory, so only strip when the
    // members really sit below a directory.
    let wrapped = entries
        .iter()
        .any(|entry| entry.name.trim_start_matches("./").contains('/'));
    Ok(root.filter(|_| wrapped))
}

fn unsafe_member(name: &str) -> FileError {
    FileError::new(
        FileErrorKind::InvalidPath,
        format!("the archive contains the unsafe member `{name}`"),
    )
}

/// Lists one archive without extracting it.
pub fn list(
    archive: &Path,
    format: ArchiveFormat,
    max_entries: usize,
) -> Result<Vec<ArchiveEntry>, FileError> {
    let mut entries = Vec::new();

    match format {
        ArchiveFormat::Zip => {
            let file = File::open(archive).map_err(|e| io_error(archive, e))?;
            let mut zip =
                zip::ZipArchive::new(BufReader::new(file)).map_err(|e| zip_error(archive, e))?;
            for index in 0..zip.len() {
                if entries.len() >= max_entries {
                    break;
                }
                let entry = zip.by_index(index).map_err(|e| zip_error(archive, e))?;
                entries.push(ArchiveEntry {
                    name: entry.name().to_string(),
                    kind: if entry.is_dir() {
                        anth_proto::FileKind::Directory
                    } else {
                        anth_proto::FileKind::File
                    },
                    size: entry.size(),
                    compressed: Some(entry.compressed_size()),
                });
            }
        }
        ArchiveFormat::SevenZ => {
            let file = File::open(archive).map_err(|e| io_error(archive, e))?;
            let reader = sevenz_rust2::SevenZReader::new(
                BufReader::new(file),
                sevenz_rust2::Password::empty(),
            )
            .map_err(|e| extract_error(&archive.display().to_string(), e))?;
            for entry in reader.archive().files.iter() {
                if entries.len() >= max_entries {
                    break;
                }
                entries.push(ArchiveEntry {
                    name: entry.name.clone(),
                    kind: if entry.is_directory() {
                        anth_proto::FileKind::Directory
                    } else {
                        anth_proto::FileKind::File
                    },
                    size: entry.size,
                    compressed: None,
                });
            }
        }
        ArchiveFormat::Tar => {
            let file = File::open(archive).map_err(|e| io_error(archive, e))?;
            list_tar(BufReader::new(file), max_entries, &mut entries)?;
        }
        ArchiveFormat::TarGz => {
            let file = File::open(archive).map_err(|e| io_error(archive, e))?;
            list_tar(
                flate2::read::GzDecoder::new(BufReader::new(file)),
                max_entries,
                &mut entries,
            )?;
        }
        ArchiveFormat::TarXz => {
            // Listing decompresses too, so it needs the same ceiling; without
            // one, merely opening a bomb in the panel was enough to exhaust
            // memory.
            list_tar(
                xz_reader(archive, MAX_LIST_BYTES)?,
                max_entries,
                &mut entries,
            )?;
        }
    }

    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_member_accepts_ordinary_names() {
        assert_eq!(safe_member("a.txt").unwrap(), PathBuf::from("a.txt"));
        assert_eq!(
            safe_member("./dir/a.txt").unwrap(),
            PathBuf::from("dir/a.txt")
        );
        assert_eq!(safe_member("a/b/c").unwrap(), PathBuf::from("a/b/c"));
    }

    #[test]
    fn safe_member_refuses_traversal_absolute_and_windows_paths() {
        // Every one of these is a known archive-escape technique.
        for name in [
            "../escaped.txt",
            "a/../../escaped.txt",
            "/etc/passwd",
            "//server/share/a",
            "..\\..\\escaped.txt",
            "a\\..\\..\\escaped.txt",
            "C:/Windows/system32",
            "",
            ".",
            "./",
            "a\0b",
        ] {
            assert!(
                safe_member(name).is_none(),
                "`{name}` should have been refused"
            );
        }
    }

    #[test]
    fn safe_member_collapses_interior_dots_but_keeps_the_name() {
        assert_eq!(
            safe_member("./a/./b/./c.txt").unwrap(),
            PathBuf::from("a/b/c.txt")
        );
    }

    #[test]
    fn detects_formats_by_extension_and_magic() {
        assert_eq!(
            detect(Path::new("/x/a.zip"), None),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(
            detect(Path::new("/x/a.tar.gz"), None),
            Some(ArchiveFormat::TarGz)
        );
        assert_eq!(
            detect(Path::new("/x/a.tgz"), None),
            Some(ArchiveFormat::TarGz)
        );
        assert_eq!(
            detect(Path::new("/x/a.tar.xz"), None),
            Some(ArchiveFormat::TarXz)
        );
        assert_eq!(
            detect(Path::new("/x/a.txz"), None),
            Some(ArchiveFormat::TarXz)
        );
        assert_eq!(
            detect(Path::new("/x/a.7z"), None),
            Some(ArchiveFormat::SevenZ)
        );
        assert_eq!(
            detect(Path::new("/x/a.tar"), None),
            Some(ArchiveFormat::Tar)
        );

        // An unknown extension falls back to the bytes.
        assert_eq!(
            detect(Path::new("/x/blob"), Some(b"PK\x03\x04rest")),
            Some(ArchiveFormat::Zip)
        );
        assert_eq!(
            detect(Path::new("/x/blob"), Some(b"7z\xbc\xaf\x27\x1c")),
            Some(ArchiveFormat::SevenZ)
        );
        assert_eq!(detect(Path::new("/x/blob"), Some(b"nope")), None);
        assert_eq!(detect(Path::new("/x/blob"), None), None);
    }

    #[test]
    fn budget_stops_an_entry_that_expands_past_the_limit() {
        let limits = ArchiveLimits {
            max_total_bytes: 1024,
            max_entries: 10,
            max_entry_bytes: 100,
        };
        let mut budget = Budget::new(limits);
        let payload = vec![0u8; 4096];
        let mut sink = Vec::new();

        let error = budget
            .copy(payload.as_slice(), &mut sink, "bomb.bin")
            .unwrap_err();

        assert_eq!(error.kind, FileErrorKind::TooLarge);
        assert!(
            sink.len() <= 100,
            "no more than the limit may be written, wrote {}",
            sink.len()
        );
    }

    #[test]
    fn budget_stops_a_total_that_exceeds_the_limit_across_entries() {
        let limits = ArchiveLimits {
            max_total_bytes: 150,
            max_entries: 10,
            max_entry_bytes: 1_000_000,
        };
        let mut budget = Budget::new(limits);
        let payload = vec![0u8; 100];

        budget
            .copy(payload.as_slice(), &mut Vec::new(), "a")
            .unwrap();
        let error = budget
            .copy(payload.as_slice(), &mut Vec::new(), "b")
            .unwrap_err();

        assert_eq!(error.kind, FileErrorKind::TooLarge);
    }

    #[test]
    fn budget_counts_entries() {
        let limits = ArchiveLimits {
            max_total_bytes: 1024,
            max_entries: 2,
            max_entry_bytes: 1024,
        };
        let mut budget = Budget::new(limits);
        budget.admit_entry().unwrap();
        budget.admit_entry().unwrap();
        assert_eq!(
            budget.admit_entry().unwrap_err().kind,
            FileErrorKind::TooLarge
        );
    }
}
