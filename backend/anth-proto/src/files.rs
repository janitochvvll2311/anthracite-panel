//! File explorer requests and responses.
//!
//! These types are the transport contract only. The daemon owns path
//! resolution, filesystem access and the write policy.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::b64;

/// Preferred chunk size for file reads and writes.
///
/// Base64 expands bytes by roughly one third, so a 256 KiB chunk still fits in
/// the protocol's 1 MiB line limit with room for the JSON envelope.
pub const DEFAULT_FILE_CHUNK: u32 = 256 * 1024;

/// Hard cap the daemon accepts for a single read or write chunk.
pub const MAX_FILE_CHUNK: u32 = 512 * 1024;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    Other,
}

/// One directory entry.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    /// Virtual absolute path. `/` is the configured filesystem root.
    pub path: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified_ms: Option<u64>,
    pub hidden: bool,
    /// Raw link target when `kind` is `Symlink`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// One directory listing.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct FileListing {
    pub path: String,
    pub parent: Option<String>,
    pub entries: Vec<FileEntry>,
    /// Number of entries found before the row cap was applied.
    pub total: usize,
    pub truncated: bool,
    /// Human-readable notice when the listing was shortened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Whether the daemon accepts the write operations.
    pub write_enabled: bool,
}

/// One chunk of a file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct FileChunk {
    pub path: String,
    pub offset: u64,
    /// Total file size in bytes.
    pub size: u64,
    pub eof: bool,
    #[serde(with = "b64")]
    pub bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileWritten {
    pub path: String,
    pub size: u64,
}

/// Archive formats the daemon can read or write.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar")]
    Tar,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.xz")]
    TarXz,
    #[serde(rename = "7z")]
    SevenZ,
}

impl ArchiveFormat {
    /// The name clients and the CLI print. Kept next to the serde attributes so
    /// the wire spelling and the display spelling cannot drift apart.
    pub fn label(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
            Self::TarXz => "tar.xz",
            Self::SevenZ => "7z",
        }
    }
}

/// One member of an archive.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub name: String,
    pub kind: FileKind,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// Size on disk inside the archive, when the format reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed: Option<u64>,
}

/// One archive and the entries it holds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ArchiveListing {
    pub path: String,
    pub format: Option<ArchiveFormat>,
    pub entries: Vec<ArchiveEntry>,
    pub total: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// One entry held by the freedesktop trash.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TrashEntry {
    /// Identifier under the trash `files/` and `info/` directories.
    pub id: String,
    /// Name the entry had before it was deleted.
    pub name: String,
    /// Virtual path the entry is restored to.
    pub original_path: String,
    /// Raw `DeletionDate` from the `.trashinfo` file, as written by the
    /// application that deleted it. Not parsed: another desktop may have
    /// written a timezone this daemon does not use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    pub kind: FileKind,
    pub size: u64,
    /// Host path of the trash directory holding the entry.
    pub location: String,
    /// Modification time of the stored entry, used to order the listing.
    pub stored_ms: Option<u64>,
}

/// Contents of every trash directory the daemon can see.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct TrashListing {
    pub entries: Vec<TrashEntry>,
    /// Number of entries found before the row cap was applied.
    pub total: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Whether `[files] trash` is on, so the panel knows if trashing works.
    pub enabled: bool,
    /// Bytes currently held, and the retention ceiling when one is set.
    pub used_bytes: u64,
    pub max_bytes: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileErrorKind {
    NotFound,
    PermissionDenied,
    NotADirectory,
    IsADirectory,
    InvalidPath,
    AlreadyExists,
    DirectoryNotEmpty,
    TooLarge,
    Disabled,
    Io,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileError {
    pub kind: FileErrorKind,
    pub detail: String,
}

impl FileError {
    pub fn new(kind: FileErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for FileError {}
