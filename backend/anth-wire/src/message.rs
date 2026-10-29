//! Data transfer objects shared by every client.
//!
//! A DTO is the JSON body of one HTTP resource. The crate carries no transport
//! and no request enum: a custom client can implement the API with any HTTP
//! stack.

use serde::{Deserialize, Serialize};

/// Bumped when a change would make an older peer misread the API.
///
/// Pre-1.0.0 breaking changes are allowed; see
/// `docs/design/protocol-changelog.md`.
pub const PROTOCOL_VERSION: u16 = 4;

/// Answer to `GET /api/hello`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol_version: u16,
}

/// Operating system identification from `os-release` and the kernel.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OsInfo {
    pub name: String,
    pub version: String,
    pub kernel: String,
    pub arch: String,
}

/// The three load averages `/proc/loadavg` reports.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// Memory and swap totals in kibibytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MemoryInfo {
    pub total_kib: u64,
    pub available_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

impl MemoryInfo {
    pub fn used_kib(&self) -> u64 {
        self.total_kib.saturating_sub(self.available_kib)
    }

    pub fn swap_used_kib(&self) -> u64 {
        self.swap_total_kib.saturating_sub(self.swap_free_kib)
    }
}

/// A point-in-time description of the host.
///
/// Unreadable fields degrade to `unknown` or zero; a missing `/proc` entry is
/// not a reason to fail the request.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub os: OsInfo,
    pub uptime_seconds: u64,
    pub cpu_model: String,
    pub logical_cores: u32,
    pub load: LoadAverage,
    pub memory: MemoryInfo,
    /// Unix epoch milliseconds when the snapshot was collected.
    pub collected_at_ms: u64,
}

/// One entry in a directory listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    /// `file`, `dir`, `symlink` or `other`.
    pub kind: String,
    pub size: u64,
    /// Unix epoch milliseconds, or 0 when the platform cannot report it.
    pub modified_ms: u64,
}

/// The default chunk a download requests when the caller does not choose one.
pub const DEFAULT_DOWNLOAD_CHUNK: u64 = 1024 * 1024;

/// The largest chunk a download may request. It keeps one base64 frame well
/// below the frame cap while still moving a large file in few round trips.
pub const MAX_DOWNLOAD_CHUNK: u64 = 8 * 1024 * 1024;

/// A chunk of a UTF-8 text file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChunk {
    /// The path relative to the file root, using `/` separators.
    pub path: String,
    pub offset: u64,
    /// Full size of the file in bytes.
    pub size: u64,
    /// Whether this chunk reaches the end of the file.
    pub eof: bool,
    pub text: String,
}

/// The result of writing a UTF-8 text file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileWritten {
    /// The path relative to the file root, using `/` separators.
    pub path: String,
    /// Bytes written.
    pub size: u64,
}

/// The kind of one entry inside an archive or the trash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    #[serde(rename = "file")]
    File,
    #[serde(rename = "dir")]
    Directory,
    #[serde(rename = "symlink")]
    Symlink,
    #[serde(rename = "other")]
    Other,
}

/// Archive formats the daemon can read or write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The name clients and the CLI print.
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub name: String,
    pub kind: FileKind,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// Size on disk inside the archive, when the format reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed: Option<u64>,
}

/// One archive and the members it holds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrashEntry {
    /// Identifier under the trash `files/` and `info/` directories.
    pub id: String,
    /// Name the entry had before it was deleted.
    pub name: String,
    /// Path the entry is restored to.
    pub original_path: String,
    /// Raw `DeletionDate` from the `.trashinfo` file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at: Option<String>,
    pub kind: FileKind,
    pub size: u64,
    /// Host path of the trash directory holding the entry.
    pub location: String,
    /// Modification time of the stored entry, used to order the listing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stored_ms: Option<u64>,
}

/// The contents of every trash directory the daemon can see.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TrashListing {
    pub entries: Vec<TrashEntry>,
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
