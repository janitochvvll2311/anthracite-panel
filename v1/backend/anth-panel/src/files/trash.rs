//! Freedesktop.org trash support.
//!
//! Layout follows the Trash specification 1.0: each volume has a trash
//! directory holding `files/<id>` and `info/<id>.trashinfo`, and an entry is
//! stored under the name it had when it was deleted. The home volume uses
//! `$XDG_DATA_HOME/Trash`.
//!
//! The spec's percentage encoding is applied to `Path=` and `DeletionDate=`
//! with no reserved set beyond the byte rules, which is why the decoder here
//! is deliberately strict: a malformed `.trashinfo` is skipped rather than
//! guessed at, because guessing would produce a restore target the desktop
//! never agreed to.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{FileError, FileErrorKind, FileKind, TrashEntry, TrashListing};

/// One trash directory and the volume it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashDirectory {
    /// Absolute path of the trash directory (the parent of `files/`).
    pub location: PathBuf,
    /// Mount point of the volume this trash belongs to. Restores are only
    /// attempted while the entry is still on it.
    pub topdir: PathBuf,
}

impl TrashDirectory {
    pub fn files(&self) -> PathBuf {
        self.location.join("files")
    }

    pub fn info(&self) -> PathBuf {
        self.location.join("info")
    }
}

/// Picks the trash directory for `topdir` following the spec's fallback rules.
///
/// The home *volume* — the filesystem holding `$HOME`, not the path equal to it
/// — always uses `$XDG_DATA_HOME/Trash`. Any other volume uses `.Trash/$uid`
/// when `.Trash` exists, is a directory, is not a symlink and is world-writable
/// with the sticky bit set; otherwise `.Trash-$uid`.
pub fn directory_for(topdir: &Path, home: &Path, uid: u32, data_home: &Path) -> TrashDirectory {
    if same_volume(topdir, home) {
        return TrashDirectory {
            location: data_home.join("Trash"),
            topdir: topdir.to_path_buf(),
        };
    }

    let shared = topdir.join(".Trash");
    if is_shared_trash(&shared) {
        return TrashDirectory {
            location: shared.join(uid.to_string()),
            topdir: topdir.to_path_buf(),
        };
    }

    TrashDirectory {
        location: topdir.join(format!(".Trash-{uid}")),
        topdir: topdir.to_path_buf(),
    }
}

/// Whether two paths live on the same filesystem.
///
/// The spec keys the home trash on the device, and on a typical desktop `/` and
/// `/home` are separate filesystems, so comparing the paths would be right by
/// accident. In a container they are one overlay, and then the home trash is the
/// only one that can be created without root.
pub fn same_volume(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    match (fs::metadata(left), fs::metadata(right)) {
        (Ok(left), Ok(right)) => left.dev() == right.dev(),
        // If either path is unreadable, fall back to the path comparison rather
        // than claiming a different volume and picking a trash nobody owns.
        _ => left == right,
    }
}

/// The spec only allows `.Trash/$uid` when the shared directory is safe to use:
/// a real directory, not a symlink, world-writable, sticky.
pub fn is_shared_trash(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    let file_type = metadata.file_type();
    if file_type.is_symlink() || !file_type.is_dir() {
        return false;
    }
    let mode = metadata.permissions().mode();
    // 0o1000 is the sticky bit, 0o002 is other-write.
    mode & 0o1000 != 0 && mode & 0o002 != 0
}

/// Percent-encodes a path or date for a `.trashinfo` file.
///
/// Only the unreserved characters plus `/` stay literal; the spec requires
/// anything that is not an ASCII letter, digit, `-`, `_`, `.`, `~` or `/` to be
/// escaped, so a UTF-8 name becomes one `%XX` per byte.
pub fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(byte as char);
            }
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// Reverses [`percent_encode`]. Rejects a truncated or non-hex escape rather
/// than emitting a replacement character, which would change the path.
pub fn percent_decode(value: &str) -> Result<String, FileError> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        let Some(hex) = bytes.get(index + 1..index + 3) else {
            return Err(invalid_info("truncated percent escape"));
        };
        let Ok(text) = std::str::from_utf8(hex) else {
            return Err(invalid_info("percent escape is not text"));
        };
        let Ok(byte) = u8::from_str_radix(text, 16) else {
            return Err(invalid_info("percent escape is not hexadecimal"));
        };
        decoded.push(byte);
        index += 3;
    }
    String::from_utf8(decoded).map_err(|_| invalid_info("percent escape does not decode to UTF-8"))
}

fn invalid_info(detail: &str) -> FileError {
    FileError::new(
        FileErrorKind::InvalidPath,
        format!("malformed .trashinfo entry: {detail}"),
    )
}

/// The parsed contents of one `.trashinfo` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashInfo {
    /// Path the entry was deleted from, as recorded by the deleter.
    pub path: String,
    /// `YYYY-MM-DDThh:mm:ss`, local time, from the deleter.
    pub deletion_date: Option<String>,
}

/// Parses a `.trashinfo` body.
///
/// The format is an INI file with a single `[Trash Info]` group. Unknown keys
/// and trailing whitespace are tolerated for the same reason the spec tolerates
/// them: other desktops write these files, not only this daemon.
pub fn parse_info(text: &str) -> Result<TrashInfo, FileError> {
    let mut in_group = false;
    let mut path = None;
    let mut deletion_date = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            in_group = name.trim() == "Trash Info";
            continue;
        }
        if !in_group {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "Path" => path = Some(percent_decode(value.trim())?),
            "DeletionDate" => deletion_date = Some(value.trim().to_string()),
            _ => {}
        }
    }

    let path = path.ok_or_else(|| invalid_info("no Path key"))?;
    Ok(TrashInfo {
        path,
        deletion_date,
    })
}

/// Renders a `.trashinfo` body. The group header and `Path` are mandatory; the
/// date is written because the spec requires it, not because this daemon reads
/// it back.
pub fn render_info(path: &str, at: SystemTime) -> String {
    format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode(path),
        format_deletion_date(at)
    )
}

/// `YYYY-MM-DDThh:mm:ss` in UTC.
///
/// The spec asks for local time, but the daemon has no reliable timezone
/// database, and readers only display the value. UTC keeps the file valid and
/// self-consistent instead of silently wrong for half the year.
pub fn format_deletion_date(at: SystemTime) -> String {
    let seconds = at
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let (year, month, day, hour, minute, second) = civil_from_unix(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}")
}

/// Converts a Unix timestamp to a calendar date. This is the standard
/// days-to-civil algorithm, kept here so the crate needs no time dependency.
fn civil_from_unix(seconds: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = seconds.div_euclid(86_400);
    let rem = seconds.rem_euclid(86_400);

    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    (
        y,
        m as u32,
        d as u32,
        hour as u32,
        minute as u32,
        second as u32,
    )
}

/// A trash root the daemon is allowed to read and write.
#[derive(Debug, Clone)]
pub struct TrashService {
    enabled: bool,
    max_bytes: Option<u64>,
    max_entries: usize,
    home: PathBuf,
    data_home: PathBuf,
    /// Every mounted topdir, so a listing does not walk the same volume twice
    /// and a restore knows which trash an entry came from.
    topdirs: Vec<PathBuf>,
}

impl TrashService {
    pub fn new(
        enabled: bool,
        max_bytes: Option<u64>,
        max_entries: usize,
        home: impl Into<PathBuf>,
        data_home: impl Into<PathBuf>,
        topdirs: Vec<PathBuf>,
    ) -> Self {
        Self {
            enabled,
            max_bytes,
            max_entries,
            home: home.into(),
            data_home: data_home.into(),
            topdirs,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }

    /// Trash directories for every known volume, de-duplicated by location.
    pub fn directories(&self) -> Vec<TrashDirectory> {
        let mut directories = Vec::new();
        for topdir in &self.topdirs {
            let directory = directory_for(topdir, &self.home, current_uid(), &self.data_home);
            if !directories
                .iter()
                .any(|seen: &TrashDirectory| seen.location == directory.location)
            {
                directories.push(directory);
            }
        }
        directories
    }

    fn require_enabled(&self) -> Result<(), FileError> {
        if self.enabled {
            Ok(())
        } else {
            Err(FileError::new(
                FileErrorKind::Disabled,
                "the trash is disabled; enable `[files] trash`".to_string(),
            ))
        }
    }

    /// Moves `host` into the trash of `topdir`.
    ///
    /// `virtual_path` is the path the entry had, which is what a restore reads
    /// back. Returns the identifier chosen for it.
    pub fn move_to_trash(
        &self,
        host: &Path,
        virtual_path: &str,
        topdir: &Path,
    ) -> Result<String, FileError> {
        self.require_enabled()?;

        let directory = directory_for(topdir, &self.home, current_uid(), &self.data_home);
        for path in [directory.files(), directory.info()] {
            fs::create_dir_all(&path).map_err(|e| {
                if e.kind() == io::ErrorKind::PermissionDenied {
                    // A `.Trash-UID` on a volume the account cannot write to is
                    // the common case, and "permission denied" alone does not
                    // say what to do about it.
                    FileError::new(
                        FileErrorKind::PermissionDenied,
                        format!(
                            "cannot create the trash directory `{}`: {e}. The volume \
                             `{}` needs a writable `.Trash/$uid` or `.Trash-$uid`; \
                             have an administrator create one, or move the entry to a \
                             volume whose trash is writable",
                            path.display(),
                            topdir.display()
                        ),
                    )
                } else {
                    io_error(&path, e)
                }
            })?;
        }

        let name = host
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".to_string());

        // The spec allows a numbered suffix when the name is taken. Probing by
        // creating the file exclusively is what makes this safe against a
        // concurrent delete of the same name.
        let mut attempt = 0u32;
        let (id, stored, info_path) = loop {
            let id = if attempt == 0 {
                name.clone()
            } else {
                format!("{name}.{attempt}")
            };
            let stored = directory.files().join(&id);
            let info_path = directory.info().join(format!("{id}.trashinfo"));
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&info_path)
            {
                Ok(_) => break (id, stored, info_path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    attempt += 1;
                    if attempt > 9999 {
                        return Err(FileError::new(
                            FileErrorKind::AlreadyExists,
                            format!("too many entries named `{name}` in the trash"),
                        ));
                    }
                }
                Err(error) => return Err(io_error(&info_path, error)),
            }
        };

        // Write the metadata before moving the file: a crash then leaves a
        // dangling `.trashinfo`, which the listing skips, rather than a file
        // nobody can restore.
        if let Err(error) = fs::write(&info_path, render_info(virtual_path, SystemTime::now())) {
            let _ = fs::remove_file(&info_path);
            return Err(io_error(&info_path, error));
        }

        if let Err(error) = fs::rename(host, &stored) {
            let _ = fs::remove_file(&info_path);
            // A cross-device move is not expected because the trash is chosen
            // from the entry's own volume, but say so instead of a bare `EXDEV`.
            return Err(io_error(host, error));
        }

        self.enforce_retention(&directory);

        Ok(id)
    }

    /// Lists every known trash directory.
    pub fn list(&self) -> Result<TrashListing, FileError> {
        let mut entries = Vec::new();
        let mut used_bytes = 0u64;

        for directory in self.directories() {
            let Ok(read_dir) = fs::read_dir(directory.info()) else {
                continue;
            };
            for item in read_dir.flatten() {
                let info_path = item.path();
                if info_path.extension().and_then(|e| e.to_str()) != Some("trashinfo") {
                    continue;
                }
                let Some(id) = info_path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                else {
                    continue;
                };
                let Ok(text) = fs::read_to_string(&info_path) else {
                    continue;
                };
                let Ok(info) = parse_info(&text) else {
                    // A malformed entry is not listed and not hidden: it stays
                    // on disk for the operator to inspect.
                    continue;
                };
                let stored = directory.files().join(&id);
                let Ok(metadata) = fs::symlink_metadata(&stored) else {
                    continue;
                };
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
                let size = if kind == FileKind::Directory {
                    directory_size(&stored)
                } else {
                    metadata.len()
                };
                used_bytes += size;

                entries.push(TrashEntry {
                    name: id.clone(),
                    id,
                    original_path: info.path,
                    deleted_at: info.deletion_date,
                    kind,
                    size,
                    location: directory.location.to_string_lossy().into_owned(),
                    stored_ms: metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64),
                });
            }
        }

        entries.sort_by(|a, b| b.stored_ms.cmp(&a.stored_ms).then_with(|| a.id.cmp(&b.id)));

        let total = entries.len();
        let truncated = total > self.max_entries;
        if truncated {
            entries.truncate(self.max_entries);
        }

        Ok(TrashListing {
            entries,
            total,
            truncated,
            warning: truncated.then(|| {
                format!(
                    "showing {} of {total} trashed entries; raise files.max_entries to see more",
                    self.max_entries
                )
            }),
            enabled: self.enabled,
            used_bytes,
            max_bytes: self.max_bytes,
        })
    }

    /// Finds one entry by id across every known trash directory.
    pub fn find(&self, id: &str) -> Result<TrashEntry, FileError> {
        if id.is_empty() || id.contains('/') || id.contains('\0') || id == "." || id == ".." {
            return Err(FileError::new(
                FileErrorKind::InvalidPath,
                format!("`{id}` is not a valid trash identifier"),
            ));
        }
        self.list()?
            .entries
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| {
                FileError::new(
                    FileErrorKind::NotFound,
                    format!("no trash entry `{id}` is available"),
                )
            })
    }

    /// Restores one entry, refusing to overwrite unless asked.
    ///
    /// The recorded path is validated by the caller, because only the caller
    /// knows the virtual root the path must stay inside.
    pub fn restore(&self, id: &str, host_target: &Path, overwrite: bool) -> Result<(), FileError> {
        self.require_enabled()?;
        let entry = self.find(id)?;

        if !overwrite {
            match fs::symlink_metadata(host_target) {
                Ok(_) => {
                    return Err(FileError::new(
                        FileErrorKind::AlreadyExists,
                        format!("`{}` already exists", host_target.display()),
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(io_error(host_target, error)),
            }
        } else if let Ok(metadata) = fs::symlink_metadata(host_target) {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                // Renaming a file over a directory fails with a bare `EISDIR`,
                // and removing the directory would be a silent second delete.
                return Err(FileError::new(
                    FileErrorKind::IsADirectory,
                    format!("`{}` is a directory", host_target.display()),
                ));
            }
            fs::remove_file(host_target).map_err(|e| io_error(host_target, e))?;
        }

        let location = PathBuf::from(&entry.location);
        let stored = location.join("files").join(&entry.id);
        let info_path = location
            .join("info")
            .join(format!("{}.trashinfo", entry.id));

        fs::rename(&stored, host_target).map_err(|e| io_error(&stored, e))?;
        // The metadata is only removed once the move succeeded, so a failed
        // restore leaves a usable entry rather than an orphaned file.
        fs::remove_file(&info_path).map_err(|e| io_error(&info_path, e))?;
        Ok(())
    }

    /// Deletes every trashed entry. Returns how many were removed.
    pub fn empty(&self) -> Result<usize, FileError> {
        self.require_enabled()?;
        let mut removed = 0;
        for directory in self.directories() {
            let Ok(read_dir) = fs::read_dir(directory.files()) else {
                continue;
            };
            for item in read_dir.flatten() {
                let path = item.path();
                let ok = match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                        fs::remove_dir_all(&path).is_ok()
                    }
                    Ok(_) => fs::remove_file(&path).is_ok(),
                    Err(_) => false,
                };
                if ok {
                    let name = item.file_name().to_string_lossy().into_owned();
                    let _ = fs::remove_file(directory.info().join(format!("{name}.trashinfo")));
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    /// Drops the oldest entries once the retention ceiling is exceeded.
    ///
    /// Best effort: a delete that cannot be undone because the trash is full is
    /// still a delete that happened, so cleanup failure does not fail the
    /// request.
    fn enforce_retention(&self, _directory: &TrashDirectory) {
        let Some(ceiling) = self.max_bytes else {
            return;
        };
        let Ok(listing) = self.list() else {
            return;
        };
        if listing.used_bytes <= ceiling {
            return;
        }

        let mut ordered = listing.entries;
        // Oldest first, so the newest deletions survive a purge.
        ordered.sort_by_key(|entry| entry.stored_ms);

        let mut used = listing.used_bytes;
        for entry in ordered {
            if used <= ceiling {
                break;
            }
            let location = PathBuf::from(&entry.location);
            let stored = location.join("files").join(&entry.id);
            let ok = match fs::symlink_metadata(&stored) {
                Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    fs::remove_dir_all(&stored).is_ok()
                }
                Ok(_) => fs::remove_file(&stored).is_ok(),
                Err(_) => false,
            };
            if ok {
                let _ = fs::remove_file(
                    location
                        .join("info")
                        .join(format!("{}.trashinfo", entry.id)),
                );
                used = used.saturating_sub(entry.size);
            }
        }
    }
}

fn current_uid() -> u32 {
    // SAFETY: `getuid` takes no arguments, touches no memory and cannot fail.
    unsafe { libc::getuid() }
}

fn directory_size(path: &Path) -> u64 {
    let Ok(read_dir) = fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for item in read_dir.flatten() {
        let item_path = item.path();
        match fs::symlink_metadata(&item_path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                total = total.saturating_add(directory_size(&item_path));
            }
            Ok(metadata) => total = total.saturating_add(metadata.len()),
            Err(_) => {}
        }
    }
    total
}

fn io_error(path: &Path, error: io::Error) -> FileError {
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
    FileError::new(kind, format!("`{}`: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn percent_encoding_round_trips_utf8_and_specials() {
        for value in [
            "/home/ana/a b.txt",
            "/tmp/ñandú/100%.txt",
            "/x/new\nline",
            "/plain",
        ] {
            let encoded = percent_encode(value);
            // The point of the encoding is that the result can live on one
            // line of an INI file: no space, no newline, all ASCII.
            assert!(
                encoded.is_ascii() && !encoded.contains(' ') && !encoded.contains('\n'),
                "`{encoded}` is not safe to write unescaped"
            );
            assert_eq!(percent_decode(&encoded).unwrap(), value);
        }
    }

    #[test]
    fn percent_encoding_escapes_non_ascii_one_byte_at_a_time() {
        assert_eq!(percent_encode("/ñ"), "/%C3%B1");
    }

    #[test]
    fn percent_decode_rejects_malformed_escapes() {
        for bad in ["/a%2", "/a%zz", "/a%F"] {
            assert_eq!(
                percent_decode(bad).unwrap_err().kind,
                FileErrorKind::InvalidPath,
                "`{bad}` should be rejected"
            );
        }
    }

    #[test]
    fn parses_a_trashinfo_written_by_the_spec() {
        let info = parse_info(
            "[Trash Info]\nPath=/home/ana/notes%20x.txt\nDeletionDate=2024-05-01T10:00:00\n",
        )
        .unwrap();
        assert_eq!(info.path, "/home/ana/notes x.txt");
        assert_eq!(info.deletion_date.as_deref(), Some("2024-05-01T10:00:00"));
    }

    #[test]
    fn ignores_unknown_keys_before_the_group() {
        // Another implementation may write its own keys; the spec says readers
        // must ignore what they do not know.
        let info =
            parse_info("[Other]\nPath=/ignored\n[Trash Info]\nPath=/real\nX-Custom=1\n").unwrap();
        assert_eq!(info.path, "/real");
    }

    #[test]
    fn rejects_info_without_a_path() {
        assert_eq!(
            parse_info("[Trash Info]\nDeletionDate=2024-01-01T00:00:00\n")
                .unwrap_err()
                .kind,
            FileErrorKind::InvalidPath
        );
    }

    #[test]
    fn renders_a_body_the_parser_reads_back() {
        let at = UNIX_EPOCH + std::time::Duration::from_secs(1_714_557_600);
        let rendered = render_info("/home/ana/sp ace.txt", at);
        assert!(rendered.starts_with("[Trash Info]\nPath=/home/ana/sp%20ace.txt\n"));
        assert_eq!(parse_info(&rendered).unwrap().path, "/home/ana/sp ace.txt");
    }

    #[test]
    fn formats_calendar_dates_across_a_leap_day() {
        // 2024-02-29T00:00:00Z and the day after.
        assert_eq!(
            format_deletion_date(UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800)),
            "2024-02-29T00:00:00"
        );
        assert_eq!(
            format_deletion_date(UNIX_EPOCH + std::time::Duration::from_secs(1_709_251_200)),
            "2024-03-01T00:00:00"
        );
        assert_eq!(format_deletion_date(UNIX_EPOCH), "1970-01-01T00:00:00");
    }

    #[test]
    fn home_volume_uses_the_data_home_trash() {
        let directory = directory_for(
            Path::new("/home/ana"),
            Path::new("/home/ana"),
            1000,
            Path::new("/home/ana/.local/share"),
        );
        assert_eq!(
            directory.location,
            PathBuf::from("/home/ana/.local/share/Trash")
        );
    }

    #[test]
    fn an_unreadable_path_falls_back_to_comparing_paths() {
        assert!(!same_volume(
            Path::new("/nonexistent-a"),
            Path::new("/nonexistent-b")
        ));
        assert!(same_volume(
            Path::new("/nonexistent-a"),
            Path::new("/nonexistent-a")
        ));
    }

    #[test]
    fn a_directory_on_the_home_volume_uses_the_home_trash() {
        // The spec keys this on the device, not on the path: a root that lives
        // in the same filesystem as $HOME must still use the home trash.
        let root = std::env::temp_dir().join(format!("anth-trash-dev-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());

        let directory = directory_for(
            &root,
            Path::new(&home),
            current_uid(),
            Path::new("/nonexistent-data-home"),
        );

        assert_eq!(
            directory.location,
            PathBuf::from("/nonexistent-data-home/Trash"),
            "a directory on the home device must not get a .Trash-UID"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn other_volumes_fall_back_when_there_is_no_shared_trash() {
        let directory = directory_for(
            Path::new("/mnt/usb"),
            Path::new("/home/ana"),
            1000,
            Path::new("/home/ana/.local/share"),
        );
        assert_eq!(directory.location, PathBuf::from("/mnt/usb/.Trash-1000"));
    }

    #[test]
    fn a_sticky_world_writable_shared_trash_is_preferred() {
        let root = std::env::temp_dir().join(format!("anth-trash-shared-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let volume = root.join("vol");
        let shared = volume.join(".Trash");
        fs::create_dir_all(&shared).unwrap();
        let mut permissions = fs::metadata(&shared).unwrap().permissions();
        permissions.set_mode(0o1777);
        fs::set_permissions(&shared, permissions).unwrap();

        let directory = directory_for(
            &volume,
            Path::new("/home/ana"),
            1000,
            Path::new("/home/ana/.local/share"),
        );
        assert_eq!(directory.location, volume.join(".Trash").join("1000"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_shared_trash_without_the_sticky_bit_is_refused() {
        let root = std::env::temp_dir().join(format!("anth-trash-nosticky-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let shared = root.join(".Trash");
        fs::create_dir_all(&shared).unwrap();
        let mut permissions = fs::metadata(&shared).unwrap().permissions();
        permissions.set_mode(0o0777);
        fs::set_permissions(&shared, permissions).unwrap();
        assert!(!is_shared_trash(&shared));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_symlinked_shared_trash_is_refused() {
        let root = std::env::temp_dir().join(format!("anth-trash-symlink-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let real = root.join("real");
        fs::create_dir_all(&real).unwrap();
        let mut permissions = fs::metadata(&real).unwrap().permissions();
        permissions.set_mode(0o1777);
        fs::set_permissions(&real, permissions).unwrap();
        let link = root.join(".Trash");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        assert!(
            !is_shared_trash(&link),
            "a symlinked .Trash must not be used"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
